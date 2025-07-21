//! ARM64 architecture support for Zerovisor HAL
#![cfg(target_arch = "aarch64")]

pub mod cpu;

pub use cpu::ArmCpu;

pub mod ept_manager;

pub mod iommu;

pub use iommu::SmmuEngine as ArchIommu;

pub use ept_manager::EptHierarchy as Stage2Manager; 