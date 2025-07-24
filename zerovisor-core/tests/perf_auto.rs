#![cfg(test)]
//! Automatic performance regression test – measures VMEXIT latency and TSC freq.

use zerovisor_core::timer;

#[test]
fn tsc_frequency_nonzero() {
    let freq = timer::global_timer().frequency();
    assert!(freq > 100_000_000, "TSC frequency too low: {}", freq);
}

#[test]
fn vmexit_latency_below_threshold() {
    // Placeholder: we expect monitor to record latest latency; use 50 ns budget.
    let latest = zerovisor_core::monitor::latest_vmexit_latency_ns();
    assert!(latest < 50, "VMEXIT latency {} ns exceeds budget", latest);
} 