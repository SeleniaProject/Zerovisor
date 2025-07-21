//! RISC-V accelerator virtualization backend (Vector / AI Engine) – Task 7.2
#![cfg(target_arch = "riscv64")]
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::accelerator::{AcceleratorVirtualization, AcceleratorInfo, AcceleratorType, AcceleratorId, AccelError};
use crate::virtualization::VmHandle;

/// Simple stub manager that pretends a Vector engine and an AI Engine are present.
pub struct RiscvAcceleratorManager {
    devices: &'static [AcceleratorInfo],
    assignments: Mutex<heapless::FnvIndexMap<AcceleratorId, VmHandle, 8>>, // up to 8 devices
}

impl RiscvAcceleratorManager {
    pub fn new() -> Self {
        static DEVICES: spin::Once<Vec<AcceleratorInfo>> = spin::Once::new();
        let slice = DEVICES.call_once(|| {
            let mut list = Vec::new();
            // Dummy Vector engine (RVV)
            list.push(AcceleratorInfo {
                id: AcceleratorId(0xABCD_EF01),
                accel_type: AcceleratorType::Vector,
                vendor_id: 0xABCD,
                device_id: 0x0001,
            });
            // Dummy AI Engine device
            list.push(AcceleratorInfo {
                id: AcceleratorId(0xABCD_EF02),
                accel_type: AcceleratorType::AiEngine,
                vendor_id: 0xABCD,
                device_id: 0x1001,
            });
            list
        });
        Self { devices: slice, assignments: Mutex::new(heapless::FnvIndexMap::new()) }
    }
}

impl AcceleratorVirtualization for RiscvAcceleratorManager {
    fn enumerate(&self) -> &'static [AcceleratorInfo] { self.devices }

    fn assign_to_vm(&self, vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
        let mut map = self.assignments.lock();
        if map.contains_key(&id) { return Err(AccelError::AlreadyAssigned); }
        map.insert(id, vm).map_err(|_| AccelError::OutOfResources)?;
        Ok(())
    }

    fn unassign_from_vm(&self, _vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
        let mut map = self.assignments.lock();
        map.remove(&id).ok_or(AccelError::NotFound)?;
        Ok(())
    }
} 