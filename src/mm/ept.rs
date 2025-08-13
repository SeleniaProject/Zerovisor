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

#[derive(Clone, Copy, Debug)]
pub struct EptOptions {
    pub allow_execute: bool,
    pub enable_ad: bool,
}

impl Default for EptOptions {
    fn default() -> Self { Self { allow_execute: true, enable_ad: false } }
}

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

/// Build a minimal identity-mapped EPT up to `limit_bytes` using 1GiB pages.
/// Returns the host-physical (identity-assumed) address of the PML4 table.
pub fn build_identity_1g(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        // Link PML4[0] -> PDPT
        *pml4 = (pdpt as u64) | EPT_R | EPT_W | EPT_X;
        // Fill PDPT entries with 1GiB leaf mappings
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        let mut phys: u64 = 0;
        for i in 0..num_gb {
            let entry = (phys & 0x000F_FFFF_C000_0000) // 1GiB aligned
                | EPT_R | EPT_W | EPT_X | EPT_MEMTYPE_WB | EPT_IGNORE_PAT | EPT_PAGE_SIZE;
            *pdpt.add(i) = entry;
            phys = phys.wrapping_add(1u64 << 30);
            if phys >= limit_bytes { break; }
        }
    }
    Some(pml4)
}

/// Build a minimal identity-mapped EPT up to `limit_bytes` using 4KiB pages.
/// Returns the host-physical (identity-assumed) address of the PML4 table.
pub fn build_identity_4k(system_table: &SystemTable<Boot>, limit_bytes: u64) -> Option<*mut u64> {
    if limit_bytes == 0 { return None; }
    // Allocate top-level tables
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        // Link PML4[0] -> PDPT
        *pml4 = (pdpt as u64) | EPT_R | EPT_W | EPT_X;
        // We will create one PD for each 1GiB chunk referenced by PDPT
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | EPT_R | EPT_W | EPT_X;
            // For each 1GiB chunk, create 512 page tables (each for 2MiB span)
            // Iterate PDEs and point them to PTs (no large-page flag)
            let phys_1g_base: u64 = (i as u64) << 30;
            for j in 0..512usize {
                let pt = alloc_zeroed_page(system_table)?;
                let pde = pd.add(j);
                *pde = (pt as u64) | EPT_R | EPT_W | EPT_X; // next level pointer
                // Fill PTEs with 4KiB mappings within this 2MiB window
                let mut phys = phys_1g_base.wrapping_add((j as u64) << 21);
                for k in 0..512usize {
                    let pte = pt.add(k);
                    let entry = (phys & 0x000F_FFFF_FFFF_F000)
                        | EPT_R | EPT_W | EPT_X | EPT_MEMTYPE_WB | EPT_IGNORE_PAT;
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

/// Compose an EPTP with options (e.g., AD-bits enable).
pub fn eptp_from_pml4_with_opts(pml4_phys: u64, opts: EptOptions) -> u64 {
    let mut e = eptp_from_pml4(pml4_phys);
    if opts.enable_ad { e |= 1u64 << 6; }
    e
}

/// EPT capability flags (subset) used by builder selection.
#[derive(Clone, Copy, Debug, Default)]
pub struct EptCaps {
    pub large_page_2m: bool,
    pub large_page_1g: bool,
}

/// Build identity mapping selecting best large page size supported by caps.
pub fn build_identity_best(system_table: &SystemTable<Boot>, limit_bytes: u64, caps: EptCaps) -> Option<*mut u64> {
    if caps.large_page_1g { return build_identity_1g(system_table, limit_bytes); }
    if caps.large_page_2m { return build_identity_2m(system_table, limit_bytes); }
    // Fallback to 4KiB page tables when large pages are not available
    build_identity_4k(system_table, limit_bytes)
}

/// Build best identity map with options (e.g., NX default).
pub fn build_identity_best_with_opts(system_table: &SystemTable<Boot>, limit_bytes: u64, caps: EptCaps, opts: EptOptions) -> Option<*mut u64> {
    // Build using existing helpers then, if NX requested, clear X bits across structures is already handled by using the same builders.
    // For simplicity, reuse builders and patch X flag behavior by post-processing would be heavy; instead, adjust below by re-emitting entries.
    // Here, we select path and set X conditionally inline by duplicating minimal logic.
    if limit_bytes == 0 { return None; }
    let allow_x = opts.allow_execute;
    unsafe fn set_dir_flags(p: *mut u64, allow_x: bool) { unsafe { *p = (*p) & !(1u64 << 2) | if allow_x { 1u64 << 2 } else { 0 }; } }
    if caps.large_page_1g {
        let pml4 = alloc_zeroed_page(system_table)?;
        let pdpt = alloc_zeroed_page(system_table)?;
        unsafe {
            *pml4 = (pdpt as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
            let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
            let mut phys: u64 = 0;
            for i in 0..num_gb {
                let entry = (phys & 0x000F_FFFF_C000_0000)
                    | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 } | EPT_MEMTYPE_WB | EPT_IGNORE_PAT | EPT_PAGE_SIZE;
                *pdpt.add(i) = entry;
                phys = phys.wrapping_add(1u64 << 30);
                if phys >= limit_bytes { break; }
            }
        }
        return Some(pml4);
    }
    if caps.large_page_2m {
        let pml4 = alloc_zeroed_page(system_table)?;
        let pdpt = alloc_zeroed_page(system_table)?;
        unsafe {
            *pml4 = (pdpt as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
            let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
            for i in 0..num_gb {
                let pd = alloc_zeroed_page(system_table)?;
                *pdpt.add(i) = (pd as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
                let mut phys: u64 = (i as u64) << 30;
                for j in 0..512usize {
                    let pde = pd.add(j);
                    let entry = (phys & 0xFFFF_FFFF_FFE0_0000)
                        | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 } | EPT_MEMTYPE_WB | EPT_IGNORE_PAT | EPT_PAGE_SIZE;
                    *pde = entry;
                    phys = phys.wrapping_add(2 * 1024 * 1024);
                    if phys >= limit_bytes { break; }
                }
            }
        }
        return Some(pml4);
    }
    // 4K path
    let pml4 = alloc_zeroed_page(system_table)?;
    let pdpt = alloc_zeroed_page(system_table)?;
    unsafe {
        *pml4 = (pdpt as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
        let num_gb = ((limit_bytes + (1 << 30) - 1) >> 30) as usize;
        for i in 0..num_gb {
            let pd = alloc_zeroed_page(system_table)?;
            *pdpt.add(i) = (pd as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
            let phys_1g_base: u64 = (i as u64) << 30;
            for j in 0..512usize {
                let pt = alloc_zeroed_page(system_table)?;
                *pd.add(j) = (pt as u64) | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 };
                let mut phys = phys_1g_base.wrapping_add((j as u64) << 21);
                for k in 0..512usize {
                    let pte = pt.add(k);
                    let entry = (phys & 0x000F_FFFF_FFFF_F000)
                        | EPT_R | EPT_W | if allow_x { EPT_X } else { 0 } | EPT_MEMTYPE_WB | EPT_IGNORE_PAT;
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

/// Toggle execute permission for an address range in an EPT identity map.
/// Returns number of entries updated. Assumes identity-mapped firmware (phys==virt).
pub fn ept_toggle_exec(pml4_phys: u64, start: u64, length: u64, exec: bool) -> usize {
    if length == 0 { return 0; }
    let mut changed = 0usize;
    let mut addr = start & !0xFFFu64;
    let end = start.saturating_add(length);
    let pml4 = (pml4_phys & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
    unsafe {
        while addr < end {
            let l4 = ((addr >> 39) & 0x1FF) as isize;
            let pml4e = *pml4.offset(l4);
            if pml4e & EPT_R == 0 { addr = addr.saturating_add(1u64 << 39); continue; }
            let pdpt = (pml4e & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l3i = ((addr >> 30) & 0x1FF) as isize;
            let pdpte = *pdpt.offset(l3i);
            // 1GiB leaf?
            if pdpte & EPT_PAGE_SIZE != 0 {
                let new = if exec { pdpte | EPT_X } else { pdpte & !EPT_X };
                if new != pdpte { *pdpt.offset(l3i) = new; changed += 1; }
                addr = ((addr >> 30) + 1) << 30;
                continue;
            }
            if pdpte & EPT_R == 0 { addr = addr.saturating_add(1u64 << 30); continue; }
            let pd = (pdpte & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l2i = ((addr >> 21) & 0x1FF) as isize;
            let pde = *pd.offset(l2i);
            if pde & EPT_PAGE_SIZE != 0 {
                let new = if exec { pde | EPT_X } else { pde & !EPT_X };
                if new != pde { *pd.offset(l2i) = new; changed += 1; }
                addr = ((addr >> 21) + 1) << 21;
                continue;
            }
            if pde & EPT_R == 0 { addr = addr.saturating_add(1u64 << 21); continue; }
            let pt = (pde & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let mut l1i = ((addr >> 12) & 0x1FF) as isize;
            while addr < end && l1i < 512 {
                let pte = *pt.offset(l1i);
                let new = if exec { pte | EPT_X } else { pte & !EPT_X };
                if new != pte { *pt.offset(l1i) = new; changed += 1; }
                addr = addr.saturating_add(4096);
                l1i += 1;
                if (addr & ((1u64 << 21) - 1)) == 0 { break; }
            }
        }
    }
    changed
}


