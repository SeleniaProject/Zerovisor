//! x86_64 SR-IOV GPU virtualization engine (Task 7.1)

#![cfg(target_arch = "x86_64")]

extern crate alloc;
use alloc::vec::Vec;
use alloc::vec;

use crate::memory::PhysicalAddress;
use crate::gpu::*;

pub struct SrIovGpuEngine {
    devices: Vec<GpuDeviceId>,
    next_handle: GpuHandle,
}

impl GpuVirtualization for SrIovGpuEngine {
    fn init() -> Result<Self, GpuError> where Self: Sized {
        // For demo, fabricate one GPU at bus 1:0.0
        Ok(Self { devices: vec![GpuDeviceId { bus: 1, device: 0, function: 0 }], next_handle: 1 })
    }

    fn is_supported() -> bool { true }

    fn list_devices(&self) -> Vec<GpuDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, _cfg: &GpuConfig) -> Result<GpuHandle, GpuError> {
        let h = self.next_handle;
        self.next_handle += 1;
        Ok(h)
    }

    fn destroy_vf(&mut self, _gpu: GpuHandle) -> Result<(), GpuError> { Ok(()) }

    fn map_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }

    fn unmap_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }
} 