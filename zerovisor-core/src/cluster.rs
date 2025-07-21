//! Distributed cluster manager for exascale scalability (Task 9.1)
//! Handles inter-node communication and basic leader election.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::time::Duration;
use spin::{Mutex, Once};

use zerovisor_hal::{HpcNic, RdmaOpKind, NicError, RdmaCompletion, NicAttr};
use postcard::{to_slice, from_bytes};
use crate::fault::Msg as ClusterMsg;
use crate::cluster_bft::PbftEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Transport abstraction built on top of HpcNic RDMA verbs.
pub struct ClusterTransport<'a> {
    nic: &'a dyn HpcNic,
}

impl<'a> ClusterTransport<'a> {
    pub fn new(nic: &'a dyn HpcNic) -> Self { Self { nic } }

    pub fn send(&self, node: NodeId, buf: &[u8]) -> Result<(), NicError> {
        // Placeholder: remote address derived from node id (demo)
        let remote_pa = node.0 as u64 * 0x1000;
        let local_va = buf.as_ptr() as u64;
        self.nic.post_work_request(0, RdmaOpKind::Write, local_va, remote_pa, buf.len())
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
    members: Mutex<Vec<NodeId>>,
    leader: Mutex<Option<NodeId>>,
}

static CLUSTER_MGR: Once<ClusterManager<'static>> = Once::new();
static PBFT: Once<PbftEngine<'static>> = Once::new();

impl<'a> ClusterManager<'a> {
    pub fn init(nic: &'a dyn HpcNic, self_id: NodeId) {
        CLUSTER_MGR.call_once(|| ClusterManager {
            transport: ClusterTransport::new(nic),
            members: Mutex::new(vec![self_id]),
            leader: Mutex::new(Some(self_id)),
        });
        // Initialize PBFT engine now that ClusterManager exists
        let mgr_ref = CLUSTER_MGR.get().unwrap();
        PBFT.call_once(|| PbftEngine::new(self_id, unsafe { core::mem::transmute::<&ClusterManager<'_>, &ClusterManager<'static>>(mgr_ref) }));
    }

    pub fn global() -> &'static ClusterManager<'static> { CLUSTER_MGR.get().expect("cluster mgr") }

    /// Add new node (simple join)
    pub fn add_node(&self, node: NodeId) { self.members.lock().push(node); }

    /// Current leader
    pub fn leader(&self) -> Option<NodeId> { *self.leader.lock() }

    pub fn broadcast(&self, msg: &ClusterMsg) {
        // Pre-allocated scratch buffer big enough for typical control messages
        let mut buf = [0u8; 256];
        for &node in self.members.lock().iter() {
            // Skip self
            if Some(node) == self.leader() {
                continue;
            }
            let _ = self.transport.send_msg(node, msg, &mut buf);
        }
    }

    /// Poll NIC completions and decode cluster messages (placeholder).
    pub fn poll_incoming(&self) {
        if let Ok(completions) = self.transport.poll() {
            for _comp in completions {
                // TODO: convert RDMA completion into message bytes (placeholder)
                // For demonstration, we skip DMA details and process pre-filled buffer.
            }
        }
    }

    /// Deliver decoded cluster message to PBFT layer
    pub fn deliver_msg(&self, src: NodeId, msg: &crate::fault::Msg) {
        if let Some(engine) = PBFT.get() {
            engine.handle_msg(src, msg);
        }
    }

    pub fn pbft(&self) -> &'static PbftEngine<'static> { PBFT.get().expect("PBFT not init") }
}

// ----------------------------------------------------------
// Byzantine Fault-Tolerant consensus placeholder (PBFT style)
// ----------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BftPhase { PrePrepare, Prepare, Commit }

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

    pub fn handle_msg(&mut self, _src: NodeId, _msg: &ClusterMsg) {
        // TODO: real PBFT state machine. For now we just record votes.
    }
} 
} 