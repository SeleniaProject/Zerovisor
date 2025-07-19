#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! x86_64 architecture support for Zerovisor hypervisor

pub mod cpu;
pub mod memory;
pub mod interrupts;
pub mod timer;
pub mod virtualization;

use zerovisor_hal::HalError;

/// Initialize x86_64 architecture support
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

/// x86_64 specific error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X86Error {
    UnsupportedCpu,
    VmxNotSupported,
    SvmNotSupported,
    InvalidMsr,
    InvalidCpuid,
    MemoryError,
    InterruptError,
}

impl From<X86Error> for HalError {
    fn from(err: X86Error) -> Self {
        match err {
            X86Error::UnsupportedCpu => HalError::HardwareNotSupported,
            X86Error::VmxNotSupported => HalError::HardwareNotSupported,
            X86Error::SvmNotSupported => HalError::HardwareNotSupported,
            _ => HalError::InitializationFailed,
        }
    }
}