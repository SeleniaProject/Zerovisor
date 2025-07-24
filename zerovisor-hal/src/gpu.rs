//! GPU virtualization abstraction (Task 7.1)
//! Supports SR-IOV / MIG style virtual functions in a hardware-agnostic way.
//! This module is *no_std* friendly and avoids any external allocations.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use bitflags::bitflags;
use crate::memory::PhysicalAddress;

/// GPU error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    NotSupported,
    InitializationFailed,
    OutOfResources,
    InvalidParameter,
    MappingFailed,
}

/// Identifier for a physical GPU device (e.g., PCI BDF on x86)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpuDeviceId {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

bitflags! {
    /// Virtualization features supported by the GPU
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GpuVirtFeatures: u32 {
        const SRIOV = 1 << 0;
        const MIG   = 1 << 1; // Multi-Instance GPU (NVIDIA)
        const PASSTHROUGH = 1 << 2;
    }
}

/// Configuration for creating a virtual GPU function
#[derive(Debug, Clone)]
pub struct GpuConfig {
    pub device: GpuDeviceId,
    pub vf_index: u16,
    pub dedicated_memory_mb: u32,
    pub features: GpuVirtFeatures,
}

/// Telemetry metrics per virtual GPU function
#[derive(Debug, Clone, Copy)]
pub struct GpuMetrics {
    pub utilization_pct: u8,      // 0-100 %
    pub memory_used_mb: u32,      // consumed framebuffer memory
    pub temperature_c: u8,        // die temperature
}

/// Handle to a virtual GPU function
pub type GpuHandle = u32;

/// GPU virtualization engine trait
pub trait GpuVirtualization {
    /// Initialize the engine (e.g., enumerate PCI devices, enable SR-IOV)
    fn init() -> Result<Self, GpuError> where Self: Sized;

    /// Query if hardware virtualization is supported
    fn is_supported() -> bool;

    /// Enumerate physical GPU devices present in the system
    fn list_devices(&self) -> Vec<GpuDeviceId>;

    /// Create a new virtual function / slice
    fn create_vf(&mut self, cfg: &GpuConfig) -> Result<GpuHandle, GpuError>;

    /// Destroy a virtual function
    fn destroy_vf(&mut self, gpu: GpuHandle) -> Result<(), GpuError>;

    /// Query real-time metrics for a given VF / MIG slice.
    fn query_metrics(&self, gpu: GpuHandle) -> Result<GpuMetrics, GpuError>;

    /// Map guest memory for DMA by the GPU
    fn map_guest_memory(&mut self, gpu: GpuHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), GpuError>;

    /// Unmap guest memory
    fn unmap_guest_memory(&mut self, gpu: GpuHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), GpuError>;
}

