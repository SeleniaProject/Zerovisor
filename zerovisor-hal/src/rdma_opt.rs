//! RDMA optimization utilities for large-scale parallel communication (Task 9.2)
//!
//! The main goal is to reduce NIC doorbell overhead and improve throughput on
//! InfiniBand / Omni-Path fabrics by batching work requests and aggressively
//! polling completions.

#![allow(dead_code)]

use core::time::Duration;
use crate::nic::{HpcNic, RdmaOpKind, NicError};
use crate::memory::{PhysicalAddress, VirtualAddress};

/// Batches up to `N` work-requests before flushing to the NIC.
/// The implementation is *transport agnostic* and relies solely on the `HpcNic`
/// trait; architecture-specific back-ends can decide how to map these calls
/// onto queue-pairs and doorbell records.
#[derive(Debug)]
pub struct RdmaBatcher<'a, const N: usize> {
    nic: &'a dyn HpcNic,
    pending: usize,
    wr_ids: [u64; N],
}

impl<'a, const N: usize> RdmaBatcher<'a, N> {
    /// Create a new batcher bound to a NIC instance.
    pub const fn new(nic: &'a dyn HpcNic) -> Self {
        Self { nic, pending: 0, wr_ids: [0; N] }
    }

    /// Queue a work request; automatically flushes once the internal buffer is full.
    pub fn push(&mut self,
        wr_id: u64,
        kind: RdmaOpKind,
        local: VirtualAddress,
        remote: PhysicalAddress,
        len: usize) -> Result<(), NicError> {
        self.nic.post_work_request(wr_id, kind, local, remote, len)?;
        self.wr_ids[self.pending] = wr_id;
        self.pending += 1;
        if self.pending == N {
            self.flush()?;
        }
        Ok(())
    }

    /// Block until all queued work-requests are completed.
    pub fn flush(&mut self) -> Result<(), NicError> {
        let mut completed = 0;
        while completed < self.pending {
            let c = self.nic.poll_completions(N - completed, Some(Duration::from_millis(1)))?;
            completed += c.len();
        }
        self.pending = 0;
        Ok(())
    }
} 