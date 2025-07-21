//! Intel VT-d IOMMU implementation
#![cfg(target_arch = "x86_64")]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::iommu::{IommuEngine, IommuError, DmaHandle};
use crate::memory::PhysicalAddress;
use crate::arch::x86_64::ept_manager::{EptHierarchy, EptFlags};
use crate::arch::x86_64::ept_manager::EptError;

/// Simple VT-d remapping structure per device (domain==device model)
struct DeviceMapping {
    next_handle: DmaHandle,
    entries: BTreeMap<DmaHandle, (u64, u64, usize)>, // handle→(gpa,hpa,size)
    ept: EptHierarchy,
}

pub struct VtdEngine {
    devices: Mutex<BTreeMap<u32, DeviceMapping>>, // key = BDF
}

impl VtdEngine {
    /// Read VT-d capability registers to determine number of DRHD units etc.
    fn detect() -> bool { true /* assume supported for demo */ }
}

impl IommuEngine for VtdEngine {
    fn init() -> Result<Self, IommuError> {
        if !Self::detect() { return Err(IommuError::Unsupported); }
        Ok(Self { devices: Mutex::new(BTreeMap::new()) })
    }

    fn attach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        if map.contains_key(&bdf) { return Err(IommuError::AlreadyAttached); }
        let ept = EptHierarchy::new().map_err(|_| IommuError::InitFailed)?;
        map.insert(bdf, DeviceMapping { next_handle: 1, entries: BTreeMap::new(), ept });
        // TODO: program VT-d context entry for device to point to per-device page table
        Ok(())
    }

    fn detach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        map.remove(&bdf).ok_or(IommuError::NotAttached)?;
        // TODO: invalidate context cache
        Ok(())
    }

    fn map(&self, bdf: u32, gpa: PhysicalAddress, hpa: PhysicalAddress, size: usize, writable: bool) -> Result<DmaHandle, IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        let handle = dev.next_handle;
        dev.next_handle += 1;
        // Map into per-device DMA page tables.
        let flags = if writable { EptFlags::READ | EptFlags::WRITE } else { EptFlags::READ };
        dev.ept
            .map(gpa as u64, hpa as u64, size as u64, flags)
            .map_err(|e| match e {
                EptError::InvalidAlignment => IommuError::MapFailed,
                EptError::OutOfMemory => IommuError::MapFailed,
                EptError::AlreadyMapped => IommuError::MapFailed,
                EptError::NotMapped => IommuError::MapFailed,
            })?;

        dev.ept.invalidate_gpa_range(gpa as u64, size as u64);

        dev.entries.insert(handle, (gpa as u64, hpa as u64, size));
        Ok(handle)
    }

    fn unmap(&self, bdf: u32, handle: DmaHandle) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        if let Some((gpa, _hpa, size)) = dev.entries.remove(&handle) {
            dev.ept
                .unmap(gpa, size as u64)
                .map_err(|_| IommuError::UnmapFailed)?;
            dev.ept.invalidate_gpa_range(gpa, size as u64);
            Ok(())
        } else {
            Err(IommuError::UnmapFailed)
        }
    }

    fn flush_tlb(&self, bdf: u32) -> Result<(), IommuError> {
        let map = self.devices.lock();
        let dev = map.get(&bdf).ok_or(IommuError::NotAttached)?;
        dev.ept.invalidate_entire_tlb();
        Ok(())
    }
} 