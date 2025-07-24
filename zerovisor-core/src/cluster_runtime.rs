//! Cluster runtime initialization (Task: Exascale scale-out >1M cores)
//!
//! Initializes `ClusterManager` with an InfiniBand/Omni-Path NIC backend and
//! spawns a background polling loop (stubbed) to process incoming RDMA
//! completions and deliver messages to PBFT layer.
//!
//! In a real deployment the poller would run on a dedicated core and leverage
//! adaptive scheduling to keep latency under microsecond scale.

#![allow(dead_code)]

extern crate alloc;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use spin::Once;

use zerovisor_hal::nic::HpcNic;
use zerovisor_core_cycles::rdtsc; // alias to existing cycles module for timer
use crate::cluster::{ClusterManager, NodeId};

#[cfg(target_arch = "x86_64")]
use zerovisor_hal::arch::x86_64::{global as hal_nic_global, init_global};

static STARTED: AtomicBool = AtomicBool::new(false);
static NIC_BOX: Once<&'static dyn HpcNic> = Once::new();

// Last cycle timestamp of NIC poll – used for adaptive backoff.
static mut LAST_POLL_CYCLES: u64 = 0;

/// Desired minimum interval between polls in CPU cycles (default: 10 µs).
const POLL_INTERVAL_CYCLES: u64 = 10_000; // Adjust according to TSC frequency later

/// Initialize cluster runtime. Safe to call multiple times.
pub fn init(self_id: NodeId) {
    if STARTED.swap(true, Ordering::SeqCst) { return; }
    // Allocate NIC on heap to obtain 'static lifetime.
    let nic: &'static dyn HpcNic = NIC_BOX.call_once(|| {
        #[cfg(target_arch = "x86_64")]
        {
            init_global();
            return hal_nic_global().expect("NIC global");
        }
        #[allow(unreachable_code)]
        panic!("Unsupported arch");
    });

    ClusterManager::init(nic, self_id);

    // Initial poll timestamp
    unsafe { LAST_POLL_CYCLES = rdtsc(); }
}

/// Non-blocking tick which should be invoked from the scheduler loop.
/// Performs adaptive RDMA completion polling every ~10 µs to keep tail-latency
/// under microsecond order while avoiding excessive CPU usage.
pub fn tick() {
    // Safety: rdtsc is constant time and side-effect free.
    let now = rdtsc();
    let elapsed = unsafe { now.wrapping_sub(LAST_POLL_CYCLES) };
    if elapsed < POLL_INTERVAL_CYCLES { return; }

    // Poll completions – ClusterManager handles decoding + PBFT delivery and returns activity flag.
    let had_completions = ClusterManager::global().poll_incoming();

    // Adaptive back-off: if no completions, double interval up to 1 ms; if there were, reset.
    static mut INTERVAL: u64 = POLL_INTERVAL_CYCLES;
    unsafe {
        if had_completions {
            INTERVAL = POLL_INTERVAL_CYCLES;
        } else if INTERVAL < 1_000_000 {
            INTERVAL *= 2;
        }
        LAST_POLL_CYCLES = now;
    }
} 