//! RDMA-backed virtual network layer (Task 9.2)
//! Provides an RDMA transport suitable for guest networking with near-zero overhead.
//! The implementation currently focuses on InfiniBand and Omni-Path but can be
//! extended to additional fabrics by plugging different `HpcNic` back-ends.

#![allow(dead_code)]

use core::time::Duration;
use crate::nic::{HpcNic, RdmaOpKind, NicError, RdmaCompletion};
use crate::rdma_opt::RdmaBatcher;
use crate::memory::{PhysicalAddress, VirtualAddress};

/// Maximum outstanding work-requests per connection.
const BATCH_SIZE: usize = 64;

/// Identifier representing a queue-pair shared with a guest VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VnetQp(pub u32);

/// Statistics exposed to the hypervisor monitor.
#[derive(Default, Debug, Clone, Copy)]
pub struct VnetStats {
    pub tx_pkts: u64,
    pub rx_pkts: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

/// RDMA virtual network instance bound to a single guest.
pub struct RdmaVirtualNet<'a> {
    nic: &'a dyn HpcNic,
    qp: VnetQp,
    batcher: RdmaBatcher<'a, BATCH_SIZE>,
    stats: VnetStats,
}

impl<'a> RdmaVirtualNet<'a> {
    pub fn new(nic: &'a dyn HpcNic, qp: VnetQp) -> Self {
        Self { nic, qp, batcher: RdmaBatcher::new(nic), stats: VnetStats::default() }
    }

    /// Transmit a buffer to the guest.
    pub fn tx(&mut self, guest_pa: PhysicalAddress, host_va: VirtualAddress, len: usize) -> Result<(), NicError> {
        let wr_id = self.stats.tx_pkts; // simplistic
        self.batcher.push(wr_id, RdmaOpKind::Write, host_va, guest_pa, len)?;
        self.stats.tx_pkts += 1;
        self.stats.tx_bytes += len as u64;
        Ok(())
    }

    /// Poll for completed receives (simplified).
    pub fn poll_rx(&mut self) -> Result<&[RdmaCompletion], NicError> {
        let comps = self.nic.poll_completions(BATCH_SIZE, Some(Duration::from_millis(0)))?;
        self.stats.rx_pkts += comps.len() as u64;
        for c in comps {
            self.stats.rx_bytes += c.bytes as u64;
        }
        Ok(comps)
    }

    /// Flush outstanding TX batches.
    pub fn flush(&mut self) -> Result<(), NicError> { self.batcher.flush() }

    pub fn stats(&self) -> VnetStats { self.stats }
} 