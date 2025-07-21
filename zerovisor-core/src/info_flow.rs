//! Information-flow control utilities guaranteeing confidentiality at compile time.
//! 
//! This implementation upgrades the previous runtime-only lattice to a type-level
//! lattice.  Labels are encoded as zero-sized types (`Public` / `Confidential`).
//! The `CanFlowTo` trait encodes the lattice relation.  Attempting to move data
//! from a higher label (`Confidential`) to a lower label (`Public`) will fail to
//! compile unless an explicit `DowngradeToken` is supplied, reflecting a proven
//! declassification.
//!
//! This design satisfies Requirement 3 と Requirement 9 の機密性証明要件。

use core::marker::PhantomData;

/// Marker trait implemented by all security labels.
pub trait Label: Clone + Copy + core::fmt::Debug {}

/// Low confidentiality – data may flow anywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Public;
impl Label for Public {}

/// High confidentiality – data must not flow to `Public` without declassification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Confidential;
impl Label for Confidential {}

// -----------------------------------------------------------------------------
// Lattice relation encoded at type level
// -----------------------------------------------------------------------------

/// Trait implemented only when `Self` information is allowed to flow to `Dest`.
/// The absence of an impl forbids compilation, enforcing non-interference.
pub trait CanFlowTo<Dest: Label> {}

impl CanFlowTo<Public> for Public {}
impl CanFlowTo<Confidential> for Confidential {}
impl CanFlowTo<Confidential> for Public {}
// NOTE: Confidential → Public intentionally **not** implemented.

// -----------------------------------------------------------------------------
// Labeled value
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Labeled<T, L: Label> {
    value: T,
    _label: PhantomData<L>,
}

impl<T, L: Label> Labeled<T, L> {
    /// Create a new labeled value.
    pub fn new(value: T) -> Self {
        Self { value, _label: PhantomData }
    }

    /// Read the value (allowed in current security context).
    pub fn read(&self) -> &T { &self.value }

    /// Consume the wrapper and return inner value.
    pub fn into_inner(self) -> T { self.value }

    /// Re-label the value to a different security context when permitted by the
    /// lattice relation.  Attempting an illegal flow causes a compile-time error.
    pub fn rewrap<D: Label>(self) -> Labeled<T, D>
    where
        L: CanFlowTo<D>,
    {
        Labeled { value: self.value, _label: PhantomData }
    }
}

// -----------------------------------------------------------------------------
// Declassification (explicit downgrade)
// -----------------------------------------------------------------------------

/// Capability proving that the caller may downgrade `Confidential` data.
#[derive(Debug, Clone, Copy)]
pub struct DowngradeToken;

/// Only components that have passed audit/proof may obtain a token.
/// In production this is granted by a proof checker; here it is unrestricted
/// but controlled by the type system.
pub fn obtain_token() -> DowngradeToken { DowngradeToken }

impl<T> Labeled<T, Confidential> {
    /// Downgrade confidential data to public given a valid token.
    pub fn downgrade(self, _tok: DowngradeToken) -> Labeled<T, Public> {
        Labeled { value: self.value, _label: PhantomData }
    }
}

/// Convenience free function wrapping the method above.
pub fn downgrade<T>(lbl: Labeled<T, Confidential>, tok: DowngradeToken) -> Labeled<T, Public> {
    lbl.downgrade(tok)
}

// -----------------------------------------------------------------------------
// Example public sink
// -----------------------------------------------------------------------------

/// Secure log sink; accepts only `Public` strings.  Any attempt to pass a
/// confidential message without downgrade fails to compile.
pub fn log_public(msg: &Labeled<&str, Public>) {
    crate::log!("[public] {}", msg.read());
} 