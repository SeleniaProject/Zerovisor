//! Comprehensive system test suite (Task 15.1)
//! - Unit-level checks already exist in individual modules.
//! - This file focuses on integration/property/stress tests that exercise multiple
//!   subsystems together under the `std` environment.
//!
//! NOTE: These tests run on the host using the *software* HAL mock compiled in
//! `tests/common.rs`. They do *not* require hardware virtualization support.

extern crate std;
use std::time::{Duration, Instant};
use proptest::prelude::*;

use zerovisor_core as corehv; // crate alias when building tests with Cargo workspace

/// Property-based test: monitor’s average exit latency math never panics or
/// produces overflow given arbitrary values.
proptest! {
    #[test]
    fn prop_monitor_avg(latency in 0u64..1_000_000, exits in 1u64..1_000) {
        corehv::monitor::record_vmexit(latency);
        // Collect metrics and ensure avg <= total/1
        let m = corehv::monitor::collect();
        prop_assert!(m.avg_exit_latency_ns <= m.total_exit_time_ns);
    }
}

/// Integration test: create a dummy VM via VM manager and ensure lifecycle.
#[test]
fn integration_vm_lifecycle() {
    use zerovisor_hal::virtualization::{VmConfig, VmType, SecurityLevel, VirtualizationFeatures};
    use zerovisor_core::{vm_manager::VmManager, ZerovisorError};

    // Mock engine implementing VirtualizationEngine is provided by tests/common.rs
    let eng = corehv::tests::common::MockVirtEngine::init().unwrap();
    let mgr = VmManager::new(eng);

    let cfg = VmConfig {
        id: 1,
        name: *b"test-vm\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0" ,
        vcpu_count: 1,
        memory_size: 2 * 1024 * 1024,
        vm_type: VmType::MicroVm,
        security_level: SecurityLevel::Basic,
        real_time_constraints: None,
        features: VirtualizationFeatures::NESTED_PAGING,
    };

    let vm = mgr.create_vm(&cfg).expect("vm create");
    mgr.start_vm(vm).expect("vm start");
    mgr.stop_vm(vm);
    mgr.destroy_vm(vm).expect("vm destroy");
}

/// Stress test: simulate 10k monitor update iterations within 100ms.
#[test]
fn stress_monitor_updates() {
    let start = Instant::now();
    for _ in 0..10_000 {
        corehv::monitor::record_vmexit(1);
    }
    assert!(start.elapsed() < Duration::from_millis(100));
} 