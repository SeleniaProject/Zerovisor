//! Intel Extended Page Table (EPT) support for Zerovisor
//!
//! This initial implementation covers only the data structures and a very
//! small helper for creating identity-mapped EPT suitable for early guest
//! bring-up.  Large-page support and advanced permissions will be added as
//! we progress through Task 3.2.
#![cfg(target_arch = "x86_64")]

use crate::memory::PhysicalAddress;

/// EPT entry flags
bitflags::bitflags! {
    #[derive(Default, Copy, Clone)]
    pub struct EptFlags: u64 {
        const READ      = 1 << 0;
        const WRITE     = 1 << 1;
        const EXEC      = 1 << 2;
        const HUGE      = 1 << 7; // 2-MiB or 1-GiB page depending on level
        const MEMORY_WB = 6 << 3; // Write-back memory type (bits 3-5)
    }
}

/// A single 512-entry EPT table (PML4/PDPT/PD/PT share same layout).
#[repr(C, align(4096))]
pub struct EptTable([u64; 512]);

impl EptTable {
    pub const fn new() -> Self { Self([0; 512]) }

    pub fn set_entry(&mut self, index: usize, addr: PhysicalAddress, flags: EptFlags) {
        self.0[index] = (addr & 0x000f_ffff_ffff_f000) | flags.bits();
    }

    /// Immutable access to raw 64-bit entry value.
    #[inline]
    pub fn entry(&self, idx: usize) -> u64 {
        self.0[idx]
    }

    /// Mutable access to raw entry (caller must ensure coherence).
    #[inline]
    pub fn entry_mut(&mut self, idx: usize) -> &mut u64 {
        &mut self.0[idx]
    }

    pub fn as_phys(&self) -> PhysicalAddress {
        self as *const _ as PhysicalAddress
    }
}

/// Very small helper that creates a 1:1 mapped EPT (4-GiB, 2-MiB pages).
pub fn build_identity_ept() -> PhysicalAddress {
    // SAFETY: static mut used only during early boot, single-threaded.
    static mut EPT_PML4: EptTable = EptTable::new();
    static mut EPT_PDPT: EptTable = EptTable::new();
    static mut EPT_PD:   EptTable = EptTable::new();

    unsafe {
        // Map first 4-GiB using one PD with 2-MiB pages (512 entries).
        for i in 0..512 {
            let phys = (i as u64) << 21; // 2 MiB pages
            EPT_PD.set_entry(i, phys, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC | EptFlags::HUGE | EptFlags::MEMORY_WB);
        }
        // Point PDPT[0] -> PD, PML4[0] -> PDPT
        EPT_PDPT.set_entry(0, EPT_PD.as_phys(), EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
        EPT_PML4.set_entry(0, EPT_PDPT.as_phys(), EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);

        EPT_PML4.as_phys()
    }
} 