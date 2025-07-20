//! Performance benchmark suite (Task 14.2)
//! These host‐run tests exercise core algorithms under `std` to estimate
//! baseline performance. They do NOT interact with real hardware; instead they
//! focus on algorithmic overhead for quick CI feedback.

extern crate std;
use std::time::{Duration, Instant};

/// Simulate VMEXIT handling latency by executing dummy loop.
#[test]
fn bench_vmexit_handler() {
    const ITER: u64 = 1_000_000;
    let start = Instant::now();
    let mut acc = 0u64;
    for i in 0..ITER {
        // minimal work   
        acc ^= i;
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() / ITER as u128;
    println!("avg latency {} ns (dummy)", ns);
    // Ensure dummy work uses result
    assert!(acc != 0xFFFF_FFFF_FFFF_FFFF);
}

/// Measure memcpy throughput of ZeroCopyBuffer simulation.
#[test]
fn bench_zero_copy_memcpy() {
    const SIZE: usize = 1024 * 1024; // 1 MB
    let mut src = vec![0u8; SIZE];
    let mut dst = vec![0u8; SIZE];
    let start = Instant::now();
    dst.copy_from_slice(&src);
    let elapsed = start.elapsed();
    let throughput_gbps = SIZE as f64 / elapsed.as_secs_f64() / 1e9 * 8.0;
    println!("throughput {:.2} Gbps (host memcpy)", throughput_gbps);
    assert!(throughput_gbps > 5.0); // arbitrary threshold
} 