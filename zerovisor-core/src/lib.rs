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
pub mod log;
pub mod monitor;
pub mod boot_manager;
pub mod vm_manager;
pub mod api;
pub mod console;
pub mod gpu;
pub mod crypto;
pub mod crypto_mem;
pub mod attestation;
pub mod microvm;
pub mod accelerator;
pub mod cluster;
pub mod fault;
pub mod energy;
pub mod kube_runtime;
#[cfg(feature = "coq_proofs")]
pub mod proofs_stub;

use zerovisor_hal::{HalError, init as hal_init};
use security::init as security_init;
use accelerator::init as accelerator_init;

/// Initialize the Zerovisor hypervisor
pub fn init() -> Result<(), ZerovisorError> {
    // Initialize the Hardware Abstraction Layer
    hal_init().map_err(ZerovisorError::HalError)?;

    // Initialize quantum-resistant security engine
    security_init().map_err(|_| ZerovisorError::SecurityInitializationFailed)?;
    
    // Initialize core hypervisor components
    hypervisor::init()?;

    // Initialize GPU virtualization subsystem
    gpu::init()?;
    
    accelerator_init()?;

    Ok(())
}

/// Initialize Zerovisor using the firmware-provided memory map
pub fn init_with_memory_map(memory_map: &[zerovisor_hal::memory::MemoryRegion]) -> Result<(), ZerovisorError> {
    // Initialize the Hardware Abstraction Layer (idempotent)
    hal_init().map_err(ZerovisorError::HalError)?;

    // Initialize hypervisor with actual memory map
    hypervisor::init_with_map(memory_map)?;

    security_init().map_err(|_| ZerovisorError::SecurityInitializationFailed)?;

    accelerator_init()?;

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
    SecurityInitializationFailed,
    AcceleratorInitializationFailed,
}

impl From<HalError> for ZerovisorError {
    fn from(err: HalError) -> Self {
        ZerovisorError::HalError(err)
    }
}