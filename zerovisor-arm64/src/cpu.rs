//! ARM64 CPU helpers – detects virtualization support, enables HCR_EL2 and preparatory settings.
//! Only compiled on `aarch64` target; on other architectures this module is an empty stub.

#![cfg(target_arch = "aarch64")]
#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::HalError;

/// Return true if the current core supports EL2 virtualization.
pub fn has_virtualization_support() -> bool {
    // ID_AA64PFR0_EL1[11:8] indicates EL2 support (0b0001 = present).
    let val: u64;
    unsafe { core::arch::asm!("mrs {0}, id_aa64pfr0_el1", out(reg) val) };
    ((val >> 8) & 0xF) >= 1
}

/// CPU early init – set HCR_EL2 default bits and enable traps.
pub fn init() -> Result<(), HalError> {
    if !has_virtualization_support() { return Err(HalError::HardwareNotSupported); }

    unsafe {
        // Set HCR_EL2 to a safe default: RW=64, AMO=1, IMO=1, FMO=1, TGE=1 (trap guest EL0/EL1).
        const HCR_RW: u64 = 1 << 31;    // 64-bit EL1
        const HCR_AMO: u64 = 1 << 5;    // Trap SError to EL2
        const HCR_IMO: u64 = 1 << 4;    // Trap IRQ
        const HCR_FMO: u64 = 1 << 3;    // Trap FIQ
        const HCR_TGE: u64 = 1 << 27;   // Trap General exceptions
        let hcr = HCR_RW | HCR_AMO | HCR_IMO | HCR_FMO | HCR_TGE;
        core::arch::asm!("msr hcr_el2, {0}", in(reg) hcr, options(nostack, preserves_flags));
        core::arch::asm!("isb");
    }
    Ok(())
} 