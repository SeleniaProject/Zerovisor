//! Tests for real-time scheduler latency and WCET tracking

extern crate std;
use zerovisor_core::scheduler;
use zerovisor_hal::virtualization::{VmHandle, VcpuHandle};

#[test]
fn wcet_within_limit() {
    // Register a few dummy entities
    for i in 0..10u32 {
        scheduler::register_vcpu(i as VmHandle, 0 as VcpuHandle, 200, Some(1_000_000));
    }

    // Simulate execution cycles and recording
    for _ in 0..100 {
        if let Some(ent) = scheduler::pick_next() {
            // Fake execution 500ns
            scheduler::record_exec_time(ent, 500);
            scheduler::quantum_expired(ent);
        }
    }

    // Check there are no WCET violations at 1µs threshold
    let violations = scheduler::wcet_violations(1_000);
    assert!(violations.is_empty(), "unexpected WCET violations: {:?}", violations);
} 