//! MonitoringEngine – UART + Prometheus metrics exporter (Task 12.1)
//!
//! This module periodically collects metrics from `monitor` and prints them in
//! Prometheus text exposition format over the hypervisor console (UART).
//! It also offers a `tick()` function that is intended to be called from the
//! main scheduler loop; the function throttles output to `INTERVAL_CYCLES` to
//! avoid flooding the serial port.

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use spin::Once;
use crate::{monitor, console};
use core::cell::Cell;

/// Simple online statistics using Welford's algorithm.
#[derive(Default)]
struct OnlineStats {
    n: Cell<u64>,
    mean: Cell<f64>,
    m2: Cell<f64>,
}

impl OnlineStats {
    fn update(&self, value: f64) {
        let n = self.n.get() + 1;
        let delta = value - self.mean.get();
        let mean = self.mean.get() + delta / n as f64;
        let m2 = self.m2.get() + delta * (value - mean);
        self.n.set(n);
        self.mean.set(mean);
        self.m2.set(m2);
    }
    fn mean(&self) -> f64 { self.mean.get() }
    fn variance(&self) -> f64 { if self.n.get() < 2 { 0.0 } else { self.m2.get() / (self.n.get() - 1) as f64 } }
    fn stddev(&self) -> f64 { self.variance().sqrt() }
}

static LAT_STATS: Once<OnlineStats> = Once::new();

const INTERVAL_NS: u64 = 1_000_000_000; // 1 second
static LAST_EXPORT_NS: AtomicU64 = AtomicU64::new(0);

#[inline] fn cycles_per_ns() -> u64 { 3 } // 3 GHz assumption
#[inline] fn rdtsc() -> u64 { unsafe { core::arch::x86_64::_rdtsc() } }
#[inline] fn now_ns() -> u64 { rdtsc() / cycles_per_ns() }

/// Alert categories
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind { HighExitLatency, VmCrash, ThermalCritical }

/// Alert record
#[derive(Debug, Clone, Copy)]
pub struct Alert { pub kind: AlertKind, pub value: u64 }

/// Listener callback type
pub type AlertCallback = fn(Alert);

/// Simple AlertManager holding up to 8 listeners.
pub struct AlertManager { listeners: Vec<AlertCallback> }

impl AlertManager {
    pub const fn new() -> Self { Self { listeners: Vec::new() } }
    pub fn register(&mut self, cb: AlertCallback) { if self.listeners.len() < 8 { self.listeners.push(cb); } }
    pub fn fire(&self, alert: Alert) { for &cb in &self.listeners { cb(alert); } }
}

static ALERT_MGR: Once<AlertManager> = Once::new();
fn alert_manager() -> &'static AlertManager { ALERT_MGR.get_or_init(|| AlertManager::new()) }

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

    // Update latency statistics
    let stats = LAT_STATS.get_or_init(|| OnlineStats::default());
    stats.update(m.avg_exit_latency_ns as f64);

    // Prometheus exposition format
    let _ = console::write_str("# TYPE zerovisor_total_exits counter\n");
    let _ = console::write_fmt(format_args!("zerovisor_total_exits {}\n", m.total_exits));

    let _ = console::write_str("# TYPE zerovisor_avg_exit_latency_ns gauge\n");
    let _ = console::write_fmt(format_args!("zerovisor_avg_exit_latency_ns {}\n", m.avg_exit_latency_ns));

    let _ = console::write_str("# TYPE zerovisor_running_vms gauge\n");
    let _ = console::write_fmt(format_args!("zerovisor_running_vms {}\n", m.running_vms));

    // Export PUE
    let (pue100, ok) = crate::energy_pue::export_prometheus();
    let _ = console::write_str("# TYPE zerovisor_pue gauge\n");
    let _ = console::write_fmt(format_args!("zerovisor_pue {:.2}\n", pue100 as f64 / 100.0));
    let _ = console::write_str("# EOF\n");

    // Anomaly detection – z-score >3 considered anomaly
    let mean = stats.mean();
    let std = stats.stddev();
    if std > 0.0 {
        let z = (m.avg_exit_latency_ns as f64 - mean) / std;
        if z > 3.0 {
            alert_manager().fire(Alert { kind: AlertKind::HighExitLatency, value: m.avg_exit_latency_ns });
        }
    }

    // Invoke energy manager periodic maintenance
    if let Some(engine) = core::panic::catch_unwind(|| crate::energy::global()).ok() {
        engine.auto_manage();
    }
} 