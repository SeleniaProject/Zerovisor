//! x86_64 power-management primitives for Zerovisor
//!
//! Implements `DvfsController` and `ThermalSensor` using Intel MSRs.
//! The implementation is intentionally *stand-alone* and does *not* rely on
//! firmware interfaces so that Zerovisor can control power states already at
//! boot time.

#![cfg(target_arch = "x86_64")]

use x86::msr::{rdmsr, wrmsr};
use crate::power::{DvfsController, ThermalSensor, PState, Temperature, PowerError};

/// IA32_PERF_CTL MSR – controls requested P-State (bits 15:0)
const IA32_PERF_CTL: u32 = 0x199;
/// IA32_PERF_STATUS MSR – reports current P-State (bits 15:0)
const IA32_PERF_STATUS: u32 = 0x198;
/// IA32_THERM_STATUS MSR – temperature information
const IA32_THERM_STATUS: u32 = 0x19C;

/// Intel Skylake-class processors typically expose P-states as the *bus ratio*
/// encoded in the lower 16 bits.  We abstract that into simple `PState(index)`
/// values where *index 0* is the *highest* performance (max ratio) and *index n*
/// the lowest.
pub struct IntelPStateController {
    available: &'static [PState],
}

impl IntelPStateController {
    pub const fn new() -> Self {
        // A conservative static list: max, medium, low
        const PSTATES: &[PState] = &[PState(0), PState(1), PState(2)];
        Self { available: PSTATES }
    }

    /// Convert a logical PState index into the raw *bus ratio* value.
    /// This simple mapping can be refined per-platform.
    #[inline(always)]
    fn index_to_ratio(idx: u8) -> u64 {
        // Max ratio assumed 0x28 (4.0 GHz @100 MHz BCLK).  Each step scales −400 MHz.
        const MAX_RATIO: u64 = 0x28;
        MAX_RATIO.saturating_sub(idx as u64 * 4)
    }

    #[inline(always)]
    fn ratio_to_index(ratio: u64) -> u8 {
        const MAX_RATIO: u64 = 0x28;
        let diff = MAX_RATIO.saturating_sub(ratio);
        (diff / 4) as u8
    }
}

impl DvfsController for IntelPStateController {
    fn available_pstates(&self) -> &'static [PState] { self.available }

    fn set_pstate(&self, core_id: usize, pstate: PState) -> Result<(), PowerError> {
        // SMP systems require per-core MSR programming – we assume the caller
        // is already running on `core_id` or that MSR accesses are broadcast.
        if (pstate.0 as usize) >= self.available.len() { return Err(PowerError::InvalidParam) }
        let ratio = Self::index_to_ratio(pstate.0);
        let value = ratio << 8; // Bits 15:8 host the bus-ratio request
        unsafe { wrmsr(IA32_PERF_CTL, value) };
        Ok(())
    }

    fn current_pstate(&self, _core_id: usize) -> PState {
        let status = unsafe { rdmsr(IA32_PERF_STATUS) } & 0xFFFF;
        let ratio = (status >> 8) & 0xFF;
        PState(Self::ratio_to_index(ratio as u64))
    }
}

/// Intel on-die digital thermal sensor backed by `IA32_THERM_STATUS`.
pub struct IntelThermalSensor;

impl ThermalSensor for IntelThermalSensor {
    fn read_temperature(&self, _core_id: usize) -> Result<Temperature, PowerError> {
        // Bit 31 "Valid" must be set for a meaningful reading.
        let value = unsafe { rdmsr(IA32_THERM_STATUS) };
        if (value & (1 << 31)) == 0 {
            return Err(PowerError::HardwareFault);
        }
        let tj_offset = ((value >> 16) & 0x7F) as i16; // distance to Tj-max
        const TJ_MAX: i16 = 100; // Conservative constant until calibrated
        Ok(Temperature { celsius: TJ_MAX - tj_offset })
    }
}

/// Provide architecture-specific power interfaces as trait objects.
pub fn interfaces() -> (&'static dyn DvfsController, &'static dyn ThermalSensor) {
    static DVFS: IntelPStateController = IntelPStateController::new();
    static THERM: IntelThermalSensor = IntelThermalSensor;
    (&DVFS, &THERM)
} 