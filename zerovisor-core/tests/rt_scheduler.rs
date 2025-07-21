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

    // Check interrupt scheduling latency under 1µs
    let latency = scheduler::last_schedule_latency_ns();
    assert!(latency <= 1_000, "scheduler latency {} ns exceeds 1µs", latency);

    // Check WCET proof helper
    assert!(scheduler::wcet_proved(1_000), "WCET proof failed");
} 