//! Kubernetes CRI-compatible runtime stub (Task 11.1)
//! Provides minimal gRPC-like handler signatures for containerd / kubelet integration.
//! 本実装は no_std 環境のため実際の gRPC サーバーは別モジュールで bridge 予定。

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::vm_manager::VmState;
use crate::vm_manager::{VmManager};

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

/// Initialize CRI runtime subsystem
pub fn init() -> Result<(), crate::ZerovisorError> {
    // Nothing yet — ensure table is instantiated
    let _ = POD_TABLE.lock();
    Ok(())
}

/// Handle RunPodSandbox request (simplified)
pub fn handle_run_pod(cfg: PodConfig) -> Result<(), crate::ZerovisorError> {
    // TODO: convert PodConfig into VmConfig and start via VmManager
    // Currently, we insert placeholder entry.
    let vm_handle: zerovisor_hal::VmHandle = 0; // dummy
    POD_TABLE.lock().insert(cfg.uid.clone(), PodVmEntry { vm: vm_handle, state: VmState::Running });
    Ok(())
}

/// Handle StopPodSandbox request.
pub fn handle_stop_pod(pod_uid: &str) -> Result<(), crate::ZerovisorError> {
    if let Some(entry) = POD_TABLE.lock().get_mut(pod_uid) {
        entry.state = VmState::Stopped;
    }
    Ok(())
} 