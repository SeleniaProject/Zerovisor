use uefi::prelude::*;
use uefi::table::boot::{MemoryDescriptor, MemoryType};

/// Memory map wrapper for UEFI memory management
pub struct MemoryMap {
    pub buffer: Vec<u8>,
    pub map_size: usize,
    pub descriptor_size: usize,
    pub descriptor_version: u32,
}

impl MemoryMap {
    /// Create a new memory map from the UEFI system table
    pub fn new(system_table: &SystemTable<Boot>) -> Result<Self, uefi::Error> {
        let boot_services = system_table.boot_services();
        
        // Get memory map size
        let map_size = boot_services.memory_map_size();
        let mut buffer = vec![0u8; map_size.map_size + 2 * map_size.descriptor_size];
        
        // Get the actual memory map
        let (_key, descriptor_iter) = boot_services
            .memory_map(&mut buffer)
            .map_err(|e| e.status())?;
        
        Ok(MemoryMap {
            buffer,
            map_size: map_size.map_size,
            descriptor_size: map_size.descriptor_size,
            descriptor_version: descriptor_iter.descriptor_version(),
        })
    }
    
    /// Get memory descriptors iterator
    pub fn descriptors(&self) -> impl Iterator<Item = &MemoryDescriptor> {
        let descriptor_size = self.descriptor_size;
        self.buffer
            .chunks_exact(descriptor_size)
            .map(|chunk| unsafe {
                &*(chunk.as_ptr() as *const MemoryDescriptor)
            })
    }
    
    /// Find suitable memory region for hypervisor
    pub fn find_hypervisor_region(&self, size: usize) -> Option<u64> {
        for descriptor in self.descriptors() {
            if descriptor.ty == MemoryType::CONVENTIONAL 
                && descriptor.page_count as usize * 4096 >= size {
                return Some(descriptor.phys_start);
            }
        }
        None
    }
    
    /// Get total available memory
    pub fn total_memory(&self) -> u64 {
        self.descriptors()
            .filter(|desc| desc.ty == MemoryType::CONVENTIONAL)
            .map(|desc| desc.page_count * 4096)
            .sum()
    }
}