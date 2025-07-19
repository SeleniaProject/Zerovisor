//! Real-time monitoring engine (Requirement 5, Task 12.1)
//! Collects performance metrics and exposes them through a memory-mapped
//! interface for external tooling.  Metrics are updated lock-free at the end
//! of each scheduling quantum and on VMEXIT.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::ZerovisorError;
use crate::security::{self, SecurityEvent};

/// 4-KiB aligned memory page that mirrors `PerformanceMetrics` in real time.
/// External monitoring agents can map this physical address to obtain
/// zero-overhead access to hypervisor statistics (Requirement 5).
#[repr(C, align(4096))]
pub struct MetricsPage(PerformanceMetrics);

// SAFETY: Writable from single core with atomic operations; readers treat as RO.
static mut METRICS_PAGE: MetricsPage = MetricsPage(PerformanceMetrics {
    total_exits: 0,
    total_exit_time_ns: 0,
    avg_exit_latency_ns: 0,
    running_vms: 0,
    shared_pages: 0,
    numa_misses: 0,
    max_wcet_ns: 0,
    timestamp_ns: 0,
});

#[derive(Debug, Clone, Copy)]
pub struct PerformanceMetrics {
    pub total_exits: u64,
    pub total_exit_time_ns: u64,
    pub avg_exit_latency_ns: u64,
    pub running_vms: u64,
    pub shared_pages: u64,
    pub numa_misses: u64,
    pub max_wcet_ns: u64,
    pub timestamp_ns: u64,
}

static TOTAL_EXITS: AtomicU64 = AtomicU64::new(0);
static TOTAL_EXIT_TIME_NS: AtomicU64 = AtomicU64::new(0);
static RUNNING_VMS: AtomicU64 = AtomicU64::new(0);
static SHARED_PAGES: AtomicU64 = AtomicU64::new(0);
static NUMA_MISSES: AtomicU64 = AtomicU64::new(0);
static MAX_WCET_NS: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn record_vmexit(latency_ns: u64) {
    TOTAL_EXITS.fetch_add(1, Ordering::Relaxed);
    TOTAL_EXIT_TIME_NS.fetch_add(latency_ns, Ordering::Relaxed);

    // Update memory-mapped metrics page (non-atomic; readers tolerate tearing)
    unsafe {
        METRICS_PAGE.0.total_exits = TOTAL_EXITS.load(Ordering::Relaxed);
        METRICS_PAGE.0.total_exit_time_ns = TOTAL_EXIT_TIME_NS.load(Ordering::Relaxed);
        let exits = METRICS_PAGE.0.total_exits;
        METRICS_PAGE.0.avg_exit_latency_ns = if exits == 0 {
            0
        } else {
            METRICS_PAGE.0.total_exit_time_ns / exits
        };
        // Security event if latency exceeds 10 ns
        if METRICS_PAGE.0.avg_exit_latency_ns > 10 {
            security::record_event(SecurityEvent::PerfWarning { avg_latency_ns: METRICS_PAGE.0.avg_exit_latency_ns, wcet_ns: None });
        }
        METRICS_PAGE.0.timestamp_ns = crate::scheduler::cycles_to_nanoseconds(crate::scheduler::get_cycle_counter());
        METRICS_PAGE.0.shared_pages = SHARED_PAGES.load(Ordering::Relaxed);
        METRICS_PAGE.0.numa_misses = NUMA_MISSES.load(Ordering::Relaxed);
        METRICS_PAGE.0.max_wcet_ns = MAX_WCET_NS.load(Ordering::Relaxed);
    }
}

#[inline]
pub fn vm_started() {
    RUNNING_VMS.fetch_add(1, Ordering::Relaxed);
    unsafe { METRICS_PAGE.0.running_vms = RUNNING_VMS.load(Ordering::Relaxed); }
}
#[inline]
pub fn vm_stopped() {
    RUNNING_VMS.fetch_sub(1, Ordering::Relaxed);
    unsafe { METRICS_PAGE.0.running_vms = RUNNING_VMS.load(Ordering::Relaxed); }
}

/// Return a pointer to the read-only MMIO metrics page for external tools.
pub fn metrics_mmio_ptr() -> *const PerformanceMetrics {
    unsafe { &METRICS_PAGE.0 as *const PerformanceMetrics }
}

pub fn collect() -> PerformanceMetrics {
    let exits = TOTAL_EXITS.load(Ordering::Relaxed);
    let exit_time = TOTAL_EXIT_TIME_NS.load(Ordering::Relaxed);
    let avg = if exits == 0 { 0 } else { exit_time / exits };
    PerformanceMetrics {
        total_exits: exits,
        total_exit_time_ns: exit_time,
        avg_exit_latency_ns: avg,
        running_vms: RUNNING_VMS.load(Ordering::Relaxed),
        shared_pages: SHARED_PAGES.load(Ordering::Relaxed),
        numa_misses: NUMA_MISSES.load(Ordering::Relaxed),
        max_wcet_ns: MAX_WCET_NS.load(Ordering::Relaxed),
        timestamp_ns: crate::scheduler::cycles_to_nanoseconds(crate::scheduler::get_cycle_counter()),
    }
}

/// Increment shared page count when a guest‐visible DMA buffer is mapped.
pub fn add_shared_pages(count: u64) {
    SHARED_PAGES.fetch_add(count, Ordering::Relaxed);
}

/// Decrement shared page count when a buffer is unmapped.
pub fn remove_shared_pages(count: u64) {
    SHARED_PAGES.fetch_sub(count, Ordering::Relaxed);
}

/// Increment NUMA miss counter when local-node allocation fails.
pub fn add_numa_miss() {
    NUMA_MISSES.fetch_add(1, Ordering::Relaxed);
}

/// Record WCET for a scheduling quantum.
pub fn record_wcet(ns: u64) {
    let mut prev = MAX_WCET_NS.load(Ordering::Relaxed);
    while ns > prev {
        match MAX_WCET_NS.compare_exchange(prev, ns, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                unsafe { METRICS_PAGE.0.max_wcet_ns = ns; }
                if ns > 10 {
                    crate::security::record_event(crate::security::SecurityEvent::PerfWarning { avg_latency_ns: 0, wcet_ns: Some(ns) });
                }
                break;
            }
            Err(cur) => prev = cur,
        }
    }
} 