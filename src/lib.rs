#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Zerovisor - World-class Type-1 Hypervisor written in Rust
//! 
//! This is the main entry point for the Zerovisor hypervisor, providing
//! a unified interface across different architectures (x86_64, ARM64, RISC-V).

pub use zerovisor_core::*;
pub use zerovisor_hal as hal;

/// Re-export architecture-specific modules
#[cfg(target_arch = "x86_64")]
pub use zerovisor_hal as arch; // x86_64 specific functionality exposed via HAL

#[cfg(target_arch = "aarch64")]
pub use zerovisor_hal as arch; // ARM64 placeholder

#[cfg(target_arch = "riscv64")]
pub use zerovisor_hal as arch; // RISC-V placeholder

/// Initialize Zerovisor hypervisor
pub fn init() -> Result<(), ZerovisorError> {
    zerovisor_core::init()
}