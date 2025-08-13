#![allow(dead_code)]

//! Minimal time utilities for UEFI bootstrap.
//!
//! Provides TSC calibration using UEFI Boot Services Stall as a fallback and
//! an optional HPET-based calibration when available, along with a TSC-based
//! busy wait.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

pub mod hpet;

/// Reads the Time Stamp Counter.
#[inline(always)]
pub(crate) fn rdtsc() -> u64 {
    let hi: u32;
    let lo: u32;
    unsafe { core::arch::asm!("rdtsc", out("edx") hi, out("eax") lo, options(nomem, nostack, preserves_flags)) };
    ((hi as u64) << 32) | (lo as u64)
}

/// Calibrate approximate TSC frequency (Hz) using UEFI Stall for reference.
pub fn calibrate_tsc_stall(system_table: &SystemTable<Boot>) -> u64 {
    // Use 50 ms window to balance precision and runtime.
    let window_us: u64 = 50_000;
    let t0 = rdtsc();
    let _ = system_table.boot_services().stall(window_us as usize);
    let t1 = rdtsc();
    let delta = t1.wrapping_sub(t0);
    (delta.saturating_mul(1_000_000)) / window_us
}

/// Calibrate TSC using HPET if present, otherwise fall back to Stall.
pub fn calibrate_tsc(system_table: &SystemTable<Boot>) -> u64 {
    if let Some(hz) = hpet::calibrate_tsc_via_hpet(system_table, 10_000) { // about ~100us on 100MHz HPET
        return hz;
    }
    calibrate_tsc_stall(system_table)
}

/// Global TSC frequency cache once calibrated.
static mut TSC_HZ: u64 = 0;

/// Initialize and cache the TSC frequency, prefer invariant TSC path.
pub fn init_time(system_table: &SystemTable<Boot>) -> u64 {
    // If already set, return cached
    unsafe { if TSC_HZ != 0 { return TSC_HZ; } }
    // Prefer HPET calibration when available; otherwise Stall.
    let hz = calibrate_tsc(system_table);
    unsafe { TSC_HZ = hz; }
    hz
}

/// Read cached TSC frequency (0 if not initialized).
#[inline(always)]
pub fn tsc_hz() -> u64 {
    unsafe { TSC_HZ }
}

/// Busy-wait for approximately the specified microseconds using TSC.
pub fn busy_wait_tsc(system_table: &SystemTable<Boot>, usec: u64, tsc_hz: u64) {
    if tsc_hz == 0 { let _ = system_table.boot_services().stall(usec as usize); return; }
    let start = rdtsc();
    let target = start.wrapping_add(usec.saturating_mul(tsc_hz) / 1_000_000);
    while rdtsc().wrapping_sub(start) < target.wrapping_sub(start) {
        core::hint::spin_loop();
    }
}


