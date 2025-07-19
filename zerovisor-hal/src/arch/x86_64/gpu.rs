//! x86_64 SR-IOV GPU virtualization engine (Task 7.1)

#![cfg(target_arch = "x86_64")]

extern crate alloc;
use alloc::vec::Vec;

use crate::memory::PhysicalAddress;
use crate::gpu::*;
use super::pci;

pub struct SrIovGpuEngine {
    devices: Vec<GpuDeviceId>,
    next_handle: GpuHandle,
}

impl GpuVirtualization for SrIovGpuEngine {
    fn init() -> Result<Self, GpuError> where Self: Sized {
        // Scan buses 0..=31 for devices with class code 0x03 (Display Controller)
        let mut devs = Vec::new();

        for bus in 0u8..=31 {
            for dev in 0u8..32 {
                // function 0 only (no multi-func check for brevity)
                let vendor = unsafe { pci::read_config_dword(bus, dev, 0, 0x00) } & 0xFFFF;
                if vendor == 0xFFFF { continue; }

                let class_code = unsafe { pci::read_config_dword(bus, dev, 0, 0x08) } >> 24;
                if class_code == 0x03 {
                    let id = GpuDeviceId { bus, device: dev, function: 0 };
                    Self::enable_sriov(id);
                    devs.push(id);
                }
            }
        }

        if devs.is_empty() { return Err(GpuError::NotSupported); }

        Ok(Self { devices: devs, next_handle: 1 })
    }

    fn is_supported() -> bool { true }

    fn list_devices(&self) -> Vec<GpuDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, cfg: &GpuConfig) -> Result<GpuHandle, GpuError> {
        let h = self.next_handle;
        self.next_handle += 1;

        // Placeholder: assume BAR0 size 16 MiB starting at 0xC0000000 + vf_index*0x1000000.
        let _bar_pa = 0xC000_0000 + (cfg.vf_index as u64) * 0x0100_0000;
        // Identity map into guest via EPT (requires EptHierarchy access; omitted for now).
        // This will be wired once the VM handle is passed in.

        Ok(h)
    }

    fn destroy_vf(&mut self, _gpu: GpuHandle) -> Result<(), GpuError> { Ok(()) }

    fn map_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }

    fn unmap_guest_memory(&mut self, _gpu: GpuHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), GpuError> { Ok(()) }
}

impl SrIovGpuEngine {
    /// Probe BAR size by writing all 1s and reading back mask.
    /// Returns (original_value, size_bytes, is_64bit).
    unsafe fn probe_bar_size(id: GpuDeviceId, bar_idx: u8) -> (u64, u64, bool) {
        let offset = 0x10 + (bar_idx as u8 * 4);
        // All PCI configuration space accesses are unsafe operations and must be
        // wrapped in an explicit `unsafe` block to comply with the
        // `unsafe_op_in_unsafe_fn` lint enforced at crate level.
        let orig_low = unsafe { pci::read_config_dword(id.bus, id.device, id.function, offset) };
        unsafe { pci::write_config_dword(id.bus, id.device, id.function, offset, 0xFFFF_FFFF) };
        let mask_low = unsafe { pci::read_config_dword(id.bus, id.device, id.function, offset) };
        unsafe { pci::write_config_dword(id.bus, id.device, id.function, offset, orig_low) };

        let is_mem_bar = (orig_low & 1) == 0;
        if !is_mem_bar { return (orig_low as u64, 0, false); }

        let bar_type = (orig_low >> 1) & 0x3;
        if bar_type == 0x2 { // 64-bit
            let orig_high = unsafe { pci::read_config_dword(id.bus, id.device, id.function, offset + 4) };
            unsafe { pci::write_config_dword(id.bus, id.device, id.function, offset + 4, 0xFFFF_FFFF) };
            let mask_high = unsafe { pci::read_config_dword(id.bus, id.device, id.function, offset + 4) };
            unsafe { pci::write_config_dword(id.bus, id.device, id.function, offset + 4, orig_high) };

            let mask = ((mask_high as u64) << 32) | (mask_low as u64);
            let size = (!mask & 0xFFFF_FFFF_FFFF_FFF0u64) + 1;
            let orig = ((orig_high as u64) << 32) | (orig_low as u64);
            (orig, size, true)
        } else {
            let size = (!(mask_low as u64) & 0xFFFF_FFF0u64) + 1;
            (orig_low as u64, size, false)
        }
    }

    /// Enable SR-IOV capability if present (writes to PF capability register)
    fn enable_sriov(id: GpuDeviceId) {
        // Capability list traverse
        let status = unsafe { pci::read_config_dword(id.bus, id.device, id.function, 0x04) } >> 16;
        if (status & 0x10) == 0 { return; } // No capabilities

        let mut cap_ptr = (unsafe { pci::read_config_dword(id.bus, id.device, id.function, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(id.bus, id.device, id.function, cap_ptr) } & 0xFF;
            if cap_id == 0x10 { // SR-IOV capability ID
                // Enable SR-IOV: write to control register (offset + 0x08)
                let ctrl_off = cap_ptr + 0x08;
                let mut ctrl = unsafe { pci::read_config_dword(id.bus, id.device, id.function, ctrl_off) };
                ctrl |= 0x1; // set VF Enable
                unsafe { pci::write_config_dword(id.bus, id.device, id.function, ctrl_off, ctrl) };
                return;
            }
            cap_ptr = (unsafe { pci::read_config_dword(id.bus, id.device, id.function, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
    }
} 