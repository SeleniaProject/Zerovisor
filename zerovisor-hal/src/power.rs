//! Dynamic power and thermal management (Task 10.1/10.2)
//! Provides DVFS and temperature monitoring hooks.

#![allow(dead_code)]

use core::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerError { Unsupported, InvalidParam, HardwareFault }

/// CPU performance state identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PState(pub u8);

/// Trait for per-core DVFS control.
pub trait DvfsController: Send + Sync {
    fn available_pstates(&self) -> &'static [PState];
    fn set_pstate(&self, core_id: usize, pstate: PState) -> Result<(), PowerError>;
    fn current_pstate(&self, core_id: usize) -> PState;
}

/// Thermal sensor reading.
#[derive(Debug, Clone, Copy)]
pub struct Temperature { pub celsius: i16 }

/// Trait for temperature monitoring.
pub trait ThermalSensor: Send + Sync {
    fn read_temperature(&self, core_id: usize) -> Result<Temperature, PowerError>;
} 