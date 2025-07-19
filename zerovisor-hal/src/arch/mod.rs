
// arch/mod.rs - Architecture specific implementations for Zerovisor HAL
// Only x86_64 is fully implemented at this time. Other architectures will follow.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod arm64; // Placeholder for future ARM64 implementation

#[cfg(target_arch = "riscv64")]
pub mod riscv; // Placeholder for future RISC-V implementation 