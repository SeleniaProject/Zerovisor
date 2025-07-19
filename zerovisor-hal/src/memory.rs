//! Memory management abstraction layer

use bitflags::bitflags;

/// Physical address type
pub type PhysicalAddress = u64;

/// Virtual address type
pub type VirtualAddress = u64;

/// Page size type
pub type PageSize = usize;

/// Memory management trait for different architectures
pub trait MemoryManager {
    /// Memory manager specific error type
    type Error;
    
    /// Initialize the memory manager
    fn init() -> Result<Self, Self::Error> where Self: Sized;
    
    /// Allocate physical memory
    fn allocate_physical(&mut self, size: usize, alignment: usize) -> Result<PhysicalAddress, Self::Error>;
    
    /// Free physical memory
    fn free_physical(&mut self, addr: PhysicalAddress, size: usize) -> Result<(), Self::Error>;
    
    /// Map virtual to physical address
    fn map_virtual(&mut self, virt: VirtualAddress, phys: PhysicalAddress, flags: MemoryFlags) -> Result<(), Self::Error>;
    
    /// Unmap virtual address
    fn unmap_virtual(&mut self, virt: VirtualAddress) -> Result<(), Self::Error>;
    
    /// Translate virtual to physical address
    fn translate(&self, virt: VirtualAddress) -> Option<PhysicalAddress>;
    
    /// Get page size for the architecture
    fn page_size(&self) -> PageSize;
    
    /// Flush TLB for specific address
    fn flush_tlb_address(&self, addr: VirtualAddress);
    
    /// Flush entire TLB
    fn flush_tlb_all(&self);
    
    /// Check if address is valid
    fn is_valid_address(&self, addr: VirtualAddress) -> bool;
}

bitflags! {
    /// Memory mapping flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MemoryFlags: u64 {
        const READABLE = 1 << 0;
        const WRITABLE = 1 << 1;
        const EXECUTABLE = 1 << 2;
        const USER_ACCESSIBLE = 1 << 3;
        const CACHE_DISABLE = 1 << 4;
        const WRITE_THROUGH = 1 << 5;
        const GLOBAL = 1 << 6;
        const NO_EXECUTE = 1 << 7;
        const ENCRYPTED = 1 << 8;
        const LARGE_PAGE = 1 << 9;
    }
}

/// Memory region descriptor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    pub start: PhysicalAddress,
    pub size: usize,
    pub region_type: MemoryType,
    pub flags: MemoryFlags,
}

/// Memory region types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    Available,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    Bootloader,
    Kernel,
    Hypervisor,
    /// UEFI Loader code
    LoaderCode,
    /// UEFI Loader data
    LoaderData,
    /// UEFI Boot services code
    BootServicesCode,
    /// UEFI Boot services data
    BootServicesData,
}

/// Convert UEFI `MemoryType` to HAL `MemoryType`
#[cfg(feature = "uefi")]
pub fn from_uefi(uefi_ty: uefi::table::boot::MemoryType) -> MemoryType {
    use uefi::table::boot::MemoryType as UefiTy;
    match uefi_ty {
        UefiTy::CONVENTIONAL => MemoryType::Available,
        UefiTy::LOADER_CODE => MemoryType::LoaderCode,
        UefiTy::LOADER_DATA => MemoryType::LoaderData,
        UefiTy::BOOT_SERVICES_CODE => MemoryType::BootServicesCode,
        UefiTy::BOOT_SERVICES_DATA => MemoryType::BootServicesData,
        UefiTy::ACPI_RECLAIM => MemoryType::AcpiReclaimable,
        UefiTy::ACPI_NON_VOLATILE => MemoryType::AcpiNvs,
        UefiTy::BAD_MEMORY => MemoryType::BadMemory,
        _ => MemoryType::Reserved,
    }
}

/// Physical memory allocator trait
pub trait PhysicalAllocator {
    type Error;
    
    /// Initialize the allocator with available memory regions
    fn init(regions: &[MemoryRegion]) -> Result<Self, Self::Error> where Self: Sized;
    
    /// Allocate contiguous physical pages
    fn allocate_pages(&mut self, count: usize) -> Result<PhysicalAddress, Self::Error>;
    
    /// Free physical pages
    fn free_pages(&mut self, addr: PhysicalAddress, count: usize) -> Result<(), Self::Error>;
    
    /// Get total available memory
    fn total_memory(&self) -> usize;
    
    /// Get free memory
    fn free_memory(&self) -> usize;
    
    /// Get used memory
    fn used_memory(&self) -> usize;
}