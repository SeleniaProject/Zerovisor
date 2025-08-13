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

/// Compose minimal NPT and return nested CR3 for smoke test purposes.
pub fn svm_prepare_npt(system_table: &uefi::table::SystemTable<uefi::prelude::Boot>, limit_bytes: u64) -> Option<u64> {
    let pml4 = crate::mm::npt::build_identity_2m(system_table, limit_bytes)?;
    Some(crate::mm::npt::ncr3_from_pml4(pml4 as u64))
}


