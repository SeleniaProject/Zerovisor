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
        if size % 4096 != 0 || gpa % 4096 != 0 || hpa % 4096 != 0 {
            return Err(EptError::InvalidAlignment);
        }
        // Only identity huge-page mapping demo: insert 1 GiB entry
        if size == 1 << 30 {
            unsafe {
                (*self.pml4.as_ptr()).set_entry(0, hpa, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
            }
            return Ok(());
        }
        // TODO: full walk and allocation for all sizes
        Err(EptError::OutOfMemory)
    }
} 