//! Real-time monitoring engine (Requirement 5, Task 12.1)
//! Collects performance metrics and exposes them through a memory-mapped
//! interface for external tooling.  Metrics are updated lock-free at the end
//! of each scheduling quantum and on VMEXIT.

use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub struct PerformanceMetrics {
    pub total_exits: u64,
    pub total_exit_time_ns: u64,
    pub avg_exit_latency_ns: u64,
    pub running_vms: u64,
    pub timestamp_ns: u64,
}

static TOTAL_EXITS: AtomicU64 = AtomicU64::new(0);
static TOTAL_EXIT_TIME_NS: AtomicU64 = AtomicU64::new(0);
static RUNNING_VMS: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn record_vmexit(latency_ns: u64) {
    TOTAL_EXITS.fetch_add(1, Ordering::Relaxed);
    TOTAL_EXIT_TIME_NS.fetch_add(latency_ns, Ordering::Relaxed);
}

#[inline]
pub fn vm_started() { RUNNING_VMS.fetch_add(1, Ordering::Relaxed); }
#[inline]
pub fn vm_stopped() { RUNNING_VMS.fetch_sub(1, Ordering::Relaxed); }

pub fn collect() -> PerformanceMetrics {
    let exits = TOTAL_EXITS.load(Ordering::Relaxed);
    let exit_time = TOTAL_EXIT_TIME_NS.load(Ordering::Relaxed);
    let avg = if exits == 0 { 0 } else { exit_time / exits };
    PerformanceMetrics {
        total_exits: exits,
        total_exit_time_ns: exit_time,
        avg_exit_latency_ns: avg,
        running_vms: RUNNING_VMS.load(Ordering::Relaxed),
        timestamp_ns: crate::scheduler::cycles_to_nanoseconds(crate::scheduler::get_cycle_counter()),
    }
} 