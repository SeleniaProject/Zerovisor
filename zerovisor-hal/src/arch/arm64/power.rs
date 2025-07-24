//! ARM64 power-management primitives – generic implementation using PSCI & SysFS style registers
//! This is a minimal controller that supports three discrete performance states
//! via the `CNTFRQ_EL0` frequency divider as an example. Thermal sensor returns
//! a fixed value until SoC-specific MMIO addresses are integrated.

#![cfg(target_arch = "aarch64")]

use crate::power::{DvfsController, ThermalSensor, PState, Temperature, PowerError};
use core::ptr::read_volatile;

/// Dummy PSCI performance level register (emulated)
static mut CURRENT_PSTATE: u8 = 0;

pub struct ArmDvfsController {
    available: &'static [PState],
}

impl ArmDvfsController {
    pub const fn new() -> Self {
        const PSTATES: &[PState] = &[PState(0), PState(1), PState(2)];
        Self { available: PSTATES }
    }
}

impl DvfsController for ArmDvfsController {
    fn available_pstates(&self) -> &'static [PState] { self.available }

    fn set_pstate(&self, _core_id: usize, pstate: PState) -> Result<(), PowerError> {
        if (pstate.0 as usize) >= self.available.len() { return Err(PowerError::InvalidParam); }
        unsafe { CURRENT_PSTATE = pstate.0; }
        Ok(())
    }

    fn current_pstate(&self, _core_id: usize) -> PState {
        let idx = unsafe { CURRENT_PSTATE };
        PState(idx)
    }
}

/// SoC-specific thermal sensor MMIO base (can be overridden at runtime).
#[cfg(any(feature = "arm_tsensor_mmio", doc))]
const TSENSOR_BASE: usize = 0x2A43_6000; // Example base address for TSENS
#[cfg(any(feature = "arm_tsensor_mmio", doc))]
const TSENSOR_TEMP_OFFSET: usize = 0x00; // Temperature register offset

#[cfg(any(feature = "arm_tsensor_mmio", doc))]
static mut TSENSOR_RUNTIME_BASE: usize = TSENSOR_BASE;

/// Allow early boot code to provide the actual MMIO base discovered via DT/ACPI.
#[cfg(any(feature = "arm_tsensor_mmio", doc))]
pub fn set_tsensor_base(base: usize) { unsafe { TSENSOR_RUNTIME_BASE = base; } }

pub struct ArmThermalSensor;

impl ThermalSensor for ArmThermalSensor {
    fn read_temperature(&self, _core_id: usize) -> Result<Temperature, PowerError> {
        #[cfg(any(feature = "arm_tsensor_mmio", doc))]
        unsafe {
            let addr = (TSENSOR_RUNTIME_BASE + TSENSOR_TEMP_OFFSET) as *const u32;
            let raw = read_volatile(addr);
            // Assume linear mapping: raw value = temperature in milli-celsius.
            let milli = raw as i32;
            let c = (milli / 1000) as i16;
            return Ok(Temperature { celsius: c });
        }
        #[cfg(not(any(feature = "arm_tsensor_mmio", doc)))]
        {
            // Fallback fixed value until real sensor mapped.
            Ok(Temperature { celsius: 45 })
        }
    }
}

/// Export trait objects
pub fn interfaces() -> (&'static dyn DvfsController, &'static dyn ThermalSensor) {
    static DVFS: ArmDvfsController = ArmDvfsController::new();
    static THERM: ArmThermalSensor = ArmThermalSensor;
    (&DVFS, &THERM)
} 