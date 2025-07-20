//! Live migration framework (Task 13.2)
//! Provides zero-downtime VM migration across nodes using pre-copy algorithm.
//! Current implementation is a stub that serializes CPU state and guest memory
//! incrementally; memory copy hooks will be filled once paging iterator is ready.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use crate::{cluster::{ClusterManager, NodeId}, monitor, ZerovisorError};
use zerovisor_hal::virtualization::{VmHandle, VcpuHandle, VirtualizationEngine, VmStats};

/// Migration phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase { PreCopy, StopAndCopy, Resume }

/// Migration state for a single VM.
pub struct MigrationCtx<'a, E: VirtualizationEngine + Send + Sync> {
    pub vm: VmHandle,
    pub vcpus: &'a [VcpuHandle],
    pub phase: Phase,
    pub dirty_pages: usize,
    pub snapshot: Vec<u8>,
    engine: &'a Mutex<E>,
}

impl<'a, E: VirtualizationEngine + Send + Sync> MigrationCtx<'a, E> {
    /// Create a new migration context capturing CPU state only (fast path).
    pub fn new(vm: VmHandle, vcpus: &'a [VcpuHandle], engine: &'a Mutex<E>) -> Self {
        Self { vm, vcpus, phase: Phase::PreCopy, dirty_pages: 0, snapshot: Vec::new(), engine }
    }

    /// Serialize CPU registers for all VCPUs into `snapshot` buffer.
    pub fn capture_cpu_state(&mut self) -> Result<(), E::Error> {
        for vcpu in self.vcpus {
            let state = self.engine.lock().get_vcpu_state(*vcpu)?;
            let bytes = unsafe { core::slice::from_raw_parts((&state as *const _ as *const u8), core::mem::size_of_val(&state)) };
            self.snapshot.extend_from_slice(bytes);
        }
        Ok(())
    }

    /// Perform iterative pre-copy of guest memory.  Because the HAL paging
    /// iterator is not yet available, we approximate by transferring a fixed
    /// chunk count and assume dirties converge.
    pub fn pre_copy_memory(&mut self, rounds: usize, bytes_per_round: usize) -> Result<(), E::Error> {
        for _ in 0..rounds {
            // Allocate a dummy buffer to simulate dirty pages.
            let dummy = [0u8; 4096];
            for _ in 0..bytes_per_round / dummy.len() {
                self.snapshot.extend_from_slice(&dummy);
            }
            // Fake convergence criteria.
            self.dirty_pages = self.dirty_pages.saturating_sub(bytes_per_round / 4096);
            if self.dirty_pages <= 16 { break; }
        }
        Ok(())
    }
}

/// Start live migration to `dest` node.
pub fn migrate_vm<E: VirtualizationEngine + Send + Sync + 'static>(
    mgr: &ClusterManager<'static>,
    engine: &Mutex<E>,
    vm: VmHandle,
    vcpus: &[VcpuHandle],
    dest: NodeId,
) -> Result<(), ZerovisorError> {
    // Phase 1: capture CPU state
    let mut ctx = MigrationCtx::<'_, E>::new(vm, vcpus, engine);
    ctx.capture_cpu_state().map_err(|_| ZerovisorError::ResourceExhausted)?;

    // Phase 2: iterative pre-copy of memory (simplified)
    ctx.dirty_pages = 512; // assume 512 pages dirty initially (demo)
    ctx.pre_copy_memory(4, 64 * 1024).map_err(|_| ZerovisorError::ResourceExhausted)?;

    // Phase 3: stop-the-world copy – here we would pause the VM and copy the
    // last set of dirty pages; we skip memory transfer and only send CPU state.

    let buf = &ctx.snapshot;
    mgr.transport.send(dest, buf).map_err(|_| ZerovisorError::InitializationFailed)?;

    monitor::add_shared_pages( (buf.len() as u64 + 4095) / 4096 );

    Ok(())
}

/// Handle received migration payload on destination node.
pub fn receive_vm_payload(_buf: &[u8]) {
    // TODO: reconstruct VM and VCPU state, allocate memory pages.
} 