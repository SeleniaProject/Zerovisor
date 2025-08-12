#![allow(dead_code)]

//! Minimal time utilities for UEFI bootstrap.
//!
//! Provides a naive TSC calibration using UEFI Boot Services Stall and a busy
//! wait based on TSC. This is sufficient for coarse timing and spin waits
//! during very early initialization.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

/// Reads the Time Stamp Counter.
#[inline(always)]
fn rdtsc() -> u64 {
    let hi: u32;
    let lo: u32;
    unsafe { core::arch::asm!("rdtsc", out("edx") hi, out("eax") lo, options(nomem, nostack, preserves_flags)) };
    ((hi as u64) << 32) | (lo as u64)
}

/// Calibrate approximate TSC frequency (Hz) using UEFI Stall for reference.
pub fn calibrate_tsc(system_table: &SystemTable<Boot>) -> u64 {
    // Use 50 ms window to balance precision and runtime.
    let window_us: u64 = 50_000;
    let t0 = rdtsc();
    // Stall takes microseconds (u64 fits comfortably)
    let _ = system_table.boot_services().stall(window_us as usize);
    let t1 = rdtsc();
    let delta = t1.wrapping_sub(t0);
    // frequency = cycles / seconds
    (delta.saturating_mul(1_000_000)) / window_us
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


