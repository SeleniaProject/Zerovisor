//! Energy manager unit tests

extern crate std;
use zerovisor_core::energy::{EnergyManager, global};
use zerovisor_hal::power::{DvfsController, ThermalSensor, PState, Temperature, PowerError};
use core::sync::atomic::{AtomicU32, Ordering};

struct DummyDvfs { cur: AtomicU32 }
impl DummyDvfs { const fn new() -> Self { Self { cur: AtomicU32::new(0) } } }
impl DvfsController for DummyDvfs {
    fn available_pstates(&self) -> &'static [PState] { const PSTATES: &[PState] = &[PState(0), PState(1)]; PSTATES }
    fn set_pstate(&self, _core_id: usize, p: PState) -> Result<(), PowerError> { self.cur.store(p.0 as u32, Ordering::Relaxed); Ok(()) }
    fn current_pstate(&self, _core_id: usize) -> PState { PState(self.cur.load(Ordering::Relaxed) as u8) }
}

struct DummyTherm;
impl ThermalSensor for DummyTherm { fn read_temperature(&self, _core_id: usize) -> Result<Temperature, PowerError> { Ok(Temperature { celsius: 60 }) } }

#[test]
fn carbon_aware_downclock() {
    static DVFS: DummyDvfs = DummyDvfs::new();
    static THERM: DummyTherm = DummyTherm;
    EnergyManager::init(&DVFS, &THERM);
    let mgr = global();
    mgr.update_carbon_intensity(500); // high carbon triggers low power
    assert_eq!(DVFS.current_pstate(0).0, 0);
} 