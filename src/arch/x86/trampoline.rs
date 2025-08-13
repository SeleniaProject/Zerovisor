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

/// Build 16-bit real-mode bootstrap that switches to 32-bit protected mode,
/// updates a mailbox counter and success flag, then halts.
fn build_pm_trampoline(mailbox_off: u16, gdtr_off: u16, pm_entry_off: u16, out: &mut [u8]) -> usize {
    // 16-bit code:
    //   cli
    //   push cs; pop ds
    //   lgdt [gdtr]
    //   mov eax, cr0; or eax, 1; mov cr0, eax    ; enable PE
    //   jmp 0x08:pm_entry
    // pm_entry:
    //   mov ax, 0x10; mov ds, ax; mov es, ax; mov ss, ax
    //   mov bx, mailbox_off; mov ax, [bx]; inc ax; mov [bx], ax
    //   mov byte [bx+4], 1
    //   hlt; jmp $
    let mut n = 0usize;
    let emit = |buf: &mut [u8], n: &mut usize, bytes: &[u8]| { for &b in bytes { buf[*n] = b; *n += 1; } };
    // cli; push cs; pop ds
    emit(out, &mut n, &[0xFA, 0x0E, 0x1F]);
    // lgdt [disp16]
    emit(out, &mut n, &[0x0F, 0x01, 0x16, (gdtr_off & 0xFF) as u8, (gdtr_off >> 8) as u8]);
    // mov eax, cr0
    emit(out, &mut n, &[0x66, 0x0F, 0x20, 0xC0]);
    // or eax, 1
    emit(out, &mut n, &[0x66, 0x83, 0xC8, 0x01]);
    // mov cr0, eax
    emit(out, &mut n, &[0x66, 0x0F, 0x22, 0xC0]);
    // far jmp 0x08:pm_entry
    emit(out, &mut n, &[0xEA, (pm_entry_off & 0xFF) as u8, (pm_entry_off >> 8) as u8, 0x08, 0x00]);
    // pm_entry label starts here
    // mov ax, 0x10
    emit(out, &mut n, &[0xB8, 0x10, 0x00]);
    // mov ds, ax; mov es, ax; mov ss, ax
    emit(out, &mut n, &[0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0]);
    // mov bx, mailbox_off
    emit(out, &mut n, &[0xBB, (mailbox_off & 0xFF) as u8, (mailbox_off >> 8) as u8]);
    // mov ax, [bx]; inc ax; mov [bx], ax
    emit(out, &mut n, &[0x8B, 0x07, 0x40, 0x89, 0x07]);
    // mov byte [bx+4], 1
    emit(out, &mut n, &[0x80, 0x47, 0x04, 0x01]);
    // hlt; jmp $
    emit(out, &mut n, &[0xF4, 0xEB, 0xFE]);
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
    // Choose GDTR and GDT placement inside the page.
    let gdtr_off: u16 = 0x08E0;
    let gdt_off: u16 = 0x0900;
    // The protected-mode entry point offset inside the page
    let pm_entry_off: u16 = 0x0020; // after initial prologue and far jump target
    // Zero page and set mailbox to 0
    unsafe {
        core::ptr::write_bytes(page, 0, 4096);
        core::ptr::write_volatile(page.add(mailbox_off as usize) as *mut u16, 0u16);
    }
    // Build and write the 16-bit/pm bootstrap code at page base.
    let mut buf = [0u8; 128];
    let code_len = build_pm_trampoline(mailbox_off, gdtr_off, pm_entry_off, &mut buf);
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), page as *mut u8, code_len); }
    // Write GDTR structure (limit + base)
    let gdt_base = (page as u64).wrapping_add(gdt_off as u64);
    let gdtr_ptr = unsafe { page.add(gdtr_off as usize) } as *mut u8;
    unsafe {
        // limit = (3*8 - 1) = 23
        core::ptr::write_volatile(gdtr_ptr as *mut u16, 23u16);
        // base 32-bit
        core::ptr::write_volatile(gdtr_ptr.add(2) as *mut u32, gdt_base as u32);
    }
    // Build GDT: null, code, data
    let gdt_ptr = unsafe { page.add(gdt_off as usize) } as *mut u8;
    unsafe {
        // null descriptor (8 bytes of zero)
        core::ptr::write_bytes(gdt_ptr, 0, 8);
        // code descriptor @ +8
        let p = gdt_ptr.add(8);
        // limit low
        core::ptr::write_volatile(p as *mut u16, 0xFFFFu16);
        // base low
        core::ptr::write_volatile(p.add(2) as *mut u16, 0u16);
        // base mid
        core::ptr::write_volatile(p.add(4) as *mut u8, 0u8);
        // access = 0x9A (present|exec|readable)
        core::ptr::write_volatile(p.add(5) as *mut u8, 0x9Au8);
        // gran = 0xCF (limit high=0xF, gran=1, 32-bit=1, long=0)
        core::ptr::write_volatile(p.add(6) as *mut u8, 0xCFu8);
        // base high
        core::ptr::write_volatile(p.add(7) as *mut u8, 0u8);
        // data descriptor @ +16
        let pd = gdt_ptr.add(16);
        core::ptr::write_volatile(pd as *mut u16, 0xFFFFu16);
        core::ptr::write_volatile(pd.add(2) as *mut u16, 0u16);
        core::ptr::write_volatile(pd.add(4) as *mut u8, 0u8);
        // access = 0x92 (present|data|writable)
        core::ptr::write_volatile(pd.add(5) as *mut u8, 0x92u8);
        core::ptr::write_volatile(pd.add(6) as *mut u8, 0xCFu8);
        core::ptr::write_volatile(pd.add(7) as *mut u8, 0u8);
    }
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

/// Build a minimal long-mode bootstrap page tables and provide CR3 value for APs.
pub fn build_ap_long_mode_tables(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<u64> {
    let pml4 = crate::mm::paging::build_identity_2m(system_table, limit_bytes)?;
    Some(pml4 as u64)
}


