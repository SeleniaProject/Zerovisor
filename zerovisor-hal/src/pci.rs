//! Cross-architecture PCIe enumeration and DMA protection helpers
//! Provides a unified interface so higher-level code can list PCI devices and
//! attach them to the architecture-specific IOMMU implementation.
//!
//! The implementation is intentionally lightweight and `no_std` friendly. Each
//! architecture backend performs the minimum hardware access necessary to
//! discover devices and enable translation protection. All comments are in
//! English as requested.

#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Once;

use crate::iommu::{IommuError};

/// PCI Bus/Device/Function triple encoded as separate 8-,5-,3-bit fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciBdf {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciBdf {
    #[inline]
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }

    /// Convert into a 32-bit integer (same layout used across Zerovisor HAL).
    #[inline]
    pub const fn as_u32(self) -> u32 {
        ((self.bus as u32) << 8) | ((self.device as u32) << 3) | (self.function as u32)
    }
}

/// High-level description of a PCI device.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bdf: PciBdf,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

/// Return a list of all PCI/PCIe devices visible to the Zerovisor HAL.
/// Currently implemented for x86_64 (I/O-port mechanism). Other
/// architectures can extend this function using ECAM MMIO access.
pub fn enumerate_all() -> Vec<PciDevice> {
    #[cfg(target_arch = "x86_64")]
    {
        enumerate_x86()
    }

    #[cfg(target_arch = "aarch64")]
    {
        enumerate_arm64()
    }

    #[cfg(target_arch = "riscv64")]
    {
        enumerate_riscv64()
    }
}

/// Attach the device to an isolated IOMMU domain so DMA is fully protected.
/// This helper abstracts away VT-d / SMMU / RISC-V IOMMU differences.
pub fn protect_dma(bdf: PciBdf) -> Result<(), IommuError> {
    let dev_u32 = bdf.as_u32();

    #[cfg(target_arch = "x86_64")]
    {
        use crate::arch::x86_64::iommu::VtdEngine;
        use crate::iommu::IommuEngine;
        static ENGINE: Once<VtdEngine> = Once::new();
        let engine = ENGINE.call_once(|| VtdEngine::init().expect("VT-d init failed"));
        return engine.attach_device(dev_u32);
    }

    #[cfg(target_arch = "aarch64")]
    {
        use crate::arch::arm64::iommu::SmmuEngine;
        static ENGINE: Once<SmmuEngine> = Once::new();
        let engine = ENGINE.call_once(|| SmmuEngine::init().expect("SMMU init failed"));
        return engine.attach_device(dev_u32);
    }

    #[cfg(target_arch = "riscv64")]
    {
        use crate::arch::riscv64::iommu::RiscvIommuEngine;
        static ENGINE: Once<RiscvIommuEngine> = Once::new();
        let engine = ENGINE.call_once(|| RiscvIommuEngine::init().expect("IOMMU init failed"));
        return engine.attach_device(dev_u32);
    }
}

// ------------------------------------------------------------------------------------------------------------------
// x86_64 backend (legacy I/O-port configuration mechanism).
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
fn enumerate_x86() -> Vec<PciDevice> {
    use crate::arch::x86_64::pci::{read_config_dword};

    let mut list = Vec::new();
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            for func in 0u8..8 {
                let vendor = unsafe { read_config_dword(bus, dev, func, 0x00) } & 0xFFFF;
                if vendor == 0xFFFF { continue; }
                let id_reg = unsafe { read_config_dword(bus, dev, func, 0x00) };
                let class_reg = unsafe { read_config_dword(bus, dev, func, 0x08) };
                list.push(PciDevice {
                    bdf: PciBdf::new(bus, dev, func),
                    vendor_id: (id_reg & 0xFFFF) as u16,
                    device_id: ((id_reg >> 16) & 0xFFFF) as u16,
                    class_code: (class_reg >> 24) as u8,
                    subclass: ((class_reg >> 16) & 0xFF) as u8,
                    prog_if: ((class_reg >> 8) & 0xFF) as u8,
                });
            }
        }
    }
    list
}

// ------------------------------------------------------------------------------------------------------------------
// ARM64 backend (ECAM MMIO configuration mechanism).
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "aarch64")]
mod arm64_backend {
    use super::{PciDevice, PciBdf};
    use alloc::vec::Vec;
    use spin::Once;

    // Default ECAM base address. On real hardware this should be discovered via
    // ACPI MCFG table or Device Tree. Call `set_ecam_base` early during boot to
    // override the value if platform firmware provides a different address.
    const DEFAULT_ECAM_BASE: usize = 0x3000_0000;

    static ECAM_BASE: Once<usize> = Once::new();

    #[inline]
    pub fn set_ecam_base(base: usize) {
        // Ignore subsequent writes – platform should initialise only once.
        let _ = ECAM_BASE.try_call_once(|| base);
    }

    #[inline]
    fn ecam_base() -> usize {
        *ECAM_BASE.call_once(|| DEFAULT_ECAM_BASE)
    }

    /// Read a 32-bit configuration dword via ECAM. Unsafe because arbitrary MMIO.
    #[inline(always)]
    unsafe fn read_config_dword(bus: u8, dev: u8, func: u8, offset: u16) -> u32 {
        let base = ecam_base();
        let addr = base
            + ((bus as usize) << 20)
            + ((dev as usize) << 15)
            + ((func as usize) << 12)
            + (offset as usize);
        core::ptr::read_volatile(addr as *const u32)
    }

    /// Enumerate all devices using a standard 256-bus scan. The loop terminates
    /// early when vendor ID reads as 0xFFFF.
    pub fn enumerate_arm64() -> Vec<PciDevice> {
        const MAX_BUS: u8 = 255;
        const MAX_DEVICE: u8 = 31;
        const MAX_FUNCTION: u8 = 7;

        let mut list = Vec::new();
        for bus in 0u8..=MAX_BUS {
            for dev in 0u8..=MAX_DEVICE {
                for func in 0u8..=MAX_FUNCTION {
                    // Safety: Accessing physical configuration space. Caller must
                    // ensure the ECAM base address is valid for the platform.
                    let id_reg = unsafe { read_config_dword(bus, dev, func, 0x00) };
                    let vendor_id = (id_reg & 0xFFFF) as u16;
                    if vendor_id == 0xFFFF {
                        // No device present – skip remaining functions.
                        if func == 0 { break; }
                        continue;
                    }
                    let device_id = ((id_reg >> 16) & 0xFFFF) as u16;
                    let class_reg = unsafe { read_config_dword(bus, dev, func, 0x08) };
                    list.push(PciDevice {
                        bdf: PciBdf::new(bus, dev, func),
                        vendor_id,
                        device_id,
                        class_code: (class_reg >> 24) as u8,
                        subclass: ((class_reg >> 16) & 0xFF) as u8,
                        prog_if: ((class_reg >> 8) & 0xFF) as u8,
                    });
                }
            }
        }
        list
    }
}

#[cfg(target_arch = "aarch64")]
use arm64_backend::enumerate_arm64;

// ------------------------------------------------------------------------------------------------------------------
// RISC-V backend (ECAM MMIO configuration mechanism).
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "riscv64")]
mod riscv_backend {
    use super::{PciDevice, PciBdf};
    use alloc::vec::Vec;
    use spin::Once;

    // Default ECAM base address for many RISC-V SoCs. Override via `set_ecam_base`.
    const DEFAULT_ECAM_BASE: usize = 0x4000_0000;

    static ECAM_BASE: Once<usize> = Once::new();

    #[inline]
    pub fn set_ecam_base(base: usize) {
        let _ = ECAM_BASE.try_call_once(|| base);
    }

    #[inline]
    fn ecam_base() -> usize {
        *ECAM_BASE.call_once(|| DEFAULT_ECAM_BASE)
    }

    #[inline(always)]
    unsafe fn read_config_dword(bus: u8, dev: u8, func: u8, offset: u16) -> u32 {
        let base = ecam_base();
        let addr = base
            + ((bus as usize) << 20)
            + ((dev as usize) << 15)
            + ((func as usize) << 12)
            + (offset as usize);
        core::ptr::read_volatile(addr as *const u32)
    }

    pub fn enumerate_riscv64() -> Vec<PciDevice> {
        const MAX_BUS: u8 = 255;
        const MAX_DEVICE: u8 = 31;
        const MAX_FUNCTION: u8 = 7;

        let mut list = Vec::new();
        for bus in 0u8..=MAX_BUS {
            for dev in 0u8..=MAX_DEVICE {
                for func in 0u8..=MAX_FUNCTION {
                    let id_reg = unsafe { read_config_dword(bus, dev, func, 0x00) };
                    let vendor_id = (id_reg & 0xFFFF) as u16;
                    if vendor_id == 0xFFFF {
                        if func == 0 { break; }
                        continue;
                    }
                    let device_id = ((id_reg >> 16) & 0xFFFF) as u16;
                    let class_reg = unsafe { read_config_dword(bus, dev, func, 0x08) };
                    list.push(PciDevice {
                        bdf: PciBdf::new(bus, dev, func),
                        vendor_id,
                        device_id,
                        class_code: (class_reg >> 24) as u8,
                        subclass: ((class_reg >> 16) & 0xFF) as u8,
                        prog_if: ((class_reg >> 8) & 0xFF) as u8,
                    });
                }
            }
        }
        list
    }
}

#[cfg(target_arch = "riscv64")]
use riscv_backend::enumerate_riscv64; 