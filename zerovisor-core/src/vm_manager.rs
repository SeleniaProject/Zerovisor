//! Basic VM manager skeleton (Task 6.1)
//! Provides simple lifecycle API wrappers around HAL virtualization engine.

extern crate alloc;

use alloc::collections::BTreeMap;
use spin::Mutex;
use zerovisor_hal::{VirtualizationEngine, HalError};
use zerovisor_hal::cpu::CpuFeatures;
use zerovisor_hal::virtualization::{VmHandle, VmConfig, VcpuHandle, VcpuConfig, VmExitAction};
use crate::scheduler::{self, register_vcpu, pick_next, quantum_expired, SchedEntity};
use crate::{log, monitor};
use crate::console;
use crate::security::{self, SecurityEvent};
use crate::zero_copy::ZeroCopyBuffer;
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
}

impl<E: VirtualizationEngine<Error = HalError> + Send + Sync + 'static> VmManager<E> {
    pub fn new(engine: E) -> Self {
        Self { engine: Mutex::new(engine), states: Mutex::new(BTreeMap::new()) }
    }

    pub fn create_vm(&self, cfg: &VmConfig) -> Result<VmHandle, HalError> {
        let mut eng = self.engine.lock();
        let handle = eng.create_vm(cfg)?;
        // Configure nested paging/EPT for the new VM
        eng.setup_nested_paging(handle)?;
        self.states.lock().insert(handle, VmState::Created);
        Ok(handle)
    }

    pub fn start_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        // placeholder: create one vcpu and run
        let mut eng = self.engine.lock();
        let vcpu_cfg = VcpuConfig {
            id: 0,
            initial_state: eng.get_vcpu_state(0).unwrap_or_default(),
            exposed_features: CpuFeatures::empty(),
            real_time_priority: None,
        };
        let vcpu = eng.create_vcpu(vm, &vcpu_cfg)?;
        // スケジューラへ登録 (デフォルト優先度 128)。
        register_vcpu(vm, vcpu, 128, None);
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
                            let mut eng = self.engine.lock();
                            eng.handle_vm_exit(vcpu, reason)
                        };
                        // record latency (placeholder 0 for now, need to compute)
                        monitor::record_vmexit(latency_ns);
                        monitor::record_wcet(latency_ns);
                        // Update per-VCPU execution statistics for WCET analysis.
                        scheduler::record_exec_time(entity, latency_ns);

                        match action {
                            Ok(VmExitAction::Continue) => quantum_expired(entity),
                            Ok(VmExitAction::Shutdown) | Ok(VmExitAction::Reset) | Ok(VmExitAction::Suspend) => {
                                // TODO: 状態更新
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
        self.states.lock().insert(vm, VmState::Stopped);
    }

    pub fn destroy_vm(&self, vm: VmHandle) -> Result<(), HalError> {
        let mut eng = self.engine.lock();
        eng.destroy_vm(vm)?;
        self.states.lock().insert(vm, VmState::Destroyed);
        Ok(())
    }
} 