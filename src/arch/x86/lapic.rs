#![allow(dead_code)]

//! Local APIC (xAPIC) minimal MMIO access and IPI helpers.
//!
//! This module implements basic xAPIC MMIO read/write and INIT/SIPI sequence
//! for AP bring-up. For simplicity we use xAPIC mode via the physical MMIO base
//! reported by ACPI MADT; x2APIC can be added later via MSRs if required.

use core::ptr::{read_volatile, write_volatile};

/// Selected Local APIC registers (offsets in bytes)
const LAPIC_ID: usize = 0x020;         // Local APIC ID (bits 24..31)
const LAPIC_EOI: usize = 0x0B0;        // End Of Interrupt
const LAPIC_SVR: usize = 0x0F0;        // Spurious Interrupt Vector Register
const LAPIC_ICR_LOW: usize = 0x300;    // Interrupt Command Register low
const LAPIC_ICR_HIGH: usize = 0x310;   // Interrupt Command Register high

/// ICR delivery modes
const ICR_DM_INIT: u32 = 0x5 << 8;
const ICR_DM_STARTUP: u32 = 0x6 << 8;

/// Destination shorthand values
const ICR_DEST_NO_SHORTHAND: u32 = 0 << 18;

#[inline(always)]
unsafe fn mmio_read32(base: usize, off: usize) -> u32 {
    let p = (base + off) as *const u32;
    read_volatile(p)
}

#[inline(always)]
unsafe fn mmio_write32(base: usize, off: usize, val: u32) {
    let p = (base + off) as *mut u32;
    write_volatile(p, val);
}

/// Extract LAPIC ID (bits 24..31) from the Local APIC ID register.
pub fn read_lapic_id(lapic_base: usize) -> u32 {
    unsafe { (mmio_read32(lapic_base, LAPIC_ID) >> 24) & 0xFF }
}

/// Write End-of-Interrupt to the LAPIC.
pub fn eoi(lapic_base: usize) {
    unsafe { mmio_write32(lapic_base, LAPIC_EOI, 0); }
}

/// Program SVR with an enable bit and a spurious vector value, returning previous.
pub fn enable_svr(lapic_base: usize, vector: u8) -> u32 {
    let prev = unsafe { mmio_read32(lapic_base, LAPIC_SVR) };
    let newv = (prev | 0x100) & 0xFFFFFF00 | (vector as u32);
    unsafe { mmio_write32(lapic_base, LAPIC_SVR, newv); }
    prev
}

/// Restore SVR to a previous value.
pub fn restore_svr(lapic_base: usize, val: u32) {
    unsafe { mmio_write32(lapic_base, LAPIC_SVR, val); }
}

/// Send a targeted IPI by writing ICR high/low.
fn send_ipi(lapic_base: usize, apic_id: u32, icr_low: u32) {
    unsafe {
        mmio_write32(lapic_base, LAPIC_ICR_HIGH, apic_id << 24);
        mmio_write32(lapic_base, LAPIC_ICR_LOW, icr_low | ICR_DEST_NO_SHORTHAND);
        // Delivery status bit (12) clears when sent; we do a small delay at caller.
    }
}

/// Send INIT IPI to a target APIC ID.
pub fn send_init(lapic_base: usize, apic_id: u32) {
    // INIT IPI with level-assert (bit 14) and edge trigger (bit 15 = 0)
    let icr = ICR_DM_INIT | (1 << 14);
    send_ipi(lapic_base, apic_id, icr);
}

/// Send Startup IPI (SIPI) to a target APIC ID with a given startup vector (20-bit page >> 12).
pub fn send_sipi(lapic_base: usize, apic_id: u32, vector: u8) {
    let icr = ICR_DM_STARTUP | (vector as u32);
    send_ipi(lapic_base, apic_id, icr);
}

/// Wait until ICR delivery status bit clears.
pub fn wait_icr_delivery(lapic_base: usize) {
    // Bit 12 of ICR low is Delivery Status (1=Send Pending)
    loop {
        let v = unsafe { mmio_read32(lapic_base, LAPIC_ICR_LOW) };
        if (v & (1 << 12)) == 0 { break; }
        core::hint::spin_loop();
    }
}

/// Read xAPIC base from IA32_APIC_BASE MSR when available.
pub fn apic_base_via_msr() -> Option<usize> {
    // IA32_APIC_BASE MSR index is 0x1B; bits 12..35 are base for xAPIC
    let v = unsafe { crate::arch::x86::msr::rdmsr(0x1B) };
    // Check APIC global enable bit (bit 11)
    if (v & (1 << 11)) == 0 { return None; }
    let base = (v & 0xFFFFF000) as usize;
    if base == 0 { None } else { Some(base) }
}

/// Enable x2APIC mode if supported: set IA32_APIC_BASE[10]=1 (x2APIC enable).
pub fn try_enable_x2apic() -> bool {
    if !crate::arch::x86::cpuid::has_x2apic() { return false; }
    // Read-modify-write IA32_APIC_BASE (0x1B): set bit 10 (x2APIC enable) and bit 11 (APIC global enable)
    let mut v = unsafe { crate::arch::x86::msr::rdmsr(0x1B) };
    v |= (1 << 11) | (1 << 10);
    unsafe { crate::arch::x86::msr::wrmsr(0x1B, v); }
    true
}

/// Returns true if IA32_APIC_BASE indicates x2APIC enabled.
pub fn is_x2apic_enabled() -> bool {
    let v = unsafe { crate::arch::x86::msr::rdmsr(0x1B) };
    (v & (1 << 10)) != 0
}

/// Send INIT via x2APIC MSR ICR (0x830) to target APIC ID.
fn send_init_x2apic(apic_id: u32) {
    // x2APIC ICR is 64-bit: [63:32] dest, [31:0] low with delivery mode/shorthand
    let low = (ICR_DM_INIT | (1 << 14)) as u64; // assert INIT
    let icr = ((apic_id as u64) << 32) | low;
    unsafe { crate::arch::x86::msr::wrmsr(0x830, icr); }
}

/// Send SIPI via x2APIC MSR ICR to target APIC ID with vector.
fn send_sipi_x2apic(apic_id: u32, vector: u8) {
    let low = (ICR_DM_STARTUP | (vector as u32)) as u64;
    let icr = ((apic_id as u64) << 32) | low;
    unsafe { crate::arch::x86::msr::wrmsr(0x830, icr); }
}

/// Auto path: send INIT using x2APIC if enabled, else xAPIC MMIO.
pub fn send_init_auto(lapic_base: usize, apic_id: u32) {
    if is_x2apic_enabled() { send_init_x2apic(apic_id); }
    else { send_init(lapic_base, apic_id); }
}

/// Auto path: send SIPI using x2APIC if enabled, else xAPIC MMIO.
pub fn send_sipi_auto(lapic_base: usize, apic_id: u32, vector: u8) {
    if is_x2apic_enabled() { send_sipi_x2apic(apic_id, vector); }
    else { send_sipi(lapic_base, apic_id, vector); }
}


