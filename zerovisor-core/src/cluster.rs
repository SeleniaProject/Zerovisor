//! Distributed cluster manager for exascale scalability (Task 9.1)
//! Handles inter-node communication and basic leader election.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use core::time::Duration;
use spin::{Mutex, Once};

use zerovisor_hal::{HpcNic, RdmaOpKind, NicError, RdmaCompletion, NicAttr};

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
}

/// Simple cluster manager maintaining membership and leader id.
pub struct ClusterManager<'a> {
    transport: ClusterTransport<'a>,
    members: Mutex<Vec<NodeId>>,
    leader: Mutex<Option<NodeId>>,
}

static CLUSTER_MGR: Once<ClusterManager<'static>> = Once::new();

impl<'a> ClusterManager<'a> {
    pub fn init(nic: &'a dyn HpcNic, self_id: NodeId) {
        CLUSTER_MGR.call_once(|| ClusterManager {
            transport: ClusterTransport::new(nic),
            members: Mutex::new(vec![self_id]),
            leader: Mutex::new(Some(self_id)),
        });
    }

    pub fn global() -> &'static ClusterManager<'static> { CLUSTER_MGR.get().expect("cluster mgr") }

    /// Add new node (simple join)
    pub fn add_node(&self, node: NodeId) { self.members.lock().push(node); }

    /// Current leader
    pub fn leader(&self) -> Option<NodeId> { *self.leader.lock() }
} 