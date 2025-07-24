#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! ARM64 architecture support for Zerovisor hypervisor

#[cfg(target_arch = "aarch64")]
use zerovisor_hal::HalError;

#[cfg(target_arch = "aarch64")]
pub mod cpu;
#[cfg(target_arch = "aarch64")]
pub mod memory;
#[cfg(target_arch = "aarch64")]
pub mod interrupts;
#[cfg(target_arch = "aarch64")]
pub mod timer;
#[cfg(target_arch = "aarch64")]
pub mod virtualization;

// On non-aarch64 targets this crate compiles to empty stubs so the workspace builds on every host.

#[cfg(target_arch = "aarch64")]
/// Initialize ARM64 architecture support
pub fn init() -> Result<(), HalError> {
    // Check for required CPU features
    if !cpu::has_virtualization_support() {
        return Err(HalError::HardwareNotSupported);
    }
    
    // Initialize CPU
    cpu::init()?;
    
    // Initialize memory management
    memory::init()?;
    
    // Initialize interrupt controller
    interrupts::init()?;
    
    // Initialize timer subsystem
    timer::init()?;
    
    // Initialize virtualization engine
    virtualization::init()?;
    
    Ok(())
}

#[cfg(not(target_arch = "aarch64"))]
pub fn init() -> Result<(), ()> { Ok(()) }

/// ARM64 specific error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm64Error {
    UnsupportedCpu,
    VirtualizationNotSupported,
    InvalidSystemRegister,
    MemoryError,
    InterruptError,
}

#[cfg(target_arch = "aarch64")]
impl From<Arm64Error> for HalError {
    fn from(err: Arm64Error) -> Self {
        match err {
            Arm64Error::UnsupportedCpu => HalError::HardwareNotSupported,
            Arm64Error::VirtualizationNotSupported => HalError::HardwareNotSupported,
            _ => HalError::InitializationFailed,
        }
    }
}