#![allow(dead_code)]

//! Minimal native x86_64 paging (PML4) identity mapping builder for long mode.
//!
//! Builds a 4-level page table hierarchy with 2MiB pages up to a specified
//! physical limit, returning the PML4 physical address suitable to load in CR3.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

const PTE_P: u64 = 1 << 0; // present
const PTE_RW: u64 = 1 << 1; // writable
const PTE_PS: u64 = 1 << 7; // page size (for PDEs -> 2MiB)

fn alloc_zeroed_page(system_table: &SystemTable<Boot>) -> Option<*mut u64> {
    let page = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)?;
    unsafe { core::ptr::write_bytes(page, 0, 4096); }
    Some(page as *mut u64)
}

/// Build identity-mapped page tables using 2MiB pages up to `limit_bytes`.
pub fn build_identity_2m(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        // PML4[0] -> PDPT
        *pml4 = (pdpt as u64) | PTE_P | PTE_RW;
        // For each 1GiB chunk, create a PD and map 2MiB leaves
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | PTE_P | PTE_RW;
            let mut phys: u64 = (i as u64) << 30;
            for j in 0..512usize {
                let pde = pd.add(j);
                let entry = (phys & 0xFFFF_FFFF_FFE0_0000) | PTE_P | PTE_RW | PTE_PS;
                *pde = entry;
                phys = phys.wrapping_add(2 * 1024 * 1024);
                if phys >= limit_bytes { break; }
            }
        }
    }
    Some(pml4)
}


