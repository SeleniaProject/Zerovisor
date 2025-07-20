// Remote attestation module – Task 4.2
//! Remote attestation engine for Zerovisor (Requirement 8.4)
//!
//! Generates and verifies attestation reports that prove the software
//! integrity of the hypervisor to a remote verifier using quantum-resistant
//! Dilithium signatures.
//!
//! Design choices:
//! • Hypervisor measurement is a SHA-256 digest of a fixed build-time
//!   identifier plus optional runtime metrics. In production this would be a
//!   cryptographic measurement of all executable pages.
//! • Random nonces are derived from the timestamp counter combined with a
//!   simple xorshift PRNG – good enough for uniqueness in early boot. A true
//!   DRBG should replace this before release.
//! • Dilithium5 keys are used (128-bit security level).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use sha2::{Digest, Sha256};
use crate::crypto::{dilithium_generate, dilithium_sign, dilithium_verify, DilithiumKeypair};
use crate::scheduler::get_cycle_counter;

/// Fixed build-time identifier included in the measurement.
const BUILD_ID: &[u8] = b"Zerovisor Core Build 0.1.0";

/// Attestation report structure.
#[derive(Debug, Clone)]
pub struct AttestationReport {
    /// 32-byte nonce supplied by verifier (or generated locally for self-check).
    pub nonce: [u8; 32],
    /// Monotonic timestamp in nanoseconds when the report was created.
    pub timestamp_ns: u64,
    /// SHA-256 digest of the hypervisor measurement.
    pub hv_measurement: [u8; 32],
    /// Dilithium detached signature over (nonce || timestamp || measurement).
    pub signature: Vec<u8>,
}

/// Remote attestation engine owning a Dilithium keypair.
pub struct RemoteAttestation {
    keypair: DilithiumKeypair,
}

impl RemoteAttestation {
    /// Generate a new keypair. In real deployments the secret key would be
    /// provisioned in secure storage instead of freshly generated.
    pub fn new() -> Self {
        Self { keypair: dilithium_generate() }
    }

    /// Return the public key so verifiers can validate reports.
    pub fn public_key(&self) -> &[u8] { &self.keypair.public }

    /// Produce a signed attestation report for the given verifier-supplied
    /// `nonce`. When `nonce` is `None`, a fresh pseudo-random nonce is used.
    pub fn generate_report(&self, nonce: Option<[u8; 32]>) -> AttestationReport {
        let nonce_val = nonce.unwrap_or_else(gen_nonce);
        let ts = crate::scheduler::cycles_to_nanoseconds(get_cycle_counter());
        let measurement = compute_measurement();

        // Build message to be signed: nonce || timestamp || measurement
        let mut msg = Vec::with_capacity(32 + 8 + 32);
        msg.extend_from_slice(&nonce_val);
        msg.extend_from_slice(&ts.to_le_bytes());
        msg.extend_from_slice(&measurement);

        let sig = dilithium_sign(&self.keypair.secret, &msg);

        AttestationReport { nonce: nonce_val, timestamp_ns: ts, hv_measurement: measurement, signature: sig }
    }

    /// Verify an attestation report with the given public key. Returns true
    /// if the signature is valid *and* the measurement matches the expected
    /// hypervisor measurement for this build.
    pub fn verify_report(report: &AttestationReport, public_key: &[u8]) -> bool {
        let mut msg = Vec::with_capacity(32 + 8 + 32);
        msg.extend_from_slice(&report.nonce);
        msg.extend_from_slice(&report.timestamp_ns.to_le_bytes());
        msg.extend_from_slice(&report.hv_measurement);

        if !dilithium_verify(public_key, &msg, &report.signature) {
            return false;
        }

        // Compare measurement with locally computed one.
        compute_measurement() == report.hv_measurement
    }
}

/// Compute hypervisor measurement (SHA-256 digest).
fn compute_measurement() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(BUILD_ID);
    // Additional runtime metrics can be included here (e.g., config hash)
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Simple xorshift-based pseudo-random generator for nonces (fallback).
fn gen_nonce() -> [u8; 32] {
    let mut x = get_cycle_counter();
    let mut out = [0u8; 32];
    for chunk in out.chunks_mut(8) {
        // xorshift64*
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        chunk.copy_from_slice(&x.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attestation_roundtrip() {
        let engine = RemoteAttestation::new();
        let report = engine.generate_report(None);
        assert!(RemoteAttestation::verify_report(&report, engine.public_key()));
    }
} 