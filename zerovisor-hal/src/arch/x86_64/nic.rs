//! InfiniBand / Omni-Path NIC backend (stub) – Task 9.2
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use core::time::Duration;
use spin::Mutex;

use crate::nic::{HpcNic, NicAttr, NicError, RdmaOpKind, RdmaCompletion};
use crate::memory::{PhysicalAddress, VirtualAddress};

/// Dummy NIC device that simulates completions for testing.
pub struct InfinibandNic {
    completions: Mutex<Vec<RdmaCompletion>>,
}

impl InfinibandNic {
    pub fn new() -> Self {
        Self { completions: Mutex::new(Vec::new()) }
    }
}

impl HpcNic for InfinibandNic {
    fn post_work_request(&self, wr_id: u64, _kind: RdmaOpKind, _local: VirtualAddress, _remote: PhysicalAddress, len: usize) -> Result<(), NicError> {
        // Immediately push fake completion
        let comp = RdmaCompletion { wr_id, status: Ok(()), bytes: len as u32 };
        self.completions.lock().push(comp);
        Ok(())
    }

    fn poll_completions(&self, _max: usize, _timeout: Option<Duration>) -> Result<&[RdmaCompletion], NicError> {
        // This stub backend does not yet support safe borrowing of the completion queue
        // with a lifetime that outlives the mutex guard. Until the full polling logic is
        // implemented we simply indicate that the operation is not supported.
        Err(NicError::NotSupported)
    }

    fn query_attr(&self) -> NicAttr {
        NicAttr { mtu: 4096, max_qp: 1024, max_wr: 16_384, link_speed_gbps: 200 }
    }
} 