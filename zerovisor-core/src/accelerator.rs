//! Accelerator manager (Task 7.2)
//! VM へ TPU/NPU/FPGA/QPU デバイスを割当てる高レベル API。

#![allow(dead_code)]

extern crate alloc;

use spin::Once;
use crate::ZerovisorError;
use zerovisor_hal::{AcceleratorVirtualization, AcceleratorInfo, AcceleratorId, AccelError};

#[cfg(target_arch = "x86_64")]
use zerovisor_hal::arch::x86_64::X86AcceleratorManager as ArchAccelMgr;

static ACCEL_MANAGER: Once<ArchAccelMgr> = Once::new();

/// 初期化
pub fn init() -> Result<(), ZerovisorError> {
    ACCEL_MANAGER.call_once(|| ArchAccelMgr::new());
    Ok(())
}

/// アクセラレータ一覧
pub fn enumerate() -> &'static [AcceleratorInfo] { ACCEL_MANAGER.get().expect("accel").enumerate() }

/// VM へ割当
pub fn assign(vm: zerovisor_hal::VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
    ACCEL_MANAGER.get().expect("accel").assign_to_vm(vm, id)
}

/// 解除
pub fn unassign(vm: zerovisor_hal::VmHandle, id: AcceleratorId) -> Result<(), AccelError> {
    ACCEL_MANAGER.get().expect("accel").unassign_from_vm(vm, id)
} 