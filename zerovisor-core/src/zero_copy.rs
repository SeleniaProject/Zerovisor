//! Zero-copy buffer abstraction enabling near-zero-overhead I/O (Task 14.1)
//! This implementation provides helper functions to share a contiguous
//! physical buffer with a guest and obtain a DMA mapping for devices.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::ptr::NonNull;
use zerovisor_hal::memory::{PhysicalAddress, VirtualAddress};
use zerovisor_hal::virtualization::{VmHandle, VirtualizationFeatures};

/// Error when sharing memory with guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareError { InvalidVm, MappingFailed }

/// Error when performing DMA mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaError { Unsupported, MapFailed }

/// DMA handle representing device‐visible address range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaHandle { pub device_addr: u64, pub size: usize }

/// A physically contiguous buffer directly accessible by devices and guests.
#[derive(Debug)]
pub struct ZeroCopyBuffer {
    pub physical_addr: PhysicalAddress,
    pub virtual_addr: VirtualAddress,
    pub size: usize,
}

impl ZeroCopyBuffer {
    /// Create a new zero-copy buffer backed by existing physical memory.
    /// Caller guarantees the memory is pinned.
    pub const fn new(phys: PhysicalAddress, virt: VirtualAddress, size: usize) -> Self {
        Self { physical_addr: phys, virtual_addr: virt, size }
    }

    /// Map the buffer into guest address space with shared page permissions.
    pub fn share_with_guest<E: zerovisor_hal::virtualization::VirtualizationEngine + Send + Sync>(
        &self,
        engine: &mut E,
        vm: VmHandle,
        guest_phys: PhysicalAddress,
    ) -> Result<(), ShareError> {
        use zerovisor_hal::memory::MemoryFlags as MF;
        let flags = MF::READABLE | MF::WRITABLE | MF::CACHE_DISABLE;
        engine
            .map_guest_memory(vm, guest_phys, self.physical_addr, self.size, flags)
            .map_err(|_| ShareError::MappingFailed)
    }

    /// Obtain a DMA handle for direct device access (identity mapping assumed).
    pub fn direct_dma_access(&self) -> Result<DmaHandle, DmaError> {
        // For platforms with IOMMU, device address may differ; here we assume 1:1 mapping.
        Ok(DmaHandle { device_addr: self.physical_addr, size: self.size })
    }
} 