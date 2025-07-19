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

// Re-export core traits
pub use cpu::{Cpu, CpuFeatures, CpuState};
pub use memory::{MemoryManager, PhysicalAddress, VirtualAddress};
pub use interrupts::{InterruptController, InterruptHandler};
pub use timer::{Timer, TimerCallback};
pub use virtualization::{VirtualizationEngine, VmHandle, VmConfig};
pub use gpu::{GpuVirtualization, GpuDeviceId, GpuConfig, GpuError};

// Re-export architecture specific CPU implementations when available
#[cfg(target_arch = "x86_64")]
pub use arch::x86_64::X86Cpu as ArchCpu;
// ARM64 and RISC-V re-exports will follow

/// Initialize the HAL for the current architecture
pub fn init() -> Result<(), HalError> {
    #[cfg(target_arch = "x86_64")]
    {
        let _cpu = ArchCpu::init().map_err(|_| HalError::HardwareNotSupported)?;
        return Ok(());
    }

    #[cfg(target_arch = "aarch64")]
    {
        // TODO: ARM64 initialization will be added soon
        return Err(HalError::UnsupportedArchitecture);
    }

    #[cfg(target_arch = "riscv64")]
    {
        return Err(HalError::UnsupportedArchitecture);
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