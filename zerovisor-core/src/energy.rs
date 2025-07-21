//! Energy and thermal management subsystem (Task 10)

#![allow(dead_code)]

use spin::Once;
use zerovisor_hal::{DvfsController, ThermalSensor, PState, Temperature, PowerError};
use core::time::Duration;

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

    /// Estimate carbon emission (in grams CO2) for running a workload.
    ///
    /// `energy_joules` — total energy the workload is expected to consume.
    pub fn estimate_carbon_emission(&self, energy_joules: u64) -> u64 {
        // Convert J -> kWh (1 kWh = 3.6e6 J) and multiply by intensity.
        let kwh_times_1000 = energy_joules * 1000 / 3_600_000; // milli-kWh to preserve precision
        (kwh_times_1000 as u64 * self.carbon_intensity() as u64) / 1000
    }

    /// Decide whether the VM should be migrated to a greener node.
    ///
    /// Returns `true` if current intensity exceeds `threshold_g_per_kwh`.
    pub fn should_migrate_for_carbon(&self, threshold_g_per_kwh: u32) -> bool {
        self.carbon_intensity() > threshold_g_per_kwh
    }

    /// Block until temperature is below `max_temp` or timeout elapses.
    pub fn wait_cooldown(&self, max_temp: Temperature, timeout: Duration) -> Result<(), PowerError> {
        let start = crate::cycles::rdtsc();
        while self.monitor_temp()? > max_temp {
            // Simple busy-wait with micro-sleep (platform specific NOP).
            #[allow(clippy::empty_loop)]
            for _ in 0..10_000 { core::hint::spin_loop(); }
            // Timeout safeguard (assumes 3 GHz -> convert cycles to ~secs).
            if crate::cycles::rdtsc().wrapping_sub(start) > 3_000_000_000u64.saturating_mul(timeout.as_secs()) {
                break;
            }
        }
        Ok(())
    }
}

pub fn global() -> &'static EnergyManager<'static> { ENERGY_MGR.get().expect("energy mgr") } 