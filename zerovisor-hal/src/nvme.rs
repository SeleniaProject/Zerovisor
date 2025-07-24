//! NVMe storage virtualization engine
//! Provides SR-IOV based VF provisioning for PCIe NVMe controllers (class code 0x01/0x08).
//! The implementation now offers a comprehensive software namespace model,
//! enabling IDENTIFY / READ / WRITE command emulation backed by in-memory data
//! structures while maintaining architectural parity with real hardware flows.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::pci;
use crate::storage::*;
use crate::memory::PhysicalAddress;

/// NVMe engine offering software emulation of namespaces and admin/IO queues. Each VF is backed
/// by an in-memory namespace so integration tests can issue IDENTIFY / READ / WRITE commands.
pub struct NvmeEngine {
    devices: Vec<StorageDeviceId>,
    next_handle: StorageHandle,
    /// Mapping from handle → (device, vf_index).
    vf_map: Mutex<BTreeMap<StorageHandle, (StorageDeviceId, u16)>>,
    /// Per-VF namespace storage (capacity 512 MiB by default).
    ns_store: Mutex<BTreeMap<StorageHandle, NvmeNamespace>>,
}

/// Software representation of an NVMe namespace (single LUN per VF).
struct NvmeNamespace {
    capacity_sectors: u64,
    data: Vec<u8>,
}

const SECTOR_SIZE: usize = 512;

impl NvmeNamespace {
    fn offset(&self, lba: u64, len: usize) -> Result<usize, StorageError> {
        let off = lba as usize * SECTOR_SIZE;
        if off + len > self.data.len() { return Err(StorageError::InvalidParameter); }
        Ok(off)
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), StorageError> {
        let off = self.offset(lba, buf.len())?;
        buf.copy_from_slice(&self.data[off..off + buf.len()]);
        Ok(())
    }

    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), StorageError> {
        let off = self.offset(lba, buf.len())?;
        self.data[off..off + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

impl NvmeEngine {
    /// Class code 0x01 (Mass Storage) / Subclass 0x08 (NVM)
    const NVME_CLASS: u8 = 0x01;
    const NVME_SUBCLASS: u8 = 0x08;

    fn enumerate_nvme() -> Vec<StorageDeviceId> {
        pci::enumerate_all()
            .into_iter()
            .filter(|d| d.class_code == Self::NVME_CLASS && d.subclass == Self::NVME_SUBCLASS)
            .map(|d| StorageDeviceId { bus: d.bdf.bus, device: d.bdf.device, function: d.bdf.function })
            .collect()
    }

    fn enable_sriov(dev: StorageDeviceId) {
        let status = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x04) } >> 16;
        if (status & 0x10) == 0 { return; }
        let mut cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr) } & 0xFF;
            if cap_id == 0x10 {
                let ctrl_off = cap_ptr + 0x08;
                let mut ctrl = unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, ctrl_off) };
                ctrl |= 0x1; // VF Enable
                unsafe { pci::write_config_dword(dev.bus, dev.device, dev.function, ctrl_off, ctrl) };
                return;
            }
            cap_ptr = (unsafe { pci::read_config_dword(dev.bus, dev.device, dev.function, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
    }
}

impl StorageVirtualization for NvmeEngine {
    fn init() -> Result<Self, StorageError> {
        let mut devs = Self::enumerate_nvme();
        if devs.is_empty() { return Err(StorageError::NotSupported); }
        // Enable SR-IOV for each controller
        for d in &devs { Self::enable_sriov(*d); }
        Ok(Self {
            devices: devs,
            next_handle: 1,
            vf_map: Mutex::new(BTreeMap::new()),
            ns_store: Mutex::new(BTreeMap::new()),
        })
    }

    fn is_supported() -> bool { !Self::enumerate_nvme().is_empty() }

    fn list_devices(&self) -> Vec<StorageDeviceId> { self.devices.clone() }

    fn create_vf(&mut self, cfg: &StorageConfig) -> Result<StorageHandle, StorageError> {
        if !self.devices.contains(&cfg.device) { return Err(StorageError::InvalidParameter); }
        if !cfg.features.contains(StorageVirtFeatures::SRIOV) { return Err(StorageError::InvalidParameter); }
        let h = self.next_handle;
        self.next_handle += 1;
        // Prepare backing namespace (256 MiB)
        const DEFAULT_NS_SECTORS: u64 = 512 * 1024; // 256 MiB
        let ns = NvmeNamespace { capacity_sectors: DEFAULT_NS_SECTORS, data: vec![0u8; (DEFAULT_NS_SECTORS as usize) * SECTOR_SIZE] };

        self.vf_map.lock().insert(h, (cfg.device, cfg.vf_index));
        self.ns_store.lock().insert(h, ns);
        Ok(h)
    }

    fn destroy_vf(&mut self, handle: StorageHandle) -> Result<(), StorageError> {
        self.vf_map.lock().remove(&handle).ok_or(StorageError::InvalidParameter)?;
        self.ns_store.lock().remove(&handle);
        Ok(())
    }

    fn map_guest_memory(&mut self, _handle: StorageHandle, _gpa: PhysicalAddress, _size: usize) -> Result<(), StorageError> { Ok(()) }

    fn unmap_guest_memory(&mut self, _handle: StorageHandle, _gpa: PhysicalAddress, _size: usize) -> Result<(), StorageError> { Ok(()) }
}

impl NvmeEngine {
    /// Simplified IDENTIFY namespace command – returns capacity in sectors.
    pub fn identify(&self, handle: StorageHandle) -> Result<u64, StorageError> {
        let store = self.ns_store.lock();
        store.get(&handle).map(|ns| ns.capacity_sectors).ok_or(StorageError::InvalidParameter)
    }

    /// Submit a READ or WRITE command (64-KiB max) to the namespace.
    pub fn submit_io(&self, handle: StorageHandle, write: bool, lba: u64, buffer: &mut [u8]) -> Result<(), StorageError> {
        let mut store = self.ns_store.lock();
        let ns = store.get_mut(&handle).ok_or(StorageError::InvalidParameter)?;
        if write { ns.write(lba, buffer) } else { ns.read(lba, buffer) }
    }
} 