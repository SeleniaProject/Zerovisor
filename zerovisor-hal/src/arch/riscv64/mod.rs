//! RISC-V64 architecture support for Zerovisor HAL
#![cfg(target_arch = "riscv64")]

pub mod cpu;

pub use cpu::RiscVCpu;

pub mod ept_manager;

pub use ept_manager::EptHierarchy as Stage2Manager; 