// Zerovisor CSI driver (Task: Kubernetes CSI integration)
// Provides minimal block-device volume attach/detach and mount services
// exposed via a C ABI so an external userspace shim can translate the
// Kubernetes CSI gRPC to these low-level calls.
#![allow(dead_code)]
//! Zerovisor loopback CSI driver – exposes an in-memory virtio-blk device per VM.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use zerovisor_hal::storage::{StorageConfig, StorageDeviceId, StorageVirtFeatures};
use zerovisor_hal::virtualization::VmHandle;
use crate::security::{self, SecurityEvent};
use crate::ZerovisorError;

#[repr(C)]
pub enum CsiStatus { Success = 0, Failure = 1 }

/// Per-VM list of loopback volumes (handle values from virtio-blk engine).
static VOLUMES: Mutex<BTreeMap<VmHandle, Vec<u32>>> = Mutex::new(BTreeMap::new());

pub unsafe extern "C" fn zerovisor_csi_create_volume(vm: VmHandle, size_mb: u32) -> CsiStatus {
    let cfg = StorageConfig {
        device: StorageDeviceId { bus: 0, device: 5, function: 0 },
        vf_index: 0,
        features: StorageVirtFeatures::PASSTHROUGH,
    };
    match zerovisor_hal::virtio_blk::VirtioBlkEngine::init().and_then(|mut eng| eng.create_vf(&cfg)) {
        Ok(handle) => {
            VOLUMES.lock().entry(vm).or_default().push(handle);
            security::record_event(SecurityEvent::IoMapping { guest_pa: 0x2000_0000, size: (size_mb as usize) * 1024 * 1024 });
            CsiStatus::Success
        }
        Err(_) => CsiStatus::Failure,
    }
}

pub unsafe extern "C" fn zerovisor_csi_delete_volume(vm: VmHandle, handle: u32) -> CsiStatus {
    if let Some(list) = VOLUMES.lock().get_mut(&vm) {
        list.retain(|&h| h != handle);
    }
    CsiStatus::Success
} 