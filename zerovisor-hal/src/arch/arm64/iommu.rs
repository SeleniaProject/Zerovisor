//! ARM SMMU (Stage-2) IOMMU implementation for Zerovisor
//! Provides device passthrough using per-device Stage-2 page tables so that DMA
//! traffic is safely remapped to guest physical addresses.
//! The design purposefully mirrors the x86_64 VT-d backend to keep higher-level
//! code architecture-agnostic.
#![cfg(target_arch = "aarch64")]

#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicU16, Ordering};

use crate::iommu::{IommuEngine, IommuError, DmaHandle};
use crate::memory::PhysicalAddress;
use super::Stage2Manager; // Re-exported alias to EptHierarchy
use super::ept_manager::S2Flags;

/// Simple per-device DMA translation context.
struct DeviceMapping {
    next_handle: DmaHandle,
    entries: BTreeMap<DmaHandle, (u64, u64, usize)>, // handle → (IPA,HPA,size)
    stage2: Stage2Manager,
    domain_id: u16,
}

/// Global engine managing the ARM System MMU (SMMU).
/// For now we assume a single SMMU unit; multi-SMMU systems would extend the
/// implementation with per-unit data structures.
pub struct SmmuEngine {
    devices: Mutex<BTreeMap<u32, DeviceMapping>>, // key = PCI BDF or platform ID
}

impl SmmuEngine {
    #[inline]
    fn detect() -> bool {
        // TODO: Probe ID registers of SMMU (IDR0/IDR1). For now we assume it is present.
        true
    }

    #[inline]
    fn allocate_domain_id() -> u16 { NEXT_DOMAIN_ID.fetch_add(1, Ordering::SeqCst) }

    /// Program per-stream context bank with Stage-2 root pointer.
    /// This requires writing to SMMU_CB_BASE + CBAR / TCR etc., which is
    /// platform-specific; we abstract it behind this helper.
    #[allow(unused_variables)]
    fn program_context_bank(stream_id: u32, stage2_root: PhysicalAddress, domain_id: u16) {
        // SAFETY: Direct MMIO access to SMMU registers. Placeholder for real HW.
        #[cfg(any(feature = "smmu_mmio", doc))]
        unsafe {
            use core::ptr::write_volatile;
            const SMMU_CB_BASE: u64 = 0x2_0000_0000; // board-specific
            let cbar = (SMMU_CB_BASE + 0x0) as *mut u32;
            let ttbr0 = (SMMU_CB_BASE + 0x20) as *mut u64;
            let tcr = (SMMU_CB_BASE + 0x30) as *mut u32;
            write_volatile(cbar, (1 << 0) /* enable */ | ((domain_id as u32) << 8));
            write_volatile(ttbr0, stage2_root);
            write_volatile(tcr, 0b10 /* TG0=4K */ | (0b0100 << 16) /* IPS=48bit */);
        }
    }
}

/// Global 16-bit domain ID allocator (0 reserved).
static NEXT_DOMAIN_ID: AtomicU16 = AtomicU16::new(1);

impl IommuEngine for SmmuEngine {
    fn init() -> Result<Self, IommuError> {
        if !Self::detect() {
            return Err(IommuError::Unsupported);
        }
        Ok(Self { devices: Mutex::new(BTreeMap::new()) })
    }

    fn attach_device(&self, bdf: u32) -> Result<(), IommuError> {
        // In PCI systems on ARM, the requester ID (stream ID) is derived from BDF.
        let stream_id = bdf;
        let domain_id = Self::allocate_domain_id();

        // Allocate fresh Stage-2 page table hierarchy for the device.
        let mut s2 = Stage2Manager::new().map_err(|_| IommuError::InitFailed)?;

        // Optionally identity-map MSI address range etc.

        // Program SMMU context bank registers.
        Self::program_context_bank(stream_id, s2.phys_root(), domain_id);

        let mut map = self.devices.lock();
        map.insert(bdf, DeviceMapping {
            next_handle: 1,
            entries: BTreeMap::new(),
            stage2: s2,
            domain_id,
        });
        Ok(())
    }

    fn detach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        let dev = map.remove(&bdf).ok_or(IommuError::NotAttached)?;

        // Disable context bank for the stream ID.
        #[allow(unused_variables)]
        {
            let stream_id = bdf;
            #[cfg(any(feature = "smmu_mmio", doc))]
            unsafe {
                const SMMU_CB_BASE: u64 = 0x2_0000_0000;
                let cbar = (SMMU_CB_BASE + 0x0) as *mut u32;
                core::ptr::write_volatile(cbar, 0);
            }
        }

        drop(dev);
        Ok(())
    }

    fn map(&self, bdf: u32, ipa: PhysicalAddress, pa: PhysicalAddress, size: usize, writable: bool) -> Result<DmaHandle, IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        let handle = dev.next_handle;
        dev.next_handle += 1;

        let flags = if writable { S2Flags::READ | S2Flags::WRITE } else { S2Flags::READ };
        dev.stage2.map(ipa as u64, pa as u64, size as u64, flags).map_err(|_| IommuError::MapFailed)?;
        dev.stage2.invalidate_ipa_range(ipa as u64, size as u64);
        dev.entries.insert(handle, (ipa as u64, pa as u64, size));
        Ok(handle)
    }

    fn unmap(&self, bdf: u32, handle: DmaHandle) -> Result<(), IommuError> {
        let mut map = self.devices.lock();
        let dev = map.get_mut(&bdf).ok_or(IommuError::NotAttached)?;
        if let Some((ipa, _pa, size)) = dev.entries.remove(&handle) {
            dev.stage2.unmap(ipa, size as u64).map_err(|_| IommuError::UnmapFailed)?;
            dev.stage2.invalidate_ipa_range(ipa, size as u64);
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

unsafe impl Send for SmmuEngine {}
unsafe impl Sync for SmmuEngine {} 