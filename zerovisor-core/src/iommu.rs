//! High-level IOMMU manager – implements device passthrough backing for VM assignment
//! Wraps HAL ArchIommu implementation and provides simple API.

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Once;

use zerovisor_hal::{iommu::{IommuEngine, IommuError, DmaHandle}, ArchIommu};
use zerovisor_hal::virtualization::VmHandle;

static IOMMU: Once<ArchIommu> = Once::new();

/// Initialise VT-d engine
pub fn init() -> Result<(), IommuError> {
    let engine = ArchIommu::init()?;
    IOMMU.call_once(|| engine);
    Ok(())
}

/// Attach device BDF to its own DMA domain
pub fn attach_device(bdf: u32) -> Result<(), IommuError> { IOMMU.get().ok_or(IommuError::InitFailed)?.attach_device(bdf) }

pub fn detach_device(bdf: u32) -> Result<(), IommuError> { IOMMU.get().ok_or(IommuError::InitFailed)?.detach_device(bdf) }

pub fn map_dma(bdf: u32, gpa: u64, hpa: u64, size: usize, writable: bool) -> Result<DmaHandle, IommuError> { IOMMU.get().ok_or(IommuError::InitFailed)?.map(bdf, gpa, hpa, size, writable) }

pub fn unmap_dma(bdf: u32, handle: DmaHandle) -> Result<(), IommuError> { IOMMU.get().ok_or(IommuError::InitFailed)?.unmap(bdf, handle) } 