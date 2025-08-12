#![allow(dead_code)]

//! Minimal EPT structures and builder (scaffold). Not yet wired to VMX.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

// EPT entry bits (subset)
const EPT_R: u64 = 1 << 0;
const EPT_W: u64 = 1 << 1;
const EPT_X: u64 = 1 << 2;
const EPT_MEMTYPE_WB: u64 = 6 << 3; // EPT memory type field at bits 5:3 (6=WB)
const EPT_IGNORE_PAT: u64 = 1 << 6;
const EPT_PAGE_SIZE: u64 = 1 << 7; // For PDE/PDPTE large pages

/// Allocate a zeroed page via UEFI and return a mutable pointer to 64-bit entries.
fn alloc_zeroed_page(system_table: &SystemTable<Boot>) -> Option<*mut u64> {
    let page = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)?;
    unsafe { core::ptr::write_bytes(page, 0, 4096); }
    Some(page as *mut u64)
}

/// Build a minimal identity-mapped EPT up to `limit_bytes` using 2MiB pages.
/// Returns the host-physical (identity-assumed) address of the PML4 table.
pub fn build_identity_2m(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        // Link PML4[0] -> PDPT
        *pml4 = (pdpt as u64) | EPT_R | EPT_W | EPT_X;
        // Fill PDPT entries pointing to PDs
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | EPT_R | EPT_W | EPT_X;
            // Fill PDEs with 2MiB large page mappings
            let mut phys: u64 = (i as u64) << 30; // base of this 1GiB chunk
            for j in 0..512usize {
                let pde = pd.add(j);
                let entry = (phys & 0xFFFF_FFFF_FFE0_0000) // 2MiB aligned
                    | EPT_R | EPT_W | EPT_X | EPT_MEMTYPE_WB | EPT_IGNORE_PAT | EPT_PAGE_SIZE;
                *pde = entry;
                phys = phys.wrapping_add(2 * 1024 * 1024);
                if phys >= limit_bytes { break; }
            }
        }
    }
    Some(pml4)
}

/// Compose an EPTP value from a PML4 physical address.
/// - Memory type: WB (6)
/// - Page-walk length: 4 levels -> encode 3
/// - A/D bits disabled for compatibility
pub fn eptp_from_pml4(pml4_phys: u64) -> u64 {
    let addr = pml4_phys & 0x000F_FFFF_FFFF_F000u64;
    let memtype_wb = 6u64; // bits 2:0
    let walk_len = 3u64 << 3; // bits 5:3 (4-level)
    addr | memtype_wb | walk_len
}


