#![cfg(target_arch = "aarch64")]
#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::HalError;

/// Initialise Generic Interrupt Controller in EL2.
pub fn init() -> Result<(), HalError> { Ok(()) } 