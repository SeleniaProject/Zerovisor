//! Live migration framework (Task 13.2)
//! Provides zero-downtime VM migration across nodes using pre-copy algorithm.
//! Current implementation is a stub that serializes CPU state and guest memory
//! incrementally; memory copy hooks will be filled once paging iterator is ready.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::time::Duration;
use core::time::Instant;
use spin::Mutex;
use core::cmp::min;

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

    /// Iterate pre-copy until dirty pages converge below threshold or round limit.
    pub fn pre_copy_memory(&mut self, mut bytes_per_round: usize) -> Result<(), E::Error> {
        for round in 0..MAX_PRECOPY_ROUNDS {
            if self.dirty_pages <= DIRTY_THRESHOLD_PAGES { break; }

            // Simulate copying `bytes_per_round` from dirty set.
            let dummy = [0u8; 4096];
            for _ in 0..bytes_per_round / dummy.len() {
                self.snapshot.extend_from_slice(&dummy);
            }

            // Assume 50% of remaining pages dirtied again each round (worst-case guest writes).
            self.dirty_pages = (self.dirty_pages / 2).max(DIRTY_THRESHOLD_PAGES);

            // Exponential back-off to bound total traffic.
            bytes_per_round = bytes_per_round.saturating_mul(2).min(8 * 1024 * 1024);

            crate::log!("[migration] pre-copy round {} done, remaining dirty pages {}", round + 1, self.dirty_pages);
        }
        Ok(())
    }

    /// Stream snapshot via ClusterManager in fixed-size chunks.
    pub fn stream_snapshot(&self, mgr: &ClusterManager, dest: NodeId) -> Result<(), ZerovisorError> {
        let buf = &self.snapshot;
        let mut offset = 0;
        while offset < buf.len() {
            let end = min(offset + CHUNK_SIZE, buf.len());
            mgr.transport.send(dest, &buf[offset..end]).map_err(|_| ZerovisorError::InitializationFailed)?;
            offset = end;
        }
        Ok(())
    }
}

/// Maximum tolerated downtime (ns) for stop-and-copy phase.
const MAX_DOWNTIME_NS: u64 = 10_000_000; // 10 ms
/// Page convergence threshold – stop precopy when remaining dirty pages below this number.
const DIRTY_THRESHOLD_PAGES: usize = 16;
/// Maximum number of pre-copy rounds to avoid livelock.
const MAX_PRECOPY_ROUNDS: usize = 10;
/// Chunk size (bytes) streamed over RDMA transport per send.
const CHUNK_SIZE: usize = 128 * 1024; // 128 KiB

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
    ctx.pre_copy_memory(64 * 1024).map_err(|_| ZerovisorError::ResourceExhausted)?;

    // If convergence failed, we still proceed but record downtime expectation.
    let stop_begin = Instant::now();

    // Phase 3: stop-and-copy – pause the VM & copy remaining dirty pages.
    // TODO: invoke engine.pause_vm when available. For now we assume fixed 1ms.
    core::hint::spin_loop();

    // Simulate copy of remaining pages (delta).
    if ctx.dirty_pages > 0 {
        let delta_bytes = ctx.dirty_pages * 4096;
        ctx.snapshot.extend(core::iter::repeat(0u8).take(delta_bytes));
    }

    let downtime_ns = stop_begin.elapsed().as_nanos() as u64;
    if downtime_ns > MAX_DOWNTIME_NS {
        crate::log!("[migration] warning: downtime {} ns exceeds target", downtime_ns);
    }

    // Phase 4: transmit snapshot
    ctx.stream_snapshot(mgr, dest)?;

    // Phase 5: send explicit DONE marker (zero-length packet)
    mgr.transport.send(dest, &[]).map_err(|_| ZerovisorError::InitializationFailed)?;

    monitor::add_shared_pages( (ctx.snapshot.len() as u64 + 4095) / 4096 );

    Ok(())
}

/// Destination-side handler to reconstruct VM. Called when zero-length packet received.
pub fn receive_vm_payload(buf: &[u8]) {
    if buf.is_empty() {
        // End-of-migration marker; actual reconstruction logic should have collected
        // chunks in higher layer. Placeholder only.
        crate::log!("Migration payload fully received – TODO: reconstruct VM state");
    }
} 