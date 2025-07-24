//! Intel Extended Page Table (EPT) support for Zerovisor
//!
//! This initial implementation covers only the data structures and a very
//! small helper for creating identity-mapped EPT suitable for early guest
//! bring-up.  Large-page support and advanced permissions will be added as
//! we progress through Task 3.2.
#![cfg(target_arch = "x86_64")]

use crate::memory::PhysicalAddress;
use crate::{vec, Vec};
use core::arch::asm;

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
    static mut EPT_PD: EptTable = EptTable::new();

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

/// Enhanced EPT implementation with full 4-level page table support
pub struct EnhancedEpt {
    pml4: *mut EptTable,
    allocated_tables: Vec<*mut EptTable>,
}


impl EnhancedEpt {
    /// Create new EPT with complete 4-level page table hierarchy
    pub fn new() -> Result<Self, EptError> {
        // Allocate PML4 table (4KB aligned)
        let pml4_addr = crate::memory::allocate_aligned(4096, 4096)
            .map_err(|_| EptError::AllocationFailed)?;
        
        // Zero out the table
        unsafe {
            core::ptr::write_bytes(pml4_addr as *mut u8, 0, 4096);
        }
        
        Ok(EnhancedEpt {
            pml4_table: PhysicalAddress::new(pml4_addr as u64),
            allocated_tables: vec![PhysicalAddress::new(pml4_addr as u64)],
        })
    }
    
    /// Map guest physical address to host physical address
    pub fn map_page(&mut self, guest_pa: u64, host_pa: u64, flags: EptFlags) -> Result<(), EptError> {
        self.map_page_size(guest_pa, host_pa, flags, PageSize::Page4KB)
    }
    
    /// Map with specific page size (4KB, 2MB, 1GB)
    pub fn map_page_size(&mut self, guest_pa: u64, host_pa: u64, flags: EptFlags, size: PageSize) -> Result<(), EptError> {
        let pml4_index = (guest_pa >> 39) & 0x1FF;
        let pdpt_index = (guest_pa >> 30) & 0x1FF;
        let pd_index = (guest_pa >> 21) & 0x1FF;
        let pt_index = (guest_pa >> 12) & 0x1FF;
        
        unsafe {
            let pml4 = self.pml4_table.as_u64() as *mut u64;
            
            // Get or create PDPT
            let pdpt_entry = pml4.add(pml4_index as usize);
            let pdpt_addr = if *pdpt_entry & EptFlags::READ.bits() == 0 {
                let addr = self.allocate_table()?;
                *pdpt_entry = addr.as_u64() | EptFlags::READ.bits() | EptFlags::WRITE.bits() | EptFlags::EXECUTE.bits();
                addr
            } else {
                PhysicalAddress::new(*pdpt_entry & !0xFFF)
            };
            
            let pdpt = pdpt_addr.as_u64() as *mut u64;
            
            match size {
                PageSize::Page1GB => {
                    // Map 1GB page directly in PDPT
                    let entry = pdpt.add(pdpt_index as usize);
                    *entry = (host_pa & !0x3FFFFFFF) | flags.bits() | EptFlags::LARGE_PAGE.bits();
                    return Ok(());
                }
                _ => {}
            }
            
            // Get or create PD
            let pd_entry = pdpt.add(pdpt_index as usize);
            let pd_addr = if *pd_entry & EptFlags::READ.bits() == 0 {
                let addr = self.allocate_table()?;
                *pd_entry = addr.as_u64() | EptFlags::READ.bits() | EptFlags::WRITE.bits() | EptFlags::EXECUTE.bits();
                addr
            } else {
                PhysicalAddress::new(*pd_entry & !0xFFF)
            };
            
            let pd = pd_addr.as_u64() as *mut u64;
            
            match size {
                PageSize::Page2MB => {
                    // Map 2MB page directly in PD
                    let entry = pd.add(pd_index as usize);
                    *entry = (host_pa & !0x1FFFFF) | flags.bits() | EptFlags::LARGE_PAGE.bits();
                    return Ok(());
                }
                _ => {}
            }
            
            // Get or create PT for 4KB pages
            let pt_entry = pd.add(pd_index as usize);
            let pt_addr = if *pt_entry & EptFlags::READ.bits() == 0 {
                let addr = self.allocate_table()?;
                *pt_entry = addr.as_u64() | EptFlags::READ.bits() | EptFlags::WRITE.bits() | EptFlags::EXECUTE.bits();
                addr
            } else {
                PhysicalAddress::new(*pt_entry & !0xFFF)
            };
            
            let pt = pt_addr.as_u64() as *mut u64;
            
            // Map 4KB page
            let entry = pt.add(pt_index as usize);
            *entry = (host_pa & !0xFFF) | flags.bits();
        }
        
        Ok(())
    }
    
    /// Unmap a page and invalidate TLB
    pub fn unmap_page(&mut self, guest_pa: u64) -> Result<(), EptError> {
        let pml4_index = (guest_pa >> 39) & 0x1FF;
        let pdpt_index = (guest_pa >> 30) & 0x1FF;
        let pd_index = (guest_pa >> 21) & 0x1FF;
        let pt_index = (guest_pa >> 12) & 0x1FF;
        
        unsafe {
            let pml4 = self.pml4_table.as_u64() as *mut u64;
            let pml4_entry = *pml4.add(pml4_index as usize);
            
            if pml4_entry & EptFlags::READ.bits() == 0 {
                return Ok(); // Already unmapped
            }
            
            let pdpt = (pml4_entry & !0xFFF) as *mut u64;
            let pdpt_entry = *pdpt.add(pdpt_index as usize);
            
            if pdpt_entry & EptFlags::READ.bits() == 0 {
                return Ok(); // Already unmapped
            }
            
            // Check for 1GB page
            if pdpt_entry & EptFlags::LARGE_PAGE.bits() != 0 {
                *pdpt.add(pdpt_index as usize) = 0;
                self.invalidate_ept_tlb();
                return Ok();
            }
            
            let pd = (pdpt_entry & !0xFFF) as *mut u64;
            let pd_entry = *pd.add(pd_index as usize);
            
            if pd_entry & EptFlags::READ.bits() == 0 {
                return Ok(); // Already unmapped
            }
            
            // Check for 2MB page
            if pd_entry & EptFlags::LARGE_PAGE.bits() != 0 {
                *pd.add(pd_index as usize) = 0;
                self.invalidate_ept_tlb();
                return Ok();
            }
            
            let pt = (pd_entry & !0xFFF) as *mut u64;
            *pt.add(pt_index as usize) = 0;
            
            self.invalidate_ept_tlb();
        }
        
        Ok(())
    }
    
    /// Create identity mapping for a memory range
    pub fn identity_map_range(&mut self, start: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        let end = start + size;
        let mut addr = start & !0xFFF; // Align to 4KB
        
        while addr < end {
            // Try to use largest possible page size
            if addr % (1024 * 1024 * 1024) == 0 && (end - addr) >= (1024 * 1024 * 1024) {
                // Use 1GB page
                self.map_page_size(addr, addr, flags, PageSize::Page1GB)?;
                addr += 1024 * 1024 * 1024;
            } else if addr % (2 * 1024 * 1024) == 0 && (end - addr) >= (2 * 1024 * 1024) {
                // Use 2MB page
                self.map_page_size(addr, addr, flags, PageSize::Page2MB)?;
                addr += 2 * 1024 * 1024;
            } else {
                // Use 4KB page
                self.map_page_size(addr, addr, flags, PageSize::Page4KB)?;
                addr += 4096;
            }
        }
        
        Ok(())
    }
    
    /// Translate guest physical address to host physical address
    pub fn translate(&self, guest_pa: u64) -> Result<u64, EptError> {
        let pml4_index = (guest_pa >> 39) & 0x1FF;
        let pdpt_index = (guest_pa >> 30) & 0x1FF;
        let pd_index = (guest_pa >> 21) & 0x1FF;
        let pt_index = (guest_pa >> 12) & 0x1FF;
        let offset = guest_pa & 0xFFF;
        
        unsafe {
            let pml4 = self.pml4_table.as_u64() as *const u64;
            let pml4_entry = *pml4.add(pml4_index as usize);
            
            if pml4_entry & EptFlags::READ.bits() == 0 {
                return Err(EptError::PageNotMapped);
            }
            
            let pdpt = (pml4_entry & !0xFFF) as *const u64;
            let pdpt_entry = *pdpt.add(pdpt_index as usize);
            
            if pdpt_entry & EptFlags::READ.bits() == 0 {
                return Err(EptError::PageNotMapped);
            }
            
            // Check for 1GB page
            if pdpt_entry & EptFlags::LARGE_PAGE.bits() != 0 {
                let page_base = pdpt_entry & !0x3FFFFFFF;
                let page_offset = guest_pa & 0x3FFFFFFF;
                return Ok(page_base + page_offset);
            }
            
            let pd = (pdpt_entry & !0xFFF) as *const u64;
            let pd_entry = *pd.add(pd_index as usize);
            
            if pd_entry & EptFlags::READ.bits() == 0 {
                return Err(EptError::PageNotMapped);
            }
            
            // Check for 2MB page
            if pd_entry & EptFlags::LARGE_PAGE.bits() != 0 {
                let page_base = pd_entry & !0x1FFFFF;
                let page_offset = guest_pa & 0x1FFFFF;
                return Ok(page_base + page_offset);
            }
            
            let pt = (pd_entry & !0xFFF) as *const u64;
            let pt_entry = *pt.add(pt_index as usize);
            
            if pt_entry & EptFlags::READ.bits() == 0 {
                return Err(EptError::PageNotMapped);
            }
            
            let page_base = pt_entry & !0xFFF;
            Ok(page_base + offset)
        }
    }
    
    /// Get EPT pointer for VMCS
    pub fn get_ept_pointer(&self) -> u64 {
        // EPT pointer format:
        // Bits 2:0 - EPT paging-structure memory type (6 = write-back)
        // Bits 5:3 - EPT page-walk length minus 1 (3 for 4-level)
        // Bits 11:6 - Reserved (0)
        // Bits 63:12 - Physical address of PML4 table
        self.pml4_table.as_u64() | (6 << 0) | (3 << 3)
    }
    
    /// Allocate a new page table
    fn allocate_table(&mut self) -> Result<PhysicalAddress, EptError> {
        let addr = crate::memory::allocate_aligned(4096, 4096)
            .map_err(|_| EptError::AllocationFailed)?;
        
        // Zero out the table
        unsafe {
            core::ptr::write_bytes(addr as *mut u8, 0, 4096);
        }
        
        let pa = PhysicalAddress::new(addr as u64);
        self.allocated_tables.push(pa);
        Ok(pa)
    }
    
    /// Invalidate EPT TLB entries
    fn invalidate_ept_tlb(&self) {
        unsafe {
            // INVEPT instruction - invalidate all EPT-derived translations
            let descriptor = [self.get_ept_pointer(), 0u64];
            asm!(
                "invept {}, [{}]",
                in(reg) 1u64, // Single-context invalidation
                in(reg) descriptor.as_ptr(),
                options(nostack, preserves_flags)
            );
        }
    }
    /// Create new enhanced EPT with full 4-level support
    pub fn new() -> Result<Self, EptError> {
        let pml4 = Self::allocate_table()?;
        Ok(EnhancedEpt {
            pml4,
            allocated_tables: vec![pml4],
        })
    }
    
    /// Map a guest physical address to host physical address
    pub fn map(&mut self, guest_phys: u64, host_phys: u64, size: u64, flags: EptFlags) -> Result<(), EptError> {
        let mut addr = guest_phys;
        let end_addr = guest_phys + size;
        
        while addr < end_addr {
            self.map_page(addr, host_phys + (addr - guest_phys), flags)?;
            addr += 0x1000; // 4KB pages
        }
        
        Ok(())
    }
    
    /// Map a single 4KB page
    fn map_page(&mut self, guest_phys: u64, host_phys: u64, flags: EptFlags) -> Result<(), EptError> {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx = ((guest_phys >> 12) & 0x1FF) as usize;
        
        unsafe {
            // Get or create PDPT
            let pdpt = if (*self.pml4).entry(pml4_idx) & 1 == 0 {
                let new_pdpt = Self::allocate_table()?;
                self.allocated_tables.push(new_pdpt);
                (*self.pml4).set_entry(pml4_idx, new_pdpt as u64, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
                new_pdpt
            } else {
                ((*self.pml4).entry(pml4_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable
            };
            
            // Get or create PD
            let pd = if (*pdpt).entry(pdpt_idx) & 1 == 0 {
                let new_pd = Self::allocate_table()?;
                self.allocated_tables.push(new_pd);
                (*pdpt).set_entry(pdpt_idx, new_pd as u64, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
                new_pd
            } else {
                ((*pdpt).entry(pdpt_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable
            };
            
            // Get or create PT
            let pt = if (*pd).entry(pd_idx) & 1 == 0 {
                let new_pt = Self::allocate_table()?;
                self.allocated_tables.push(new_pt);
                (*pd).set_entry(pd_idx, new_pt as u64, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC);
                new_pt
            } else {
                ((*pd).entry(pd_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable
            };
            
            // Map the page
            (*pt).set_entry(pt_idx, host_phys, flags);
        }
        
        Ok(())
    }
    
    /// Unmap guest physical address range
    pub fn unmap(&mut self, guest_phys: u64, size: u64) -> Result<(), EptError> {
        let mut addr = guest_phys;
        let end_addr = guest_phys + size;
        
        while addr < end_addr {
            self.unmap_page(addr)?;
            addr += 0x1000; // 4KB pages
        }
        
        Ok(())
    }
    
    /// Unmap a single 4KB page
    fn unmap_page(&mut self, guest_phys: u64) -> Result<(), EptError> {
        let pml4_idx = ((guest_phys >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((guest_phys >> 30) & 0x1FF) as usize;
        let pd_idx = ((guest_phys >> 21) & 0x1FF) as usize;
        let pt_idx = ((guest_phys >> 12) & 0x1FF) as usize;
        
        unsafe {
            if (*self.pml4).entry(pml4_idx) & 1 == 0 { return Ok(()); }
            let pdpt = ((*self.pml4).entry(pml4_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable;
            
            if (*pdpt).entry(pdpt_idx) & 1 == 0 { return Ok(()); }
            let pd = ((*pdpt).entry(pdpt_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable;
            
            if (*pd).entry(pd_idx) & 1 == 0 { return Ok(()); }
            let pt = ((*pd).entry(pd_idx) & 0x000f_ffff_ffff_f000) as *mut EptTable;
            
            // Clear the page entry
            *(*pt).entry_mut(pt_idx) = 0;
        }
        
        Ok(())
    }
    
    /// Allocate a new EPT table
    fn allocate_table() -> Result<*mut EptTable, EptError> {
        static mut TABLE_STORAGE: [[u64; 512]; 1024] = [[0; 512]; 1024];
        static mut NEXT_TABLE: usize = 0;
        
        unsafe {
            if NEXT_TABLE >= TABLE_STORAGE.len() {
                return Err(EptError::OutOfMemory);
            }
            let table = &mut TABLE_STORAGE[NEXT_TABLE] as *mut [u64; 512] as *mut EptTable;
            NEXT_TABLE += 1;
            
            // Zero the table
            for i in 0..512 {
                unsafe {
                    *(*table).entry_mut(i) = 0;
                }
            }
            
            Ok(table)
        }
    }
    
    /// Get physical address of PML4 table
    pub fn pml4_phys(&self) -> PhysicalAddress {
        self.pml4 as PhysicalAddress
    }
}

/// Page sizes supported by EPT
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    Page4KB,
    Page2MB,
    Page1GB,
}

/// EPT-related errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EptError {
    AllocationFailed,
    PageNotMapped,
    InvalidAddress,
    PermissionDenied,
}
 