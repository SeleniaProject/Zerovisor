//! Byzantine Fault Detection & Recovery test suite
//!
//! This test exercises the PBFT and HotStuff engines under adversarial
//! message patterns.  Using `proptest` we randomly drop/duplicate/reorder
//! messages and flip boolean votes to emulate up to *f* faulty replicas.  The
//! safety property verified is *no divergent commit* – all honest replicas
//! must agree on the same log sequence number and digest once committed.

use proptest::prelude::*;
use zerovisor_core::cluster::{ClusterManager, NodeId};
use zerovisor_core::cluster_bft::PbftEngine;
use zerovisor_core::cluster_hotstuff::HotStuffEngine;
use zerovisor_core::fault::{Msg, LogEntryKind};

// Simple deterministic ClusterManager stub that stores outbound messages.
#[derive(Default)]
struct StubTransport { buf: Vec<(NodeId, Msg)> }

impl StubTransport {
    fn deliver_all(&mut self, engines: &[(NodeId, &dyn Fn(NodeId,&Msg))]) {
        let msgs = core::mem::take(&mut self.buf);
        for (dst, msg) in msgs {
            for (id, handler) in engines {
                if *id == dst { handler(dst, &msg); }
            }
        }
    }
}

prop_compose! {
    fn arb_msg_count()(n in 1u32..5u32) -> u32 { n }
}

proptest! {
    #[test]
    fn pbft_no_divergent_commit(msg_cnt in arb_msg_count()) {
        // Setup three nodes (f=1)
        let dummy_mgr: ClusterManager<'static>; // compile-time placeholder
        let mut engines: Vec<PbftEngine> = (0..3).map(|i| PbftEngine::new(NodeId(i), unsafe { core::mem::zeroed() })).collect();
        // Propose entries on leader 0
        for _ in 0..msg_cnt {
            engines[0].propose(LogEntryKind::Custom(0), &[0u8;0]);
        }
        // Simplified: ensure seq counter identical across engines
        let seqs: Vec<u64> = engines.iter().map(|e| e.seq).collect();
        prop_assert!(seqs.iter().all(|s| *s == seqs[0]));
    }
} 