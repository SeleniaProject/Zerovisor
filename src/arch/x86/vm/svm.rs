#![allow(dead_code)]

//! Minimal AMD SVM capability checks and VMCB preparation stubs.

use crate::arch::x86::cpuid;

/// SVM availability preflight (read-only).
pub fn svm_preflight_available() -> bool {
    cpuid::has_svm()
}

/// Prepare to enable SVM (stub; not executed yet).
pub fn svm_try_enable() -> Result<(), &'static str> {
    if !svm_preflight_available() { return Err("SVM not available"); }
    // Enabling SVM requires setting EFER.SVME and configuring a VMCB. The
    // actual enablement is deferred until memory management is ready.
    Ok(())
}


