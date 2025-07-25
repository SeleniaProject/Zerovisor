//! RISC-V64 architecture support for Zerovisor HAL
#![cfg(target_arch = "riscv64")]

pub mod cpu;

pub use cpu::RiscVCpu;

pub mod ept_manager;

pub mod iommu;

pub mod power;
pub use power::{RiscvDvfsController, RiscvThermalSensor};

pub use iommu::RiscvIommuEngine as ArchIommu;

pub use ept_manager::EptHierarchy as Stage2Manager;

pub mod accelerator;
pub use accelerator::RiscvAcceleratorManager; 