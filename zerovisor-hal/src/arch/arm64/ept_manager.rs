//! Stage-2 translation table manager for ARMv8-A (AArch64)
//! Provides basic 4-level translation table creation, mapping and permission
//! management for Zerovisor VMs. The design mirrors `EptHierarchy` on x86_64 so
//! higher-level code can remain architecture-independent.
//!
//! Note: This implementation targets a 4 KiB granule with 48-bit IPA and PA.
//! Huge-page mapping (2 MiB / 1 GiB) is supported via block descriptors.
//! All addresses are expected to be page-aligned.

#![cfg(target_arch = "aarch64")]

extern crate alloc;
use alloc::boxed::Box;
use core::ptr::NonNull;
use core::arch::asm;

use crate::memory::PhysicalAddress;

bitflags::bitflags! {
    #[derive(Default, Copy, Clone)]
    pub struct S2Flags: u64 {
        const VALID     = 1 << 0;
        const TABLE     = 1 << 1; // Next-level table pointer when set together with VALID
        const READ      = 1 << 6;
        const WRITE     = 1 << 7;
        const EXECUTE   = 1 << 54; // XN == 0 means executable
        const PXN       = 1 << 53; // Privileged-execute-never
        const UXN       = 1 << 54; // User-execute-never
        const AF        = 1 << 10; // Access flag
        const SH_INNER  = 3 << 8;  // Inner-shareable
        const ATTR_IDX0 = 0;       // MAIR index 0 (WB/RWA)
        const BLOCK_DESC = Self::VALID.bits | Self::AF.bits;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S2Error {
    InvalidAlignment,
    OutOfMemory,
    AlreadyMapped,
    NotMapped,
}

/// A single 512-entry Stage-2 table.
#[repr(C, align(4096))]
pub struct S2Table([u64; 512]);

impl S2Table {
    pub const fn new() -> Self { Self([0; 512]) }

    #[inline]
    pub fn set_entry(&mut self, idx: usize, phys: PhysicalAddress, flags: S2Flags) {
        self.0[idx] = (phys & 0x0000_FFFF_FFFF_F000) | flags.bits();
    }

    #[inline]
    pub fn entry(&self, idx: usize) -> u64 { self.0[idx] }

    #[inline]
    pub fn entry_mut(&mut self, idx: usize) -> &mut u64 { &mut self.0[idx] }

    #[inline]
    pub fn as_phys(&self) -> PhysicalAddress { self as *const _ as PhysicalAddress }
}

/// Owned hierarchy of Stage-2 tables.
pub struct EptHierarchy { // Keep same type name as x86 implementation
    l0: NonNull<S2Table>, // Level-0 (TTBR0) table
}

impl EptHierarchy {
    /// Allocate an empty hierarchy (all zero-filled tables).
    pub fn new() -> Result<Self, S2Error> {
        let boxed: Box<S2Table> = Box::new(S2Table::new());
        Ok(Self { l0: NonNull::new(Box::leak(boxed)).unwrap() })
    }

    /// Physical address of root table (used to write VTTBR_EL2).
    #[inline]
    pub fn phys_root(&self) -> PhysicalAddress { self.l0.as_ptr() as PhysicalAddress }

    /// Simple identity map helper – delegates to internal implementation.
    pub fn map(&mut self, ipa: u64, pa: u64, size: u64, flags: S2Flags) -> Result<(), S2Error> {
        self.map_internal(ipa, pa, size, flags)?;
        self.invalidate_ipa_range(ipa, size);
        Ok(())
    }

    fn map_internal(&mut self, ipa: u64, pa: u64, size: u64, flags: S2Flags) -> Result<(), S2Error> {
        if size == 0 { return Ok(()); }
        if ipa % 0x1000 != 0 || pa % 0x1000 != 0 || size % 0x1000 != 0 {
            return Err(S2Error::InvalidAlignment);
        }

        const SZ_4K: u64 = 0x1000;
        const SZ_2M: u64 = 2 * 1024 * 1024;
        const SZ_1G: u64 = 1024 * 1024 * 1024;

        let mut remaining = size;
        let mut cur_ipa = ipa;
        let mut cur_pa = pa;

        while remaining > 0 {
            if remaining >= SZ_1G && cur_ipa % SZ_1G == 0 && cur_pa % SZ_1G == 0 {
                self.map_block(cur_ipa, cur_pa, 0, flags)?;
                cur_ipa += SZ_1G; cur_pa += SZ_1G; remaining -= SZ_1G;
            } else if remaining >= SZ_2M && cur_ipa % SZ_2M == 0 && cur_pa % SZ_2M == 0 {
                self.map_block(cur_ipa, cur_pa, 1, flags)?;
                cur_ipa += SZ_2M; cur_pa += SZ_2M; remaining -= SZ_2M;
            } else {
                self.map_page(cur_ipa, cur_pa, flags)?;
                cur_ipa += SZ_4K; cur_pa += SZ_4K; remaining -= SZ_4K;
            }
        }
        Ok(())
    }

    /// Map a single 4 KiB page.
    fn map_page(&mut self, ipa: u64, pa: u64, flags: S2Flags) -> Result<(), S2Error> {
        self.map_desc(ipa, pa, 2, flags)
    }

    /// Map a block at specified level (0=1 GiB,1=2 MiB).
    fn map_block(&mut self, ipa: u64, pa: u64, level: usize, flags: S2Flags) -> Result<(), S2Error> {
        self.map_desc(ipa, pa, level, flags)
    }

    fn map_desc(&mut self, ipa: u64, pa: u64, target_level: usize, flags: S2Flags) -> Result<(), S2Error> {
        // Calculate indices
        let l0_idx = ((ipa >> 39) & 0x1FF) as usize;
        let l1_idx = ((ipa >> 30) & 0x1FF) as usize;
        let l2_idx = ((ipa >> 21) & 0x1FF) as usize;
        let l3_idx = ((ipa >> 12) & 0x1FF) as usize;

        unsafe {
            let l0 = &mut *self.l0.as_ptr();
            let mut l1_phys = l0.entry(l0_idx) & 0x0000_FFFF_FFFF_F000;
            if l1_phys == 0 {
                l1_phys = Self::alloc_table()?.as_phys();
                l0.set_entry(l0_idx, l1_phys, S2Flags::VALID | S2Flags::TABLE);
            }

            if target_level == 0 { // 1 GiB block maps at L1
                let l1 = &mut *(l1_phys as *mut S2Table);
                if l1.entry(l1_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
                l1.set_entry(l1_idx, pa, flags | S2Flags::BLOCK_DESC);
                return Ok(());
            }

            let l1 = &mut *(l1_phys as *mut S2Table);
            let mut l2_phys = l1.entry(l1_idx) & 0x0000_FFFF_FFFF_F000;
            if l2_phys == 0 {
                l2_phys = Self::alloc_table()?.as_phys();
                l1.set_entry(l1_idx, l2_phys, S2Flags::VALID | S2Flags::TABLE);
            }

            if target_level == 1 { // 2 MiB block maps at L2
                let l2 = &mut *(l2_phys as *mut S2Table);
                if l2.entry(l2_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
                l2.set_entry(l2_idx, pa, flags | S2Flags::BLOCK_DESC);
                return Ok(());
            }

            let l2 = &mut *(l2_phys as *mut S2Table);
            let mut l3_phys = l2.entry(l2_idx) & 0x0000_FFFF_FFFF_F000;
            if l3_phys == 0 {
                l3_phys = Self::alloc_table()?.as_phys();
                l2.set_entry(l2_idx, l3_phys, S2Flags::VALID | S2Flags::TABLE);
            }

            let l3 = &mut *(l3_phys as *mut S2Table);
            if l3.entry(l3_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
            l3.set_entry(l3_idx, pa, flags | S2Flags::VALID | S2Flags::AF);
        }
        Ok(())
    }

    fn alloc_table() -> Result<&'static mut S2Table, S2Error> {
        let boxed: Box<S2Table> = Box::new(S2Table::new());
        Ok(Box::leak(boxed))
    }

    /// Simple permission-update helper (RWX bits only).
    pub fn set_permissions(&mut self, ipa: u64, size: u64, flags: S2Flags) -> Result<(), S2Error> {
        if size % 0x1000 != 0 { return Err(S2Error::InvalidAlignment); }
        let pages = size / 0x1000;
        for i in 0..pages {
            self.update_perm_single(ipa + i*0x1000, flags)?;
        }
        self.invalidate_ipa_range(ipa, size);
        Ok(())
    }

    fn update_perm_single(&mut self, ipa: u64, flags: S2Flags) -> Result<(), S2Error> {
        let l0_idx = ((ipa >> 39) & 0x1FF) as usize;
        let l1_idx = ((ipa >> 30) & 0x1FF) as usize;
        let l2_idx = ((ipa >> 21) & 0x1FF) as usize;
        let l3_idx = ((ipa >> 12) & 0x1FF) as usize;

        unsafe {
            let l0 = &mut *self.l0.as_ptr();
            let l1_phys = l0.entry(l0_idx) & 0x0000_FFFF_FFFF_F000;
            if l1_phys == 0 { return Err(S2Error::NotMapped); }
            let l1 = &mut *(l1_phys as *mut S2Table);

            let l2_phys = l1.entry(l1_idx) & 0x0000_FFFF_FFFF_F000;
            if l2_phys == 0 { return Err(S2Error::NotMapped); }
            let l2 = &mut *(l2_phys as *mut S2Table);

            let l3_phys = l2.entry(l2_idx) & 0x0000_FFFF_FFFF_F000;
            if l3_phys == 0 { return Err(S2Error::NotMapped); }
            let l3 = &mut *(l3_phys as *mut S2Table);

            let entry = l3.entry_mut(l3_idx);
            if *entry & 1 == 0 { return Err(S2Error::NotMapped); }

            let phys_part = *entry & 0xFFFF_FFFF_FFFF_F000u64;
            let misc_bits = *entry & !(0xFFF); // preserve upper attribute bits
            *entry = phys_part | misc_bits | flags.bits();
        }
        Ok(())
    }

    /// Unmap helper (4 KiB granularity)
    pub fn unmap(&mut self, ipa: u64, size: u64) -> Result<(), S2Error> {
        if size % 0x1000 != 0 { return Err(S2Error::InvalidAlignment); }
        let pages = size / 0x1000;
        for i in 0..pages {
            self.unmap_internal(ipa + i*0x1000)?;
        }
        self.invalidate_ipa_range(ipa, size);
        Ok(())
    }

    fn unmap_internal(&mut self, ipa: u64) -> Result<(), S2Error> {
        let l0_idx = ((ipa >> 39) & 0x1FF) as usize;
        let l1_idx = ((ipa >> 30) & 0x1FF) as usize;
        let l2_idx = ((ipa >> 21) & 0x1FF) as usize;
        let l3_idx = ((ipa >> 12) & 0x1FF) as usize;

        unsafe {
            let l0 = &mut *self.l0.as_ptr();
            let l1_phys = l0.entry(l0_idx) & 0x0000_FFFF_FFFF_F000;
            if l1_phys == 0 { return Err(S2Error::NotMapped); }
            let l1 = &mut *(l1_phys as *mut S2Table);

            let l2_phys = l1.entry(l1_idx) & 0x0000_FFFF_FFFF_F000;
            if l2_phys == 0 { return Err(S2Error::NotMapped); }
            let l2 = &mut *(l2_phys as *mut S2Table);

            let l3_phys = l2.entry(l2_idx) & 0x0000_FFFF_FFFF_F000;
            if l3_phys == 0 { return Err(S2Error::NotMapped); }
            let l3 = &mut *(l3_phys as *mut S2Table);

            if l3.entry(l3_idx) & 1 == 0 { return Err(S2Error::NotMapped); }
            *l3.entry_mut(l3_idx) = 0;
        }
        Ok(())
    }

    /// Flush complete Stage-2 TLB for all VCPUs (VMALLS12E1IS).
    #[inline]
    pub fn invalidate_entire_tlb(&self) {
        unsafe {
            asm!("dsb ishst; tlbi vmalls12e1is; dsb ish; isb", options(nostack, preserves_flags));
        }
    }

    /// Flush only the specified IPA range using VMALLE1IS where available.
    #[inline]
    pub fn invalidate_ipa_range(&self, ipa: u64, size: u64) {
        let mut addr = ipa & !0xFFFu64;
        let end = ipa + size;
        unsafe {
            while addr < end {
                asm!("tlbi ipas2e1is, {addr}", addr = in(reg) addr >> 12, options(nostack, preserves_flags));
                addr += 0x1000;
            }
            asm!("dsb ish; isb", options(nostack, preserves_flags));
        }
    }
}

unsafe impl Send for EptHierarchy {}
unsafe impl Sync for EptHierarchy {} 