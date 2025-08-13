#![allow(dead_code)]

//! Real-mode trampoline builder for AP startup via SIPI.
//!
//! This module constructs a tiny 16-bit code sequence placed at a 4KiB-aligned
//! physical page (preferably below 1MiB) so that APs started by SIPI begin
//! execution at that page base. The code increments a mailbox counter located
//! within the same page and halts, enabling the BSP to observe successful AP
//! wakeups without requiring protected/long mode transitions yet.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

/// Information about the prepared trampoline.
#[derive(Clone, Copy, Debug)]
pub struct TrampolineInfo {
    pub phys_base: u64,
    pub vector: u8,
    pub mailbox_offset: u16,
}

/// Build 16-bit real-mode code that increments a 16-bit counter at [cs:mailbox]
/// and then halts forever.
fn build_rm_trampoline(mailbox_off: u16, out: &mut [u8]) -> usize {
    // 16-bit machine code (NASM-like annotation):
    //   cli
    //   push cs
    //   pop ds
    //   mov bx, imm16          ; mailbox offset within the same 64KiB segment
    //   mov ax, [bx]
    //   inc ax
    //   mov [bx], ax
    //   hlt
    //   jmp $                  ; infinite loop
    // Encoding assembled by hand to avoid external tools.
    let mut n = 0usize;
    let emit = |buf: &mut [u8], n: &mut usize, bytes: &[u8]| { for &b in bytes { buf[*n] = b; *n += 1; } };
    emit(out, &mut n, &[0xFA]); // cli
    emit(out, &mut n, &[0x0E]); // push cs
    emit(out, &mut n, &[0x1F]); // pop ds
    emit(out, &mut n, &[0xBB, (mailbox_off & 0xFF) as u8, (mailbox_off >> 8) as u8]); // mov bx, imm16
    emit(out, &mut n, &[0x8B, 0x07]); // mov ax, [bx]
    emit(out, &mut n, &[0x40]); // inc ax
    emit(out, &mut n, &[0x89, 0x07]); // mov [bx], ax
    emit(out, &mut n, &[0xF4]); // hlt
    emit(out, &mut n, &[0xEB, 0xFE]); // jmp $ (short -2)
    n
}

/// Prepare the real-mode trampoline at a preferred address and return info.
pub fn prepare_real_mode_trampoline(system_table: &SystemTable<Boot>) -> Option<TrampolineInfo> {
    // Prefer a classic low real-mode area such as 0x7000. Use LOADER_CODE.
    let desired = 0x0000_7000u64;
    let page = crate::mm::uefi::alloc_pages_at(system_table, desired, 1, uefi::table::boot::MemoryType::LOADER_CODE)
        .or_else(|| crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_CODE))?;
    // Mailbox at offset 0x800 within the same page to remain within segment.
    let mailbox_off: u16 = 0x0800;
    // Zero page and set mailbox to 0
    unsafe {
        core::ptr::write_bytes(page, 0, 4096);
        core::ptr::write_volatile(page.add(mailbox_off as usize) as *mut u16, 0u16);
    }
    // Build and write the 16-bit code at page base.
    let mut buf = [0u8; 64];
    let code_len = build_rm_trampoline(mailbox_off, &mut buf);
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), page as *mut u8, code_len); }
    // Compute SIPI vector (page number >> 12 lower 8 bits)
    let phys = page as u64;
    let vector = ((phys >> 12) & 0xFF) as u8;
    Some(TrampolineInfo { phys_base: phys, vector, mailbox_offset: mailbox_off })
}

/// Read the 16-bit mailbox counter from the trampoline page.
pub fn read_mailbox_count(info: TrampolineInfo) -> u16 {
    let p = (info.phys_base as usize + info.mailbox_offset as usize) as *const u16;
    unsafe { core::ptr::read_volatile(p) }
}


