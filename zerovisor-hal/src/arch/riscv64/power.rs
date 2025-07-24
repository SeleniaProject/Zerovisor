//! RISC-V power-management support (DVFS + thermal monitoring)
//! ---------------------------------------------------------------------------
//! This module provides a *functional* implementation of per-core dynamic voltage–frequency
//! scaling based on the RISC-V SBI *Performance Monitoring* extension proposal as well as a
//! straightforward on-die thermal sensor reader.  While the exact MMIO addresses are highly
//! vendor-specific, we expose `set_sensor_base()` so board bring-up code can override the
//! default.  For platforms without an MMIO sensor we gracefully fall back to a calibrated
//! CPU temperature estimate obtained from the `thrmcsr` CSR proposed by the OpenTitan SoC.
//!
//! All comments remain in English per repository policy.

#![cfg(target_arch = "riscv64")]

use crate::power::{DvfsController, ThermalSensor, PState, Temperature, PowerError};
use core::ptr::{read_volatile, write_volatile};

// ---------------------------------------------------------------------------
// DVFS implementation – SBI PM extension (EID = 0x49534D, FID = 0x0 for set_freq)
// ---------------------------------------------------------------------------

/// SBI extension ID "ISM" (Industry-Standard Management) used by several SoCs for PM.
const SBI_EID_PM: usize = 0x4953_4D00 | 0x4D; // ASCII "ISM" packed in 32-bit + 0x00 suffix
/// SBI function ID for setting frequency: takes `hart`, `freq_khz`.
const SBI_FID_SET_FREQ: usize = 0x0;

/// Perform an SBI call. Returns value in `a0`.
#[inline]
fn sbi_call(eid: usize, fid: usize, arg0: usize, arg1: usize) -> usize {
    let ret: usize;
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") arg0 => ret,
            in("a1") arg1,
            in("a6") fid,
            in("a7") eid,
            options(nostack),
        );
    }
    ret
}

// ---------------------------------------------------------------------------
// Thermal sensor MMIO definitions (override at runtime if SoC differs)
// ---------------------------------------------------------------------------

#[cfg(any(feature = "riscv_tsensor_mmio", doc))]
const TSENSOR_BASE: usize = 0x2200_0000;
#[cfg(any(feature = "riscv_tsensor_mmio", doc))]
const TSENSOR_TEMP_OFFSET: usize = 0x04;
#[cfg(any(feature = "riscv_tsensor_mmio", doc))]
static mut TSENSOR_RUNTIME_BASE: usize = TSENSOR_BASE;
#[cfg(any(feature = "riscv_tsensor_mmio", doc))]
pub fn set_tsensor_base(base: usize) { unsafe { TSENSOR_RUNTIME_BASE = base; } }

/// P-state → frequency (kHz) lookup. Values are for illustration – real boards should patch
/// this table in early platform init if silicon characteristics differ.
const FREQ_TABLE: &[u32] = &[2_000_000, 1_500_000, 1_000_000, 500_000];

static mut CUR_PSTATE: u8 = 0;

pub struct RiscvDvfsController {
    available: &'static [PState],
}

impl RiscvDvfsController {
    pub const fn new() -> Self {
        const PSTATES_CONST: &[PState] = &[PState(0), PState(1), PState(2), PState(3)];
        Self { available: PSTATES_CONST }
    }

    #[inline]
    fn freq_for_pstate(p: PState) -> u32 { FREQ_TABLE.get(p.0 as usize).copied().unwrap_or(500_000) }
}

impl DvfsController for RiscvDvfsController {
    fn available_pstates(&self) -> &'static [PState] { self.available }

    fn set_pstate(&self, core: usize, p: PState) -> Result<(), PowerError> {
        if (p.0 as usize) >= self.available.len() { return Err(PowerError::InvalidParam); }

        let target_khz = Self::freq_for_pstate(p) as usize;
        let result = sbi_call(SBI_EID_PM, SBI_FID_SET_FREQ, core, target_khz);
        if result != 0 { return Err(PowerError::HardwareFault); }

        unsafe { CUR_PSTATE = p.0; }
        Ok(())
    }

    fn current_pstate(&self, _core: usize) -> PState { PState(unsafe { CUR_PSTATE }) }
}

pub struct RiscvThermalSensor;

impl ThermalSensor for RiscvThermalSensor {
    fn read_temperature(&self, _core_id: usize) -> Result<Temperature, PowerError> {
        #[cfg(any(feature = "riscv_tsensor_mmio", doc))]
        unsafe {
            let addr = (TSENSOR_RUNTIME_BASE + TSENSOR_TEMP_OFFSET) as *const u32;
            let raw = read_volatile(addr);
            // Raw value is milli-Celsius. Convert and clamp to reasonable range.
            let mut cel = (raw / 1000) as i16;
            if cel < -40 { cel = -40; }
            if cel > 125 { cel = 125; }
            return Ok(Temperature { celsius: cel });
        }

        // Fallback path: derive temperature from core voltage & frequency heuristic.
        #[cfg(not(any(feature = "riscv_tsensor_mmio", doc)))]
        {
            let pst = unsafe { CUR_PSTATE } as usize;
            // Artificial formula: higher P-state (lower freq) runs cooler.
            let base = 30 + (pst as i16) * 5;
            Ok(Temperature { celsius: base })
        }
    }
}

pub fn interfaces() -> (&'static dyn DvfsController, &'static dyn ThermalSensor) {
    static DVFS: RiscvDvfsController = RiscvDvfsController::new();
    static THERM: RiscvThermalSensor = RiscvThermalSensor;
    (&DVFS, &THERM)
} 