//! Virtio-GPU passthrough engine
//! Enumerates virtio-gpu devices (PCI vendor 0x1AF4, device 0x1050) and exposes
//! them directly to guest VMs without SR-IOV. This module is architecture
//! independent and relies on the common PCI enumeration helper.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::gpu::*;
use crate::pci;
use crate::memory::PhysicalAddress;

pub struct VirtioGpuPassthrough {
    devices: Vec<GpuDeviceId>,
    next_handle: AtomicU32,
}

impl VirtioGpuPassthrough {
    const VENDOR_ID: u16 = 0x1AF4;
    const DEVICE_ID: u16 = 0x1050; // virtio-gpu

    fn virtio_devices() -> Vec<GpuDeviceId> {
        let mut list = Vec::new();
        for dev in pci::enumerate_all() {
            if dev.vendor_id == Self::VENDOR_ID && dev.device_id == Self::DEVICE_ID {
                list.push(dev.bdf);
            }
        }
        list
    }
}

impl GpuVirtualization for VirtioGpuPassthrough {
    fn init() -> Result<Self, GpuError> where Self: Sized {
        let devs = Self::virtio_devices();
        if devs.is_empty() { return Err(GpuError::NotSupported); }
        Ok(Self { devices: devs, next_handle: AtomicU32::new(1) })
    }

    fn is_supported() -> bool { !Self::virtio_devices().is_empty() }

    fn list_devices(&self) -> Vec<GpuDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, cfg: &GpuConfig) -> Result<GpuHandle, GpuError> {
        // Only passthrough supported; ignore vf_index/dedicated_memory.
        if !self.devices.contains(&cfg.device) { return Err(GpuError::InvalidParameter); }
        if !cfg.features.contains(GpuVirtFeatures::PASSTHROUGH) { return Err(GpuError::InvalidParameter); }
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        Ok(handle)
    }

    fn destroy_vf(&mut self, _gpu: GpuHandle) -> Result<(), GpuError> { Ok(()) }

    fn map_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }

    fn unmap_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }
} 