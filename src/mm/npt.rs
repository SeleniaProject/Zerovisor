#![allow(dead_code)]

//! Minimal NPT (Nested Page Tables) builder for AMD SVM.
//!
//! This module provides a tiny identity-mapping NPT builder using 2MiB pages
//! suitable for early smoke tests. It mirrors the EPT builder structure but
//! uses AMD NPT permission bits and page size flag semantics.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

// NPT entry bits (subset)
const NPT_READ: u64 = 1 << 0;
const NPT_WRITE: u64 = 1 << 1;
const NPT_EXEC: u64 = 1 << 2;
const NPT_PAGE_SIZE: u64 = 1 << 7; // For PDE large pages
const NPT_PTE_PRESENT: u64 = NPT_READ | NPT_WRITE | NPT_EXEC;

/// Allocate a zeroed page and return as 64-bit entry pointer.
fn alloc_zeroed_page(system_table: &SystemTable<Boot>) -> Option<*mut u64> {
    let page = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)?;
    unsafe { core::ptr::write_bytes(page, 0, 4096); }
    Some(page as *mut u64)
}

/// Build a minimal identity-mapped NPT up to `limit_bytes` using 2MiB pages.
/// Returns the physical address (identity-assumed) of the PML4 table.
pub fn build_identity_2m(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        // Link PML4[0] -> PDPT with RWX permissions
        *pml4 = (pdpt as u64) | NPT_READ | NPT_WRITE | NPT_EXEC;
        // For each 1GiB chunk, create a PD and fill 2MiB entries
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | NPT_READ | NPT_WRITE | NPT_EXEC;
            let mut phys: u64 = (i as u64) << 30;
            for j in 0..512usize {
                let pde = pd.add(j);
                let entry = (phys & 0xFFFF_FFFF_FFE0_0000)
                    | NPT_READ | NPT_WRITE | NPT_EXEC | NPT_PAGE_SIZE;
                *pde = entry;
                phys = phys.wrapping_add(2 * 1024 * 1024);
                if phys >= limit_bytes { break; }
            }
        }
    }
    Some(pml4)
}

/// Build a minimal identity-mapped NPT up to `limit_bytes` using 1GiB pages.
/// Returns the physical address (identity-assumed) of the PML4 table.
pub fn build_identity_1g(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        *pml4 = (pdpt as u64) | NPT_PTE_PRESENT;
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        let mut phys: u64 = 0;
        for i in 0..num_gb {
            // PDPTE with 1GiB page (bit 7 set)
            let entry = (phys & 0x000F_FFFF_C000_0000) | NPT_PTE_PRESENT | NPT_PAGE_SIZE;
            *pdpt.add(i) = entry;
            phys = phys.wrapping_add(1u64 << 30);
            if phys >= limit_bytes { break; }
        }
    }
    Some(pml4)
}

/// Build a minimal identity-mapped NPT up to `limit_bytes` using 4KiB pages.
pub fn build_identity_4k(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        *pml4 = (pdpt as u64) | NPT_PTE_PRESENT;
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | NPT_PTE_PRESENT;
            let phys_1g_base: u64 = (i as u64) << 30;
            for j in 0..512usize {
                let pt = alloc_zeroed_page(system_table)?;
                *pd.add(j) = (pt as u64) | NPT_PTE_PRESENT; // next level pointer
                let mut phys = phys_1g_base.wrapping_add((j as u64) << 21);
                for k in 0..512usize {
                    let pte = pt.add(k);
                    let entry = (phys & 0x000F_FFFF_FFFF_F000) | NPT_PTE_PRESENT;
                    *pte = entry;
                    phys = phys.wrapping_add(4096);
                    if phys >= limit_bytes { break; }
                }
                if phys_1g_base.wrapping_add(((j + 1) as u64) << 21) >= limit_bytes { break; }
            }
        }
    }
    Some(pml4)
}

/// Compose an NCr3 value (nested CR3) from a PML4 physical address.
#[inline(always)]
pub fn ncr3_from_pml4(pml4_phys: u64) -> u64 {
    pml4_phys & 0x000F_FFFF_FFFF_F000u64
}


