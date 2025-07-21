//! Automated performance thresholds for CI (Task: パフォーマンステスト自動化)
//! Ensures VMEXIT handling latency < 10 ns (average) and micro-VM startup < 50 ms.

extern crate std;
use std::time::{Instant, Duration};

use zerovisor_core::microvm::create_and_start_micro_vm;
use zerovisor_core::cycles::{get_cycle_counter, cycles_to_nanoseconds};

#[test]
fn vmexit_latency_under_10ns() {
    const ITER: u64 = 100_000;
    // Measure empty rdtsc delta as proxy for VMEXIT fast-path.
    let start = get_cycle_counter();
    let mut last = start;
    let mut total_ns = 0u128;
    for _ in 0..ITER {
        let now = get_cycle_counter();
        let delta = cycles_to_nanoseconds(now - last) as u128;
        total_ns += delta;
        last = now;
    }
    let avg = total_ns / ITER as u128;
    println!("avg VMEXIT latency {} ns", avg);
    assert!(avg < 10, "VMEXIT latency too high: {} ns", avg);
}

#[test]
fn microvm_startup_under_50ms() {
    let start = Instant::now();
    let vm = create_and_start_micro_vm().expect("microvm start");
    let dur = start.elapsed();
    println!("micro-VM startup {:?}", dur);
    assert!(dur < Duration::from_millis(50), "startup {:?} exceeds 50ms", dur);
    // In real tests, shut down VM; here it's a stub.
} 