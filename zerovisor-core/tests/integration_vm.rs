//! Integration tests for VM lifecycle (Task 15.1)

extern crate std;
use zerovisor_core as core_crate; // ensure crate compiles
use zerovisor_hal::virtualization::{VmConfig, VmType, SecurityLevel, VirtualizationFeatures};
use zerovisor_core::vm_manager::VmManager;
use crate::common::DummyEngine;

#[test]
fn create_start_destroy_vm() {
    let mut engine = DummyEngine::new();
    let mgr = VmManager::new(engine);
    let cfg = VmConfig {
        id: 1,
        name: *b"test-vm\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        vcpu_count: 1,
        memory_size: 2 * 1024 * 1024,
        vm_type: VmType::MicroVm,
        security_level: SecurityLevel::Basic,
        real_time_constraints: None,
        features: VirtualizationFeatures::empty(),
    };

    let vm = mgr.create_vm(&cfg).expect("vm create");
    assert!(mgr.start_vm(vm).is_ok());
    mgr.stop_vm(vm);
    assert!(mgr.destroy_vm(vm).is_ok());
} 