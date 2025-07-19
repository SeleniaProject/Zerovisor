//! Basic VM manager skeleton (Task 6.1)
//! Provides simple lifecycle API wrappers around HAL virtualization engine.

use alloc::collections::BTreeMap;
use spin::Mutex;
use zerovisor_hal::{VirtualizationEngine, HalError};
use zerovisor_hal::virtualization::{VmHandle, VmConfig, VcpuHandle, VcpuConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Created,
    Running,
    Stopped,
    Destroyed,
}

pub struct VmManager<E: VirtualizationEngine + Send + Sync + 'static> {
    engine: Mutex<E>,
    states: Mutex<BTreeMap<VmHandle, VmState>>,
}

impl<E: VirtualizationEngine<Error = HalError> + Send + Sync + 'static> VmManager<E> {
    pub fn new(engine: E) -> Self {
        Self { engine: Mutex::new(engine), states: Mutex::new(BTreeMap::new()) }
    }

    pub fn create_vm(&self, cfg: &VmConfig) -> Result<VmHandle, HalError> {
        let mut eng = self.engine.lock();
        let handle = eng.create_vm(cfg)?;
        self.states.lock().insert(handle, VmState::Created);
        Ok(handle)
    }

    pub fn start_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        // placeholder: create one vcpu and run
        let mut eng = self.engine.lock();
        let vcpu_cfg = VcpuConfig {
            id: 0,
            initial_state: eng.get_vcpu_state(0).unwrap_or_default(),
            exposed_features: eng.get_vcpu_state(0).unwrap_or_default().flags.into(),
            real_time_priority: None,
        };
        let vcpu = eng.create_vcpu(vm, &vcpu_cfg)?;
        eng.run_vcpu(vcpu)?;
        self.states.lock().insert(vm, VmState::Running);
        Ok(())
    }

    pub fn stop_vm(&self, vm: VmHandle) {
        self.states.lock().insert(vm, VmState::Stopped);
    }

    pub fn destroy_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        let mut eng = self.engine.lock();
        eng.destroy_vm(vm)?;
        self.states.lock().insert(vm, VmState::Destroyed);
        Ok(())
    }
} 