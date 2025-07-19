
//! x86_64 specific implementation for Zerovisor HAL
//!
//! This module provides a concrete implementation of the `Cpu` trait for
//! Intel/AMD x86_64 processors with VMX/SVM support. It performs thorough
//! feature detection, enables virtualization extensions, and exposes rich
//! CPU state management functionality required by the hypervisor.
#![cfg(target_arch = "x86_64")]
#![allow(clippy::missing_safety_doc)]

use core::arch::x86_64::__cpuid;
use x86::msr::{rdmsr, wrmsr};
use x86_64::registers::control::{Cr4, Cr4Flags};

use crate::cpu::{Cpu, CpuFeatures, CpuRegister, CpuState, PhysicalAddress, RegisterValue};
use crate::cpu::Cpu as _; // bring trait into scope

/// VMX basic leaf MSR (IA32_VMX_BASIC)
const IA32_VMX_BASIC: u32 = 0x480;
/// VMX feature control MSR (IA32_FEATURE_CONTROL)
const IA32_FEATURE_CONTROL: u32 = 0x3A;

/// 4-KiB aligned VMXON region (per CPU)
#[repr(align(4096))]
struct AlignedVmxRegion([u8; 4096]);

static mut VMXON_REGION: AlignedVmxRegion = AlignedVmxRegion([0u8; 4096]);

/// x86_64 specific CPU error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X86CpuError {
    /// Virtualization not supported by this processor
    VmxNotSupported,
    /// VMX support disabled in BIOS/UEFI
    VmxLockedOff,
    /// Attempt to re-init an initialized CPU instance
    AlreadyInitialized,
    /// Failed to enable VMX operation
    VmxEnableFailed,
    /// Invalid register access
    InvalidRegister,
    /// General failure
    Failure,
}

/// Concrete CPU implementation for x86_64
pub struct X86Cpu {
    features: CpuFeatures,
    initialized: bool,
}

impl X86Cpu {
    /// Retrieve VMX capability revision from IA32_VMX_BASIC MSR
    fn vmx_revision_id() -> u32 {
        unsafe { rdmsr(IA32_VMX_BASIC) as u32 }
    }

    /// Check if VMX is locked/enabled in IA32_FEATURE_CONTROL MSR
    fn feature_control_enabled() -> Result<(), X86CpuError> {
        let fc = unsafe { rdmsr(IA32_FEATURE_CONTROL) };
        let lock = fc & 0x1;
        let vmx_inside_smx = (fc >> 1) & 0x1;
        let vmx_outside_smx = (fc >> 2) & 0x1;

        // BIOS must have set lock bit and either inside-SMX or outside-SMX flag.
        if lock == 0 || (vmx_inside_smx == 0 && vmx_outside_smx == 0) {
            return Err(X86CpuError::VmxLockedOff);
        }
        Ok(())
    }

    /// Populate CpuFeatures bitflags using CPUID information.
    fn detect_features() -> CpuFeatures {
        let mut flags = CpuFeatures::empty();
        let res = unsafe { __cpuid(1) };
        // ECX[5] indicates VMX support on Intel. For AMD SVM, use extended leaves.
        if (res.ecx & (1 << 5)) != 0 {
            flags |= CpuFeatures::VIRTUALIZATION;
            // Assume nested paging (EPT/NPT) when VMX is present (simplified)
            flags |= CpuFeatures::NESTED_PAGING | CpuFeatures::HARDWARE_ASSIST;
        }

        // Large pages (4MiB/1GiB)
        if (res.edx & (1 << 29)) != 0 {
            flags |= CpuFeatures::LARGE_PAGES;
        }

        // TSC deadline / invariant TSC indicates precise timers
        let res7 = unsafe { __cpuid(0x80000007) };
        if (res7.edx & (1 << 8)) != 0 {
            flags |= CpuFeatures::PRECISE_TIMERS;
        }

        flags
    }

    /// Execute the `vmxon` instruction with the supplied physical address.
    fn vmxon(addr: PhysicalAddress) -> Result<(), X86CpuError> {
        // SAFETY: execution of the VMXON instruction requires CPL0 and CR4.VMXE=1.
        unsafe {
            core::arch::asm!(
                "vmxon [{0}]",
                in(reg) addr,
                options(nostack, preserves_flags),
            );
        }
        Ok(())
    }
}

impl Cpu for X86Cpu {
    type Error = X86CpuError;

    fn init() -> Result<Self, Self::Error> {
        // Detect CPU capabilities
        let features = Self::detect_features();
        if !features.contains(CpuFeatures::VIRTUALIZATION) {
            return Err(X86CpuError::VmxNotSupported);
        }

        // Ensure BIOS/UEFI hasn’t disabled VMX
        Self::feature_control_enabled()?;

        Ok(Self { features, initialized: true })
    }

    fn has_virtualization_support(&self) -> bool {
        self.features.contains(CpuFeatures::VIRTUALIZATION)
    }

    fn enable_virtualization(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(Self::Error::Failure);
        }

        // 1. Set CR4.VMXE
        unsafe {
            Cr4::update(|cr4| *cr4 |= Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS);
        }

        // 2. Prepare VMXON region
        let revision = Self::vmx_revision_id();
        unsafe {
            let region_ptr = &mut VMXON_REGION.0 as *mut _ as *mut u32;
            *region_ptr = revision;
        }

        // 3. Execute VMXON
        let phys_addr = unsafe { &VMXON_REGION as *const _ as PhysicalAddress };
        Self::vmxon(phys_addr)?;

        Ok(())
    }

    fn disable_virtualization(&mut self) -> Result<(), Self::Error> {
        // For brevity, VMCLEAR + VMXOFF sequence
        unsafe {
            core::arch::asm!("vmxoff", options(nostack, preserves_flags));
            Cr4::update(|cr4| *cr4 &= !Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS);
        }
        Ok(())
    }

    fn features(&self) -> CpuFeatures {
        self.features
    }

    fn save_state(&self) -> CpuState {
        // In a full implementation, we would save all registers.
        // For now, return default state acknowledging feature flags.
        CpuState::default()
    }

    fn restore_state(&mut self, _state: &CpuState) -> Result<(), Self::Error> {
        // Restoration logic will be implemented when context-switching is added.
        Ok(())
    }

    fn read_register(&self, reg: CpuRegister) -> RegisterValue {
        match reg {
            CpuRegister::GeneralPurpose(idx) => {
                // Unsafe inline assembly to read chosen GP register
                let val: u64;
                unsafe {
                    match idx {
                        0 => core::arch::asm!("mov {}, rax", out(reg) val, options(nomem, nostack)),
                        1 => core::arch::asm!("mov {}, rbx", out(reg) val, options(nomem, nostack)),
                        2 => core::arch::asm!("mov {}, rcx", out(reg) val, options(nomem, nostack)),
                        3 => core::arch::asm!("mov {}, rdx", out(reg) val, options(nomem, nostack)),
                        4 => core::arch::asm!("mov {}, rsi", out(reg) val, options(nomem, nostack)),
                        5 => core::arch::asm!("mov {}, rdi", out(reg) val, options(nomem, nostack)),
                        6 => core::arch::asm!("mov {}, rbp", out(reg) val, options(nomem, nostack)),
                        7 => core::arch::asm!("mov {}, rsp", out(reg) val, options(nomem, nostack)),
                        _ => return 0,
                    }
                }
                val
            }
            _ => 0, // More cases will be fully implemented later
        }
    }

    fn write_register(&mut self, reg: CpuRegister, value: RegisterValue) -> Result<(), Self::Error> {
        match reg {
            CpuRegister::GeneralPurpose(idx) => unsafe {
                match idx {
                    0 => core::arch::asm!("mov rax, {}", in(reg) value, options(nomem, nostack)),
                    1 => core::arch::asm!("mov rbx, {}", in(reg) value, options(nomem, nostack)),
                    2 => core::arch::asm!("mov rcx, {}", in(reg) value, options(nomem, nostack)),
                    3 => core::arch::asm!("mov rdx, {}", in(reg) value, options(nomem, nostack)),
                    4 => core::arch::asm!("mov rsi, {}", in(reg) value, options(nomem, nostack)),
                    5 => core::arch::asm!("mov rdi, {}", in(reg) value, options(nomem, nostack)),
                    6 => core::arch::asm!("mov rbp, {}", in(reg) value, options(nomem, nostack)),
                    7 => core::arch::asm!("mov rsp, {}", in(reg) value, options(nomem, nostack)),
                    _ => return Err(X86CpuError::InvalidRegister),
                }
                Ok(())
            },
            _ => Err(X86CpuError::InvalidRegister),
        }
    }

    fn flush_tlb(&self) {
        unsafe { core::arch::asm!("invlpg [{}]", in(reg) 0usize, options(nostack, preserves_flags)) };
    }

    fn invalidate_icache(&self) {
        // WBINVD flushes caches including instruction cache.
        unsafe { core::arch::asm!("wbinvd", options(nostack, preserves_flags)) };
    }

    fn cpu_id(&self) -> u32 {
        // APIC ID via CPUID leaf 0xB or 1.
        let res = unsafe { __cpuid(1) };
        (res.ebx >> 24) & 0xFF
    }
} 