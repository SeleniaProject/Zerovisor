#![cfg(target_arch = "aarch64")]
#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::HalError;

pub fn init() -> Result<(), HalError> {
    // Enable physical timer at EL2: CNTVOFF_EL2 = 0
    unsafe { core::arch::asm!("msr cntvoff_el2, xzr", options(nostack, preserves_flags)); }
    Ok(())
} 