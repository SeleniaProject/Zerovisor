//! Micro-VM utilities (Task 6.2)
//! Provides one-shot helper to create and start an ultra-light VM targeting
//! <50 ms boot and ≤1 ms cold-start for serverless workloads.

// Internal module runs in `no_std` context via crate attribute in lib.rs.

extern crate alloc;

use zerovisor_hal::virtualization::{VmType, VmConfig, VirtualizationFeatures, VcpuConfig, VirtualizationEngine, SecurityLevel, VmHandle};
use zerovisor_hal::virtualization::arch::vmx::VmxEngine;
use zerovisor_hal::cpu::CpuFeatures;
use spin::Mutex;

use crate::scheduler::{register_vcpu, quantum_expired, SchedEntity};
use crate::monitor;
use crate::ZerovisorError;

/// Default Micro-VM memory size (64 MiB)
const MICROVM_DEFAULT_MEM: u64 = 64 * 1024 * 1024;

/// Global VMX engine instance protected by a spin lock.
static ENGINE: Mutex<Option<VmxEngine>> = Mutex::new(None);

fn get_engine<'a>() -> Result<spin::MutexGuard<'a, Option<VmxEngine>>, ZerovisorError> {
    let mut guard = ENGINE.lock();
    if guard.is_none() {
        let eng = VmxEngine::init().map_err(|_| ZerovisorError::InitializationFailed)?;
        *guard = Some(eng);
    }
    Ok(guard)
}

/// Create and start a Micro-VM, returning its `VmHandle`.
pub fn create_and_start_micro_vm() -> Result<VmHandle, ZerovisorError> {
    let mut engine_guard = get_engine()?;
    let engine = engine_guard.as_mut().unwrap();

    // Build 64-byte null-terminated name
    let mut name = [0u8; 64];
    name[..7].copy_from_slice(b"microvm");

    let vm_cfg = VmConfig {
        id: 0,
        name,
        vcpu_count: 1,
        memory_size: MICROVM_DEFAULT_MEM,
        vm_type: VmType::MicroVm,
        security_level: SecurityLevel::Basic,
        real_time_constraints: None,
        features: VirtualizationFeatures::empty(),
    };

    let vm = engine.create_vm(&vm_cfg).map_err(|_| ZerovisorError::InitializationFailed)?;
    engine.setup_nested_paging(vm).map_err(|_| ZerovisorError::InitializationFailed)?;

    // Create single VCPU
    let vcpu_cfg = VcpuConfig {
        id: 0,
        initial_state: engine.get_vcpu_state(0).unwrap_or_default(),
        exposed_features: CpuFeatures::empty(),
        real_time_priority: None,
    };
    let vcpu = engine.create_vcpu(vm, &vcpu_cfg).map_err(|_| ZerovisorError::InitializationFailed)?;

    // Register to scheduler with high priority for fast boot
    register_vcpu(vm, vcpu, 200, None);
    monitor::vm_started();

    // Run the VCPU until first VMEXIT (HLT expected for dummy payload)
    if let Ok(reason) = engine.run_vcpu(vcpu) {
        let _ = engine.handle_vm_exit(vcpu, reason);
    }
    quantum_expired(SchedEntity { vm, vcpu, priority: 200, deadline_ns: None });
    Ok(vm)
} 