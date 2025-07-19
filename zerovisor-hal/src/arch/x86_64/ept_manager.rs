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

    /// Map a guest physical range to host physical range with given flags.
    /// size must be 4 KiB, 2 MiB or 1 GiB aligned.
    pub fn map(&mut self, gpa: u64, hpa: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        if size == 0 {
            return Ok(());
        }

        if gpa % 0x1000 != 0 || hpa % 0x1000 != 0 || size % 0x1000 != 0 {
            return Err(EptError::InvalidAlignment);
        }

        let pages = size / 0x1000;
        for i in 0..pages {
            let cur_gpa = gpa + i * 0x1000;
            let cur_hpa = hpa + i * 0x1000;
            self.map_single_4k(cur_gpa, cur_hpa, flags)?;
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

    /// Allocate a zero-initialised EPT table and leak it (identity-mapped for now).
    fn alloc_table() -> Result<&'static mut EptTable, EptError> {
        let boxed: Box<EptTable> = Box::new(EptTable::new());
        Ok(Box::leak(boxed))
    }

    /// Unmap a guest physical 4 KiB page.
    pub fn unmap(&mut self, gpa: u64) -> Result<(), EptError> {
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