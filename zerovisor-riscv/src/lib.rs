#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! RISC-V architecture support for Zerovisor hypervisor

#[cfg(target_arch = "riscv64")]
pub mod cpu;
#[cfg(target_arch = "riscv64")]
pub mod memory;
#[cfg(target_arch = "riscv64")]
pub mod interrupts;
#[cfg(target_arch = "riscv64")]
pub mod timer;
#[cfg(target_arch = "riscv64")]
pub mod virtualization;

#[cfg(target_arch = "riscv64")]
use zerovisor_hal::HalError;

#[cfg(target_arch = "riscv64")]
/// Initialize RISC-V architecture support
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

#[cfg(not(target_arch = "riscv64"))]
pub fn init() -> Result<(), ()> { Ok(()) }

/// RISC-V specific error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiscVError {
    UnsupportedCpu,
    HypervisorExtensionNotSupported,
    InvalidCsr,
    MemoryError,
    InterruptError,
}

#[cfg(target_arch = "riscv64")]
impl From<RiscVError> for HalError {
    fn from(err: RiscVError) -> Self {
        match err {
            RiscVError::UnsupportedCpu => HalError::HardwareNotSupported,
            RiscVError::HypervisorExtensionNotSupported => HalError::HardwareNotSupported,
            _ => HalError::InitializationFailed,
        }
    }
}