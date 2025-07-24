#![cfg(target_arch = "aarch64")]
#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::virtualization::VirtualizationEngine;
use zerovisor_hal::virtualization::VmConfig;
use zerovisor_hal::virtualization::VmHandle;
use zerovisor_hal::HalError;

/// ARM64 virtualization engine placeholder using nested stage-2 page tables.
pub struct ArmVheEngine;

impl VirtualizationEngine for ArmVheEngine {
    type Error = HalError;
    fn init() -> Result<Self, Self::Error> { Ok(Self) }
    fn is_supported() -> bool { true }
    fn enable(&mut self) -> Result<(), Self::Error> { Ok(()) }
    fn disable(&mut self) -> Result<(), Self::Error> { Ok(()) }
    fn create_vm(&mut self, _cfg: &VmConfig) -> Result<VmHandle, Self::Error> { Ok(1) }
    fn destroy_vm(&mut self, _vm: VmHandle) -> Result<(), Self::Error> { Ok(()) }
} 