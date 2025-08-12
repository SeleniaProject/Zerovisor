//! Virtualization entry layer (Intel VMX / AMD SVM) minimal scaffolding.

#[cfg(any(target_arch = "x86_64"))]
pub mod vmx;
#[cfg(any(target_arch = "x86_64"))]
pub mod svm;
#[cfg(any(target_arch = "x86_64"))]
pub mod vmcs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendor {
    Intel,
    Amd,
    Unknown,
}

pub fn detect_vendor() -> Vendor {
    // CPUID.0: vendor string
    let r = unsafe { core::arch::x86_64::__cpuid_count(0, 0) };
    let mut s = [0u8; 12];
    s[0..4].copy_from_slice(&r.ebx.to_le_bytes());
    s[4..8].copy_from_slice(&r.edx.to_le_bytes());
    s[8..12].copy_from_slice(&r.ecx.to_le_bytes());
    match &s {
        b"GenuineIntel" => Vendor::Intel,
        b"AuthenticAMD" => Vendor::Amd,
        _ => Vendor::Unknown,
    }
}


