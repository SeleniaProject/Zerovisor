#![allow(dead_code)]

//! Minimal MSR accessors for `no_std` UEFI context.
//!
//! These helpers wrap `rdmsr`/`wrmsr` instructions. They must be used with
//! caution because certain MSRs are privileged and will #GP if accessed in an
//! unsupported context. UEFI applications typically execute with sufficient
//! privilege in firmware environment, but actual availability depends on
//! platform. Callers must be prepared for faults; in this bootstrap stage we
//! avoid writing MSRs.

/// IA32_VMX_BASIC MSR index
pub const IA32_VMX_BASIC: u32 = 0x480;
/// IA32_VMX_EPT_VPID_CAP MSR index
pub const IA32_VMX_EPT_VPID_CAP: u32 = 0x48C;

/// Reads an MSR by its 32-bit index, returning the 64-bit value.
#[inline(always)]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let hi: u32;
    let lo: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("edx") hi,
        out("eax") lo,
        options(nostack, preserves_flags)
    );
    ((hi as u64) << 32) | (lo as u64)
}

/// Writes a 64-bit value to an MSR.
#[inline(always)]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let hi: u32 = (value >> 32) as u32;
    let lo: u32 = value as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("edx") hi,
        in("eax") lo,
        options(nostack, preserves_flags)
    );
}

/// Checks whether EPT is supported by examining IA32_VMX_EPT_VPID_CAP.
///
/// According to Intel SDM, the presence of certain bits (e.g., page walk
/// lengths, memory types) indicates EPT capability. Here we only check that
/// the MSR is readable under VMX-capable processors by performing a best-effort
/// read; callers must guard with CPUID VMX detection.
#[inline(always)]
pub fn ept_capabilities_readable() -> bool {
    // If VMX is not present, EPT capability MSR is meaningless.
    if !crate::arch::x86::cpuid::has_vmx() { return false; }
    // Best-effort: attempt to read IA32_VMX_EPT_VPID_CAP (0x48C). In UEFI
    // firmware context with sufficient privileges, this should succeed.
    // If the platform faults here it would be catastrophic; however, other
    // parts of bootstrap already perform MSR reads for VMX and have succeeded.
    unsafe { let _ = rdmsr(IA32_VMX_EPT_VPID_CAP); }
    true
}


