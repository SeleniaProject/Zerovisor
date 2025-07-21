//! NVMe SR-IOV storage virtualization engine (x86_64)
#![cfg(target_arch = "x86_64")]
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::storage::*;
use crate::memory::PhysicalAddress;
use super::pci;

/// Simple NVMe PF/VF manager.
pub struct NvmeSrioVEngine {
    devices: Vec<StorageDeviceId>,
    next_handle: StorageHandle,
    assignments: Mutex<heapless::FnvIndexMap<StorageHandle, StorageDeviceId, 64>>,
}

impl StorageVirtualization for NvmeSrioVEngine {
    fn init() -> Result<Self, StorageError> where Self: Sized {
        let mut devs = Vec::new();
        // Scan PCI bus for class code 0x01 (Mass Storage) subclass 0x08 (NVMe).
        for bus in 0u8..=31 {
            for dev in 0u8..32 {
                let vendor = unsafe { pci::read_config_dword(bus, dev, 0, 0x00) } & 0xFFFF;
                if vendor == 0xFFFF { continue; }
                let class = unsafe { pci::read_config_dword(bus, dev, 0, 0x08) };
                let class_code = (class >> 24) & 0xFF;
                let subclass = (class >> 16) & 0xFF;
                if class_code == 0x01 && subclass == 0x08 {
                    devs.push(StorageDeviceId { bus, device: dev, function: 0 });
                }
            }
        }
        if devs.is_empty() { return Err(StorageError::NotSupported); }
        Ok(Self { devices: devs, next_handle: 1, assignments: Mutex::new(heapless::FnvIndexMap::new()) })
    }

    fn is_supported() -> bool { true }

    fn list_devices(&self) -> Vec<StorageDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, cfg: &StorageConfig) -> Result<StorageHandle, StorageError> {
        if !self.devices.contains(&cfg.device) { return Err(StorageError::InvalidParameter); }
        let h = self.next_handle;
        self.next_handle += 1;
        self.assignments.lock().insert(h, cfg.device).map_err(|_| StorageError::OutOfResources)?;
        // In real implementation: enable SR-IOV and map BARs.
        Ok(h)
    }

    fn destroy_vf(&mut self, handle: StorageHandle) -> Result<(), StorageError> {
        self.assignments.lock().remove(&handle).ok_or(StorageError::InvalidParameter)?;
        Ok(())
    }

    fn map_guest_memory(&mut self, _handle: StorageHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), StorageError> { Ok(()) }
    fn unmap_guest_memory(&mut self, _handle: StorageHandle, _guest_pa: PhysicalAddress, _size: usize) -> Result<(), StorageError> { Ok(()) }
} 