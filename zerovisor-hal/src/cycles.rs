//! Cycle counter utilities for performance measurement (x86_64)
#![cfg(target_arch = "x86_64")]

/// Read Time Stamp Counter (TSC)
#[inline]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Convert cycles to nanoseconds using given TSC frequency (Hz)
#[inline]
pub fn cycles_to_ns(cycles: u64, tsc_hz: u64) -> u64 {
    (cycles * 1_000_000_000) / tsc_hz
} 