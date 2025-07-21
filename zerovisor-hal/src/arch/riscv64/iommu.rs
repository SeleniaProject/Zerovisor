//! RISC-V IOMMU (Sv48 Stage-2) implementation for Zerovisor
//! Uses per-device second-stage page tables backed by Sv48 to isolate device DMA.
#![cfg(target_arch = "riscv64")]

#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::iommu::{IommuEngine, IommuError, DmaHandle};
use crate::memory::PhysicalAddress;
use super::Stage2Manager;
use super::ept_manager::S2Flags;

struct DeviceMapping {
    next_handle: DmaHandle,
    entries: BTreeMap<DmaHandle, (u64, u64, usize)>,
    stage2: Stage2Manager,
    domain_id: u16,
}

pub struct RiscvIommuEngine {
    devices: Mutex<BTreeMap<u32, DeviceMapping>>, // key = PCI BDF or platform dev id
}

static NEXT_DOMAIN_ID: AtomicU16 = AtomicU16::new(1);

impl RiscvIommuEngine {
    #[inline] fn detect() -> bool { true /* assume supported (IOMMU draft) */ }
    #[inline] fn allocate_domain_id() -> u16 { NEXT_DOMAIN_ID.fetch_add(1, Ordering::SeqCst) }

    #[allow(unused_variables)]
    fn program_iommu(stream_id: u32, stage2_root: PhysicalAddress, domain_id: u16) {
        // Placeholder for writing to IMSIC / IOMMU configuration CSRs or MMIO.
        #[cfg(any(feature = "riscv_iommu_mmio", doc))]
        unsafe {
            const IOMMU_BASE: u64 = 0x2400_0000;
            let guest_pa_reg = (IOMMU_BASE + 0x1000) as *mut u64;
            core::ptr::write_volatile(guest_pa_reg, stage2_root);
        }
    }
}

impl IommuEngine for RiscvIommuEngine {
    fn init() -> Result<Self, IommuError> {
        if !Self::detect() { return Err(IommuError::Unsupported); }
        Ok(Self { devices: Mutex::new(BTreeMap::new()) })
    }

    fn attach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let stream_id = bdf;
        let domain_id = Self::allocate_domain_id();
        let stage2 = Stage2Manager::new().map_err(|_| IommuError::InitFailed)?;
        Self::program_iommu(stream_id, stage2.phys_root(), domain_id);

        let mut map = self.devices.lock();
        map.insert(bdf, DeviceMapping { next_handle: 1, entries: BTreeMap::new(), stage2, domain_id });
        Ok(())
    }

    fn detach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        map.remove(&bdf).ok_or(IommuError::NotAttached)?;
        // TODO: disable stream mapping in hardware
        Ok(())
    }

    fn map(&self, bdf: u32, gpa: PhysicalAddress, hpa: PhysicalAddress, size: usize, writable: bool) -> Result<DmaHandle, IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        let handle = dev.next_handle;
        dev.next_handle += 1;

        let flags = if writable { S2Flags::READ | S2Flags::WRITE } else { S2Flags::READ };
        dev.stage2.map(gpa as u64, hpa as u64, size as u64, flags).map_err(|_| IommuError::MapFailed)?;
        dev.stage2.invalidate_gpa_range(gpa as u64, size as u64);
        dev.entries.insert(handle, (gpa as u64, hpa as u64, size));
        Ok(handle)
    }

    fn unmap(&self, bdf: u32, handle: DmaHandle) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        if let Some((gpa, _hpa, size)) = dev.entries.remove(&handle) {
            dev.stage2.unmap(gpa, size as u64).map_err(|_| IommuError::UnmapFailed)?;
            dev.stage2.invalidate_gpa_range(gpa, size as u64);
            Ok(())
        } else {
            Err(IommuError::UnmapFailed)
        }
    }

    fn flush_tlb(&self, bdf: u32) -> Result<(), IommuError> {
        let map = self.devices.lock();
        let dev = map.get(&bdf).ok_or(IommuError::NotAttached)?;
        dev.stage2.invalidate_entire_tlb();
        Ok(())
    }
}

unsafe impl Send for RiscvIommuEngine {}
unsafe impl Sync for RiscvIommuEngine {} 