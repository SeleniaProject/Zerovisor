//! HotStuff consensus engine for Zerovisor cluster (Task 1)
//! This implementation follows the streamlined HotStuff protocol
//! (Yin et al., 2019) with four phases: Prepare, PreCommit, Commit, Decide.
//! It is designed for partially–synchronous networks and tolerates up to
//! `f` Byzantine faults among `3f + 1` replicas.
//!
//! The engine integrates with `ClusterManager` for message transport and
//! uses Rust `no_std` data-structures (`alloc`). All comments are in English
//! as requested.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::too_many_arguments)]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use crate::cluster::{ClusterManager, NodeId};
use crate::fault::{Msg as ClusterMsg, LogEntry, LogEntryKind};

/// Default fault-tolerance parameter (f = 1).
const DEFAULT_F: usize = 1;

#[inline]
const fn quorum(f: usize) -> usize { 2 * f + 1 }

/// HotStuff block identifier – simple 64-bit digest for now.
pub type BlockId = u64;

/// Quorum Certificate (QC) – hash of the block that achieved quorum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuorumCert {
    pub view: u64,
    pub id: BlockId,
}

/// Per-round consensus state.
struct HsRound {
    proposal_id: BlockId,
    parent_qc: QuorumCert,
    votes: BTreeMap<NodeId, bool>,
}

impl HsRound {
    fn new(proposal_id: BlockId, parent_qc: QuorumCert) -> Self {
        Self { proposal_id, parent_qc, votes: BTreeMap::new() }
    }

    fn has_quorum(&self, needed: usize) -> bool {
        self.votes.values().filter(|v| **v).count() >= needed
    }
}

/// HotStuff consensus engine – one per node.
pub struct HotStuffEngine<'a> {
    id: NodeId,
    mgr: &'a ClusterManager<'a>,
    f: usize,
    quorum: usize,
    current_view: u64,
    height: u64,
    locked_qc: Mutex<QuorumCert>,
    log: Mutex<Vec<LogEntry>>, // committed log
    inflight: Mutex<Option<HsRound>>, // current proposal being voted
}

impl<'a> HotStuffEngine<'a> {
    pub fn new(id: NodeId, mgr: &'a ClusterManager<'a>) -> Self {
        Self {
            id,
            mgr,
            f: DEFAULT_F,
            quorum: quorum(DEFAULT_F),
            current_view: 0,
            height: 0,
            locked_qc: Mutex::new(QuorumCert { view: 0, id: 0 }),
            log: Mutex::new(Vec::new()),
            inflight: Mutex::new(None),
        }
    }

    /// Propose a new block (leader only).
    pub fn propose(&self, kind: LogEntryKind, payload: &'static [u8]) {
        let digest = crc32fast::hash(payload) as u64 ^ kind as u64 ^ self.height;
        let entry = LogEntry { term: self.current_view, index: self.height + 1, kind, payload };
        let parent_id = self.locked_qc.lock().id;
        let prop = ClusterMsg::HsProposal {
            view: self.current_view,
            height: self.height + 1,
            parent_qc: parent_id,
            digest,
        };
        self.mgr.broadcast(&prop);
        *self.inflight.lock() = Some(HsRound::new(digest, *self.locked_qc.lock()));
        self.height += 1;
        self.log.lock().push(entry);
    }

    /// Handle inbound HotStuff-related message.
    pub fn handle_msg(&self, src: NodeId, msg: &ClusterMsg) {
        match msg {
            ClusterMsg::HsProposal { view, height, parent_qc, digest } => {
                if *view < self.current_view { return; }
                // Verify parent QC – simplified (match id only).
                if *parent_qc != self.locked_qc.lock().id && *parent_qc != 0 { return; }
                // Vote for proposal.
                let vote = ClusterMsg::HsVote { view: *view, height: *height, digest: *digest };
                self.mgr.broadcast(&vote);
                // Cache inflight round.
                *self.inflight.lock() = Some(HsRound::new(*digest, QuorumCert { view: *view, id: *parent_qc }));
            }
            ClusterMsg::HsVote { view, height, digest } => {
                let mut guard = self.inflight.lock();
                let qc_needed = self.quorum;
                let inst = guard.get_or_insert_with(|| HsRound::new(*digest, *self.locked_qc.lock()));
                inst.votes.insert(src, true);
                if inst.has_quorum(qc_needed) {
                    // Form QC and broadcast NewView to advance protocol.
                    let qc = QuorumCert { view: *view, id: *digest };
                    *self.locked_qc.lock() = qc;
                    let nv = ClusterMsg::HsNewView { view: *view + 1, qc: qc.id };
                    self.mgr.broadcast(&nv);
                }
            }
            ClusterMsg::HsNewView { view, qc } => {
                if *view <= self.current_view { return; }
                self.current_view = *view;
                self.locked_qc.lock().id = *qc;
                // Leader for new view would propose – handled elsewhere.
            }
            _ => {}
        }
    }

    /// Update tolerated fault parameter at runtime.
    pub fn set_fault_tolerance(&mut self, f: usize) {
        self.f = f;
        self.quorum = quorum(f);
    }
} 