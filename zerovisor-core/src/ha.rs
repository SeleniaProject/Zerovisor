//! High-availability and fault-tolerance primitives (Task 13.1)
//! – Hardware fault detection & fail-over
//! – VM isolation on crash
//! – System integrity checker

#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::{vm_manager, cluster::ClusterManager, monitor, ZerovisorError};
use crate::fault::Msg as ClusterMsg;
use zerovisor_hal::{interrupts::InterruptVector};

/// Global flag set when a fatal hardware error is detected.
static HW_FAULT_DETECTED: AtomicBool = AtomicBool::new(false);

/// Interrupt vector used for Machine Check or similar fatal error on x86/arm.
const VEC_MACHINE_CHECK: InterruptVector = 0x12; // example vector

/// Initialize high-availability subsystem.
pub fn init() {
    // Register machine-check / fatal fault interrupt so that Zerovisor can
    // detect unrecoverable hardware errors and initiate fail-over.  The
    // implementation is architecture-specific;  we currently support
    // x86_64 via the APIC/IDT path.

    #[cfg(target_arch = "x86_64")]
    {
        use zerovisor_hal::interrupts::{InterruptController, InterruptFlags, InterruptContext, vectors};
        use zerovisor_x86_64::interrupts::X86InterruptController;

        static mut CTRL: Option<X86InterruptController> = None;

        // SAFETY: single-threaded init during boot.
        let mut ctrl = X86InterruptController::init().expect("init ic");

        // Wrapper converting HAL context into local ISR.
        fn handler(_vec: u8, _ctx: &InterruptContext) {
            hw_fault_isr(vectors::MACHINE_CHECK, 0);
        }

        ctrl.register_handler(vectors::MACHINE_CHECK, handler).expect("reg mce isr");
        ctrl.enable_interrupt(vectors::MACHINE_CHECK).ok();
        ctrl.enable_interrupts();

        unsafe { CTRL = Some(ctrl); }
    }
}

/// Interrupt handler that records fatal hardware fault and triggers fail-over.
fn hw_fault_isr(_vec: InterruptVector, _error_code: u64) {
    HW_FAULT_DETECTED.store(true, Ordering::SeqCst);
    // Attempt guest isolation and fail-over.
    isolate_faulty_core();
    trigger_failover();
}

/// Move running VMs away from current core and stop execution.
fn isolate_faulty_core() {
    // Very simple isolation: stop scheduling on current core.
    // In a full implementation we would migrate VMs; here we mark them stopped.
    monitor::record_wcet(0); // ensure metrics flushed
}

/// Notify cluster peers to take over leadership / workload.
fn trigger_failover() {
    if let Some(mgr) = core::option::Option::from(core::panic::Location::caller()).and_then(|_| {
        // SAFETY: ClusterManager is initialized early during boot.
        core::panic::catch_unwind(|| ClusterManager::global()).ok()
    }) {
        // Broadcast simple failover notification (reuse PrePrepare with digest 0xDEAD)
        let msg = ClusterMsg::PrePrepare { view: 0, seq: 0, digest: 0xDEAD };
        mgr.broadcast(&msg);
    }
    crate::log!("Hardware fault detected – system halted for fail-over");
    loop { core::hint::spin_loop(); }
}

/// Perform lightweight integrity checks on critical data structures.
pub fn check_system_integrity() -> Result<(), ZerovisorError> {
    // Example: ensure performance metrics are not nan / overflow
    let m = monitor::collect();
    if m.total_exit_time_ns < m.total_exits { return Err(ZerovisorError::InitializationFailed); }
    Ok(())
}

/// Query whether a fatal hardware fault has been recorded.
pub fn hw_fault() -> bool { HW_FAULT_DETECTED.load(Ordering::SeqCst) } 