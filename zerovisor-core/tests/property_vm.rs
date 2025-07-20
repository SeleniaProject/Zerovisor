//! Property-based tests for VM manager (Task 15.1)

extern crate std;
use proptest::prelude::*;
use zerovisor_core::vm_manager::VmManager;
use crate::common::DummyEngine;
use zerovisor_hal::virtualization::{VmConfig, VmHandle, VmType, SecurityLevel, VirtualizationFeatures};

proptest! {
    #[test]
    fn unique_vm_ids(ids in proptest::collection::vec(1u32..1000u32, 1..50)) {
        let mut engine = DummyEngine::new();
        let mgr = VmManager::new(engine);
        for id in &ids {
            let cfg = VmConfig {
                id: *id as VmHandle,
                name: [0u8; 64],
                vcpu_count: 1,
                memory_size: 1024*1024,
                vm_type: VmType::Standard,
                security_level: SecurityLevel::Basic,
                real_time_constraints: None,
                features: VirtualizationFeatures::empty(),
            };
            let vm = mgr.create_vm(&cfg).unwrap();
            prop_assert_eq!(vm, *id as VmHandle);
        }
    }
} 