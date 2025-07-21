//! Kubernetes CRI-compatible runtime stub (Task 11.1)
//! Provides minimal gRPC-like handler signatures for containerd / kubelet integration.
//! 本実装は no_std 環境のため実際の gRPC サーバーは別モジュールで bridge 予定。

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::collections::BTreeMap;
use spin::Mutex;

// VmManager is accessed indirectly via the VmOps trait to avoid monomorphisation
// issues when the CRI layer is compiled as a separate crate.

/// Pod UID
pub type PodUid = String;
/// Container name inside Pod
pub type ContainerName = String;

#[derive(Debug, Clone)]
pub struct PodConfig {
    pub uid: PodUid,
    pub name: String,
    pub namespace: String,
    pub annotations: BTreeMap<String, String>,
    pub image: String,
    pub cpu_millis: u32,
    pub mem_bytes: u64,
}

/// Mapping from Pod to Hypervisor VM handle (one-to-one for now)
struct PodVmEntry {
    vm: zerovisor_hal::VmHandle,
    state: VmState,
}

static POD_TABLE: Mutex<BTreeMap<PodUid, PodVmEntry>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Hypervisor interaction layer
// ---------------------------------------------------------------------------

use zerovisor_hal::virtualization::{VmConfig, VmType, SecurityLevel, VirtualizationFeatures, VmHandle};

/// Hypervisor-side VM operations required by the CRI runtime.
pub trait VmOps {
    fn create_vm(&self, cfg: &VmConfig) -> Result<VmHandle, crate::ZerovisorError>;
    fn start_vm(&self, handle: VmHandle) -> Result<(), crate::ZerovisorError>;
    fn stop_vm(&self, handle: VmHandle) -> Result<(), crate::ZerovisorError>;
}

/// Globally registered `VmOps` implementation.
static VM_OPS: Mutex<Option<&'static dyn VmOps>> = Mutex::new(None);

/// Register callbacks – must be called by the hypervisor during startup.
pub fn register_vm_ops(ops: &'static dyn VmOps) {
    *VM_OPS.lock() = Some(ops);
}

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

/// Convert a `PodConfig` (Kubernetes) into a hypervisor `VmConfig`.
fn pod_to_vm_config(p: &PodConfig) -> VmConfig {
    // Simple 32-bit FNV-1a hash to derive a deterministic VM identifier.
    fn fnv1a32(s: &str) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for b in s.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    let mut name_arr = [0u8; 64];
    let name_bytes = p.name.as_bytes();
    let copy_len = core::cmp::min(name_bytes.len(), name_arr.len());
    name_arr[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

    // 1000 mCPU == 1 vCPU baseline; clamp minimum to 1.
    let vcpus = core::cmp::max(1, (p.cpu_millis + 999) / 1000);

    VmConfig {
        id: fnv1a32(&p.uid),
        name: name_arr,
        vcpu_count: vcpus,
        memory_size: p.mem_bytes,
        vm_type: VmType::Container,
        security_level: SecurityLevel::Enhanced,
        real_time_constraints: None,
        features: VirtualizationFeatures::NESTED_PAGING
            | VirtualizationFeatures::HARDWARE_ASSIST
            | VirtualizationFeatures::DEVICE_ASSIGNMENT,
    }
}

/// Initialize CRI runtime subsystem
pub fn init() -> Result<(), crate::ZerovisorError> {
    // Nothing yet — ensure table is instantiated
    let _ = POD_TABLE.lock();
    Ok(())
}

/// Handle RunPodSandbox request (simplified)
pub fn handle_run_pod(cfg: PodConfig) -> Result<(), crate::ZerovisorError> {
    let vm_cfg = pod_to_vm_config(&cfg);

    // Lookup hypervisor callbacks.
    let vm_ops_guard = VM_OPS.lock();
    let ops = vm_ops_guard.as_ref().ok_or(crate::ZerovisorError::InitializationFailed)?;

    // Create & start VM.
    let vm_handle = ops.create_vm(&vm_cfg)?;
    ops.start_vm(vm_handle)?;

    POD_TABLE.lock().insert(cfg.uid.clone(), PodVmEntry { vm: vm_handle, state: VmState::Running });
    Ok(())
}

/// Handle StopPodSandbox request.
pub fn handle_stop_pod(pod_uid: &str) -> Result<(), crate::ZerovisorError> {
    if let Some(entry) = POD_TABLE.lock().get_mut(pod_uid) {
        if let Some(ops) = VM_OPS.lock().as_ref() {
            ops.stop_vm(entry.vm)?;
        }
        entry.state = VmState::Stopped;
    }
    Ok(())
} 