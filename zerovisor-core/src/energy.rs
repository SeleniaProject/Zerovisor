//! Energy and thermal management subsystem (Task 10)

#![allow(dead_code)]

use spin::Once;
use zerovisor_hal::{DvfsController, ThermalSensor, PState, Temperature, PowerError};

pub struct EnergyManager<'a> {
    dvfs: &'a dyn DvfsController,
    thermal: &'a dyn ThermalSensor,
    carbon_intensity_g_per_kwh: core::sync::atomic::AtomicU32,
}

static ENERGY_MGR: Once<EnergyManager<'static>> = Once::new();

impl<'a> EnergyManager<'a> {
    pub fn init(dvfs: &'a dyn DvfsController, thermal: &'a dyn ThermalSensor) {
        use core::sync::atomic::AtomicU32;
        ENERGY_MGR.call_once(|| EnergyManager {
            dvfs,
            thermal,
            carbon_intensity_g_per_kwh: AtomicU32::new(0),
        });
    }

    pub fn set_low_power(&self) {
        if let Some(pstate) = self.dvfs.available_pstates().first().copied() {
            let _ = self.dvfs.set_pstate(0, pstate);
        }
    }

    pub fn monitor_temp(&self) -> Result<Temperature, PowerError> { self.thermal.read_temperature(0) }

    /// Update current grid carbon intensity (gCO2/kWh).
    /// Orchestrator or out-of-band agent should call this periodically.
    pub fn update_carbon_intensity(&self, grams_per_kwh: u32) {
        self.carbon_intensity_g_per_kwh.store(grams_per_kwh, core::sync::atomic::Ordering::Relaxed);
        // If intensity is high (>400 g/kWh), switch to low-power mode.
        if grams_per_kwh > 400 {
            self.set_low_power();
        }
    }

    /// Retrieve last reported carbon intensity.
    pub fn carbon_intensity(&self) -> u32 {
        self.carbon_intensity_g_per_kwh.load(core::sync::atomic::Ordering::Relaxed)
    }
}

pub fn global() -> &'static EnergyManager<'static> { ENERGY_MGR.get().expect("energy mgr") } 