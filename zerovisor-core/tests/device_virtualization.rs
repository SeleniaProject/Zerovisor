//! Basic device virtualization smoke tests

extern crate std;
use zerovisor_hal::gpu::{GpuVirtualization, GpuConfig, GpuVirtFeatures};
use zerovisor_hal::storage::{StorageVirtualization, StorageConfig, StorageVirtFeatures};

#[test]
fn gpu_device_enumeration() {
    if let Ok(engine) = zerovisor_hal::arch::x86_64::gpu::SrIovGpuEngine::init() {
        let list = engine.list_devices();
        assert!(!list.is_empty(), "GPU devices should be detected");
    }
}

#[test]
fn storage_device_enumeration() {
    if zerovisor_hal::arch::x86_64::storage::NvmeSrioVEngine::is_supported() {
        let eng = zerovisor_hal::arch::x86_64::storage::NvmeSrioVEngine::init().expect("init storage");
        assert!(!eng.list_devices().is_empty(), "NVMe devices should be present (stub)");
    }
} 