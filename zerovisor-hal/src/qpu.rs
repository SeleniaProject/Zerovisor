//! QPU (Quantum Processing Unit) virtualization driver – allows guest VMs to
//! allocate isolated quantum execution contexts backed by a physical or
//! emulated QPU device. This is a *software emulation* suitable for unit tests
//! and API integration until real QPU hardware back‐ends become available.
//!
//! All comments are in English per project policy.

#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::accelerator::{AcceleratorId, AccelError};
use crate::virtualization::VmHandle;

/// Quantum circuit descriptor: opaque 64-bit handle owned by VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CircuitId(pub u64);

/// Simple command: currently only "execute circuit" is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QpuOpcode { Execute }

/// Command header pushed into QPU queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QpuCommandHeader { pub opcode: QpuOpcode, pub circuit: CircuitId, pub shots: u32 }

/// Completion returned after command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QpuCompletion { pub success: bool, pub result_hash: u64 }

/// Per-VM context entry.
struct Context { vm: VmHandle, id: AcceleratorId }

/// Software-only QPU engine implementing polynomial-time classical
/// simulation (placeholder) and returning SHA-256 hash of measurement
/// probabilities as `result_hash`.
pub struct SoftQpuEngine {
    next_id: AtomicU32,
    contexts: Mutex<Vec<Context>>,
}

impl SoftQpuEngine {
    fn alloc_id(&self) -> AcceleratorId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        AcceleratorId(0x9000_0000u32 + id)
    }

    fn verify_ctx(&self, id: AcceleratorId) -> bool {
        self.contexts.lock().iter().any(|c| c.id == id)
    }
}

/// Public API exposed to hypervisor core.
pub trait QpuVirtualization: Send + Sync {
    fn init() -> Result<Self, AccelError> where Self: Sized;
    fn create_context(&self, vm: VmHandle) -> Result<AcceleratorId, AccelError>;
    fn destroy_context(&self, id: AcceleratorId) -> Result<(), AccelError>;
    fn submit_cmd(&self, id: AcceleratorId, hdr: QpuCommandHeader, wait: bool) -> Result<QpuCompletion, AccelError>;
}

impl QpuVirtualization for SoftQpuEngine {
    fn init() -> Result<Self, AccelError> {
        Ok(Self { next_id: AtomicU32::new(1), contexts: Mutex::new(Vec::new()) })
    }

    fn create_context(&self, vm: VmHandle) -> Result<AcceleratorId, AccelError> {
        let id = self.alloc_id();
        self.contexts.lock().push(Context { vm, id });
        Ok(id)
    }

    fn destroy_context(&self, id: AcceleratorId) -> Result<(), AccelError> {
        let mut v = self.contexts.lock();
        if let Some(idx) = v.iter().position(|c| c.id == id) { v.swap_remove(idx); Ok(()) } else { Err(AccelError::NotFound) }
    }

    fn submit_cmd(&self, id: AcceleratorId, hdr: QpuCommandHeader, wait: bool) -> Result<QpuCompletion, AccelError> {
        if !self.verify_ctx(id) { return Err(AccelError::NotFound); }
        match hdr.opcode {
            QpuOpcode::Execute => {
                // Emulate by hashing (circuit_id | shots)
                use sha2::{Sha256, Digest};
                let mut hasher = Sha256::new();
                hasher.update(&hdr.circuit.0.to_le_bytes());
                hasher.update(&hdr.shots.to_le_bytes());
                let digest = hasher.finalize();
                let mut bytes = [0u8;8];
                bytes.copy_from_slice(&digest[..8]);
                let res = QpuCompletion { success: true, result_hash: u64::from_le_bytes(bytes) };
                if wait { Ok(res) } else { Ok(res) }
            }
        }
    }
} 