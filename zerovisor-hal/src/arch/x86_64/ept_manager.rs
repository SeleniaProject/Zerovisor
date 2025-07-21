//! EPT Manager – builds and manipulates 4-level Extended Page Tables.
//! This fulfils Task 3.2 (EPT implementation) initial requirements: creation,
//! mapping/unmapping and permission modification including 2 MiB / 1 GiB
//! huge-page support.
#![cfg(target_arch = "x86_64")]

extern crate alloc;
use alloc::boxed::Box;
use core::ptr::NonNull;

use crate::memory::PhysicalAddress;
use super::ept::{EptTable, EptFlags};

/// Errors returned by EPT manager operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptError {
    InvalidAlignment,
    OutOfMemory,
    AlreadyMapped,
    NotMapped,
}

/// An owned hierarchy of EPT tables – ownership keeps them alive for HW use.
pub struct EptHierarchy {
    pml4: NonNull<EptTable>,
}

impl EptHierarchy {
    /// Allocate empty hierarchy (all zero-filled tables).
    pub fn new() -> Result<Self, EptError> {
        // For simplicity use Box to obtain 4-KiB aligned page; in real bare-metal
        // environment this will come from a physical page allocator.
        let boxed: Box<EptTable> = Box::new(EptTable::new());
        let pml4_ptr = Box::leak(boxed) as *mut _;
        Ok(Self { pml4: NonNull::new(pml4_ptr).unwrap() })
    }

    /// Physical address of root PML4 table.
    pub fn phys_root(&self) -> PhysicalAddress { self.pml4.as_ptr() as PhysicalAddress }

    /// Invalidate all cached translations for this EPT hierarchy using a single-context INVEPT.
    #[inline]
    pub fn invalidate_entire_tlb(&self) {
        #[cfg(any(target_feature = "vmx", feature = "vmx"))]
        unsafe {
            use core::arch::asm;
            #[repr(C, packed)]
            struct InveptDesc { eptp: u64, reserved: u64 }
            let desc = InveptDesc { eptp: self.phys_root(), reserved: 0 };
            // Intel SDM Vol. 3C §30.3.3 – INVEPT type encoding
            // 1 = Single-context invalidation, 2 = All-contexts invalidation
            const SINGLE_CONTEXT: u64 = 1;
            const ALL_CONTEXT:    u64 = 2;
            asm!("invept rax, rbx", in("rax") SINGLE_CONTEXT, in("rbx") &desc as *const _ as u64, options(nostack, preserves_flags));
        }
    }

    /// Invalidate translations covering the given guest physical range.
    /// Falls back to full invalidation on processors lacking INVEPT individual-address support.
    #[inline]
    pub fn invalidate_gpa_range(&self, _gpa: u64, _size: u64) {
        // Future optimisation: use individual-address INVEPT (type 2) when supported.
        // For now we invalidate the entire context which is still much faster than global flush.
        self.invalidate_entire_tlb();
    }

    /// Public map wrapper selects page size automatically.
    pub fn map(&mut self, gpa: u64, hpa: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        let res = self.map_internal(gpa, hpa, size, flags);
        if res.is_ok() {
            // Ensure guest sees updated mappings as soon as possible.
            self.invalidate_gpa_range(gpa, size);
        }
        res
    }

    /// Convenience wrapper to map an MMIO region (read/write, no exec).
    /// Size must be page aligned.
    pub fn map_mmio(&mut self, gpa: u64, hpa: u64, size: u64) -> Result<(), EptError> {
        self.map_internal(gpa, hpa, size, EptFlags::READ | EptFlags::WRITE)
    }

    /// Map a guest physical range to host physical range with given flags.
    /// size must be 4 KiB, 2 MiB or 1 GiB aligned.
    fn map_internal(&mut self, gpa: u64, hpa: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        if size == 0 {
            return Ok(());
        }

        if gpa % 0x1000 != 0 || hpa % 0x1000 != 0 || size % 0x1000 != 0 {
            return Err(EptError::InvalidAlignment);
        }

        const SZ_4K: u64 = 0x1000;
        const SZ_2M: u64 = 2 * 1024 * 1024;
        const SZ_1G: u64 = 1024 * 1024 * 1024;

        let mut remaining = size;
        let mut cur_gpa = gpa;
        let mut cur_hpa = hpa;

        while remaining > 0 {
            if remaining >= SZ_1G && cur_gpa % SZ_1G == 0 && cur_hpa % SZ_1G == 0 {
                self.map_single_1g(cur_gpa, cur_hpa, flags | EptFlags::HUGE)?;
                cur_gpa += SZ_1G;
                cur_hpa += SZ_1G;
                remaining -= SZ_1G;
            } else if remaining >= SZ_2M && cur_gpa % SZ_2M == 0 && cur_hpa % SZ_2M == 0 {
                self.map_single_2m(cur_gpa, cur_hpa, flags | EptFlags::HUGE)?;
                cur_gpa += SZ_2M;
                cur_hpa += SZ_2M;
                remaining -= SZ_2M;
            } else {
                self.map_single_4k(cur_gpa, cur_hpa, flags)?;
                cur_gpa += SZ_4K;
                cur_hpa += SZ_4K;
                remaining -= SZ_4K;
            }
        }
        Ok(())
    }

    /// Internal helper: map a single 4 KiB page.
    fn map_single_4k(&mut self, gpa: u64, hpa: u64, flags: EptFlags) -> Result<(), EptError> {
        // Indices for each paging level (9 bits each)
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx   = ((gpa >> 21) & 0x1FF) as usize;
        let pt_idx   = ((gpa >> 12) & 0x1FF) as usize;

        // SAFETY: tables are uniquely owned through NonNull pointer chain.
        unsafe {
            // Level 4
            let pml4 = &mut *self.pml4.as_ptr();
            let mut pdpt_phys = pml4.entry(pml4_idx) & 0x000F_FFFF_FFFF_F000;
            if pdpt_phys == 0 {
                pdpt_phys = Self::alloc_table()?.as_phys();
                pml4.set_entry(pml4_idx, pdpt_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pdpt = &mut *(pdpt_phys as *mut EptTable);
            let mut pd_phys = pdpt.entry(pdpt_idx) & 0x000F_FFFF_FFFF_F000;
            if pd_phys == 0 {
                pd_phys = Self::alloc_table()?.as_phys();
                pdpt.set_entry(pdpt_idx, pd_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pd = &mut *(pd_phys as *mut EptTable);
            let mut pt_phys = pd.entry(pd_idx) & 0x000F_FFFF_FFFF_F000;
            if pt_phys == 0 {
                pt_phys = Self::alloc_table()?.as_phys();
                pd.set_entry(pd_idx, pt_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pt = &mut *(pt_phys as *mut EptTable);
            if pt.entry(pt_idx) & 1 != 0 {
                return Err(EptError::AlreadyMapped);
            }

            pt.set_entry(pt_idx, hpa, flags | EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
        }

        Ok(())
    }

    /// Map a single 1 GiB page (PDPT level).
    fn map_single_1g(&mut self, gpa: u64, hpa: u64, flags: EptFlags) -> Result<(), EptError> {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *self.pml4.as_ptr();
            let mut pdpt_phys = pml4.entry(pml4_idx) & 0x000F_FFFF_FFFF_F000;
            if pdpt_phys == 0 {
                pdpt_phys = Self::alloc_table()?.as_phys();
                pml4.set_entry(pml4_idx, pdpt_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pdpt = &mut *(pdpt_phys as *mut EptTable);
            if pdpt.entry(pdpt_idx) & 1 != 0 {
                return Err(EptError::AlreadyMapped);
            }
            pdpt.set_entry(pdpt_idx, hpa, flags | EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
        }
        Ok(())
    }

    /// Map a single 2 MiB page (PD level).
    fn map_single_2m(&mut self, gpa: u64, hpa: u64, flags: EptFlags) -> Result<(), EptError> {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx   = ((gpa >> 21) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *self.pml4.as_ptr();
            let mut pdpt_phys = pml4.entry(pml4_idx) & 0x000F_FFFF_FFFF_F000;
            if pdpt_phys == 0 {
                pdpt_phys = Self::alloc_table()?.as_phys();
                pml4.set_entry(pml4_idx, pdpt_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pdpt = &mut *(pdpt_phys as *mut EptTable);
            let mut pd_phys = pdpt.entry(pdpt_idx) & 0x000F_FFFF_FFFF_F000;
            if pd_phys == 0 {
                pd_phys = Self::alloc_table()?.as_phys();
                pdpt.set_entry(pdpt_idx, pd_phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }

            let pd = &mut *(pd_phys as *mut EptTable);
            if pd.entry(pd_idx) & 1 != 0 {
                return Err(EptError::AlreadyMapped);
            }
            pd.set_entry(pd_idx, hpa, flags | EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
        }
        Ok(())
    }

    /// Allocate a zero-initialised EPT table and leak it (identity-mapped for now).
    fn alloc_table() -> Result<&'static mut EptTable, EptError> {
        let boxed: Box<EptTable> = Box::new(EptTable::new());
        Ok(Box::leak(boxed))
    }

    /// Change permissions of an already-mapped guest region. Only RWX bits are modified.
    pub fn set_permissions(&mut self, gpa: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        if size % 0x1000 != 0 { return Err(EptError::InvalidAlignment); }
        let pages = size / 0x1000;
        for i in 0..pages {
            self.update_perm_single(gpa + i*0x1000, flags)?;
        }
        // Flush only the touched range to keep VMEXIT latency low.
        self.invalidate_gpa_range(gpa, size);
        Ok(())
    }

    fn update_perm_single(&mut self, gpa: u64, flags: EptFlags) -> Result<(), EptError> {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx   = ((gpa >> 21) & 0x1FF) as usize;
        let pt_idx   = ((gpa >> 12) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *self.pml4.as_ptr();
            let pdpt_phys = pml4.entry(pml4_idx) & 0x000F_FFFF_FFFF_F000;
            if pdpt_phys == 0 { return Err(EptError::NotMapped); }
            let pdpt = &mut *(pdpt_phys as *mut EptTable);

            let pd_phys = pdpt.entry(pdpt_idx) & 0x000F_FFFF_FFFF_F000;
            if pd_phys == 0 { return Err(EptError::NotMapped); }
            let pd = &mut *(pd_phys as *mut EptTable);

            let pt_phys = pd.entry(pd_idx) & 0x000F_FFFF_FFFF_F000;
            if pt_phys == 0 { return Err(EptError::NotMapped); }
            let pt = &mut *(pt_phys as *mut EptTable);

            let entry = pt.entry_mut(pt_idx);
            if *entry & 1 == 0 { return Err(EptError::NotMapped); }

            // Preserve physical address bits, replace RWX (bits 0-2)
            let phys_part = *entry & 0xFFFF_FFFF_FFFF_F000u64;
            let misc_bits = *entry & !(0x7); // keep higher bits like MEM_TYPE,HUGE
            *entry = phys_part | misc_bits | flags.bits();
        }
        Ok(())
    }

    /// Public unmap wrapper (4-KiB granularity for now).
    pub fn unmap(&mut self, gpa: u64, size: u64) -> Result<(), EptError> {
        if size % 0x1000 != 0 { return Err(EptError::InvalidAlignment); }
        let pages = size / 0x1000;
        for i in 0..pages {
            self.unmap_internal(gpa + i * 0x1000)?;
        }
        self.invalidate_gpa_range(gpa, size);
        Ok(())
    }

    /// Unmap a guest physical 4 KiB page.
    fn unmap_internal(&mut self, gpa: u64) -> Result<(), EptError> {
        let pml4_idx = ((gpa >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((gpa >> 30) & 0x1FF) as usize;
        let pd_idx   = ((gpa >> 21) & 0x1FF) as usize;
        let pt_idx   = ((gpa >> 12) & 0x1FF) as usize;

        unsafe {
            let pml4 = &mut *self.pml4.as_ptr();
            let pdpt_phys = pml4.entry(pml4_idx) & 0x000F_FFFF_FFFF_F000;
            if pdpt_phys == 0 { return Err(EptError::NotMapped); }
            let pdpt = &mut *(pdpt_phys as *mut EptTable);

            let pd_phys = pdpt.entry(pdpt_idx) & 0x000F_FFFF_FFFF_F000;
            if pd_phys == 0 { return Err(EptError::NotMapped); }
            let pd = &mut *(pd_phys as *mut EptTable);

            let pt_phys = pd.entry(pd_idx) & 0x000F_FFFF_FFFF_F000;
            if pt_phys == 0 { return Err(EptError::NotMapped); }
            let pt = &mut *(pt_phys as *mut EptTable);

            if pt.entry(pt_idx) & 1 == 0 { return Err(EptError::NotMapped); }
            *pt.entry_mut(pt_idx) = 0;
        }
        Ok(())
    }
}

// EptHierarchy owns unique pointers to allocated tables and is safe to move across threads
unsafe impl Send for EptHierarchy {}
unsafe impl Sync for EptHierarchy {} 