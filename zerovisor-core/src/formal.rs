//! Runtime orchestrator for formal verification checks.
//!
//! This module centralizes every proof and model-checking harness that can
//! be executed *at runtime* (or immediately after boot) when the corresponding
//! Cargo feature flags are enabled.  Failing a check results in a fatal error
//! early in the initialization sequence so that a compromised build **never**
//! proceeds to run guest code.

#![cfg(any(feature = "formal_verification", feature = "coq_proofs"))]

/// Execute *all* enabled formal verification checks.
///
/// On success the function returns `Ok(())`.  If *any* proof or refinement
/// fails an `Err` string containing a human-readable description is returned.
#[inline(always)]
pub fn run_all() -> Result<(), &'static str> {
    // --------------------------------------------------------------
    // TLA+ model-checking harnesses (feature = "formal_verification")
    // --------------------------------------------------------------
    #[cfg(feature = "formal_verification")]
    {
        use crate::formal_tests::{
            verify_cluster_fault_tolerance, verify_memory_safety,
        };

        if !verify_memory_safety() {
            return Err("TLA+ memory safety refinement failed");
        }
        if !verify_cluster_fault_tolerance() {
            return Err("TLA+ cluster fault-tolerance refinement failed");
        }
    }

    // --------------------------------------------------------------
    // Coq extracted proofs (feature = "coq_proofs")
    // --------------------------------------------------------------
    #[cfg(feature = "coq_proofs")]
    {
        extern "C" {
            fn verify_memory_safety_proof() -> bool;
            fn verify_isolation_proof() -> bool;
            fn verify_real_time_proof() -> bool;
        }
        unsafe {
            if !verify_memory_safety_proof() {
                return Err("Coq memory safety proof failed");
            }
            if !verify_isolation_proof() {
                return Err("Coq isolation proof failed");
            }
            if !verify_real_time_proof() {
                return Err("Coq real-time proof failed");
            }
        }
    }

    Ok(())
} 