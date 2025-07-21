//! Intel VT-d IOMMU implementation
#![cfg(target_arch = "x86_64")]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::iommu::{IommuEngine, IommuError, DmaHandle};
use crate::memory::PhysicalAddress;

/// Simple VT-d remapping structure per device (domain==device model)
struct DeviceMapping {
    next_handle: DmaHandle,
    entries: BTreeMap<DmaHandle, (u64, u64, usize)>, // handle→(gpa,hpa,size)
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
        map.insert(bdf, DeviceMapping { next_handle: 1, entries: BTreeMap::new() });
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
        dev.entries.insert(handle, (gpa as u64, hpa as u64, size));
        // TODO: populate VT-d page tables & invalidate IOTLB
        Ok(handle)
    }

    fn unmap(&self, bdf: u32, handle: DmaHandle) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        dev.entries.remove(&handle).ok_or(IommuError::UnmapFailed)?;
        // TODO: update page tables and invalidate IOTLB
        Ok(())
    }

    fn flush_tlb(&self, _bdf: u32) -> Result<(), IommuError> { Ok(()) }
} 