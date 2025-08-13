#![allow(dead_code)]

use core::fmt::Write as _;
use core::sync::atomic::{AtomicPtr, Ordering};

/// Raw pointer to UEFI text output for emergency printing in panic context.
/// This is a best-effort facility and intentionally uses raw pointers to avoid
/// lifetime and borrow restrictions during panic unwinding in `no_std`.
static UEFI_STDOUT_PTR: AtomicPtr<uefi::proto::console::text::Output> = AtomicPtr::new(core::ptr::null_mut());

/// Install a raw pointer to UEFI Text Output for emergency printing.
/// Caller must pass a valid pointer obtained from `SystemTable::stdout()`.
pub unsafe fn install_stdout_ptr(ptr: *mut uefi::proto::console::text::Output) {
    UEFI_STDOUT_PTR.store(ptr, Ordering::Relaxed);
}

/// Try to print a panic banner using the installed stdout pointer.
pub fn try_print_emergency(msg: &str) {
    let p = UEFI_STDOUT_PTR.load(Ordering::Relaxed);
    if p.is_null() { return; }
    // SAFETY: The pointer is provided by firmware and assumed to remain valid
    // during program lifetime. We avoid any allocation and keep printing minimal.
    let out = unsafe { &mut *p };
    let _ = out.write_str(msg);
}

/// Best-effort panic reporter. Avoids allocation and complex formatting.
pub fn report_panic(_info: &core::panic::PanicInfo) {
    try_print_emergency("PANIC: unrecoverable error\r\n");
    // Best-effort recent log dump to assist diagnosis
    let p = UEFI_STDOUT_PTR.load(core::sync::atomic::Ordering::Relaxed);
    if !p.is_null() {
        unsafe {
            let out = &mut *p;
            crate::obs::log::dump_with_writer(|bytes| { let _ = out.write_str(core::str::from_utf8(bytes).unwrap_or("\r\n")); });
            // Also emit recent trace lines
            crate::obs::trace::dump_with_writer(|bytes| { let _ = out.write_str(core::str::from_utf8(bytes).unwrap_or("\r\n")); });
        }
    }
}


