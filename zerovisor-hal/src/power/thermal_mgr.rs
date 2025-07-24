//! ThermalManager – Polls on-die temperature sensors and applies DVFS throttling
//! thresholds: 80 °C (warning) → step down one P-state, 90 °C (critical) → lowest P-state.
//! Runs on a background timer every 100 ms.

#![deny(unsafe_op_in_unsafe_fn)]

use zerovisor_hal::timer::{Timer, TimerCallback};
use zerovisor_hal::power::{DvfsController, ThermalSensor, PState, PowerError};
use zerovisor_hal::log::info;
use spin::Once;
use core::sync::atomic::{AtomicBool, Ordering};

static THERMAL_THREAD_STARTED: AtomicBool = AtomicBool::new(false);

pub struct ThermalManager {
    dvfs: &'static dyn DvfsController,
    therm: &'static dyn ThermalSensor,
}

impl ThermalManager {
    pub const fn new(dvfs: &'static dyn DvfsController, therm: &'static dyn ThermalSensor) -> Self {
        Self { dvfs, therm }
    }

    fn sample_and_throttle(&self) {
        let temp = match self.therm.read_temperature(0) { Ok(t) => t.celsius, Err(_) => return };
        let current = self.dvfs.current_pstate(0);
        let available = self.dvfs.available_pstates();
        if temp >= 90 {
            info!("ThermalManager: critical temperature {}°C – forcing lowest P-state", temp);
            let lowest = *available.last().unwrap_or(&current);
            let _ = self.dvfs.set_pstate(0, lowest);
        } else if temp >= 80 {
            info!("ThermalManager: high temperature {}°C – stepping down P-state", temp);
            let next_idx = core::cmp::min(current.0 as usize + 1, available.len() - 1);
            let _ = self.dvfs.set_pstate(0, available[next_idx]);
        }
    }

    pub fn start(&'static self) {
        if THERMAL_THREAD_STARTED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            Timer::periodic(100_000, Self::timer_callback as TimerCallback, self as *const _ as u64);
        }
    }

    extern "C" fn timer_callback(arg: u64) {
        let mgr = unsafe { &*(arg as *const ThermalManager) };
        mgr.sample_and_throttle();
    }
}

pub fn init_global() {
    if let Some((dvfs, therm)) = zerovisor_hal::power_interfaces() {
        static MANAGER: Once<ThermalManager> = Once::new();
        let m = MANAGER.call_once(|| ThermalManager::new(dvfs, therm));
        m.start();
    }
} 