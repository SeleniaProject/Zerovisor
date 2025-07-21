
// arch/mod.rs - Architecture specific implementations for Zerovisor HAL
// Provides per-architecture modules and re-exports common aliases so that
// higher-level code can remain architecture-independent. All supported
// architectures MUST add their re-exports here.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod arm64;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "aarch64")]
pub use arm64::{ArmCpu, Stage2Manager};

#[cfg(target_arch = "riscv64")]
pub use riscv64::{RiscVCpu, Stage2Manager};

// For x86_64 export aliases similar to other architectures so that generic
// code can simply use `crate::arch::Stage2Manager`.

#[cfg(target_arch = "x86_64")]
pub use x86_64::Stage2Manager; 