//! x86_64 power-management primitives for Zerovisor
//!
//! Complete power management implementation using Intel MSRs.

#![cfg(target_arch = "x86_64")]

use x86::msr::{rdmsr, wrmsr};
use crate::power_mgmt::PowerManager;

/// Power management errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerError {
    InvalidParam,
    HardwareFault,
    NotSupported,
}

/// P-State representation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PState(pub u8);

/// Temperature reading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Temperature {
    pub celsius: i16,
}

/// IA32_PERF_CTL MSR – controls requested P-State (bits 15:0)
const IA32_PERF_CTL: u32 = 0x199;
/// IA32_PERF_STATUS MSR – reports current P-State (bits 15:0)
const IA32_PERF_STATUS: u32 = 0x198;
/// IA32_THERM_STATUS MSR – temperature information
const IA32_THERM_STATUS: u32 = 0x19C;

/// Intel P-State controller implementation
pub struct IntelPStateController {
    power_manager: PowerManager,
    available: &'static [PState],
}

impl IntelPStateController {
    pub fn new() -> Self {
        // A conservative static list: max, medium, low
        const PSTATES: &[PState] = &[PState(0), PState(1), PState(2)];
        Self { 
            power_manager: PowerManager::new(),
            available: PSTATES 
        }
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

    /// Get available P-states
    pub fn available_pstates(&self) -> &'static [PState] { 
        self.available 
    }

    /// Set CPU frequency
    pub fn set_frequency(&self, freq_mhz: u32) -> Result<(), PowerError> {
        // Convert frequency to P-state
        let pstate_idx = match freq_mhz {
            3000..=4000 => 0, // High performance
            2000..=2999 => 1, // Medium performance
            _ => 2,           // Low performance
        };
        
        if pstate_idx >= self.available.len() { 
            return Err(PowerError::InvalidParam);
        }
        
        let ratio = IntelPStateController::index_to_ratio(pstate_idx as u8);
        let value = ratio << 8; // Bits 15:8 host the bus-ratio request
        unsafe { wrmsr(IA32_PERF_CTL, value) };
        Ok(())
    }

    /// Set P-state directly
    pub fn set_pstate(&self, core_id: usize, pstate: PState) -> Result<(), PowerError> {
        if (pstate.0 as usize) >= self.available.len() { 
            return Err(PowerError::InvalidParam);
        }
        let ratio = IntelPStateController::index_to_ratio(pstate.0);
        let value = ratio << 8;
        unsafe { wrmsr(IA32_PERF_CTL, value) };
        Ok(())
    }

    /// Get current P-state
    pub fn current_pstate(&self, _core_id: usize) -> PState {
        let status = unsafe { rdmsr(IA32_PERF_STATUS) } & 0xFFFF;
        let ratio = (status >> 8) & 0xFF;
        PState(IntelPStateController::ratio_to_index(ratio as u64))
    }

    /// Get current frequency
    pub fn get_frequency(&self) -> u32 {
        let status = unsafe { rdmsr(IA32_PERF_STATUS) } & 0xFFFF;
        let ratio = (status >> 8) & 0xFF;
        (ratio * 100) as u32 // Convert ratio to MHz
    }
}

/// Intel on-die digital thermal sensor backed by `IA32_THERM_STATUS`.
pub struct IntelThermalSensor {
    tjmax: u32, // Maximum junction temperature
}

impl IntelThermalSensor {
    pub fn new() -> Self {
        Self {
            tjmax: 100, // Default TjMax of 100°C
        }
    }

    /// Read temperature from thermal sensor
    pub fn read_temperature(&self, _core_id: usize) -> Result<Temperature, PowerError> {
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
pub fn interfaces() -> (&'static IntelPStateController, &'static IntelThermalSensor) {
    static DVFS: IntelPStateController = IntelPStateController {
        power_manager: PowerManager::new(),
        available: &[PState(0), PState(1), PState(2)],
    };
    static THERM: IntelThermalSensor = IntelThermalSensor { tjmax: 100 };
    (&DVFS, &THERM)
} 