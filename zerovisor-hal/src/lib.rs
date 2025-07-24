#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Hardware Abstraction Layer for Zerovisor Hypervisor
//! 
//! This crate provides a unified interface for different CPU architectures,
//! enabling the hypervisor to run on x86_64, ARM64, and RISC-V platforms.

extern crate alloc;

// Re-export alloc for internal use
pub use alloc::{vec, vec::Vec, boxed::Box, string::String};

pub mod cpu;
pub mod memory;
pub mod interrupts;
pub mod timer;
pub mod virtualization;
pub mod cycles;
pub mod arch; // New architecture-specific module tree
pub mod gpu;
pub mod nic;
pub mod power_mgmt;
pub mod storage;
pub mod accelerator;
pub mod rdma_opt;
pub mod rdma_vnet;
pub mod iommu;
pub mod dirty;
pub mod pci;
pub mod tpu;
pub mod qpu;
pub mod fpga;
pub mod tpm;
pub mod virtio_blk;
pub mod numa;

// Re-export core traits
pub use cpu::{Cpu, CpuFeatures, CpuState};
pub use memory::{MemoryManager, PhysicalAddress, VirtualAddress};
pub use interrupts::{InterruptController, InterruptHandler};
pub use timer::{Timer, TimerCallback};
pub use virtualization::{VirtualizationEngine, VmHandle, VmConfig};
pub use gpu::{GpuVirtualization, GpuDeviceId, GpuConfig, GpuError};
pub use accelerator::{AcceleratorVirtualization, AcceleratorId, AcceleratorInfo, AcceleratorType, AccelError};
pub use qpu::QpuVirtualization;
pub use nic::{HpcNic, NicAttr, NicError, RdmaOpKind, RdmaCompletion};
pub use power_mgmt::PowerManager;
pub use dirty::{DirtyPageTracker, DirtyRange};
pub use numa::{Topology as NumaTopology, NodeInfo as NumaNodeInfo};

// Re-export architecture specific CPU implementations when available
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::X86Cpu as ArchCpu;

#[cfg(target_arch = "aarch64")]
pub use arch::arm64::ArmCpu as ArchCpu;

#[cfg(target_arch = "riscv64")]
pub use arch::riscv64::RiscVCpu as ArchCpu;

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::iommu::VtdEngine as ArchIommu;

#[cfg(target_arch = "aarch64")]
pub use arch::arm64::iommu::SmmuEngine as ArchIommu;

#[cfg(target_arch = "riscv64")]
pub use arch::riscv64::iommu::RiscvIommuEngine as ArchIommu;

/// Initialize the HAL for the current architecture
pub fn init() -> Result<(), HalError> {
    #[cfg(target_arch = "x86_64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        // Start power management
        power_mgmt::init();
        return Ok(());
    }

    #[cfg(target_arch = "aarch64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        power_mgmt::init();
        return Ok(());
    }

    #[cfg(target_arch = "riscv64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        power_mgmt::init();
        return Ok(());
    }
}

/// HAL-specific error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    UnsupportedArchitecture,
    HardwareNotSupported,
    InitializationFailed,
    InvalidConfiguration,
}

/// Return power management interface
pub fn power_interfaces() -> Option<PowerManager> {
    power_mgmt::power_manager()
}