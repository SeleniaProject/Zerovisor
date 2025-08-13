#![allow(dead_code)]

//! Minimal CPUID utilities for feature detection in a `no_std` UEFI context.
//!
//! This module provides very small wrappers around the `cpuid` instruction to
//! detect virtualization extensions that are relevant for a hypervisor.

/// CPUID leaf constants commonly used for feature detection.
pub mod leaf {
    /// Basic feature flags (EAX=1).
    pub const BASIC_FEATURES: u32 = 0x0000_0001;

    /// AMD extended features (EAX=0x80000001).
    pub const AMD_EXT_FEATURES: u32 = 0x8000_0001;

    /// AMD SVM features (EAX=0x8000000A).
    pub const AMD_SVM: u32 = 0x8000_000A;
    /// Advanced Power Management Information (EAX=0x80000007).
    pub const AMD_APM: u32 = 0x8000_0007;
}

/// Result of a `cpuid` call.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// Executes the `cpuid` instruction with the given `eax` and `ecx` using the
/// architecture intrinsic to avoid inline-asm register constraints.
#[inline(always)]
pub fn cpuid(eax: u32, ecx: u32) -> CpuidResult {
    let r = unsafe { core::arch::x86_64::__cpuid_count(eax, ecx) };
    CpuidResult { eax: r.eax, ebx: r.ebx, ecx: r.ecx, edx: r.edx }
}

/// Indicates the presence of Intel VMX by CPUID.1:ECX.VMX [bit 5].
#[inline(always)]
pub fn has_vmx() -> bool {
    // CPUID.1:ECX bit 5 = VMX
    let r = cpuid(leaf::BASIC_FEATURES, 0);
    (r.ecx & (1 << 5)) != 0
}

/// Indicates the presence of AMD SVM by CPUID.80000001:ECX.SVM [bit 2].
#[inline(always)]
pub fn has_svm() -> bool {
    // CPUID.80000001:ECX bit 2 = SVM
    let r = cpuid(leaf::AMD_EXT_FEATURES, 0);
    (r.ecx & (1 << 2)) != 0
}

/// Indicates Intel EPT support via VMX capability MSR check.
///
/// Note: There is no direct CPUID bit for EPT presence in basic leaves. Intel
/// specifies EPT capabilities in IA32_VMX_EPT_VPID_CAP (MSR 0x48C). This
/// function only reports `true` if VMX is present; actual EPT capability check
/// must read the MSR. This method is kept separate for clarity.
#[inline(always)]
pub fn may_support_ept() -> bool {
    has_vmx()
}

/// Indicates AMD NPT support via SVM feature leaf 0x8000000A.
#[inline(always)]
pub fn has_npt() -> bool {
    // CPUID.8000000A: EDX[bit 0] Nested Paging support
    // According to AMD64 APM Vol.3, SVM feature identification.
    let r = cpuid(leaf::AMD_SVM, 0);
    (r.edx & (1 << 0)) != 0
}

/// Indicates presence of Invariant TSC via CPUID.80000007:EDX[8].
#[inline(always)]
pub fn has_invariant_tsc() -> bool {
    let r = cpuid(leaf::AMD_APM, 0);
    (r.edx & (1 << 8)) != 0
}

/// Indicates presence of x2APIC via CPUID.1:ECX[21].
#[inline(always)]
pub fn has_x2apic() -> bool {
    let r = cpuid(leaf::BASIC_FEATURES, 0);
    (r.ecx & (1 << 21)) != 0
}


