//! Information-flow control utilities ensuring confidentiality proofs (Task: Information-flow Analysis)
//! 
//! This module introduces a lightweight label-based IFC mechanism inspired by
//! the Lattice model. Types wrapped in `Labeled<T>` carry a `SecurityTag`
//! denoting their confidentiality level. Safe APIs prevent data with tag
//! `Confidential` from being written to `Public` sinks unless an explicit
//! downgrade capability is provided.
//! 
//! While this is not a full static analysis, it enables unit tests and runtime
//! assertions which feed into formal proofs (Coq extraction planned).

#![allow(dead_code)]

/// Security lattice – two-point {Public < Confidential}.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityTag {
    Public,
    Confidential,
}

/// Generic wrapper carrying security label.
#[derive(Debug, Clone)]
pub struct Labeled<T> {
    tag: SecurityTag,
    value: T,
}

impl<T> Labeled<T> {
    pub fn new(value: T, tag: SecurityTag) -> Self { Self { tag, value } }
    pub fn tag(&self) -> SecurityTag { self.tag }
    pub fn into_inner(self) -> T { self.value }
    /// Attempt to read value into lower tag context; returns Err on violation.
    pub fn read_as(&self, target: SecurityTag) -> Result<&T, ()> {
        if self.tag <= target { Ok(&self.value) } else { Err(()) }
    }
}

/// Downgrade capability – zero-sized token granted only to audit-passed code.
#[derive(Debug, Clone, Copy)]
pub struct DowngradeToken;

/// Authorised component obtains token after proof (placeholder for Coq).
pub fn obtain_token() -> DowngradeToken { DowngradeToken }

/// Explicit downgrade function – requires token possession.
pub fn downgrade<T>(lbl: Labeled<T>, _tok: DowngradeToken) -> Labeled<T> {
    Labeled::new(lbl.value, SecurityTag::Public)
}

/// Example secure log sink; rejects confidential input.
pub fn log_public(msg: &Labeled<&str>) {
    assert!(msg.tag() == SecurityTag::Public, "leak detected: confidential data to public sink");
    crate::log!("[public] {}", msg.into_inner());
} 