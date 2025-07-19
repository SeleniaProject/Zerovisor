//! Zerovisor hypervisor logging subsystem (Requirement 5)
//!
//! Provides UART‐optional, lock-free ring-buffer logging that works in a `no_std`
//! environment.  The buffer is memory-mapped so that an external debugger or
//! monitoring agent can fetch logs without hypervisor intervention.
//!
//! • Logs are pushed via the `log!` macro which accepts standard `format!`
//!   syntax.
//! • The ring buffer is 64 KiB and overwrites old data when full.
//! • If the architecture exposes a UART (16550A compatible on x86_64), bytes are
//!   also sent out the serial port for early debugging.
//!
//! This module is *self-contained* and does not rely on `alloc`.

#![allow(dead_code)]

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

const LOG_BUF_SIZE: usize = 64 * 1024; // 64 KiB
static mut LOG_BUFFER: [u8; LOG_BUF_SIZE] = [0; LOG_BUF_SIZE];
static WRITE_POS: AtomicUsize = AtomicUsize::new(0);

/// UART MMIO base address (legacy COM1). Override per-arch if needed.
#[cfg(target_arch = "x86_64")]
const UART_BASE: u16 = 0x3F8;

#[inline(always)]
fn uart_write_byte(byte: u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") UART_BASE, in("al") byte, options(nomem, nostack, preserves_flags));
    }
}

/// Internal writer implementing `core::fmt::Write`.
pub struct RingBufferWriter;

impl Write for RingBufferWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            write_byte(b);
        }
        Ok(())
    }
}

#[inline]
fn write_byte(b: u8) {
    // 1. Write to memory ring buffer
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed) % LOG_BUF_SIZE;
    unsafe { LOG_BUFFER[pos] = b; }

    // 2. Optionally to UART for early debug
    uart_write_byte(b);
}

/// Low-level logging macro (English comments required by user rules)
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = core::fmt::write(&mut $crate::log::RingBufferWriter, format_args!($($arg)*));
        let _ = $crate::log::RingBufferWriter.write_str("\r\n");
    }};
}

/// Expose log buffer for external tools (read-only).
pub fn get_buffer() -> &'static [u8; LOG_BUF_SIZE] {
    unsafe { &LOG_BUFFER }
} 