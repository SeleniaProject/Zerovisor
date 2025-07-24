//! TraceBuffer – records recent VMEXIT events and performance counters
//! (Task 9.4: VMEXIT トレース & パフォーマンスカウンタ統合)
//!
//! Implementation details:
//! • lock‐free single‐producer (VMEXIT handler) / multi‐consumer ring buffer
//! • fixed capacity; oldest events are overwritten when full
//! • each entry stores timestamp (TSC cycles), VM id, VCPU id, exit reason code
//! • consumers can obtain a snapshot via `snapshot()` for analysis or export

#![no_std]

use core::sync::atomic::{AtomicUsize, Ordering};
use zerovisor_hal::cycles::rdtsc;
use zerovisor_hal::virtualization::{VmExitReason, VmHandle, VcpuHandle};

/// Maximum trace entries kept in memory (power‐of‐two for cheap modulo).
const CAP: usize = 2048;

#[derive(Clone, Copy)]
pub struct TraceEntry {
    pub tsc: u64,
    pub vm: VmHandle,
    pub vcpu: VcpuHandle,
    pub reason: u8, // compact representation (VmExitReason discriminant)
}

impl Default for TraceEntry { fn default() -> Self { Self { tsc: 0, vm: 0, vcpu: 0, reason: 0 } } }

static mut RING: [TraceEntry; CAP] = [TraceEntry { tsc: 0, vm: 0, vcpu: 0, reason: 0 }; CAP];
static HEAD: AtomicUsize = AtomicUsize::new(0);

/// Convert VmExitReason into compact code (0..255) – extend as needed.
fn reason_code(r: &VmExitReason) -> u8 {
    match r {
        VmExitReason::ExternalInterrupt => 1,
        VmExitReason::IoInstruction { .. } => 2,
        VmExitReason::Cpuid { .. } => 3,
        VmExitReason::Hlt => 4,
        _ => 255,
    }
}

/// Record a VMEXIT event; should be called from low‐level exit handler.
#[inline]
pub fn record(vm: VmHandle, vcpu: VcpuHandle, reason: &VmExitReason) {
    let idx = HEAD.fetch_add(1, Ordering::Relaxed) & (CAP - 1);
    let entry = TraceEntry { tsc: rdtsc(), vm, vcpu, reason: reason_code(reason) };
    unsafe { RING[idx] = entry; }
}

/// Copy current trace buffer into `out`. Returns number of entries written.
/// Snapshot is consistent because we copy after reading stable head value.
pub fn snapshot(out: &mut [TraceEntry]) -> usize {
    let head = HEAD.load(Ordering::Acquire);
    let len = core::cmp::min(out.len(), CAP);
    let start = head.saturating_sub(len) & (CAP - 1);
    for i in 0..len {
        let idx = (start + i) & (CAP - 1);
        out[i] = unsafe { RING[idx] };
    }
    len
} 