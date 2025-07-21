//! VmxManager – High-level VMX control and VM lifecycle manager
//!
//! This module provides a *thin* wrapper around the fully-featured
//! `VmxEngine` implementation living in the HAL crate.  Its purpose is to
//! expose the operations defined in the high-level design (`design.md`) while
//! delegating the low-level intricacies to the engine.  All public functions
//! are **lock-free** from the caller’s perspective and thread-safe internally
//! by using a `spin::Mutex`.

#![cfg(target_arch = "x86_64")]

use spin::Mutex;
use zerovisor_hal::virtualization::arch::vmx::{VmxEngine, VmxError};
use zerovisor_hal::virtualization::{VmConfig, VmHandle, VcpuConfig, VcpuHandle, VmExitReason, VmExitAction};
use zerovisor_hal::cpu::CpuState;

/// Global VMX manager responsible for VMCS pooling & VM lifecycle
pub struct VmxManager {
    engine: Mutex<VmxEngine>,
}

impl VmxManager {
    /// Construct a new `VmxManager` and initialise the underlying `VmxEngine`.
    pub fn new() -> Result<Self, VmxError> {
        let engine = VmxEngine::init()?;
        Ok(Self { engine: Mutex::new(engine) })
    }

    /// Enable VMX operation (already done by BootManager on BSP but required
    /// for APs). Safe to call multiple times.
    pub fn enable_vmx(&self) -> Result<(), VmxError> {
        self.engine.lock().enable()
    }

    /// Disable VMX operation (does not tear down existing VMs).
    pub fn disable_vmx(&self) -> Result<(), VmxError> {
        self.engine.lock().disable()
    }

    /// Create a new virtual machine and return its handle.
    pub fn create_vm(&self, config: &VmConfig) -> Result<VmHandle, VmxError> {
        self.engine.lock().create_vm(config)
    }

    /// Destroy an existing VM.
    pub fn destroy_vm(&self, vm: VmHandle) -> Result<(), VmxError> {
        self.engine.lock().destroy_vm(vm)
    }

    /// Create a new virtual CPU belonging to `vm`.
    pub fn create_vcpu(&self, vm: VmHandle, cfg: &VcpuConfig) -> Result<VcpuHandle, VmxError> {
        self.engine.lock().create_vcpu(vm, cfg)
    }

    /// Launch or resume execution of the given VCPU.
    pub fn run_vcpu(&self, vcpu: VcpuHandle) -> Result<VmExitReason, VmxError> {
        self.engine.lock().run_vcpu(vcpu)
    }

    /// Handle a VM exit and decide how to proceed.
    pub fn handle_vmexit(&self, vcpu: VcpuHandle, reason: VmExitReason) -> Result<VmExitAction, VmxError> {
        self.engine.lock().handle_vm_exit(vcpu, reason)
    }

    /// Retrieve guest CPU state snapshot.
    pub fn get_vcpu_state(&self, vcpu: VcpuHandle) -> Result<CpuState, VmxError> {
        self.engine.lock().get_vcpu_state(vcpu)
    }

    /// Restore guest CPU state.
    pub fn set_vcpu_state(&self, vcpu: VcpuHandle, state: &CpuState) -> Result<(), VmxError> {
        self.engine.lock().set_vcpu_state(vcpu, state)
    }
} 