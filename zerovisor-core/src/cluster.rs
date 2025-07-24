//! Distributed cluster manager for exascale scalability (Task 9.1)
//! Handles inter-node communication and basic leader election.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use bitvec::prelude::*;
use alloc::collections::BTreeMap;
use core::time::Duration;
use spin::{Mutex, Once};
use zerovisor_hal::rdma_opt::RdmaBatcher;

use zerovisor_hal::{HpcNic, RdmaOpKind, NicError, RdmaCompletion, NicAttr};
use postcard::{to_slice, from_bytes};
use crate::fault::Msg as ClusterMsg;
use crate::cluster_bft::PbftEngine;
use crate::cluster_hotstuff::HotStuffEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Transport abstraction built on top of HpcNic RDMA verbs.
pub struct ClusterTransport<'a> {
    nic: &'a dyn HpcNic,
}

impl<'a> ClusterTransport<'a> {
    pub fn new(nic: &'a dyn HpcNic) -> Self { Self { nic } }

    pub fn send(&self, node: NodeId, buf: &[u8]) -> Result<(), NicError> {
        const BATCH: usize = 32;
        let mtu = self.nic_attr().mtu as usize;
        let mut offset = 0usize;
        let mut batcher = RdmaBatcher::<BATCH>::new(self.nic);
        while offset < buf.len() {
            let chunk = core::cmp::min(mtu, buf.len() - offset);
            let remote_pa = (node.0 as u64) * 0x1_0000_0000 + offset as u64;
            let local_va = unsafe { buf.as_ptr().add(offset) as u64 };
            batcher.push(offset as u64, RdmaOpKind::Write, local_va, remote_pa, chunk)?;
            offset += chunk;
        }
        batcher.flush()
    }

    pub fn poll(&self) -> Result<Vec<RdmaCompletion>, NicError> {
        let completions = self.nic.poll_completions(32, Some(Duration::from_millis(1)))?;
        Ok(completions.to_vec())
    }

    pub fn nic_attr(&self) -> NicAttr { self.nic.query_attr() }

    pub fn send_msg(&self, node: NodeId, msg: &ClusterMsg, buf: &mut [u8]) -> Result<(), NicError> {
        let used = to_slice(msg, buf).map_err(|_| NicError::SerializeError)?;
        self.send(node, used)
    }
}

/// Simple cluster manager maintaining membership and leader id.
pub struct ClusterManager<'a> {
    transport: ClusterTransport<'a>,
    members: Mutex<BitVec<Lsb0, u8>>, // bitmap up to MAX_NODES
    leader: Mutex<Option<NodeId>>,
}

static CLUSTER_MGR: Once<ClusterManager<'static>> = Once::new();
static PBFT: Once<PbftEngine<'static>> = Once::new();
static HOTSTUFF: Once<HotStuffEngine<'static>> = Once::new();

const MAX_NODES: usize = 1_048_576; // 1M cores assumption (one node per core sample)

impl<'a> ClusterManager<'a> {
    pub fn init(nic: &'a dyn HpcNic, self_id: NodeId) {
        CLUSTER_MGR.call_once(|| {
            let mut bv = bitvec![u8, Lsb0; 0; MAX_NODES];
            bv.set(self_id.0 as usize, true);
            ClusterManager {
                transport: ClusterTransport::new(nic),
                members: Mutex::new(bv),
                leader: Mutex::new(Some(self_id)),
            }
        });
        // Initialize PBFT & HotStuff engines now that ClusterManager exists
        let mgr_ref = CLUSTER_MGR.get().unwrap();
        let static_ref: &'static ClusterManager<'static> = unsafe { core::mem::transmute::<&ClusterManager<'_>, &ClusterManager<'static>>(mgr_ref) };
        PBFT.call_once(|| PbftEngine::new(self_id, static_ref));
        HOTSTUFF.call_once(|| HotStuffEngine::new(self_id, static_ref));
    }

    pub fn global() -> &'static ClusterManager<'static> { CLUSTER_MGR.get().expect("cluster mgr") }

    /// Add new node (simple join)
    pub fn add_node(&self, node: NodeId) { self.members.lock().set(node.0 as usize, true); }

    /// Remove node (failure detected) and trigger leader re-election if necessary
    pub fn remove_node(&self, node: NodeId) {
        self.members.lock().set(node.0 as usize, false);
        // Trigger PBFT reconfiguration to new f value
        let new_member_cnt = self.members.lock().count_ones();
        if let Some(engine) = PBFT.get() {
            let new_f = core::cmp::max(1, (new_member_cnt - 1) / 3);
            let _ = engine.set_fault_tolerance(new_f);
        }
        // If leader failed, elect lowest NodeId as new leader
        let mut leader_guard = self.leader.lock();
        if leader_guard.map_or(false, |l| l == node) {
            let mut new_leader = None;
            self.each_member(|n| if new_leader.is_none() { new_leader = Some(n); });
            *leader_guard = new_leader;
            // Broadcast leader change message (reuse PrePrepare with digest 0)
            if let Some(new_l) = new_leader {
                let msg = ClusterMsg::PrePrepare { view: 0, seq: 0, digest: 0 };
                self.broadcast(&msg);
            }
        }
    }

    /// Iterate members efficiently
    pub fn each_member<F: FnMut(NodeId)>(&self, mut f: F) {
        let bv = self.members.lock();
        for (idx, bit) in bv.iter().enumerate() {
            if *bit { f(NodeId(idx as u32)); }
        }
    }

    /// Current leader
    pub fn leader(&self) -> Option<NodeId> { *self.leader.lock() }

    pub fn broadcast(&self, msg: &ClusterMsg) {
        // Pre-allocated scratch buffer big enough for typical control messages
        let mut buf = [0u8; 256];
        self.each_member(|node| {
            if Some(node) == self.leader() { return; }
            let _ = self.transport.send_msg(node, msg, &mut buf);
        });
    }

    /// Poll NIC completions and decode cluster messages (placeholder).
    pub fn poll_incoming(&self) -> bool {
        let mut processed = false;
        if let Ok(completions) = self.transport.poll() {
            for comp in completions {
                processed = true;
                // Each completion gives us the local buffer address used in the RDMA READ.
                // We assume fixed-size control messages (<=256B) placed at that address.
                let local_va = comp.local_va;
                let len = comp.bytes as usize;
                if len == 0 { continue; }
                // SAFETY: NIC ensures DMA has completed; we map kernel virtual address.
                let slice = unsafe { core::slice::from_raw_parts(local_va as *const u8, len) };
                if let Ok(msg) = postcard::from_bytes::<ClusterMsg>(slice) {
                    let src_node = NodeId((comp.remote_qp as u32) & 0xFFFF); // simplistic mapping
                    self.deliver_msg(src_node, &msg);
                }
            }
        }
        processed
    }

    pub fn deliver_msg(&self, src: NodeId, msg: &crate::fault::Msg) {
        if let crate::fault::Msg::IsolateVm { vm } = msg {
            crate::vm_manager::global().isolate_vm(*vm).ok();
            return;
        }
        if let Some(engine) = PBFT.get() {
            engine.handle_msg(src, msg);
        }
        if let Some(hs) = HOTSTUFF.get() {
            hs.handle_msg(src, msg);
        }

        if let crate::fault::Msg::Custom(kind, payload) = msg {
            if *kind == 0xCA && payload.len() >= 4 {
                // carbon intensity update
                let mut arr = [0u8;4];
                arr.copy_from_slice(&payload[..4]);
                let val = u32::from_le_bytes(arr);
                crate::carbon_aware::on_msg(src, val);
            } else if *kind == 0xCB && !payload.is_empty() {
                crate::renewable_api::on_msg(src, payload[0]);
            } else if *kind == 0xCC {
                crate::lattice_kex::on_msg(src, payload);
            }
        }
    }

    pub fn pbft(&self) -> &'static PbftEngine<'static> { PBFT.get().expect("PBFT not init") }
    pub fn hotstuff(&self) -> &'static HotStuffEngine<'static> { HOTSTUFF.get().expect("HotStuff not init") }
}

// ----------------------------------------------------------
// Byzantine Fault-Tolerant consensus placeholder (PBFT style)
// ----------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BftPhase { PrePrepare, Prepare, Commit }

// Default Byzantine fault tolerance parameter (f=1 by default, can be updated by the higher-level engine).
const DEFAULT_F: usize = 1;
/// Quorum size required for prepare/commit phases.
#[inline]
fn quorum() -> usize { 2 * DEFAULT_F + 1 }

pub struct BftState {
    pub view: u64,
    pub sequence: u64,
    pub phase: BftPhase,
    // Map <node_id, prepared?>, simplified for demo
    pub prepare_votes: BTreeMap<NodeId, bool>,
    pub commit_votes: BTreeMap<NodeId, bool>,
}

impl BftState {
    pub fn new(view: u64) -> Self {
        Self { view, sequence: 0, phase: BftPhase::PrePrepare, prepare_votes: BTreeMap::new(), commit_votes: BTreeMap::new() }
    }

    /// Process an inbound PBFT message originating from `src`.
    ///
    /// The implementation follows the standard three-phase PBFT protocol:
    /// 1. PrePrepare (sent by the leader)
    /// 2. Prepare    (replica votes)
    /// 3. Commit     (replica votes)
    ///
    /// For simplicity we target `f = 1` which yields a quorum size of 3.
    pub fn handle_msg(&mut self, src: NodeId, msg: &ClusterMsg) {
        match msg {
            ClusterMsg::PrePrepare { view, seq, .. } => {
                // Accept PrePrepare only for the current view and next sequence.
                if view != self.view { return; }

                // Reset vote tracking for the new instance.
                self.sequence = seq;
                self.phase = BftPhase::PrePrepare;
                self.prepare_votes.clear();
                self.commit_votes.clear();

                // Leader implicitly counts as a prepare vote; replicas will add theirs.
                self.prepare_votes.insert(src, true);
            }
            ClusterMsg::Prepare { view, seq, .. } => {
                if view != self.view || seq != self.sequence { return; }
                self.prepare_votes.insert(src, true);

                // Transition to PREPARE-complete once quorum reached.
                if self.phase == BftPhase::PrePrepare {
                    let votes = self.prepare_votes.values().filter(|v| **v).count();
                    if votes >= quorum() {
                        self.phase = BftPhase::Prepare;
                    }
                }
            }
            ClusterMsg::Commit { view, seq, .. } => {
                if view != self.view || seq != self.sequence { return; }
                self.commit_votes.insert(src, true);

                if self.phase == BftPhase::Prepare {
                    let votes = self.commit_votes.values().filter(|v| **v).count();
                    if votes >= quorum() {
                        self.phase = BftPhase::Commit;
                    }
                }
            }
            _ => { /* Ignore non-PBFT messages */ }
        }
    }
} 