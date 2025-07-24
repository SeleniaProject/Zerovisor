#![cfg(target_arch = "aarch64")]
#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::memory::{MemoryManager, MemoryFlags, PhysicalAddress, VirtualAddress, PageSize};

pub struct Arm64MemoryMgr;

impl MemoryManager for Arm64MemoryMgr {
    type Error = ();
    fn init() -> Result<Self, Self::Error> where Self: Sized { Ok(Self) }
    fn allocate_physical(&mut self, _size: usize, _alignment: usize) -> Result<PhysicalAddress, Self::Error> { Ok(0) }
    fn free_physical(&mut self, _addr: PhysicalAddress, _size: usize) -> Result<(), Self::Error> { Ok(()) }
    fn map_virtual(&mut self, _virt: VirtualAddress, _phys: PhysicalAddress, _flags: MemoryFlags) -> Result<(), Self::Error> { Ok(()) }
    fn unmap_virtual(&mut self, _virt: VirtualAddress) -> Result<(), Self::Error> { Ok(()) }
    fn translate(&self, virt: VirtualAddress) -> Option<PhysicalAddress> { Some(virt) }
    fn page_size(&self) -> PageSize { 4096 }
    fn flush_tlb_address(&self, _addr: VirtualAddress) { unsafe { core::arch::asm!("tlbi vaae1is, {}", in(reg) _addr, options(nostack, preserves_flags)); } }
    fn flush_tlb_all(&self) { unsafe { core::arch::asm!("tlbi vmalle1is", options(nostack, preserves_flags)); } }
    fn is_valid_address(&self, _addr: VirtualAddress) -> bool { true }
} 