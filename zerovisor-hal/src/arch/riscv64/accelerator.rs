//! RISC-V accelerator virtualization backend (Vector / AI Engine) – Task 7.2
#![cfg(target_arch = "riscv64")]
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::accelerator::{AcceleratorVirtualization, AcceleratorInfo, AcceleratorType, AcceleratorId, AccelError};
use crate::virtualization::VmHandle;

/// Fully-featured accelerator manager that discovers Vector (RVV) and AI-Engine blocks at
/// runtime and allows exclusive assignment to guest VMs.  Discovery is performed via:
/// 1. The `misa` CSR (bit V) – indicates availability of the standard Vector extension.
/// 2. A vendor-specific `aieinfo` CSR (0x7C0) – exposes number of AI Engine tiles.
///
/// Boards lacking either capability are automatically filtered so upper layers receive an
/// accurate device list.
pub struct RiscvAcceleratorManager {
    devices: &'static [AcceleratorInfo],
    assignments: Mutex<heapless::FnvIndexMap<AcceleratorId, VmHandle, 8>>, // up to 8 devices
}

impl RiscvAcceleratorManager {
    pub fn new() -> Self {
        static DEVICES: spin::Once<Vec<AcceleratorInfo>> = spin::Once::new();
        let slice = DEVICES.call_once(|| {
            let mut list = Vec::new();

            // ------------------------------
            // Vector Extension discovery
            // ------------------------------
            if has_vector_extension() {
                list.push(AcceleratorInfo {
                    id: AcceleratorId(0x5653_5631), // "VSV1" – Vector Subsystem Version 1
                    accel_type: AcceleratorType::Vector,
                    vendor_id: 0x10EE, // Example Xilinx
                    device_id: 0x0001,
                });
            }

            // ------------------------------
            // AI Engine discovery (vendor CSR 0x7C0): number of tiles > 0 ?
            // ------------------------------
            if let Some(tiles) = read_aieinfo() {
                if tiles > 0 {
                    list.push(AcceleratorInfo {
                        id: AcceleratorId(0x4149_E100),
                        accel_type: AcceleratorType::AiEngine,
                        vendor_id: 0x10EE,
                        device_id: tiles as u16,
                    });
                }
            }

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

/// Check `misa` CSR bit [21] ('V') to detect Vector extension.
#[inline]
fn has_vector_extension() -> bool {
    let misa: usize;
    unsafe { core::arch::asm!("csrr {0}, misa", out(reg) misa, options(nomem, nostack, preserves_flags)); }
    (misa & (1 << ('V' as u8 - 'A' as u8))) != 0
}

/// Read vendor-specific CSR 0x7C0 that encodes AI Engine tile count.
#[inline]
fn read_aieinfo() -> Option<u32> {
    let val: u32;
    unsafe {
        core::arch::asm!("csrr {0}, 0x7C0", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    if val == 0 { None } else { Some(val) }
} 