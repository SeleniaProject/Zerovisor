//! MonitoringEngine – UART + Prometheus metrics exporter (Task 12.1)
//!
//! This module periodically collects metrics from `monitor` and prints them in
//! Prometheus text exposition format over the hypervisor console (UART).
//! It also offers a `tick()` function that is intended to be called from the
//! main scheduler loop; the function throttles output to `INTERVAL_CYCLES` to
//! avoid flooding the serial port.

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::{monitor, console};

const INTERVAL_NS: u64 = 1_000_000_000; // 1 second
static LAST_EXPORT_NS: AtomicU64 = AtomicU64::new(0);

#[inline] fn cycles_per_ns() -> u64 { 3 } // 3 GHz assumption
#[inline] fn rdtsc() -> u64 { unsafe { core::arch::x86_64::_rdtsc() } }
#[inline] fn now_ns() -> u64 { rdtsc() / cycles_per_ns() }

/// Call this function regularly (e.g. each scheduler quantum). It will export
/// metrics at most once per `INTERVAL_NS`.
pub fn tick() {
    let now = now_ns();
    let last = LAST_EXPORT_NS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < INTERVAL_NS { return; }
    if LAST_EXPORT_NS.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return; // another core already exported
    }

    let m = monitor::collect();
    // Prometheus exposition format
    let _ = console::write_str("# TYPE zerovisor_total_exits counter\n");
    let _ = console::write_fmt(format_args!("zerovisor_total_exits {}\n", m.total_exits));

    let _ = console::write_str("# TYPE zerovisor_avg_exit_latency_ns gauge\n");
    let _ = console::write_fmt(format_args!("zerovisor_avg_exit_latency_ns {}\n", m.avg_exit_latency_ns));

    let _ = console::write_str("# TYPE zerovisor_running_vms gauge\n");
    let _ = console::write_fmt(format_args!("zerovisor_running_vms {}\n", m.running_vms));

    let _ = console::write_str("# EOF\n");

    // Invoke energy manager periodic maintenance
    if let Some(engine) = core::panic::catch_unwind(|| crate::energy::global()).ok() {
        engine.auto_manage();
    }
} 