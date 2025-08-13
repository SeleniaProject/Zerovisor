#![allow(dead_code)]

//! Minimal 64-bit IDT setup for early exception safety.
//!
//! This module defines a tiny IDT with all vectors pointing to a non-returning
//! halt stub. It avoids full exception decoding to keep the bootstrap small and
//! safe. The intent is to prevent triple faults by providing a valid IDT, and
//! to allow future integration of proper handlers.

use core::mem::size_of;

#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry {
    offset_low: 0,
    selector: 0,
    ist: 0,
    type_attr: 0,
    offset_mid: 0,
    offset_high: 0,
    zero: 0,
}; 256];

/// Naked non-returning stub used for all vectors.
#[naked]
pub unsafe extern "C" fn isr_halt_forever() -> ! {
    core::arch::asm!(
        "cli",       // do not allow re-entry
        "1:",
        "hlt",       // sleep to reduce power
        "jmp 1b",    // loop forever
        options(noreturn)
    );
}

#[inline(always)]
fn split_u64(v: u64) -> (u16, u16, u32) {
    let low = (v & 0xFFFF) as u16;
    let mid = ((v >> 16) & 0xFFFF) as u16;
    let high = (v >> 32) as u32;
    (low, mid, high)
}

fn get_cs_selector() -> u16 {
    let cs_val: u64;
    unsafe { core::arch::asm!("mov {}, cs", out(reg) cs_val); }
    cs_val as u16
}

fn set_gate(idx: usize, handler: u64, selector: u16, ist: u8, type_attr: u8) {
    let (lo, mid, hi) = split_u64(handler);
    unsafe {
        IDT[idx] = IdtEntry {
            offset_low: lo,
            selector,
            ist,
            type_attr,
            offset_mid: mid,
            offset_high: hi,
            zero: 0,
        };
    }
}

/// Initialize IDT with a default non-returning handler for all vectors and load it.
pub fn init() {
    let cs = get_cs_selector();
    let handler = isr_addr();
    // 0x8E = present | DPL=0 | type=0xE (interrupt gate)
    for i in 0..256usize {
        set_gate(i, handler, cs, 0, 0x8E);
    }
    unsafe { load_idt(); }
}

#[inline(always)]
fn isr_addr() -> u64 { unsafe { isr_halt_forever as extern "C" fn() as u64 } }

unsafe fn load_idt() {
    let desc = IdtDescriptor { limit: (size_of::<IdtEntry>() * 256 - 1) as u16, base: (&IDT as *const _) as u64 };
    core::arch::asm!("lidt [{}]", in(reg) &desc, options(readonly, nostack, preserves_flags));
}

/// Enable maskable interrupts (STI). Use only after IDT is valid.
pub fn sti() {
    unsafe { core::arch::asm!("sti", options(nostack, preserves_flags)); }
}

/// Disable maskable interrupts (CLI).
pub fn cli() {
    unsafe { core::arch::asm!("cli", options(nostack, preserves_flags)); }
}


