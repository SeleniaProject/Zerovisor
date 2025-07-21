//! Cross-Architecture CPU state translation traits and stubs
//! Used by live migration layer to convert guest CPU/VMCS/VMSA state between
//! different ISAs (x86_64 ↔ ARM64 ↔ RISC-V).  The real implementation must
//! enumerate and map each register field; here we provide compile-time stubs
//! so that higher-level code can be integrated incrementally.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

/// Abstract serialisable CPU state blob (architecture-specific encoding).
#[derive(Debug, Clone)]
pub struct CpuStateBlob(pub Vec<u8>);

/// ID of architecture for migration negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchId { X86_64, Arm64, Riscv64 }

/// Translate CPU state from source ISA to destination ISA.
pub trait ArchStateTranslator {
    /// Return true if this translator supports given source/destination pair.
    fn supports(src: ArchId, dst: ArchId) -> bool where Self: Sized;

    /// Convert `input` blob (encoded in `src` format) into `dst` format.
    fn translate(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob where Self: Sized;
}

/// Fallback identity translator – used when src == dst.
pub struct IdentityTranslator;

impl ArchStateTranslator for IdentityTranslator {
    fn supports(src: ArchId, dst: ArchId) -> bool { src == dst }
    fn translate(_s: ArchId, _d: ArchId, blob: &CpuStateBlob) -> CpuStateBlob { blob.clone() }
} 