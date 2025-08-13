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
fn build_pm_trampoline(mailbox_off: u16, gdtr_off: u16, pm_entry_off: u16, lm_entry_off: u16, out: &mut [u8]) -> usize {
    // 16-bit code:
    //   cli
    //   push cs; pop ds
    //   lgdt [gdtr]
    //   mov eax, cr0; or eax, 1; mov cr0, eax    ; enable PE
    //   jmp 0x08:pm_entry
    // pm_entry:
    //   mov ax, 0x10; mov ds, ax; mov es, ax; mov ss, ax
    //   mov ebx, mailbox_off; mov ax, [bx]; inc ax; mov [bx], ax
    //   mov byte [bx+4], 1
    //   mov eax, [ebx+2]; mov cr3, eax             ; load AP CR3 (low 32 bits)
    //   mov eax, cr4; or eax, 0x20; mov cr4, eax   ; set PAE
    //   mov ecx, 0xC0000080; rdmsr; or eax, 0x100; wrmsr  ; set EFER.LME
    //   mov eax, cr0; or eax, 0x80000000; mov cr0, eax    ; enable PG
    //   jmp 0x18:lm_entry
    // lm_entry (64-bit):
    //   mov byte [rip+disp32_to_mailbox_plus_5], 1; hlt; jmp $
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
    // mov ebx, mailbox_off
    emit(out, &mut n, &[0xBB, (mailbox_off as u32 & 0xFF) as u8, ((mailbox_off as u32 >> 8) & 0xFF) as u8, 0x00, 0x00]);
    // mov ax, [bx]; inc ax; mov [bx], ax
    emit(out, &mut n, &[0x66, 0x8B, 0x03, 0x66, 0x40, 0x66, 0x89, 0x03]);
    // mov byte [bx+4], 1
    emit(out, &mut n, &[0x80, 0x47, 0x04, 0x01]);
    // mov eax, [ebx+2]
    emit(out, &mut n, &[0x8B, 0x43, 0x02]);
    // mov cr3, eax
    emit(out, &mut n, &[0x0F, 0x22, 0xD8]);
    // mov eax, cr4
    emit(out, &mut n, &[0x0F, 0x20, 0xE0]);
    // or eax, 0x20 (PAE)
    emit(out, &mut n, &[0x83, 0xC8, 0x20]);
    // mov cr4, eax
    emit(out, &mut n, &[0x0F, 0x22, 0xE0]);
    // mov ecx, 0xC0000080 (EFER)
    emit(out, &mut n, &[0xB9, 0x80, 0x00, 0x00, 0xC0]);
    // rdmsr
    emit(out, &mut n, &[0x0F, 0x32]);
    // or eax, 0x100 (LME)
    emit(out, &mut n, &[0x81, 0xC8, 0x00, 0x01, 0x00, 0x00]);
    // wrmsr
    emit(out, &mut n, &[0x0F, 0x30]);
    // mov eax, cr0
    emit(out, &mut n, &[0x0F, 0x20, 0xC0]);
    // or eax, 0x80000000 (PG)
    emit(out, &mut n, &[0x0D, 0x00, 0x00, 0x00, 0x80]);
    // mov cr0, eax
    emit(out, &mut n, &[0x0F, 0x22, 0xC0]);
    // far jmp 0x18:lm_entry_off (32-bit offset)
    emit(out, &mut n, &[0xEA,
        (lm_entry_off & 0xFF) as u8, (lm_entry_off >> 8) as u8, 0x00, 0x00,
        0x18, 0x00]);
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
    let lm_entry_off: u16 = 0x0060;
    let mut buf = [0u8; 192];
    let code_len = build_pm_trampoline(mailbox_off, gdtr_off, pm_entry_off, lm_entry_off, &mut buf);
    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), page as *mut u8, code_len); }
    // Write GDTR structure (limit + base)
    let gdt_base = (page as u64).wrapping_add(gdt_off as u64);
    let gdtr_ptr = unsafe { page.add(gdtr_off as usize) } as *mut u8;
    unsafe {
        // limit = (4*8 - 1) = 31 (null + 32-bit code + data + 64-bit code)
        core::ptr::write_volatile(gdtr_ptr as *mut u16, 31u16);
        // base 32-bit
        core::ptr::write_volatile(gdtr_ptr.add(2) as *mut u32, gdt_base as u32);
    }
    // Build GDT: null, code, data
    let gdt_ptr = unsafe { page.add(gdt_off as usize) } as *mut u8;
    unsafe {
        // null descriptor (8 bytes of zero)
        core::ptr::write_bytes(gdt_ptr, 0, 8);
        // 32-bit code descriptor @ +8
        let p = gdt_ptr.add(8);
        // limit low
        core::ptr::write_volatile(p as *mut u16, 0xFFFFu16);
        // base low
        core::ptr::write_volatile(p.add(2) as *mut u16, 0u16);
        // base mid
        core::ptr::write_volatile(p.add(4) as *mut u8, 0u8);
        // access = 0x9A (present|exec|readable)
        core::ptr::write_volatile(p.add(5) as *mut u8, 0x9Au8);
        // gran = 0xCF (32-bit code)
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
        // 64-bit code descriptor @ +24 (selector 0x18): access 0x9A, gran L=1 (0x20), D=0
        let p64 = gdt_ptr.add(24);
        core::ptr::write_volatile(p64 as *mut u16, 0x0000u16); // limit low (ignored)
        core::ptr::write_volatile(p64.add(2) as *mut u16, 0u16); // base low
        core::ptr::write_volatile(p64.add(4) as *mut u8, 0u8); // base mid
        core::ptr::write_volatile(p64.add(5) as *mut u8, 0x9Au8); // access
        core::ptr::write_volatile(p64.add(6) as *mut u8, 0x20u8); // gran: L=1
        core::ptr::write_volatile(p64.add(7) as *mut u8, 0u8); // base high
    }
    // Write tiny 64-bit code at lm_entry_off: mov byte [rip+disp32],1 ; hlt ; jmp $
    let lm_ptr = unsafe { page.add(lm_entry_off as usize) } as *mut u8;
    let disp = (mailbox_off as i32 + 5) - (lm_entry_off as i32 + 6);
    unsafe {
        // C6 05 disp32 imm8
        core::ptr::write_volatile(lm_ptr.add(0), 0xC6u8);
        core::ptr::write_volatile(lm_ptr.add(1), 0x05u8);
        core::ptr::write_volatile(lm_ptr.add(2) as *mut i32, disp);
        core::ptr::write_volatile(lm_ptr.add(6), 0x01u8);
        // hlt; jmp $
        core::ptr::write_volatile(lm_ptr.add(7), 0xF4u8);
        core::ptr::write_volatile(lm_ptr.add(8), 0xEBu8);
        core::ptr::write_volatile(lm_ptr.add(9), 0xFEu8);
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

/// Read the protected-mode success flag at mailbox+4.
pub fn read_mailbox_pm_ok(info: TrampolineInfo) -> bool {
    let p = (info.phys_base as usize + info.mailbox_offset as usize + 4) as *const u8;
    unsafe { core::ptr::read_volatile(p) != 0 }
}

/// Read the long-mode entry success flag at mailbox+5.
pub fn read_mailbox_lm_ok(info: TrampolineInfo) -> bool {
    let p = (info.phys_base as usize + info.mailbox_offset as usize + 5) as *const u8;
    unsafe { core::ptr::read_volatile(p) != 0 }
}


