//! Intel VT-d IOMMU implementation
#![cfg(target_arch = "x86_64")]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use spin::Mutex;

use core::sync::atomic::{AtomicU16, Ordering};

/// Intel VT-d Root-Entry definition (64-bit) – only low 64 bits used in 48-bit address mode.
#[repr(C, align(4096))]
#[derive(Copy, Clone)]
struct RootEntry(u64);

/// VT-d Context-Entry (128-bit)
#[repr(C, align(4096))]
#[derive(Copy, Clone)]
struct ContextEntry {
    low: u64,
    high: u64,
}

impl ContextEntry {
    #[inline]
    fn new(ept_root: PhysicalAddress, domain_id: u16, aw_bit: u8) -> Self {
        // low 64 bits: present(0) | fn_type(2:0) | lower 52 bits of PML4 addr (12..63) | AW (3 bits)
        // We choose translation-type 0 = EPT-like paging (PT = 0). AW encodes addr width – we use 3 (48-bit).
        let addr_bits = (ept_root & 0x000F_FFFF_FFFF_F000) as u64;
        let low = 1               // present
                | (addr_bits)    // root pointer
                | ((aw_bit as u64) << 3); // AW bits at 3..5 for 48-bit

        // high 64 bits: Domain-ID (15:0) | reserved | Translation Type Specific
        let high = domain_id as u64;
        Self { low, high }
    }
}

/// Per-bus context table (256 entries). We allocate lazily when the first device on the bus is attached.
#[repr(C, align(4096))]
struct ContextTable([ContextEntry; 256]);

impl ContextTable {
    const fn new_empty() -> Self { Self([ContextEntry { low: 0, high: 0 }; 256]) }
}

/// Global VT-d root table (256 root entries – one per PCI bus).
#[repr(C, align(4096))]
struct RootTable([RootEntry; 256]);

impl RootTable {
    const fn new_empty() -> Self { Self([RootEntry(0); 256]) }
}

// Allocate single static instance of the root table. It will live for the life of the hypervisor.
static mut ROOT_TABLE: Option<&'static mut RootTable> = None;

/// Atomic Domain-ID allocator (16-bit space, 0 is reserved)
static NEXT_DOMAIN_ID: AtomicU16 = AtomicU16::new(1);

use crate::iommu::{IommuEngine, IommuError, DmaHandle};
use crate::memory::PhysicalAddress;
use crate::arch::x86_64::ept_manager::EptHierarchy;
use crate::arch::x86_64::ept::EptFlags;
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

    /// Initialise global root table if not done yet and program VT-d hardware registers.
    fn ensure_root_table() -> Result<(), IommuError> {
        // Safety: only called under global lock or single-threaded early boot.
        unsafe {
            if ROOT_TABLE.is_none() {
                let boxed: Box<RootTable> = Box::new(RootTable::new_empty());
                ROOT_TABLE = Some(Box::leak(boxed));

                // Program VT-d Root-Table Address Register (RTADDR). We assume single DRHD unit.
                // NOTE: This is hardware specific MMIO – replace 0xFED90000 offset with DMAR base.
                const DMAR_BASE: u64 = 0xFED9_0000;
                const RTADDR_OFFSET: u64 = 0x20; // per Intel VT-d spec
                const GCMD_OFFSET: u64 = 0x18;
                const GSTS_OFFSET: u64 = 0x1C;

                let rtaddr_reg = (DMAR_BASE + RTADDR_OFFSET) as *mut u64;
                rtaddr_reg.write_volatile(ROOT_TABLE.as_ref().unwrap() as *const _ as u64);

                // Enable Translation (TE) bit in GCMD and wait until GSTS.TES = 1
                let gcmd = (DMAR_BASE + GCMD_OFFSET) as *mut u32;
                let gsts = (DMAR_BASE + GSTS_OFFSET) as *const u32;
                // Set TE (bit 31)
                unsafe { gcmd.write_volatile(gcmd.read_volatile() | (1 << 31)); }
                // Busy-wait until hardware reports enabled
                while unsafe { gsts.read_volatile() & (1 << 31) } == 0 {}
            }
        }
        Ok(())
    }

    /// Allocate a new domain identifier (1-65535).
    fn allocate_domain_id() -> u16 { NEXT_DOMAIN_ID.fetch_add(1, Ordering::SeqCst) }
}

impl IommuEngine for VtdEngine {
    fn init() -> Result<Self, IommuError> {
        if !Self::detect() { return Err(IommuError::Unsupported); }
        Ok(Self { devices: Mutex::new(BTreeMap::new()) })
    }

    fn attach_device(&self, bdf: u32) -> Result<(), IommuError> {
        Self::ensure_root_table()?;

        let bus = ((bdf >> 8) & 0xFF) as usize;
        let devfn = (bdf & 0xFF) as usize; // device (5 bits) + function (3 bits)

        // Allocate new EPT hierarchy which will become the device DMA translation table.
        let ept = EptHierarchy::new().map_err(|_| IommuError::InitFailed)?;

        // Allocate a domain id.
        let domain_id = Self::allocate_domain_id();

        // Lazily allocate context table for the bus if not existing.
        unsafe {
            if let Some(root) = ROOT_TABLE.as_mut() {
                let root_entry = &mut root.0[bus];
                let ctx_table: &mut ContextTable = if root_entry.0 & 1 == 0 {
                    // allocate
                    let boxed: Box<ContextTable> = Box::new(ContextTable::new_empty());
                    let phys = boxed.as_ref() as *const _ as u64;
                    root_entry.0 = phys | 1; // present bit
                    Box::leak(boxed)
                } else {
                    &mut *((root_entry.0 & !0xFFF) as *mut ContextTable)
                };

                // Program context entry for device/function.
                let entry = ContextEntry::new(ept.phys_root(), domain_id, 3); // 48-bit addr width
                ctx_table.0[devfn] = entry;

                // Flush context cache for this device – write in CCMD register.
                const DMAR_BASE: u64 = 0xFED9_0000;
                const CCMD_OFFSET: u64 = 0x28;
                let ccmd = (DMAR_BASE + CCMD_OFFSET) as *mut u64;
                // Set Device-invalidate (bit 0) and specify device function (bus/dev/fn) and domain
                let ccmd_val = (1u64) /* ICC */
                                | ((bus as u64) << 16)
                                | ((devfn as u64) << 8)
                                | ((domain_id as u64) << 32);
                unsafe {
                    ccmd.write_volatile(ccmd_val);
                    // Wait for completion bit (bit 0 cleared by HW)
                    while ccmd.read_volatile() & 1 != 0 {}
                }
            }
        }

        // Book-keeping for software structures.
        let mut map = self.devices.lock();
        map.insert(bdf, DeviceMapping { next_handle: 1, entries: BTreeMap::new(), ept });
        Ok(())
    }

    fn detach_device(&self, bdf: u32) -> Result<(), IommuError> {
        let bus = ((bdf >> 8) & 0xFF) as usize;
        let devfn = (bdf & 0xFF) as usize;

        let mut map = self.devices.lock();
        let dev_map = map.remove(&bdf).ok_or(IommuError::NotAttached)?;

        // Remove context entry from hardware table.
        unsafe {
            if let Some(root) = ROOT_TABLE.as_mut() {
                let root_entry = &mut root.0[bus];
                if root_entry.0 & 1 != 0 {
                    let ctx_table = &mut *((root_entry.0 & !0xFFF) as *mut ContextTable);
                    ctx_table.0[devfn] = ContextEntry { low: 0, high: 0 };
                }
            }

            // Invalidate context cache for device again.
            const DMAR_BASE: u64 = 0xFED9_0000;
            const CCMD_OFFSET: u64 = 0x28;
            let ccmd = (DMAR_BASE + CCMD_OFFSET) as *mut u64;
            let ccmd_val = (1u64) | ((bus as u64) << 16) | ((devfn as u64) << 8);
            ccmd.write_volatile(ccmd_val);
            while ccmd.read_volatile() & 1 != 0 {}
        }

        drop(dev_map);
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