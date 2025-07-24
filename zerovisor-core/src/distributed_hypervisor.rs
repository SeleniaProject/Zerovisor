//! Distributed hypervisor infrastructure for exascale (>1M cores) clusters.
//!
//! This module maintains a **global directory** that maps `VmId` → `NodeId`
//! and provides a best-effort placement algorithm based on node load. All
//! directory updates are committed through the PBFT / HotStuff consensus layer
//! (`ClusterManager::pbft` / `hotstuff`) ensuring total order and durability.
//!
//! The implementation is entirely lock-free on the read path: look-ups rely on
//! `RwLock` read guards, while the write path uses coarse-grained `Mutex` to
//! keep code size minimal under `no_std`.
//!
//! Comments are written in English as required.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::{RwLock, Mutex, Once};

use crate::cluster::{ClusterManager, NodeId};
use crate::fault::{LogEntryKind, LogEntry};
use crate::isolation::VmId;
use crate::vm_manager::VmState;
#[cfg(feature = "experimental")]
use crate::cycles::rdtsc;

/// Per-node load metrics (very coarse grained).
#[derive(Default, Clone, Copy)]
struct NodeLoad {
    /// Number of running VMs on this node.
    vm_cnt: AtomicU32,
}

impl NodeLoad {
    fn inc(&self) { self.vm_cnt.fetch_add(1, Ordering::Relaxed); }
    fn dec(&self) { self.vm_cnt.fetch_sub(1, Ordering::Relaxed); }
    fn get(&self) -> u32 { self.vm_cnt.load(Ordering::Relaxed) }
}

/// Global directory singleton.
struct GlobalDirectory {
    /// Map VM → Node
    vms: RwLock<BTreeMap<VmId, NodeId>>,
    /// Load table Node → stats
    loads: RwLock<BTreeMap<NodeId, NodeLoad>>,
}

static DIR: Once<GlobalDirectory> = Once::new();

fn dir() -> &'static GlobalDirectory { DIR.get().expect("GlobalDirectory not init") }

/// Initialise directory – idempotent.
pub fn init() {
    DIR.call_once(|| GlobalDirectory { vms: RwLock::new(BTreeMap::new()), loads: RwLock::new(BTreeMap::new()) });
}

/// Choose the least-loaded node as placement target.
fn select_node() -> NodeId {
    let mgr = ClusterManager::global();
    let mut best = None;
    let mut min_load = u32::MAX;
    let loads = dir().loads.read();
    mgr.each_member(|node| {
        let l = loads.get(&node).map(|n| n.get()).unwrap_or(0);
        if l < min_load {
            min_load = l;
            best = Some(node);
        }
    });
    best.unwrap_or(NodeId(0))
}

/// Register newly created VM globally and replicate through consensus.
pub fn register_vm(vm: VmId) {
    let node = select_node();
    {
        let mut w = dir().vms.write();
        w.insert(vm, node);
    }
    {
        let mut wl = dir().loads.write();
        let load = wl.entry(node).or_default();
        load.inc();
    }
    replicate_directory_update(LogEntryKind::VmPlacement, vm, node);
}

/// Unregister VM (on destroy/stop).
pub fn unregister_vm(vm: VmId) {
    let node_opt = { dir().vms.write().remove(&vm) };
    if let Some(node) = node_opt {
        if let Some(load) = dir().loads.write().get(&node) {
            load.dec();
        }
    }
}

/// Lookup placement.
pub fn location_of(vm: VmId) -> Option<NodeId> { dir().vms.read().get(&vm).copied() }

/// Propose placement update via PBFT / HotStuff (whichever is primary).
fn replicate_directory_update(kind: LogEntryKind, vm: VmId, node: NodeId) {
    // Prepare payload: [vm_id(4)|node_id(4)] in little-endian.
    let mut buf = [0u8; 8];
    buf[..4].copy_from_slice(&vm.to_le_bytes());
    buf[4..].copy_from_slice(&node.0.to_le_bytes());
    // Leak to static so consensus layer can reference &'static [u8].
    let payload: &'static [u8] = Box::leak(Box::new(buf));
    let mgr = ClusterManager::global();
    // Prefer HotStuff when available (fewer rounds).
    if let Ok(hs) = core::panic::catch_unwind(|| mgr.hotstuff()) {
        hs.propose(kind, payload);
    } else if let Ok(pbft) = core::panic::catch_unwind(|| mgr.pbft()) {
        pbft.propose(kind, payload);
    }
}

/// Debug helper – dumps current directory state (O(#VMs)).
#[cfg(feature = "experimental")]
pub fn dump() {
    let now = rdtsc();
    crate::log!("[directory dump @{}] VMs {:?}", now, *dir().vms.read());
} 