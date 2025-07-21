//! IOMMU abstraction layer – supports Intel VT-d / AMD IOMMU / ARM SMMU
//! (Task: IOMMU / VT-d integration & device passthrough)
//! Provides unified DMA remapping interface for device assignment.

#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
use alloc::vec::Vec;
use crate::memory::PhysicalAddress;
use crate::AcceleratorId;
use crate::gpu::GpuDeviceId;
use crate::nic::NicAttr;

/// Generic error codes for IOMMU operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuError {
    Unsupported,
    InitFailed,
    MapFailed,
    UnmapFailed,
    DeviceNotFound,
    AlreadyAttached,
    NotAttached,
}

/// Represents a DMA mapping handle.
pub type DmaHandle = u64;

/// Trait implemented by architecture-specific IOMMU back-ends.
pub trait IommuEngine: Send + Sync {
    /// Initialise the engine and detect capabilities.
    fn init() -> Result<Self, IommuError> where Self: Sized;

    /// Attach a PCI/SoC device to an isolated DMA domain.
    fn attach_device(&self, bdf: u32) -> Result<(), IommuError>;

    /// Detach device from its domain.
    fn detach_device(&self, bdf: u32) -> Result<(), IommuError>;

    /// Map guest physical address for device DMA access.
    fn map(&self, bdf: u32, guest_pa: PhysicalAddress, host_pa: PhysicalAddress, size: usize, writable: bool) -> Result<DmaHandle, IommuError>;

    /// Unmap a previously mapped range.
    fn unmap(&self, bdf: u32, handle: DmaHandle) -> Result<(), IommuError>;

    /// Flush IOTLB for device.
    fn flush_tlb(&self, bdf: u32) -> Result<(), IommuError>;
} 