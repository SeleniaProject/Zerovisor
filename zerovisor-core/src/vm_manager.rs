//! Basic VM manager skeleton (Task 6.1)
//! Provides simple lifecycle API wrappers around HAL virtualization engine.

extern crate alloc;

use alloc::collections::BTreeMap;
use spin::Mutex;
use zerovisor_hal::{VirtualizationEngine, HalError};
use zerovisor_hal::cpu::CpuFeatures;
use zerovisor_hal::virtualization::{VmHandle, VmConfig, VcpuHandle, VcpuConfig, VmExitAction};
use crate::scheduler::{self, register_vcpu, pick_next, quantum_expired, SchedEntity};
use crate::scheduler::remove_vm as sched_remove_vm;
use crate::{log, monitor};
use crate::console;
use crate::security::{self, SecurityEvent};
use crate::zero_copy::ZeroCopyBuffer;
use crate::numa_optimizer;
use crate::distributed_hypervisor;
// logging macro is imported via crate root

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
    vcpu_counts: Mutex<BTreeMap<VmHandle, u32>>, // track allocated vcpus per VM
}

impl<E: VirtualizationEngine<Error = HalError> + Send + Sync + 'static> VmManager<E> {
    pub fn new(engine: E) -> Self {
        Self { engine: Mutex::new(engine), states: Mutex::new(BTreeMap::new()), vcpu_counts: Mutex::new(BTreeMap::new()) }
    }

    pub fn create_vm(&self, cfg: &VmConfig) -> Result<VmHandle, HalError> {
        let mut eng = self.engine.lock();
        let handle = eng.create_vm(cfg)?;
        // Configure nested paging/EPT for the new VM
        eng.setup_nested_paging(handle)?;
        let node = numa_optimizer::optimizer().optimize_vm_placement(cfg);
        log!("VM {} placed on NUMA node {}", handle, node);
        self.states.lock().insert(handle, VmState::Created);
        self.vcpu_counts.lock().insert(handle, 0);

        // Register VM globally for distributed placement accounting
        distributed_hypervisor::register_vm(handle as u32);
        Ok(handle)
    }

    pub fn start_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        let mut eng = self.engine.lock();
        // Determine requested CPU count from VM config (fallback 1)
        let cfg = VmConfig { id: vm, ..Default::default() }; // placeholder to get count; real path would cache config
        let count = cfg.cpu_count.max(1).min(64);
        for cpu_id in 0..count {
            let vcpu_cfg = VcpuConfig {
                id: cpu_id,
                initial_state: eng.get_vcpu_state(cpu_id).unwrap_or_default(),
                exposed_features: CpuFeatures::empty(),
                real_time_priority: None,
            };
            let vcpu = eng.create_vcpu(vm, &vcpu_cfg)?;
            register_vcpu(vm, vcpu, 128, None);
        }
        self.vcpu_counts.lock().insert(vm, count);
        monitor::vm_started();

        // Map a zero-copy shared buffer to the new VM (demo for Task 14.1).
        static mut SHARED_PAGE: [u8; 4096] = [0u8; 4096];
        let phys = unsafe { &SHARED_PAGE as *const _ as u64 };
        let virt = phys;
        let zbuf = ZeroCopyBuffer::new(phys, virt, 4096);
        if let Err(e) = zbuf.share_with_guest(&mut *eng, vm, 0x1000_0000) {
            log!("zero-copy buffer mapping failed {:?}", e);
        } else {
            monitor::add_shared_pages(1);
        }

        self.states.lock().insert(vm, VmState::Running);
        Ok(())
    }

    /// システム全体のスケジューラループを駆動（ブート CPU で呼び出し）。
    pub fn run(&self) -> ! {
        loop {
            if let Some(entity) = pick_next() {
                // handle management console input
                console::poll();
                crate::debug_interface::poll();
                crate::monitoring_engine::tick();
                crate::cluster_runtime::tick();
                let _vm = entity.vm;
                let vcpu = entity.vcpu;
                // 実行
                let start_cycle = crate::scheduler::get_cycle_counter();
                let exit_result = {
                    let mut eng = self.engine.lock();
                    eng.run_vcpu(vcpu)
                };
                let end_cycle = crate::scheduler::get_cycle_counter();
                let latency_ns = crate::scheduler::cycles_to_nanoseconds(end_cycle - start_cycle);

                match exit_result {
                    Ok(reason) => {
                        crate::log!("VMEXIT reason {:?} on VCPU {}", reason, vcpu);
                        let action = {
                            if let Some(a) = crate::plugin_manager::global().handle_vmexit(&reason) {
                                Ok(a)
                            } else {
                                let mut eng = self.engine.lock();
                                eng.handle_vm_exit(vcpu, reason)
                            }
                        };
                        monitor::record_vmexit(latency_ns);
                        crate::vm::record_vmexit(entity.vm, &reason, latency_ns);
                        monitor::record_wcet(latency_ns);
                        // Update per-VCPU execution statistics for WCET analysis.
                        scheduler::record_exec_time(entity, latency_ns);

                        match action {
                            Ok(VmExitAction::Continue) => quantum_expired(entity),
                            Ok(VmExitAction::Shutdown) => {
                                // Graceful shutdown: mark stopped and purge scheduler entries.
                                self.stop_vm(entity.vm);
                                sched_remove_vm(entity.vm);
                                // No re-queue – VM is halted.
                            }
                            Ok(VmExitAction::Reset) => {
                                // Reset maps to destroy + recreate path. Here we destroy resources.
                                if let Err(e) = self.destroy_vm(entity.vm) {
                                    log!("VM reset failed during destroy: {:?}", e);
                                }
                                sched_remove_vm(entity.vm);
                            }
                            Ok(VmExitAction::Suspend) => {
                                // Suspend keeps VM memory but stops scheduling.
                                self.stop_vm(entity.vm);
                                sched_remove_vm(entity.vm);
                            }
                            _ => quantum_expired(entity),
                        }

                        // Security event logging for EPT violations
                        if let zerovisor_hal::virtualization::VmExitReason::NestedPageFault { guest_phys, guest_virt, error_code } = reason {
                            security::record_event(SecurityEvent::EptViolation {
                                guest_pa: guest_phys,
                                guest_va: guest_virt,
                                error: error_code,
                            });
                        }
                    }
                    Err(_) => {
                        // ハンドルエラー後に量子終了処理
                        quantum_expired(entity);
                    }
                }
            }
        }
    }

    pub fn stop_vm(&self, vm: VmHandle) {
        // Remove from scheduler to prevent further execution.
        sched_remove_vm(vm);
        self.states.lock().insert(vm, VmState::Stopped);
    }

    /// Resume a previously stopped VM (e.g., after suspend).
    pub fn resume_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        if let Some(VmState::Stopped) = self.states.lock().get(&vm).copied() {
            // For simplicity we resume only VCPU 0; real implementation would restore all.
            let mut eng = self.engine.lock();
            let state = eng.get_vcpu_state(0).unwrap_or_default();
            eng.set_vcpu_state(0, &state)?; // ensure state is valid
            let vcpu = 0;
            register_vcpu(vm, vcpu, 128, None);
            self.states.lock().insert(vm, VmState::Running);
        }
        Ok(())
    }

    /// Query current state of a VM.
    pub fn state_of(&self, vm: VmHandle) -> Option<VmState> {
        self.states.lock().get(&vm).copied()
    }

    pub fn destroy_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        let mut eng = self.engine.lock();
        eng.destroy_vm(vm)?;
        self.states.lock().insert(vm, VmState::Destroyed);
        distributed_hypervisor::unregister_vm(vm as u32);
        Ok(())
    }

    /// Forcefully isolate a VM after security violation.
    pub fn isolate_vm(&self, vm: VmHandle) -> Result<(), ()> {
        if let Some(state) = self.states.lock().get(&vm).copied() {
            if state == VmState::Running {
                self.stop_vm(vm);
                // Release devices (GPU/NIC) – best effort
                if let Some(handles) = crate::gpu::list_assigned(vm).into_iter().collect::<Vec<_>>().first().cloned() { let _ = crate::gpu::release_gpu(vm, handles); }
                // Broadcast isolation event
                crate::cluster::ClusterManager::global().broadcast(&crate::fault::Msg::IsolateVm { vm });
            }
        }
        Ok(())
    }
} 