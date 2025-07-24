//! x86_64 accelerator virtualization backend (Task 7.2)
//! 現状は SR-IOV / PCIe ベースのアクセラレータのみ簡易対応。

#![allow(clippy::module_name_repetitions)]

use crate::accelerator::{AcceleratorVirtualization, AcceleratorInfo, AcceleratorType, AcceleratorId, AccelError};
use crate::virtualization::VmHandle;
use crate::arch::x86_64::pci;
use crate::arch::x86_64::iommu::VtdEngine;
use crate::iommu::IommuError;
use spin::Mutex;
use spin::Once;

extern crate alloc;
use alloc::vec::Vec;

// Create a global VT-d engine instance initialised on first use
static IOMMU_ENGINE: Once<VtdEngine> = Once::new();

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
            // Enumerate PCI buses and collect accelerator-class devices (class code 0x12xx)
            for bus in 0u8..=255 {
                for dev in 0u8..32 {
                    for func in 0u8..8 {
                        let vendor = unsafe { pci::read_config_dword(bus, dev, func, 0x00) } & 0xFFFF;
                        if vendor == 0xFFFF { continue; }
                        let class_reg = unsafe { pci::read_config_dword(bus, dev, func, 0x08) };
                        let class_code = (class_reg >> 24) as u8;
                        if class_code != 0x12 { continue; } // 0x12 == Processing Accelerator
                        // Build AcceleratorInfo entry. Sub-class can hint the specific type.
                        let sub_class = ((class_reg >> 16) & 0xFF) as u8;
                        let accel_type = match sub_class {
                            0x00 => AcceleratorType::Tpu,
                            0x01 => AcceleratorType::Npu,
                            0x02 => AcceleratorType::Fpga,
                            0x03 => AcceleratorType::Qpu,
                            0x04 => AcceleratorType::Vector,
                            _ => AcceleratorType::Tpu, // default fall-back
                        };
                        let id = (bus as u32) << 8 | (dev as u32) << 3 | (func as u32);
                        list.push(AcceleratorInfo {
                            id: AcceleratorId(id),
                            accel_type,
                            vendor_id: vendor as u16,
                            device_id: ((unsafe { pci::read_config_dword(bus, dev, func, 0x00) } >> 16) & 0xFFFF) as u16,
                        });
                    }
                }
            }
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
        // Perform SR-IOV enable and VT-d domain attachment for secure device assignment
        const fn bdf_from_accel(id: AcceleratorId) -> u32 { id.0 }
        let bdf = bdf_from_accel(id);
        if let Err(_e) = Self::enable_sriov(bdf) {
            // Best-effort: continue even if SR-IOV capability is absent.
        }
        let iommu = IOMMU_ENGINE.call_once(|| {
            VtdEngine::init().expect("VT-d initialization failed")
        });
        iommu.attach_device(bdf).map_err(|e| match e {
            IommuError::NotAttached | IommuError::AlreadyAttached => AccelError::AlreadyAssigned,
            _ => AccelError::HardwareFailure,
        })?;
        Ok(())
    }

    fn unassign_from_vm(&self, _vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
        let mut map = self.assignments.lock();
        map.remove(&id).ok_or(AccelError::NotFound)?;
        // Perform secure detach: remove SR-IOV VF from VM and release VT-d domain assignment
        const fn bdf_from_accel(id: AcceleratorId) -> u32 { id.0 }
        let bdf = bdf_from_accel(id);
        if let Some(engine) = IOMMU_ENGINE.get() {
            let _ = engine.detach_device(bdf);
        }
        let _ = Self::disable_sriov(bdf);
        Ok(())
    }
}

impl X86AcceleratorManager {
    /// Enable SR-IOV capability for an arbitrary PCI device (generic helper)
    fn enable_sriov(bdf: u32) -> Result<(), ()> {
        let bus  = ((bdf >>  8) & 0xFF) as u8;
        let dev  = ((bdf >>  3) & 0x1F) as u8;
        let func = (bdf & 0x7) as u8;
        // Check if capability list is present.
        let status = unsafe { pci::read_config_dword(bus, dev, func, 0x04) } >> 16;
        if (status & 0x10) == 0 { return Err(()); } // capabilities absent
        // Traverse capability list
        let mut cap_ptr = (unsafe { pci::read_config_dword(bus, dev, func, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(bus, dev, func, cap_ptr) } & 0xFF;
            if cap_id == 0x10 {
                // SR-IOV capability found
                let ctrl_off = cap_ptr + 0x08;
                let mut ctrl = unsafe { pci::read_config_dword(bus, dev, func, ctrl_off) };
                ctrl |= 0x1; // VF Enable bit
                unsafe { pci::write_config_dword(bus, dev, func, ctrl_off, ctrl) };
                return Ok(());
            }
            cap_ptr = (unsafe { pci::read_config_dword(bus, dev, func, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
        Err(())
    }

    /// Disable SR-IOV VF Enable bit so physical function returns to default state
    fn disable_sriov(bdf: u32) -> Result<(), ()> {
        let bus  = ((bdf >>  8) & 0xFF) as u8;
        let dev  = ((bdf >>  3) & 0x1F) as u8;
        let func = (bdf & 0x7) as u8;
        let status = unsafe { pci::read_config_dword(bus, dev, func, 0x04) } >> 16;
        if (status & 0x10) == 0 { return Err(()); }
        let mut cap_ptr = (unsafe { pci::read_config_dword(bus, dev, func, 0x34) } & 0xFF) as u8;
        while cap_ptr != 0 {
            let cap_id = unsafe { pci::read_config_dword(bus, dev, func, cap_ptr) } & 0xFF;
            if cap_id == 0x10 {
                let ctrl_off = cap_ptr + 0x08;
                let mut ctrl = unsafe { pci::read_config_dword(bus, dev, func, ctrl_off) };
                ctrl &= !0x1; // clear VF Enable
                unsafe { pci::write_config_dword(bus, dev, func, ctrl_off, ctrl) };
                return Ok(());
            }
            cap_ptr = (unsafe { pci::read_config_dword(bus, dev, func, cap_ptr + 1) } >> 8 & 0xFF) as u8;
        }
        Err(())
    }
} 