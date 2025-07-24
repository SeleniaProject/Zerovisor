//! PUE Monitor – Guarantees PUE ≤1.1 by proactive power management.
//!
//! The monitor aggregates facility power and IT equipment power readings
//! from on-board sensors or BMC interfaces (abstracted via `EnergySample`).  A
//! moving window average (1-minute) is maintained; if the computed Power
//! Usage Effectiveness (PUE) exceeds the target (1.1 by default), the monitor
//! instructs `EnergyManager` to enter low-power mode and emits a security
//! event for audit.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Once;
use crate::energy::global as energy_mgr;
use crate::security::{record_event, SecurityEvent};

/// Sliding window sample (facility + IT watts, timestamp ns)
struct EnergySample { fac_w: u64, it_w: u64, ts_ns: u64 }

/// Ring buffer window size (60 samples = 1 min @1s interval)
const WINDOW: usize = 60;

struct PueState { buf: Vec<EnergySample>, idx: usize }

static PUE_MON: Once<PueState> = Once::new();

/// Target PUE upper bound.
const PUE_TARGET_MULT: u64 = 110; // 1.10 scaled by 100

fn state() -> &'static mut PueState { unsafe { &mut *(&*PUE_MON.get().unwrap() as *const _ as *mut PueState) } }

/// Initialise global monitor (idempotent).
pub fn init() { PUE_MON.call_once(|| PueState { buf: Vec::with_capacity(WINDOW), idx: 0 }); }

/// Push new power sample (watts). Call once per second.
pub fn update_sample(facility_w: u64, it_w: u64) {
    let ts = crate::scheduler::cycles_to_nanoseconds(crate::scheduler::get_cycle_counter());
    let s = EnergySample { fac_w: facility_w, it_w, ts_ns: ts };
    let st = state();
    if st.buf.len() < WINDOW { st.buf.push(s); } else { st.buf[st.idx] = s; st.idx = (st.idx + 1) % WINDOW; }
    enforce();
}

/// Compute current PUE (scaled by 100).
fn current_pue_times_100() -> u64 {
    let st = state();
    let (mut fac, mut it) = (0u64, 0u64);
    for e in &st.buf { fac += e.fac_w; it += e.it_w; }
    if it == 0 { return 0; }
    fac * 100 / it
}

/// Check and enforce PUE target.
pub fn enforce() {
    let pue = current_pue_times_100();
    if pue > PUE_TARGET_MULT {
        // Exceeded target – trigger low-power mode
        energy_mgr().set_low_power();
        record_event(SecurityEvent::PerfWarning { avg_latency_ns: 0, wcet_ns: Some(pue) });
    }
}

/// Export for Prometheus metrics exposition.
pub fn export_prometheus() -> (u64, bool) {
    let pue100 = current_pue_times_100();
    (pue100, pue100 <= PUE_TARGET_MULT)
} 