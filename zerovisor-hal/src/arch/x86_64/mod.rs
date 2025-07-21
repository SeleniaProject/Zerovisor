pub mod vmcs;
pub mod ept_manager;
pub mod ept;
pub mod cpu;
pub mod gpu;
pub mod pci;
pub mod vmx;
pub mod vmexit_fast;
pub mod accelerator;
pub mod nic;
pub mod power;
pub mod iommu;
pub mod storage;

pub use cpu::X86Cpu;
pub use gpu::SrIovGpuEngine;
pub use vmx::cached_cpuid;
pub use accelerator::X86AcceleratorManager;
pub use nic::InfinibandNic;
pub use power::{IntelPStateController, IntelThermalSensor};
pub use iommu::VtdEngine;
pub use storage::NvmeSrioVEngine;

// Provide unified alias so higher-level code can use `Stage2Manager` across architectures.
pub use ept_manager::EptHierarchy as Stage2Manager; 