//! Fault tolerance state machine definitions (Task 9.1)

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEntryKind {
    VmCreate,
    VmDestroy,
    MemMapUpdate,
    VmPlacement,
    Custom(u8),
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub kind: LogEntryKind,
    pub payload: &'static [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    RequestVote { term: u64, candidate_id: u32, last_log_index: u64, last_log_term: u64 },
    Vote { term: u64, vote_granted: bool },
    AppendEntries { term: u64, leader_id: u32, prev_log_index: u64, prev_log_term: u64, entries: &'static [LogEntry], leader_commit: u64 },
    AppendResponse { term: u64, success: bool, match_index: u64 },
    // PBFT consensus messages
    PrePrepare { view: u64, seq: u64, digest: u64 },
    Prepare { view: u64, seq: u64, digest: u64 },
    Commit { view: u64, seq: u64, digest: u64 },
    // HotStuff consensus messages (prepare -> pre-commit -> commit -> decide)
    HsProposal { view: u64, height: u64, parent_qc: u64, digest: u64 },
    HsVote { view: u64, height: u64, digest: u64 },
    HsNewView { view: u64, qc: u64 },
    // Generic small control message with 1-byte discriminator.
    Custom(u8, &'static [u8]),
    IsolateVm { vm: u32 },
} 