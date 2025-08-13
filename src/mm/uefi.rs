#![allow(dead_code)]

//! Thin wrappers around UEFI Boot Services page allocation for identity-mapped
//! early memory usage (e.g., VMXON region).

use uefi::table::boot::{AllocateType, MemoryType};
use uefi::table::SystemTable;
use uefi::prelude::Boot;

/// Allocate pages (4KiB each) from UEFI Boot Services.
pub fn alloc_pages(system_table: &SystemTable<Boot>, pages: usize, mem_type: MemoryType) -> Option<*mut u8> {
    let st = system_table.boot_services();
    // AllocateAnyPages gives firmware the choice of address. Identity mapping
    // is typical in firmware space; we assume physical==virtual for early use.
    match st.allocate_pages(AllocateType::AnyPages, mem_type, pages) {
        Ok(addr) => Some(addr as *mut u8),
        Err(_) => None,
    }
}

/// Free pages previously allocated.
pub fn free_pages(system_table: &SystemTable<Boot>, ptr: *mut u8, pages: usize) {
    unsafe { let _ = system_table.boot_services().free_pages(ptr as u64, pages); }
}

/// Allocate pages at a specific physical address (4KiB aligned). Returns pointer on success.
pub fn alloc_pages_at(system_table: &SystemTable<Boot>, phys: u64, pages: usize, mem_type: MemoryType) -> Option<*mut u8> {
    let st = system_table.boot_services();
    match st.allocate_pages(AllocateType::Address(phys), mem_type, pages) {
        Ok(addr) => Some(addr as *mut u8),
        Err(_) => None,
    }
}


