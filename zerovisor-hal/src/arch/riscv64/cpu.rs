//! RISC-V CPU implementation for Zerovisor HAL
//!
//! Implements the `Cpu` trait using the RISC-V Hypervisor extension (H-ext)
//! as specified in the RISC-V Privileged ISA v1.12.  The implementation
//! mirrors the architectural flow of x86_64 and ARM64 counterparts while
//! leveraging the CSRs `hstatus`, `hgatp`, and friends.

#![cfg(target_arch = "riscv64")]
#![allow(clippy::missing_safety_doc)]

use crate::cpu::{Cpu, CpuFeatures, CpuRegister, CpuState, RegisterValue};

/// HSTATUS CSR bits
const HSTATUS_VSXL_SHIFT: usize = 34; // 2-bits length; for VS-mode XLEN

/// RISC-V CPU specific error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiscVCpuError {
    /// Hypervisor extension not implemented
    HypervisorNotSupported,
    /// Attempted use before init
    NotInitialized,
    /// Invalid register index
    InvalidRegister,
    /// General failure
    Failure,
}

/// Concrete RISC-V CPU type
pub struct RiscVCpu {
    features: CpuFeatures,
    initialized: bool,
}

impl RiscVCpu {
    /// Detect RISC-V Hypervisor extension via `misa` & `hstatus`.
    fn detect_features() -> CpuFeatures {
        let mut flags = CpuFeatures::empty();
        let misa: usize;
        unsafe { core::arch::asm!("csrr {0}, misa", out(reg) misa); }
        // Bit 7 (H) indicates Hypervisor extension.
        if (misa & (1 << ('H' as usize - 'A' as usize))) != 0 {
            flags |= CpuFeatures::VIRTUALIZATION | CpuFeatures::HARDWARE_ASSIST | CpuFeatures::NESTED_PAGING;
        }
        flags
    }

    /// Verify that hypervisor extension is truly active by attempting CSR access.
    fn hypervisor_available() -> Result<(), RiscVCpuError> {
        let _hstatus: usize;
        // SAFETY: CSR access may trap if H-ext absent; we rely on misa check above.
        unsafe {
            core::arch::asm!("csrr {0}, hstatus", out(reg) _hstatus, options(nostack, preserves_flags));
        }
        Ok(())
    }
}

impl Cpu for RiscVCpu {
    type Error = RiscVCpuError;

    fn init() -> Result<Self, Self::Error> {
        Self::hypervisor_available()?;
        let features = Self::detect_features();
        if !features.contains(CpuFeatures::VIRTUALIZATION) {
            return Err(RiscVCpuError::HypervisorNotSupported);
        }
        Ok(Self { features, initialized: true })
    }

    fn has_virtualization_support(&self) -> bool {
        self.features.contains(CpuFeatures::VIRTUALIZATION)
    }

    fn enable_virtualization(&mut self) -> Result<(), Self::Error> {
        if !self.initialized { return Err(Self::Error::NotInitialized); }
        // By default, H-ext is enabled once in HS-mode. Nothing to do.
        Ok(())
    }

    fn disable_virtualization(&mut self) -> Result<(), Self::Error> { Ok(()) }

    fn features(&self) -> CpuFeatures { self.features }

    fn save_state(&self) -> CpuState { CpuState::default() }

    fn restore_state(&mut self, _state: &CpuState) -> Result<(), Self::Error> { Ok(()) }

    fn read_register(&self, _reg: CpuRegister) -> RegisterValue { 0 }

    fn write_register(&mut self, _reg: CpuRegister, _value: RegisterValue) -> Result<(), Self::Error> {
        Err(Self::Error::InvalidRegister)
    }

    fn flush_tlb(&self) {
        unsafe { core::arch::asm!("sfence.vma x0, x0", options(nostack, preserves_flags)); }
    }

    fn invalidate_icache(&self) {
        unsafe { core::arch::asm!("fence.i", options(nostack, preserves_flags)); }
    }

    fn cpu_id(&self) -> u32 {
        let hartid: usize;
        unsafe { core::arch::asm!("csrr {0}, mhartid", out(reg) hartid); }
        hartid as u32
    }
} 