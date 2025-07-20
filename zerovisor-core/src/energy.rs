//! Energy and thermal management subsystem (Task 10)

#![allow(dead_code)]

use spin::Once;
use zerovisor_hal::{DvfsController, ThermalSensor, PState, Temperature, PowerError};

pub struct EnergyManager<'a> {
    dvfs: &'a dyn DvfsController,
    thermal: &'a dyn ThermalSensor,
}

static ENERGY_MGR: Once<EnergyManager<'static>> = Once::new();

impl<'a> EnergyManager<'a> {
    pub fn init(dvfs: &'a dyn DvfsController, thermal: &'a dyn ThermalSensor) {
        ENERGY_MGR.call_once(|| EnergyManager { dvfs, thermal });
    }

    pub fn set_low_power(&self) {
        if let Some(pstate) = self.dvfs.available_pstates().first().copied() {
            let _ = self.dvfs.set_pstate(0, pstate);
        }
    }

    pub fn monitor_temp(&self) -> Result<Temperature, PowerError> { self.thermal.read_temperature(0) }
}

pub fn global() -> &'static EnergyManager<'static> { ENERGY_MGR.get().expect("energy mgr") } 