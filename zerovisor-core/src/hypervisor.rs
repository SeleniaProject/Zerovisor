//! Core hypervisor functionality

use crate::{ZerovisorError, memory, vm, scheduler, security};
use zerovisor_hal::memory::MemoryRegion;

/// Main hypervisor structure
pub struct Hypervisor {
    initialized: bool,
    memory_initialized: bool,
}

impl Hypervisor {
    /// Create a new hypervisor instance
    pub fn new() -> Self {
        Self {
            initialized: false,
            memory_initialized: false,
        }
    }
    
    /// Initialize the hypervisor
    pub fn init(&mut self) -> Result<(), ZerovisorError> {
        if self.initialized {
            return Ok(());
        }
        
        // Initialize physical memory management (Task 2.2)
        self.init_memory_management()?;
        
        // Initialize other subsystems
        vm::init()?;
        scheduler::init()?;
        security::init()?;
        
        self.initialized = true;
        Ok(())
    }

    /// Initialize physical memory management (Task 2.2 implementation)
    fn init_memory_management(&mut self) -> Result<(), ZerovisorError> {
        if self.memory_initialized {
            return Ok(());
        }

        // Create a sample memory map for initialization
        // In a real implementation, this would come from the bootloader
        let memory_map = [
            MemoryRegion {
                start: 0x100000,  // 1MB
                size: 0x3FF00000, // ~1GB available memory
                region_type: zerovisor_hal::memory::MemoryType::Available,
                flags: zerovisor_hal::memory::MemoryFlags::READABLE | zerovisor_hal::memory::MemoryFlags::WRITABLE,
            },
            MemoryRegion {
                start: 0x0,
                size: 0x100000,   // First 1MB reserved
                region_type: zerovisor_hal::memory::MemoryType::Reserved,
                flags: zerovisor_hal::memory::MemoryFlags::empty(),
            },
        ];

        // Initialize the memory manager with the memory map
        memory::init_memory_manager(&memory_map)?;

        self.memory_initialized = true;
        Ok(())
    }

    /// Check if memory management is initialized
    pub fn is_memory_initialized(&self) -> bool {
        self.memory_initialized
    }
    
    /// Check if hypervisor is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// Global hypervisor instance
static mut HYPERVISOR: Option<Hypervisor> = None;

/// Initialize the global hypervisor instance
pub fn init() -> Result<(), ZerovisorError> {
    unsafe {
        if HYPERVISOR.is_none() {
            let mut hypervisor = Hypervisor::new();
            hypervisor.init()?;
            HYPERVISOR = Some(hypervisor);
        }
    }
    Ok(())
}

/// Get reference to the global hypervisor instance
pub fn get_hypervisor() -> Option<&'static Hypervisor> {
    unsafe { HYPERVISOR.as_ref() }
}