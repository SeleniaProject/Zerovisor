//! Accelerator virtualization (Task 7.2)
//! 提供するアクセラレータ: TPU, NPU, FPGA, QPU を抽象化し、VM への割当を管理する。

#![allow(dead_code)]

use crate::virtualization::VmHandle;

/// 種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorType {
    Tpu,
    Npu,
    Fpga,
    Qpu,
    /// RISC-V Vector Engine (RVV)
    Vector,
    /// AI Engine / DSP blocks (e.g., Xilinx AIE)
    AiEngine,
}

/// アクセラレータ ID (Bus/Device/Function などを抽象化)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AcceleratorId(pub u32);

/// デバイス情報
#[derive(Debug, Clone, Copy)]
pub struct AcceleratorInfo {
    pub id: AcceleratorId,
    pub accel_type: AcceleratorType,
    pub vendor_id: u16,
    pub device_id: u16,
}

/// 操作結果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelError {
    NotSupported,
    NotFound,
    AlreadyAssigned,
    InvalidVm,
    HardwareFailure,
}

/// Virtualization interface to be implemented by architecture-specific back-ends.
pub trait AcceleratorVirtualization: Send + Sync {
    /// Enumerate available accelerator devices on the host.
    fn enumerate(&self) -> &'static [AcceleratorInfo];

    /// Assign an accelerator to a VM (SR-IOV PF/VF, PCIe PASID, etc.)
    fn assign_to_vm(&self, vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError>;

    /// Detach an accelerator from a VM and return it to the pool.
    fn unassign_from_vm(&self, vm: VmHandle, id: AcceleratorId) -> Result<(), AccelError>;
} 