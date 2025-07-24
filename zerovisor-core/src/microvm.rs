//! Micro-VM utilities (Task 6.2)
//! Provides one-shot helper to create and start an ultra-light VM targeting
//! <50 ms boot and ≤1 ms cold-start for serverless workloads.

// Internal module runs in `no_std` context via crate attribute in lib.rs.

extern crate alloc;

use zerovisor_hal::virtualization::{VmType, VmConfig, VirtualizationFeatures, VcpuConfig, VirtualizationEngine, SecurityLevel, VmHandle};
use zerovisor_hal::virtualization::arch::vmx::VmxEngine;
use zerovisor_hal::cpu::CpuFeatures;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::scheduler::{register_vcpu, quantum_expired, SchedEntity};
use crate::monitor;
use crate::ZerovisorError;

/// Default Micro-VM memory size (64 MiB)
const MICROVM_DEFAULT_MEM: u64 = 64 * 1024 * 1024;

/// Global atomic counter for unique VM identifiers
static NEXT_VM_ID: AtomicU32 = AtomicU32::new(1);

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
        id: NEXT_VM_ID.fetch_add(1, Ordering::SeqCst),
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

/// Fast path that skips device enumeration and boots VM in <50 ms.
/// It reuses an engine instance and caches VMConfig to minimise allocations.
pub fn create_fast_micro_vm() -> Result<VmHandle, ZerovisorError> {
    use core::time::Instant;
    let start = Instant::now();

    // Reuse global engine (already initialised).
    let mut engine_guard = get_engine()?;
    let engine = engine_guard.as_mut().unwrap();

    // Build cached VMConfig once.
    static mut CACHED_CFG: Option<VmConfig> = None;
    let cfg = unsafe {
        CACHED_CFG.get_or_insert_with(|| {
            let mut name = [0u8; 64];
            name[..6].copy_from_slice(b"fastvm");
            VmConfig {
                id: NEXT_VM_ID.fetch_add(1, Ordering::SeqCst),
                name,
                vcpu_count: 1,
                memory_size: MICROVM_DEFAULT_MEM,
                vm_type: VmType::MicroVm,
                security_level: SecurityLevel::Basic,
                real_time_constraints: None,
                features: VirtualizationFeatures::empty(),
            }
        })
    };

    let vm = engine.create_vm(cfg).map_err(|_| ZerovisorError::InitializationFailed)?;
    engine.setup_nested_paging(vm).map_err(|_| ZerovisorError::InitializationFailed)?;

    let vcpu_cfg = VcpuConfig {
        id: 0,
        initial_state: engine.get_vcpu_state(0).unwrap_or_default(),
        exposed_features: CpuFeatures::empty(),
        real_time_priority: None,
    };
    let vcpu = engine.create_vcpu(vm, &vcpu_cfg).map_err(|_| ZerovisorError::InitializationFailed)?;

    register_vcpu(vm, vcpu, 200, None);
    monitor::vm_started();

    // No initial run; caller responsible for scheduling.

    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 50, "fast microVM startup exceeded 50 ms");

    Ok(vm)
}

/// Gracefully shut down a running Micro-VM.
///
/// The shutdown sequence performs the following steps:
/// 1. Remove all pending scheduling entities so the VM will no longer be
///    selected by the quantum scheduler.
/// 2. Destroy the VM via the underlying virtualization engine, which
///    tears down VMCS/EPT and frees host resources.
/// 3. Update monitoring counters so external observers learn that the VM has
///    stopped.
pub fn shutdown_micro_vm(vm: VmHandle) -> Result<(), ZerovisorError> {
    // 1. Remove from scheduler queues.
    crate::scheduler::remove_vm(vm);

    // 2. Destroy via virtualization engine.
    let mut eng_guard = get_engine()?;
    if let Some(engine) = eng_guard.as_mut() {
        engine.destroy_vm(vm).map_err(|_| ZerovisorError::InitializationFailed)?;
    } else {
        return Err(ZerovisorError::InvalidConfiguration);
    }

    // 3. Update monitoring.
    monitor::vm_stopped();

    Ok(())
} 