//! x86_64 memory management implementation

use zerovisor_hal::memory::{MemoryManager, PhysicalAddress, VirtualAddress, MemoryFlags, PageSize};
use crate::X86Error;
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::structures::paging::{PageTable, Page, Size4KiB, PhysFrame, MapperAllSizes};
use x86_64::VirtAddr;

/// x86_64 memory manager
pub struct X86MemoryManager {
    page_size: PageSize,
    phys_base: PhysicalAddress,
}

impl MemoryManager for X86MemoryManager {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        // Assume boot allocator starts after 2 MiB region (kernel loaded).
        static INIT: AtomicU64 = AtomicU64::new(0x200000);
        let _ = INIT.load(Ordering::Relaxed); // touch to ensure mut reference not optimised

        Ok(Self { page_size: 4096, phys_base: 0x0 })
    }
    
    fn allocate_physical(&mut self, size: usize, alignment: usize) -> Result<PhysicalAddress, Self::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0x400000); // start after 4 MiB
        let align_mask = (alignment.max(4096) as u64) - 1;
        let mut cur = NEXT.load(Ordering::Relaxed);
        if cur & align_mask != 0 { cur = (cur + align_mask) & !align_mask; }
        let end = cur + size as u64;
        NEXT.store(end, Ordering::Relaxed);
        Ok(cur)
    }
    
    fn free_physical(&mut self, _addr: PhysicalAddress, _size: usize) -> Result<(), Self::Error> {
        // Simplified implementation
        Ok(())
    }
    
    fn map_virtual(&mut self, _virt: VirtualAddress, _phys: PhysicalAddress, _flags: MemoryFlags) -> Result<(), Self::Error> {
        // Would implement page table mapping
        Ok(())
    }
    
    fn unmap_virtual(&mut self, _virt: VirtualAddress) -> Result<(), Self::Error> {
        // Would implement page table unmapping
        Ok(())
    }
    
    fn translate(&self, virt: VirtualAddress) -> Option<PhysicalAddress> {
        // For early stage we assume identity mapping for lower 1GiB and direct-map at 0xFFFF800000000000
        if virt < 0x4000_0000 {
            Some(virt)
        } else if virt >= 0xFFFF_8000_0000_0000 {
            Some(virt - 0xFFFF_8000_0000_0000)
        } else { None }
    }
    
    fn page_size(&self) -> PageSize {
        self.page_size
    }
    
    fn flush_tlb_address(&self, addr: VirtualAddress) {
        unsafe {
            x86_64::instructions::tlb::flush(x86_64::VirtAddr::new(addr));
        }
    }
    
    fn flush_tlb_all(&self) {
        unsafe {
            x86_64::instructions::tlb::flush_all();
        }
    }
    
    fn is_valid_address(&self, addr: VirtualAddress) -> bool {
        // Check if address is in valid virtual address space
        addr < 0x0000_8000_0000_0000 || addr >= 0xFFFF_8000_0000_0000
    }
}

/// Initialize x86_64 memory management
pub fn init() -> Result<(), X86Error> {
    Ok(())
}

/// Convert physical to virtual using direct-map window.
pub fn phys_to_virt(pa: PhysicalAddress) -> VirtualAddress { pa + 0xFFFF_8000_0000_0000 }

/// Convert virtual address in direct-map to physical.
pub fn virt_to_phys(va: VirtualAddress) -> PhysicalAddress { va - 0xFFFF_8000_0000_0000 }