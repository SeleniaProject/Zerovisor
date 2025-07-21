//! Stage-2 translation manager for RISC-V Sv48 (Hypervisor extension)
//! Provides functionality equivalent to `EptHierarchy` on x86_64 making the
//! hypervisor paging subsystem portable.
//!
//! This code targets 4 KiB pages with support for 2 MiB and 1 GiB leafs.

#![cfg(target_arch = "riscv64")]

extern crate alloc;
use alloc::boxed::Box;
use core::ptr::NonNull;
use core::arch::asm;

use crate::memory::PhysicalAddress;

bitflags::bitflags! {
    #[derive(Default, Copy, Clone)]
    pub struct S2Flags: u64 {
        const VALID   = 1 << 0;
        const READ    = 1 << 1;
        const WRITE   = 1 << 2;
        const EXEC    = 1 << 3;
        const USER    = 1 << 4;
        const GLOBAL  = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY    = 1 << 7;
        const HUGE     = 1 << 8; // Software bit for 2M/1G leafs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S2Error { InvalidAlignment, OutOfMemory, AlreadyMapped, NotMapped }

#[repr(C, align(4096))]
pub struct S2Table([u64; 512]);

impl S2Table {
    pub const fn new() -> Self { Self([0; 512]) }
    #[inline] fn set_entry(&mut self, idx: usize, phys: PhysicalAddress, flags: S2Flags) {
        self.0[idx] = (phys >> 2) | flags.bits(); // PPN[53:2]
    }
    #[inline] fn entry(&self, idx: usize) -> u64 { self.0[idx] }
    #[inline] fn entry_mut(&mut self, idx: usize) -> &mut u64 { &mut self.0[idx] }
    #[inline] fn as_phys(&self) -> PhysicalAddress { self as *const _ as PhysicalAddress }
}

pub struct EptHierarchy { root: NonNull<S2Table> }

impl EptHierarchy {
    pub fn new() -> Result<Self, S2Error> {
        let boxed: Box<S2Table> = Box::new(S2Table::new());
        Ok(Self { root: NonNull::new(Box::leak(boxed)).unwrap() })
    }
    #[inline] pub fn phys_root(&self) -> PhysicalAddress { self.root.as_ptr() as PhysicalAddress }

    pub fn map(&mut self, gpa: u64, hpa: u64, size: u64, flags: S2Flags) -> Result<(), S2Error> {
        self.map_internal(gpa, hpa, size, flags)?;
        self.invalidate_gpa_range(gpa, size);
        Ok(())
    }

    fn map_internal(&mut self, mut gpa: u64, mut hpa: u64, mut size: u64, flags: S2Flags) -> Result<(), S2Error> {
        if gpa % 0x1000 != 0 || hpa % 0x1000 != 0 || size % 0x1000 != 0 {
            return Err(S2Error::InvalidAlignment);
        }
        const SZ_4K: u64 = 0x1000;
        const SZ_2M: u64 = 2*1024*1024;
        const SZ_1G: u64 = 1024*1024*1024;
        while size > 0 {
            if size >= SZ_1G && gpa % SZ_1G == 0 && hpa % SZ_1G == 0 {
                self.map_leaf(gpa, hpa, 0, flags | S2Flags::HUGE)?;
                gpa += SZ_1G; hpa += SZ_1G; size -= SZ_1G;
            } else if size >= SZ_2M && gpa % SZ_2M == 0 && hpa % SZ_2M == 0 {
                self.map_leaf(gpa, hpa, 1, flags | S2Flags::HUGE)?;
                gpa += SZ_2M; hpa += SZ_2M; size -= SZ_2M;
            } else {
                self.map_leaf(gpa, hpa, 2, flags)?;
                gpa += SZ_4K; hpa += SZ_4K; size -= SZ_4K;
            }
        }
        Ok(())
    }

    fn map_leaf(&mut self, gpa: u64, hpa: u64, level: usize, flags: S2Flags) -> Result<(), S2Error> {
        let l3_idx = ((gpa >> 12) & 0x1FF) as usize;
        let l2_idx = ((gpa >> 21) & 0x1FF) as usize;
        let l1_idx = ((gpa >> 30) & 0x1FF) as usize;
        let l0_idx = ((gpa >> 39) & 0x1FF) as usize;

        unsafe {
            let l0 = &mut *self.root.as_ptr();
            let mut l1_phys = l0.entry(l0_idx) & !0x3FFu64 << 2; // bits 53:10 are PPN
            if l1_phys == 0 {
                l1_phys = Self::alloc_table()?.as_phys();
                l0.set_entry(l0_idx, l1_phys, S2Flags::VALID);
            }
            if level == 0 {
                let l1 = &mut *(l1_phys as *mut S2Table);
                if l1.entry(l1_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
                l1.set_entry(l1_idx, hpa, flags | S2Flags::VALID);
                return Ok(());
            }
            let l1 = &mut *(l1_phys as *mut S2Table);
            let mut l2_phys = l1.entry(l1_idx) & !0x3FFu64 << 2;
            if l2_phys == 0 {
                l2_phys = Self::alloc_table()?.as_phys();
                l1.set_entry(l1_idx, l2_phys, S2Flags::VALID);
            }
            if level == 1 {
                let l2 = &mut *(l2_phys as *mut S2Table);
                if l2.entry(l2_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
                l2.set_entry(l2_idx, hpa, flags | S2Flags::VALID);
                return Ok(());
            }
            let l2 = &mut *(l2_phys as *mut S2Table);
            let mut l3_phys = l2.entry(l2_idx) & !0x3FFu64 << 2;
            if l3_phys == 0 {
                l3_phys = Self::alloc_table()?.as_phys();
                l2.set_entry(l2_idx, l3_phys, S2Flags::VALID);
            }
            let l3 = &mut *(l3_phys as *mut S2Table);
            if l3.entry(l3_idx) & 1 != 0 { return Err(S2Error::AlreadyMapped); }
            l3.set_entry(l3_idx, hpa, flags | S2Flags::VALID);
        }
        Ok(())
    }

    fn alloc_table() -> Result<&'static mut S2Table, S2Error> {
        Ok(Box::leak(Box::new(S2Table::new())))
    }

    /// Simple TLB shootdown helpers using HFENCE.GVMA.
    #[inline] pub fn invalidate_entire_tlb(&self) {
        unsafe { asm!("hfence.gvma zero, zero"); }
    }
    #[inline] pub fn invalidate_gpa_range(&self, gpa: u64, size: u64) {
        let mut addr = gpa & !0xFFFu64;
        let end = gpa + size;
        unsafe {
            while addr < end {
                asm!("hfence.gvma {addr}, zero", addr = in(reg) addr >> 12);
                addr += 0x1000;
            }
        }
    }
}

unsafe impl Send for EptHierarchy {}
unsafe impl Sync for EptHierarchy {} 