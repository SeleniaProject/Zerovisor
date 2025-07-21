// Storage device virtualization (SR-IOV NVMe / virtio-blk)
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use crate::memory::PhysicalAddress;

/// Generic error type for virtual storage operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    NotSupported,
    InitFailed,
    InvalidParameter,
    OutOfResources,
    MapFailed,
    UnmapFailed,
}

/// Identifier for a physical storage controller (PCI BDF on x86)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageDeviceId {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

/// Configuration for creating a virtual storage function (SR-IOV VF)
#[derive(Debug, Clone, Copy)]
pub struct StorageConfig {
    pub device: StorageDeviceId,
    pub vf_index: u16,
    pub features: StorageVirtFeatures,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StorageVirtFeatures: u32 {
        const SRIOV = 1 << 0;
        const PASSTHROUGH = 1 << 1;
    }
}

/// Handle to a virtual storage function.
pub type StorageHandle = u32;

/// Trait implemented by architecture-specific storage virtualization engines.
pub trait StorageVirtualization {
    fn init() -> Result<Self, StorageError> where Self: Sized;
    fn is_supported() -> bool;
    fn list_devices(&self) -> Vec<StorageDeviceId>;
    fn create_vf(&mut self, cfg: &StorageConfig) -> Result<StorageHandle, StorageError>;
    fn destroy_vf(&mut self, handle: StorageHandle) -> Result<(), StorageError>;
    fn map_guest_memory(&mut self, handle: StorageHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), StorageError>;
    fn unmap_guest_memory(&mut self, handle: StorageHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), StorageError>;
} 