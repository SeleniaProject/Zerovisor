#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Hardware Abstraction Layer for Zerovisor Hypervisor
//! 
//! This crate provides a unified interface for different CPU architectures,
//! enabling the hypervisor to run on x86_64, ARM64, and RISC-V platforms.

pub mod cpu;
pub mod memory;
pub mod interrupts;
pub mod timer;
pub mod virtualization;
pub mod cycles;
pub mod arch; // New architecture-specific module tree
pub mod gpu;
pub mod nic;
pub mod power;
pub mod accelerator;
pub mod rdma_opt;
pub mod rdma_vnet;
pub mod iommu;

// Re-export core traits
pub use cpu::{Cpu, CpuFeatures, CpuState};
pub use memory::{MemoryManager, PhysicalAddress, VirtualAddress};
pub use interrupts::{InterruptController, InterruptHandler};
pub use timer::{Timer, TimerCallback};
pub use virtualization::{VirtualizationEngine, VmHandle, VmConfig};
pub use gpu::{GpuVirtualization, GpuDeviceId, GpuConfig, GpuError};
pub use accelerator::{AcceleratorVirtualization, AcceleratorId, AcceleratorInfo, AcceleratorType, AccelError};
pub use nic::{HpcNic, NicAttr, NicError, RdmaOpKind, RdmaCompletion};
pub use power::{DvfsController, ThermalSensor, PState, Temperature, PowerError};

// Re-export architecture specific CPU implementations when available
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::X86Cpu as ArchCpu;

#[cfg(target_arch = "aarch64")]
pub use arch::arm64::ArmCpu as ArchCpu;

#[cfg(target_arch = "riscv64")]
pub use arch::riscv64::RiscVCpu as ArchCpu;

#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::iommu::VtdEngine as ArchIommu;

/// Initialize the HAL for the current architecture
pub fn init() -> Result<(), HalError> {
    #[cfg(target_arch = "x86_64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        return Ok(());
    }

    #[cfg(target_arch = "aarch64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        return Ok(());
    }

    #[cfg(target_arch = "riscv64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
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

/// Return architecture-specific DVFS + thermal sensor interfaces when available.
pub fn power_interfaces() -> Option<(&'static dyn DvfsController, &'static dyn ThermalSensor)> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some(arch::x86_64::power::interfaces());
    }
    #[allow(unreachable_code)]
    None
}