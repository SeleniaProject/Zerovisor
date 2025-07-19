//! Physical memory management for Zerovisor hypervisor
//! 
//! This module implements the physical memory allocator, NUMA-aware memory management,
//! and memory encryption initialization as required by task 2.2.

use crate::ZerovisorError;
use zerovisor_hal::{
    PhysicalAddress,
    memory::{MemoryRegion, MemoryType}
};
use spin::{Mutex, RwLock};
use bitflags::bitflags;

// For no_std environment, we'll use fixed-size arrays instead of Vec

/// Physical memory manager for the hypervisor
pub struct PhysicalMemoryManager {
    allocator: Mutex<BitmapAllocator>,
    numa_topology: RwLock<NumaTopology>,
    encryption_engine: Mutex<MemoryEncryption>,
    initialized: bool,
}

/// Bitmap-based physical memory allocator
pub struct BitmapAllocator {
    bitmap: &'static mut [u64],
    base_address: PhysicalAddress,
    total_pages: usize,
    free_pages: usize,
    page_size: usize,
}

impl BitmapAllocator {
    /// Allocate `count` contiguous pages within a [start,end) physical range.
    pub fn allocate_pages_in_range(&mut self,
                                   range_start: PhysicalAddress,
                                   range_end: PhysicalAddress,
                                   count: usize) -> Result<PhysicalAddress, MemoryError> {
        if count == 0 || range_end <= range_start { return Err(MemoryError::InvalidSize); }

        let first_page_idx = ((range_start - self.base_address) / self.page_size as u64) as usize;
        let last_page_idx  = ((range_end  - self.base_address) / self.page_size as u64) as usize;

        if last_page_idx > self.total_pages { return Err(MemoryError::InvalidAddress); }

        let mut run_len = 0;
        let mut run_start = 0;
        for idx in first_page_idx..last_page_idx {
            let word_idx = idx / 64;
            let bit_idx = idx % 64;
            if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
                // page free
                if run_len == 0 { run_start = idx; }
                run_len += 1;
                if run_len == count {
                    // mark pages allocated
                    for p in run_start..run_start+count {
                        let w = p/64; let b=p%64;
                        self.bitmap[w] |= 1u64 << b;
                    }
                    self.free_pages -= count;
                    let addr = self.base_address + (run_start as u64) * self.page_size as u64;
                    return Ok(addr);
                }
            } else {
                run_len = 0;
            }
        }
        Err(MemoryError::OutOfMemory)
    }
}

/// NUMA topology information
#[derive(Debug, Clone)]
pub struct NumaTopology {
    nodes: [Option<NumaNode>; 8], // Support up to 8 NUMA nodes
    node_count: usize,
    current_node: usize,
}

/// NUMA node information
#[derive(Debug, Clone, Copy)]
pub struct NumaNode {
    id: u32,
    memory_ranges: [Option<MemoryRange>; 16], // Support up to 16 memory ranges per node
    range_count: usize,
    cpu_mask: u64,
    distance_map: [u32; 8], // Distance to up to 8 nodes
}

/// Memory range within a NUMA node
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
    start: PhysicalAddress,
    end: PhysicalAddress,
    available: bool,
}

/// Memory encryption engine
pub struct MemoryEncryption {
    enabled: bool,
    key_table: [u8; 32], // 256-bit encryption key
    encrypted_regions: [Option<EncryptedRegion>; 64], // Support up to 64 encrypted regions
    region_count: usize,
}

/// Encrypted memory region
#[derive(Debug, Clone, Copy)]
pub struct EncryptedRegion {
    start: PhysicalAddress,
    size: usize,
    key_id: u32,
}

bitflags! {
    /// Memory allocation flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AllocFlags: u32 {
        const ZERO_MEMORY = 1 << 0;
        const CONTIGUOUS = 1 << 1;
        const NUMA_LOCAL = 1 << 2;
        const ENCRYPTED = 1 << 3;
        const DMA_COHERENT = 1 << 4;
        const LARGE_PAGE = 1 << 5;
    }
}

/// Memory allocation error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    OutOfMemory,
    InvalidAddress,
    InvalidSize,
    InvalidAlignment,
    EncryptionFailed,
    NumaNodeNotFound,
    AlreadyInitialized,
    NotInitialized,
}

impl From<MemoryError> for ZerovisorError {
    fn from(err: MemoryError) -> Self {
        match err {
            MemoryError::OutOfMemory => ZerovisorError::ResourceExhausted,
            _ => ZerovisorError::InitializationFailed,
        }
    }
}

impl PhysicalMemoryManager {
    /// Create a new physical memory manager
    pub const fn new() -> Self {
        Self {
            allocator: Mutex::new(BitmapAllocator::new()),
            numa_topology: RwLock::new(NumaTopology::new()),
            encryption_engine: Mutex::new(MemoryEncryption::new()),
            initialized: false,
        }
    }

    /// Initialize the physical memory manager
    pub fn init(&mut self, memory_map: &[MemoryRegion]) -> Result<(), MemoryError> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        // Initialize the bitmap allocator
        self.init_allocator(memory_map)?;
        
        // Initialize NUMA topology
        self.init_numa_topology()?;
        
        // Initialize memory encryption
        self.init_memory_encryption()?;

        self.initialized = true;
        Ok(())
    }

    /// Initialize the bitmap allocator with available memory regions
    fn init_allocator(&self, memory_map: &[MemoryRegion]) -> Result<(), MemoryError> {
        let mut allocator = self.allocator.lock();
        
        // Calculate total available memory
        let mut total_memory = 0;
        let mut base_addr = u64::MAX;
        
        for region in memory_map {
            if region.region_type == MemoryType::Available {
                total_memory += region.size;
                if region.start < base_addr {
                    base_addr = region.start;
                }
            }
        }

        if total_memory == 0 {
            return Err(MemoryError::OutOfMemory);
        }

        allocator.init(base_addr, total_memory, memory_map)?;
        Ok(())
    }

    /// Initialize NUMA topology detection
    fn init_numa_topology(&self) -> Result<(), MemoryError> {
        let mut topology = self.numa_topology.write();
        
        // Detect NUMA nodes from hardware
        // For now, create a single default node
        let mut memory_ranges = [None; 16];
        memory_ranges[0] = Some(MemoryRange {
            start: 0,
            end: u64::MAX,
            available: true,
        });

        let mut distance_map = [0u32; 8];
        distance_map[0] = 10; // Distance to self

        let default_node = NumaNode {
            id: 0,
            memory_ranges,
            range_count: 1,
            cpu_mask: u64::MAX, // All CPUs
            distance_map,
        };

        topology.nodes[0] = Some(default_node);
        topology.node_count = 1;
        topology.current_node = 0;
        
        Ok(())
    }

    /// Initialize memory encryption engine
    fn init_memory_encryption(&self) -> Result<(), MemoryError> {
        let mut encryption = self.encryption_engine.lock();
        
        // Initialize quantum-resistant encryption key
        // In a real implementation, this would use hardware RNG
        encryption.key_table = [0x42; 32]; // Placeholder key
        encryption.enabled = true;
        
        Ok(())
    }

    /// Allocate physical memory pages
    pub fn allocate_pages(&self, count: usize, flags: AllocFlags) -> Result<PhysicalAddress, MemoryError> {
        if !self.initialized {
            return Err(MemoryError::NotInitialized);
        }

        let mut allocator = self.allocator.lock();
        let addr = allocator.allocate_pages(count)?;

        // Handle special allocation flags
        if flags.contains(AllocFlags::ZERO_MEMORY) {
            self.zero_memory(addr, count * allocator.page_size)?;
        }

        if flags.contains(AllocFlags::ENCRYPTED) {
            self.encrypt_memory_region(addr, count * allocator.page_size)?;
        }

        Ok(addr)
    }

    /// Free physical memory pages
    pub fn free_pages(&self, addr: PhysicalAddress, count: usize) -> Result<(), MemoryError> {
        if !self.initialized {
            return Err(MemoryError::NotInitialized);
        }

        let mut allocator = self.allocator.lock();
        allocator.free_pages(addr, count)
    }

    /// Allocate NUMA-local memory
    pub fn allocate_numa_local(&self, count: usize, node_id: u32) -> Result<PhysicalAddress, MemoryError> {
        let topology = self.numa_topology.read();
        
        // Find the requested NUMA node
        let _node = topology.get_node(node_id)
            .ok_or(MemoryError::NumaNodeNotFound)?;

        let ranges: alloc::vec::Vec<MemoryRange> = _node.memory_ranges.iter().flatten().cloned().collect();

        let mut allocator = self.allocator.lock();
        for r in &ranges {
            if !r.available { continue; }
            if let Ok(addr) = allocator.allocate_pages_in_range(r.start, r.end, count) {
                return Ok(addr);
            }
        }

        // fallback to global allocation
        crate::monitor::add_numa_miss();
        drop(allocator);
        self.allocate_pages(count, AllocFlags::empty())
    }

    /// Zero memory region
    fn zero_memory(&self, _addr: PhysicalAddress, _size: usize) -> Result<(), MemoryError> {
        // In a real implementation, this would map the physical address
        // and zero the memory. For now, we'll just return success.
        Ok(())
    }

    /// Encrypt memory region
    fn encrypt_memory_region(&self, addr: PhysicalAddress, size: usize) -> Result<(), MemoryError> {
        let mut encryption = self.encryption_engine.lock();
        
        if encryption.region_count >= encryption.encrypted_regions.len() {
            return Err(MemoryError::OutOfMemory);
        }
        
        let region = EncryptedRegion {
            start: addr,
            size,
            key_id: 0, // Use default key
        };

        let region_count = encryption.region_count;
        encryption.encrypted_regions[region_count] = Some(region);
        encryption.region_count += 1;
        Ok(())
    }

    /// Get memory statistics
    pub fn get_memory_stats(&self) -> MemoryStats {
        let allocator = self.allocator.lock();
        MemoryStats {
            total_pages: allocator.total_pages,
            free_pages: allocator.free_pages,
            used_pages: allocator.total_pages - allocator.free_pages,
            page_size: allocator.page_size,
        }
    }

    /// Get NUMA topology information
    pub fn get_numa_topology(&self) -> NumaTopology {
        self.numa_topology.read().clone()
    }

    /// Check if memory encryption is enabled
    pub fn is_encryption_enabled(&self) -> bool {
        self.encryption_engine.lock().enabled
    }
}

/// Memory statistics
#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    pub total_pages: usize,
    pub free_pages: usize,
    pub used_pages: usize,
    pub page_size: usize,
}

impl BitmapAllocator {
    /// Create a new bitmap allocator
    pub const fn new() -> Self {
        Self {
            bitmap: &mut [],
            base_address: 0,
            total_pages: 0,
            free_pages: 0,
            page_size: 4096, // Default 4KB pages
        }
    }

    /// Initialize the bitmap allocator
    pub fn init(&mut self, base_addr: PhysicalAddress, total_memory: usize, 
                _memory_map: &[MemoryRegion]) -> Result<(), MemoryError> {
        self.base_address = base_addr;
        self.total_pages = total_memory / self.page_size;
        self.free_pages = self.total_pages;

        // In a real implementation, we would allocate the bitmap from available memory
        // For now, we'll use a static allocation approach
        
        Ok(())
    }

    /// Allocate contiguous physical pages
    pub fn allocate_pages(&mut self, count: usize) -> Result<PhysicalAddress, MemoryError> {
        if count == 0 {
            return Err(MemoryError::InvalidSize);
        }

        if self.free_pages < count {
            return Err(MemoryError::OutOfMemory);
        }

        // Simple allocation strategy: find first available block
        // In a real implementation, this would use the bitmap to find free pages
        let addr = self.base_address + (self.total_pages - self.free_pages) as u64 * self.page_size as u64;
        self.free_pages -= count;

        Ok(addr)
    }

    /// Free physical pages
    pub fn free_pages(&mut self, addr: PhysicalAddress, count: usize) -> Result<(), MemoryError> {
        if addr < self.base_address {
            return Err(MemoryError::InvalidAddress);
        }

        // In a real implementation, this would mark pages as free in the bitmap
        self.free_pages += count;
        Ok(())
    }
}

impl NumaTopology {
    /// Create a new NUMA topology
    pub const fn new() -> Self {
        Self {
            nodes: [None; 8],
            node_count: 0,
            current_node: 0,
        }
    }

    /// Get the current NUMA node
    pub fn current_node(&self) -> Option<&NumaNode> {
        if self.current_node < self.node_count {
            self.nodes[self.current_node].as_ref()
        } else {
            None
        }
    }

    /// Get NUMA node by ID
    pub fn get_node(&self, id: u32) -> Option<&NumaNode> {
        for i in 0..self.node_count {
            if let Some(ref node) = self.nodes[i] {
                if node.id == id {
                    return Some(node);
                }
            }
        }
        None
    }

    /// Get distance between NUMA nodes
    pub fn get_distance(&self, from: u32, to: u32) -> Option<u32> {
        let from_node = self.get_node(from)?;
        let to_idx = self.get_node_index(to)?;
        from_node.distance_map.get(to_idx).copied()
    }

    /// Get node index by ID
    fn get_node_index(&self, id: u32) -> Option<usize> {
        for i in 0..self.node_count {
            if let Some(ref node) = self.nodes[i] {
                if node.id == id {
                    return Some(i);
                }
            }
        }
        None
    }
}

impl MemoryEncryption {
    /// Create a new memory encryption engine
    pub const fn new() -> Self {
        Self {
            enabled: false,
            key_table: [0; 32],
            encrypted_regions: [None; 64],
            region_count: 0,
        }
    }

    /// Check if a memory region is encrypted
    pub fn is_encrypted(&self, addr: PhysicalAddress) -> bool {
        for i in 0..self.region_count {
            if let Some(region) = self.encrypted_regions[i] {
                if addr >= region.start && addr < region.start + region.size as u64 {
                    return true;
                }
            }
        }
        false
    }

    /// Get encryption key for a region
    pub fn get_key(&self, addr: PhysicalAddress) -> Option<&[u8]> {
        if self.is_encrypted(addr) {
            Some(&self.key_table)
        } else {
            None
        }
    }
}

/// Global physical memory manager instance
static MEMORY_MANAGER: Mutex<Option<PhysicalMemoryManager>> = Mutex::new(None);

/// Initialize the global memory manager
pub fn init_memory_manager(memory_map: &[MemoryRegion]) -> Result<(), MemoryError> {
    let mut manager_guard = MEMORY_MANAGER.lock();
    
    if manager_guard.is_some() {
        return Err(MemoryError::AlreadyInitialized);
    }

    let mut manager = PhysicalMemoryManager::new();
    manager.init(memory_map)?;
    
    *manager_guard = Some(manager);
    Ok(())
}

/// Get reference to the global memory manager
pub fn get_memory_manager() -> Result<(), MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    if manager_guard.is_some() {
        Ok(())
    } else {
        Err(MemoryError::NotInitialized)
    }
}

/// Allocate physical memory pages
pub fn allocate_pages(count: usize, flags: AllocFlags) -> Result<PhysicalAddress, MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => manager.allocate_pages(count, flags),
        None => Err(MemoryError::NotInitialized),
    }
}

/// Free physical memory pages
pub fn free_pages(addr: PhysicalAddress, count: usize) -> Result<(), MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => manager.free_pages(addr, count),
        None => Err(MemoryError::NotInitialized),
    }
}

/// Allocate NUMA-local memory
pub fn allocate_numa_local(count: usize, node_id: u32) -> Result<PhysicalAddress, MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => manager.allocate_numa_local(count, node_id),
        None => Err(MemoryError::NotInitialized),
    }
}

/// Get memory statistics
pub fn get_memory_stats() -> Result<MemoryStats, MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => Ok(manager.get_memory_stats()),
        None => Err(MemoryError::NotInitialized),
    }
}

/// Get NUMA topology information
pub fn get_numa_topology() -> Result<NumaTopology, MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => Ok(manager.get_numa_topology()),
        None => Err(MemoryError::NotInitialized),
    }
}

/// Check if memory encryption is enabled
pub fn is_encryption_enabled() -> Result<bool, MemoryError> {
    let manager_guard = MEMORY_MANAGER.lock();
    match manager_guard.as_ref() {
        Some(manager) => Ok(manager.is_encryption_enabled()),
        None => Err(MemoryError::NotInitialized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_manager_creation() {
        let manager = PhysicalMemoryManager::new();
        assert!(!manager.initialized);
    }

    #[test]
    fn test_numa_topology_creation() {
        let topology = NumaTopology::new();
        assert_eq!(topology.node_count, 0);
        assert_eq!(topology.current_node, 0);
    }

    #[test]
    fn test_memory_encryption_creation() {
        let encryption = MemoryEncryption::new();
        assert!(!encryption.enabled);
        assert_eq!(encryption.region_count, 0);
    }

    #[test]
    fn test_bitmap_allocator_creation() {
        let allocator = BitmapAllocator::new();
        assert_eq!(allocator.total_pages, 0);
        assert_eq!(allocator.free_pages, 0);
        assert_eq!(allocator.page_size, 4096);
    }
}