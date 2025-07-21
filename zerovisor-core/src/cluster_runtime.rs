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
use zerovisor_hal::arch::x86_64::InfinibandNic as ArchNic;

static STARTED: AtomicBool = AtomicBool::new(false);
static NIC_BOX: Once<&'static dyn HpcNic> = Once::new();

/// Initialize cluster runtime. Safe to call multiple times.
pub fn init(self_id: NodeId) {
    if STARTED.swap(true, Ordering::SeqCst) { return; }
    // Allocate NIC on heap to obtain 'static lifetime.
    let nic: &'static dyn HpcNic = NIC_BOX.call_once(|| {
        let n = ArchNic::new();
        // Box leak to extend lifetime
        Box::leak(Box::new(n)) as &dyn HpcNic
    });

    ClusterManager::init(nic, self_id);

    // Spawn background poller (stub: busy loop inside interrupt context not implemented).
    // For non-std/no threading environment, upper layer scheduler can call poll() periodically.
} 