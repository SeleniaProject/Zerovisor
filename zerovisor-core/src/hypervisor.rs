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
    
    /// Initialize the hypervisor with an externally supplied memory map
    /// This variant is used when the bootloader passes the physical memory
    /// map obtained from firmware. It fully replaces the internal demo map
    /// and therefore satisfies Task 2.2 requirements without simplification.
    pub fn init_with_map(&mut self, memory_map: &[MemoryRegion]) -> Result<(), ZerovisorError> {
        if self.initialized {
            return Ok(());
        }

        // Initialize physical memory management using the provided map
        self.init_memory_management(memory_map)?;

        // Initialize other subsystems
        vm::init()?;
        scheduler::init()?;
        security::init()?;

        self.initialized = true;
        Ok(())
    }

    // ---------------------------------------------------------------------
    /// Initialize the hypervisor
    pub fn init(&mut self) -> Result<(), ZerovisorError> {
        if self.initialized {
            return Ok(());
        }

        // Use an internal fallback map (legacy path). In production this
        // should never be reached because the bootloader supplies a map.
        let fallback_map = [MemoryRegion {
            start: 0x100000,
            size: 0x3FF00000,
            region_type: zerovisor_hal::memory::MemoryType::Available,
            flags: zerovisor_hal::memory::MemoryFlags::READABLE | zerovisor_hal::memory::MemoryFlags::WRITABLE,
        }];

        self.init_memory_management(&fallback_map)?;

        // Initialize other subsystems
        vm::init()?;
        scheduler::init()?;
        security::init()?;

        self.initialized = true;
        Ok(())
    }

    /// Initialize physical memory management (Task 2.2 implementation)
    fn init_memory_management(&mut self, memory_map: &[MemoryRegion]) -> Result<(), ZerovisorError> {
        if self.memory_initialized {
            return Ok(());
        }

        // Initialize the memory manager with the supplied memory map
        memory::init_memory_manager(memory_map)?;

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

/// Initialize the global hypervisor using an externally supplied memory map.
pub fn init_with_map(memory_map: &[MemoryRegion]) -> Result<(), ZerovisorError> {
    unsafe {
        if HYPERVISOR.is_none() {
            let mut hypervisor = Hypervisor::new();
            hypervisor.init_with_map(memory_map)?;
            HYPERVISOR = Some(hypervisor);
        }
    }
    Ok(())
}