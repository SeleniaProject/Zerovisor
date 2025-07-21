//! High-performance NIC abstraction for RDMA / exascale networking (Task 9.2)
//! Provides a unified interface for InfiniBand, Omni-Path, and future fabrics.

#![allow(dead_code)]

use core::time::Duration;
use crate::memory::{PhysicalAddress, VirtualAddress};

/// Error codes for NIC operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicError {
    NotSupported,
    InvalidParam,
    QueueFull,
    Timeout,
    HardwareFault,
    SerializeError,
}

/// RDMA work request kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdmaOpKind {
    Send,
    Recv,
    Read,
    Write,
    AtomicCompareSwap,
}

/// Completion entry returned by NIC after work completion.
#[derive(Debug, Clone, Copy)]
pub struct RdmaCompletion {
    pub wr_id: u64,
    pub status: Result<(), NicError>,
    pub bytes: u32,
}

/// Trait representing a high-performance RDMA capable NIC.
/// Implemented by architecture specific back-ends.
pub trait HpcNic: Send + Sync {
    /// Submit a work request to the NIC.
    fn post_work_request(&self,
        wr_id: u64,
        kind: RdmaOpKind,
        local: VirtualAddress,
        remote: PhysicalAddress,
        len: usize) -> Result<(), NicError>;

    /// Poll for completions with optional timeout.
    fn poll_completions(&self, max: usize, timeout: Option<Duration>) -> Result<&[RdmaCompletion], NicError>;

    /// Query NIC attributes (MTU, max_qp, etc.).
    fn query_attr(&self) -> NicAttr;
}

/// Basic NIC attributes.
#[derive(Debug, Clone, Copy)]
pub struct NicAttr {
    pub mtu: u32,
    pub max_qp: u32,
    pub max_wr: u32,
    pub link_speed_gbps: u32,
} 