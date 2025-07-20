//! Common test helpers and dummy implementations used across integration tests.

extern crate std;
use std::sync::atomic::{AtomicU32, Ordering};

use zerovisor_hal::virtualization::*;
use zerovisor_hal::cpu::{CpuState, CpuFeatures};
use zerovisor_hal::memory::{PhysicalAddress, VirtualAddress};
use zerovisor_hal::memory::MemoryFlags;

/// Dummy in-memory virtualization engine for tests.
pub struct DummyEngine {
    next_vm: AtomicU32,
}

impl DummyEngine {
    pub fn new() -> Self { Self { next_vm: AtomicU32::new(1) } }
}

impl VirtualizationEngine for DummyEngine {
    type Error = ();

    fn init() -> Result<Self, Self::Error> where Self: Sized { Ok(Self::new()) }

    fn is_supported() -> bool { true }

    fn enable(&mut self) -> Result<(), Self::Error> { Ok(()) }
    fn disable(&mut self) -> Result<(), Self::Error> { Ok(()) }

    fn create_vm(&mut self, _config: &VmConfig) -> Result<VmHandle, Self::Error> {
        Ok(self.next_vm.fetch_add(1, Ordering::SeqCst))
    }

    fn destroy_vm(&mut self, _vm: VmHandle) -> Result<(), Self::Error> { Ok(()) }

    fn create_vcpu(&mut self, _vm: VmHandle, _cfg: &VcpuConfig) -> Result<VcpuHandle, Self::Error> { Ok(0) }

    fn run_vcpu(&mut self, _vcpu: VcpuHandle) -> Result<VmExitReason, Self::Error> { Ok(VmExitReason::Hlt) }

    fn get_vcpu_state(&self, _vcpu: VcpuHandle) -> Result<CpuState, Self::Error> { Ok(CpuState::default()) }
    fn set_vcpu_state(&mut self, _vcpu: VcpuHandle, _state: &CpuState) -> Result<(), Self::Error> { Ok(()) }

    fn handle_vm_exit(&mut self, _vcpu: VcpuHandle, _reason: VmExitReason) -> Result<VmExitAction, Self::Error> { Ok(VmExitAction::Shutdown) }

    fn setup_nested_paging(&mut self, _vm: VmHandle) -> Result<(), Self::Error> { Ok(()) }

    fn map_guest_memory(&mut self, _vm: VmHandle, _guest_phys: PhysicalAddress, _host_phys: PhysicalAddress, _size: usize, _flags: MemoryFlags) -> Result<(), Self::Error> { Ok(()) }
    fn unmap_guest_memory(&mut self, _vm: VmHandle, _guest_phys: PhysicalAddress, _size: usize) -> Result<(), Self::Error> { Ok(()) }
    fn modify_guest_memory(&mut self, _vm: VmHandle, _guest_phys: PhysicalAddress, _size: usize, _new_flags: MemoryFlags) -> Result<(), Self::Error> { Ok(()) }
} 