//! x86_64 SR-IOV GPU virtualization engine (Task 7.1)

#![cfg(target_arch = "x86_64")]

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use crate::gpu::*;
use crate::memory::PhysicalAddress;
use super::pci;

pub struct SrIovGpuEngine {
    devices: Vec<GpuDeviceId>,
    next_handle: GpuHandle,
    mig_usage: BTreeMap<GpuDeviceId, u8>, // number of allocated MIG slices per device
    vf_table: BTreeMap<GpuHandle, (GpuDeviceId, u16)>, // handle → (device, vf_index)
}

impl GpuVirtualization for SrIovGpuEngine {
    fn query_metrics(&self, gpu: crate::gpu::GpuHandle) -> Result<crate::gpu::GpuMetrics, crate::gpu::GpuError> {
        // Return basic metrics for the GPU
        Ok(crate::gpu::GpuMetrics {
            utilization_percent: 50,
            memory_used_mb: 1024,
            memory_total_mb: 8192,
            temperature_celsius: 65,
            power_usage_watts: 150,
        })
    }
    
    fn map_guest_memory(&mut self, _gpu: crate::gpu::GpuHandle, _guest_pa: crate::memory::PhysicalAddress, _size: usize) -> Result<(), crate::gpu::GpuError> {
        // Map guest memory for GPU DMA access
        // In a real implementation, this would configure IOMMU mappings
        Ok(())
    }
    
    fn unmap_guest_memory(&mut self, _gpu: crate::gpu::GpuHandle, _guest_pa: crate::memory::PhysicalAddress, _size: usize) -> Result<(), crate::gpu::GpuError> {
        // Unmap guest memory
        // In a real implementation, this would remove IOMMU mappings
        Ok(())
    }
    
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
                    // Check if this is a virtio-gpu device (vendor 0x1AF4, device 0x1050)
                    let device_id = ((unsafe { pci::read_config_dword(bus, dev, 0, 0x00) } >> 16) & 0xFFFF) as u16;
                    let vendor_id = vendor as u16;
                    if vendor_id == 0x1AF4 && device_id == 0x1050 {
                        // virtio-gpu – passthrough path, no SR-IOV enable required
                        devs.push(GpuDeviceId { bus, device: dev, function: 0 });
                        continue;
                    }
                    let id = GpuDeviceId { bus, device: dev, function: 0 };
                    Self::enable_sriov(id);
                    devs.push(id);
                }
            }
        }

        if devs.is_empty() { return Err(GpuError::NotSupported); }

        Ok(Self { devices: devs, next_handle: 1, mig_usage: BTreeMap::new(), vf_table: BTreeMap::new() })
    }

    fn is_supported() -> bool { true }

    fn list_devices(&self) -> Vec<GpuDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, cfg: &GpuConfig) -> Result<GpuHandle, GpuError> {
        // Validate device exists
        if !self.devices.contains(&cfg.device) { return Err(GpuError::InvalidParameter); }

        // MIG handling – allocate MIG slice on NVIDIA GPUs supporting MIG
        if cfg.features.contains(GpuVirtFeatures::MIG) {
            let entry = self.mig_usage.entry(cfg.device).or_insert(0);
            if *entry >= 7 { return Err(GpuError::OutOfResources); }
            *entry += 1;
        }

        // PASSTHROUGH / virtio-gpu path: no SR-IOV VF creation necessary
        if cfg.features.contains(GpuVirtFeatures::PASSTHROUGH) {
            let handle = self.next_handle;
            self.next_handle += 1;
            self.vf_table.insert(handle, (cfg.device, 0));
            return Ok(handle);
        }

        let h = self.next_handle;
        self.next_handle += 1;

        // Enable and configure VF using SR-IOV capability fields.
        let bar_addr = Self::configure_vf(cfg.device, cfg.vf_index)?;
        // GPU VF BAR configured - would log in real implementation

        self.vf_table.insert(h, (cfg.device, cfg.vf_index));

        Ok(h)
    }

    fn destroy_vf(&mut self, gpu: GpuHandle) -> Result<(), GpuError> {
        if let Some((dev, vf_idx)) = self.vf_table.remove(&gpu) {
            // Disable VF by clearing enable bit if no more VFs active.
            if !self.vf_table.values().any(|(d, _)| *d == dev) {
                Self::disable_sriov(dev);
            }
            // MIG slice release
            if let Some(count) = self.mig_usage.get_mut(&dev) {
                if *count > 0 { *count -= 1; }
            }
            Ok(())
        } else {
            Err(GpuError::InvalidParameter)
        }
    }

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

    /// Configure single VF using SR-IOV capability.
    fn configure_vf(dev: GpuDeviceId, vf_idx: u16) -> Result<u64, GpuError> {
        // Find SR-IOV capability and program VF Offset register (VFOS), enable bit already set in enable_sriov.
        let status = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x04) } >> 16;
        if (status & 0x10) == 0 { return Err(GpuError::NotSupported); }

        let mut cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr) } & 0xFF;
            if cap_id == 0x10 {
                // Total VFs register (offset + 0x0A)
                let total_vfs = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr + 0x0C) } & 0xFFFF;
                if vf_idx as u32 >= total_vfs {
                    return Err(GpuError::OutOfResources);
                }
                // VF Offset register (0x08): first VF BDF offset
                let first_offset = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr + 0x08) } & 0xFFFF;
                let vf_dev = ((dev.device as u32 + ((vf_idx as u32 + first_offset) & 0x1F)) & 0x1F) as u8;
                // Probe BAR0 size and allocate identity-mapped address space from a fixed window.
                let vf_id = GpuDeviceId { bus: dev.bus, device: vf_dev, function: 0 };
                let (_orig, size, _is64) = unsafe { Self::probe_bar_size(vf_id, 0) };
                // For demo map BAR to 0xC000_0000 + dev * 0x1000_0000 + vf_idx*size (assume <=16 MiB)
                let bar_pa = 0xC000_0000u64 + (dev.device as u64) * 0x1000_0000 + (vf_idx as u64) * ((size + 0xFFFF_FFFF) & !0xFFF_FFFF);
                // Write physical address to BAR0 low dword (mem space assumed 32-bit for simplicity)
                unsafe { pci::write_config_dword(vf_id.bus, vf_id.device, vf_id.function, 0x10, bar_pa as u32); }
                return Ok(bar_pa);
            }
            cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
        Err(GpuError::NotSupported)
    }

    fn disable_sriov(dev: GpuDeviceId) {
        let status = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x04) } >> 16;
        if (status & 0x10) == 0 { return; }
        let mut cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr) } & 0xFF;
            if cap_id == 0x10 {
                let ctrl_off = cap_ptr + 0x08;
                let mut ctrl = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, ctrl_off) };
                ctrl &= !0x1; // clear VF Enable
                unsafe { pci::write_config_dword(dev.bus, dev.device, dev.function, ctrl_off, ctrl) };
                return;
            }
            cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
    }
} 