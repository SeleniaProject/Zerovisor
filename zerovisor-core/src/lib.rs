#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Zerovisor core hypervisor functionality
//! 
//! This crate provides the main hypervisor logic that works across
//! different architectures using the HAL (Hardware Abstraction Layer).

extern crate alloc;

pub mod hypervisor;
pub mod memory;
pub mod vm;
pub mod scheduler;
pub mod security;
pub mod boot_manager;
pub mod vm_manager;

use zerovisor_hal::{HalError, init as hal_init};

/// Initialize the Zerovisor hypervisor
pub fn init() -> Result<(), ZerovisorError> {
    // Initialize the Hardware Abstraction Layer
    hal_init().map_err(ZerovisorError::HalError)?;
    
    // Initialize core hypervisor components
    hypervisor::init()?;
    
    Ok(())
}

/// Zerovisor core error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZerovisorError {
    HalError(HalError),
    InitializationFailed,
    InvalidConfiguration,
    ResourceExhausted,
    SecurityViolation,
}

impl From<HalError> for ZerovisorError {
    fn from(err: HalError) -> Self {
        ZerovisorError::HalError(err)
    }
}