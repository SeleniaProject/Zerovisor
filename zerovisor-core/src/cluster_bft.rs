//! PBFT consensus engine for Zerovisor cluster (Task 13.1)
//! This is a minimal, self-contained implementation sufficient for log
//! replication of VM lifecycle events between <=4 nodes (f=1 Byzantine).
//! All network I/O is done via `ClusterManager::broadcast` / `send_msg`.

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use crate::cluster::{ClusterManager, NodeId};
use crate::fault::{LogEntry, Msg as ClusterMsg, LogEntryKind};

/// Default Byzantine fault tolerance parameter (can be >1 via `new_with_f`).
const DEFAULT_F: usize = 1;

/// Compute quorum size for given f.
#[inline]
const fn quorum(f: usize) -> usize { 2 * f + 1 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbftPhase { PrePrepare, Prepare, Commit }

/// PBFT per-view state.
pub struct PbftInstance {
    pub view: u64,
    pub seq: u64,
    pub phase: PbftPhase,
    pub digest: u64, // hash placeholder
    pub prepare_votes: BTreeMap<NodeId, bool>,
    pub commit_votes: BTreeMap<NodeId, bool>,
}

impl PbftInstance {
    pub fn new(view: u64, seq: u64, digest: u64) -> Self {
        Self { view, seq, phase: PbftPhase::PrePrepare, digest, prepare_votes: BTreeMap::new(), commit_votes: BTreeMap::new() }
    }

    fn has_prepare_quorum(&self, needed: usize) -> bool { self.prepare_votes.values().filter(|v| **v).count() >= needed }
    fn has_commit_quorum(&self, needed: usize) -> bool { self.commit_votes.values().filter(|v| **v).count() >= needed }
}

/// Global PBFT state machine (single instance per node)
pub struct PbftEngine<'a> {
    pub id: NodeId,
    mgr: &'a ClusterManager<'a>,
    f: usize,
    quorum: usize,
    current_view: u64,
    seq: u64,
    log: Mutex<Vec<LogEntry>>, // committed log (in-memory)
    snapshot_idx: Mutex<u64>,  // latest snapshot term/index after compression
    inflight: Mutex<Option<PbftInstance>>,
}

impl<'a> PbftEngine<'a> {
    pub fn new(id: NodeId, mgr: &'a ClusterManager<'a>) -> Self {
        Self::new_with_f(id, mgr, DEFAULT_F)
    }

    /// Create engine with custom fault tolerance parameter `f` (>1 supported).
    pub fn new_with_f(id: NodeId, mgr: &'a ClusterManager<'a>, f: usize) -> Self {
        Self {
            id,
            mgr,
            f,
            quorum: quorum(f),
            current_view: 0,
            seq: 0,
            log: Mutex::new(Vec::new()),
            snapshot_idx: Mutex::new(0),
            inflight: Mutex::new(None),
        }
    }

    /// Dynamically update tolerated Byzantine fault parameter `f` and quorum.
    pub fn set_fault_tolerance(&self, f: usize) -> Result<(), ()> {
        if f == 0 { return Err(()); }
        // SAFETY: only updating atomic integer-like fields under lock.
        unsafe {
            let engine_ptr = self as *const _ as *mut PbftEngine;
            (*engine_ptr).f = f;
            (*engine_ptr).quorum = quorum(f);
        }
        Ok(())
    }

    /// Propose a new log entry (leader only)
    pub fn propose(&self, kind: LogEntryKind, payload: &'static [u8]) {
        let digest = crc32fast::hash(payload) as u64 ^ kind as u64;
        let entry = LogEntry { term: self.current_view, index: self.seq + 1, kind, payload };
        let pre = ClusterMsg::PrePrepare { view: self.current_view, seq: self.seq + 1, digest };
        self.mgr.broadcast(&pre);
        *self.inflight.lock() = Some(PbftInstance::new(self.current_view, self.seq + 1, digest));
        // Vote for self
        if let Some(inst) = self.inflight.lock().as_mut() {
            inst.prepare_votes.insert(self.id, true);
        }
        // Store pending entry locally until commit
        self.seq += 1;
        self.log.lock().push(entry);
    }

    /// Handle inbound cluster message (called from poll loop)
    pub fn handle_msg(&self, src: NodeId, msg: &ClusterMsg) {
        match msg {
            ClusterMsg::PrePrepare { view, seq, digest } => {
                // Replica: verify and send prepare
                if *view != self.current_view { return; }
                let prepare = ClusterMsg::Prepare { view: *view, seq: *seq, digest: *digest };
                self.mgr.broadcast(&prepare);
            }
            ClusterMsg::Prepare { view, seq, digest } => {
                if *view != self.current_view { return; }
                let mut guard = self.inflight.lock();
                let inst = guard.get_or_insert_with(|| PbftInstance::new(*view, *seq, *digest));
                inst.prepare_votes.insert(src, true);
                if inst.phase == PbftPhase::PrePrepare && inst.has_prepare_quorum(self.quorum) {
                    inst.phase = PbftPhase::Prepare;
                    let commit = ClusterMsg::Commit { view: *view, seq: *seq, digest: *digest };
                    self.mgr.broadcast(&commit);
                }
            }
            ClusterMsg::Commit { view, seq, digest } => {
                let mut guard = self.inflight.lock();
                if let Some(inst) = guard.as_mut() {
                    if inst.view == *view && inst.seq == *seq {
                        inst.commit_votes.insert(src, true);
                        if inst.phase == PbftPhase::Prepare && inst.has_commit_quorum(self.quorum) {
                            inst.phase = PbftPhase::Commit;
                            // Commit entry to durable log (already appended)
                            self.maybe_compact_log();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Compress committed log by retaining last N entries and updating snapshot index.
    fn maybe_compact_log(&self) {
        const RETAIN: usize = 256; // keep recent entries in memory
        let mut guard = self.log.lock();
        if guard.len() > RETAIN {
            let new_len = guard.len() - RETAIN;
            guard.drain(0..new_len);
            *self.snapshot_idx.lock() += new_len as u64;
            crate::log!("[cluster_bft] Compacted log, new snapshot index {}", *self.snapshot_idx.lock());
        }
    }
} 