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
pub mod ha;
pub mod migration;
pub mod zero_copy;
pub mod cluster;
pub mod fault;
pub mod energy;
pub mod kube_runtime;
pub mod wasm_runtime;
pub mod debug_stub;
#[cfg(feature = "coq_proofs")]
pub mod proofs_stub;
#[cfg(feature = "formal_verification")]
pub mod formal_tests;
#[cfg(any(feature = "formal_verification", feature = "coq_proofs"))]
pub mod formal;
pub mod vmx_manager;
pub mod isolation;
pub mod iommu;
pub mod numa_optimizer;
pub mod cluster_bft;

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

    // Initialize environment-adaptive power management.
    if let Some((dvfs, thermal)) = zerovisor_hal::power_interfaces() {
        energy::EnergyManager::init(dvfs, thermal);
    }

    accelerator_init()?;

    // Initialize NUMA optimizer
    numa_optimizer::init();

    // Initialize IOMMU engine for device passthrough
    iommu::init().map_err(|_| ZerovisorError::InitializationFailed)?;

    // Initialize Isolation Engine
    isolation::init();

    // Invoke formal verification checks when enabled.
    #[cfg(any(feature = "formal_verification", feature = "coq_proofs"))]
    {
        formal::run_all().map_err(|_| ZerovisorError::FormalVerificationFailed)?;
    }

    // Initialize high-availability subsystem (fault detection & fail-over)
    ha::init();

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

    // Initialize NUMA optimizer
    numa_optimizer::init();

    // Initialize IOMMU engine for device passthrough
    iommu::init().map_err(|_| ZerovisorError::InitializationFailed)?;

    // Initialize Isolation Engine
    isolation::init();

    if let Some((dvfs, thermal)) = zerovisor_hal::power_interfaces() {
        energy::EnergyManager::init(dvfs, thermal);
    }

    #[cfg(any(feature = "formal_verification", feature = "coq_proofs"))]
    {
        formal::run_all().map_err(|_| ZerovisorError::FormalVerificationFailed)?;
    }

    ha::init();

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
    #[cfg(any(feature = "formal_verification", feature = "coq_proofs"))]
    FormalVerificationFailed,
}

impl From<HalError> for ZerovisorError {
    fn from(err: HalError) -> Self {
        ZerovisorError::HalError(err)
    }
}