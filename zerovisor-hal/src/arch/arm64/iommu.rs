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
use core::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

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

/// Cached capabilities read from the SMMU ID registers.
#[derive(Debug, Copy, Clone)]
struct SmmuCapabilities {
    major: u8,
    minor: u8,
    stage2_levels: u8,   // 2-bit STLEVELS + 1
    oas: u8,             // Output address size bits (32-52)
}

static CAPS_RAW: AtomicU32 = AtomicU32::new(0);

impl SmmuCapabilities {
    /// Parse IDR0/IDR5 raw register values into a compact struct and cache it.
    #[cfg(any(feature = "smmu_mmio", doc))]
    unsafe fn read_from_hardware() -> Self {
        const SMMU_BASE: u64 = 0x2_0000_0000; // Platform-specific base address
        const IDR0_OFFSET: u64 = 0x0;
        const IDR5_OFFSET: u64 = 0x14; // v3.2 spec offset (IDR5)

        let idr0 = core::ptr::read_volatile((SMMU_BASE + IDR0_OFFSET) as *const u32);
        let idr5 = core::ptr::read_volatile((SMMU_BASE + IDR5_OFFSET) as *const u32);

        // Extract fields according to ARM ARM for SMMUv3.x
        let major = (idr0 & 0xF) as u8;
        let minor = ((idr0 >> 4) & 0x3) as u8;
        let stlevels = (((idr0 >> 16) & 0x7) as u8) + 1;
        let oas_enc = ((idr5 >> 16) & 0x7) as u8; // IDR5[18:16] OAS
        let oas_bits = match oas_enc {
            0b000 => 32,
            0b001 => 36,
            0b010 => 40,
            0b011 => 42,
            0b100 => 44,
            0b101 => 48,
            0b110 => 52,
            _ => 48, // Reserved → default safe 48-bit
        };

        Self { major, minor, stage2_levels: stlevels, oas: oas_bits }
    }

    #[cfg(not(any(feature = "smmu_mmio", doc)))]
    fn default_sim() -> Self {
        // Reasonable defaults for emulation / unit tests
        Self { major: 3, minor: 2, stage2_levels: 3, oas: 48 }
    }

    /// Return cached capabilities; initialise on first call.
    fn get() -> Self {
        // CAPS_RAW == 0 means uninitialised
        let raw = CAPS_RAW.load(AtomicOrdering::Acquire);
        if raw != 0 {
            let major = (raw & 0xFF) as u8;
            let minor = ((raw >> 8) & 0xFF) as u8;
            let stlvl = ((raw >> 16) & 0xFF) as u8;
            let oas = ((raw >> 24) & 0xFF) as u8;
            return Self { major, minor, stage2_levels: stlvl, oas };
        }

        // Read hardware or defaults, then cache into a u32
        #[cfg(any(feature = "smmu_mmio", doc))]
        let caps = unsafe { Self::read_from_hardware() };
        #[cfg(not(any(feature = "smmu_mmio", doc)))]
        let caps = Self::default_sim();

        let packed: u32 = (caps.major as u32)
            | ((caps.minor as u32) << 8)
            | ((caps.stage2_levels as u32) << 16)
            | ((caps.oas as u32) << 24);
        CAPS_RAW.store(packed, AtomicOrdering::Release);
        caps
    }
}

impl SmmuEngine {
    #[inline]
    fn detect() -> bool {
        // Probe SMMU IDR0/IDR1 registers to confirm presence and capabilities.
        // The probe is optional in simulation builds where direct MMIO access is
        // unavailable. When the `smmu_mmio` feature (or docs build) is enabled
        // we perform a best-effort read of the ID registers and validate that
        // the architecture version field is non-zero.

        #[cfg(any(feature = "smmu_mmio", doc))]
        unsafe {
            const SMMU_BASE: u64 = 0x2_0000_0000; // Platform-specific base address
            const IDR0_OFFSET: u64 = 0x0;
            const IDR1_OFFSET: u64 = 0x4;

            let idr0 = (SMMU_BASE + IDR0_OFFSET) as *const u32;
            let idr1 = (SMMU_BASE + IDR1_OFFSET) as *const u32;

            let val0 = core::ptr::read_volatile(idr0);
            let _val1 = core::ptr::read_volatile(idr1);

            // IDR0[3:0] encodes the major architecture version (1-4 for SMMUv3.x).
            // A value of zero indicates either an invalid read or a non-existent IP.
            let major = val0 & 0xF;
            major != 0
        }

        #[cfg(not(any(feature = "smmu_mmio", doc)))]
        {
            // Fallback to assume presence when MMIO probing is disabled.
            true
        }
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

            // Dynamic IPS field based on SMMU capabilities (OAS bits)
            let caps = SmmuCapabilities::get();
            let ips_field = match caps.oas {
                32 => 0b000,
                36 => 0b001,
                40 => 0b010,
                42 => 0b011,
                44 => 0b100,
                48 => 0b101,
                52 => 0b110,
                _ => 0b101, // Fallback 48-bit
            } << 16;

            // TG0=4K (0b10) | IPS=variable | SH=Inner Shareable (0b11 << 12) | ORGN/IRGN = Write-Back (0b01,0b01)
            let tcr_val = 0b10 | (0b01 << 8) | (0b01 << 10) | (0b11 << 12) | ips_field;

            write_volatile(cbar, (1 << 0) /* enable */ | ((domain_id as u32) << 8));
            write_volatile(ttbr0, stage2_root);
            write_volatile(tcr, tcr_val);
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