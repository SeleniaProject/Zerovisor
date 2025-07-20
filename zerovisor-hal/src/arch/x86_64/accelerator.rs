//! x86_64 accelerator virtualization backend (Task 7.2)
//! 現状は SR-IOV / PCIe ベースのアクセラレータのみ簡易対応。

#![allow(clippy::module_name_repetitions)]

// PCI enumeration not yet implemented; placeholder static list.
use crate::accelerator::{AcceleratorVirtualization, AcceleratorInfo, AcceleratorType, AcceleratorId, AccelError};
use crate::virtualization::VmHandle;
use spin::Mutex;

extern crate alloc;
use alloc::vec::Vec;

/// 単純なアクセラレータマネージャ (スレッドセーフ)
pub struct X86AcceleratorManager {
    devices: &'static [AcceleratorInfo],
    assignments: Mutex<heapless::FnvIndexMap<AcceleratorId, VmHandle, 32>>, // up to 32 devices
}

impl X86AcceleratorManager {
    pub fn new() -> Self {
        // スキャンしてアクセラレータ候補を抽出 (TPU/NPU/FPGA/QPU の PCIe デバイス)
        static DEVICES: spin::Once<Vec<AcceleratorInfo>> = spin::Once::new();
        let slice = DEVICES.call_once(|| {
            let mut list = Vec::new();
            // PCI enumeration not yet implemented; placeholder static list.
            // In a real scenario, this would scan for PCIe devices.
            // For now, we'll add a dummy device for demonstration.
            list.push(AcceleratorInfo {
                id: AcceleratorId(0x12345678), // Placeholder ID
                accel_type: AcceleratorType::Tpu, // Placeholder type
                vendor_id: 0x1234, // Placeholder vendor
                device_id: 0x1234, // Placeholder device
            });
            list
        });
        Self { devices: slice, assignments: Mutex::new(heapless::FnvIndexMap::new()) }
    }
}

impl AcceleratorVirtualization for X86AcceleratorManager {
    fn enumerate(&self) -> &'static [AcceleratorInfo] {
        self.devices
    }

    fn assign_to_vm(&self, vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
        // 簡易排他制御: assignments マップに登録
        let mut map = self.assignments.lock();
        if map.contains_key(&id) {
            return Err(AccelError::AlreadyAssigned);
        }
        map.insert(id, vm).map_err(|_| AccelError::HardwareFailure)?;
        // TODO: 実際には IOMMU/VFIO や SR-IOV VF 有効化処理を行う
        Ok(())
    }

    fn unassign_from_vm(&self, _vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
        let mut map = self.assignments.lock();
        map.remove(&id).ok_or(AccelError::NotFound)?;
        // TODO: デタッチ処理
        Ok(())
    }
} 