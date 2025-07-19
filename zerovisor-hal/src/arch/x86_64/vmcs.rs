//! VMCS (Virtual Machine Control Structure) helpers for Intel VMX
//!
//! This module provides low-level wrappers around the VMX instructions
//! `VMCLEAR`, `VMPTRLD`, `VMREAD`, and `VMWRITE` as well as a safe Rust
//! abstraction `Vmcs` that encapsulates a 4-KiB-aligned VMCS region in
//! physical memory. Only a minimal subset of VMCS field encodings is
//! defined for now – enough to bootstrap a 64-bit guest. Additional
//! fields will be added as Task 3.x progresses.
#![cfg(target_arch = "x86_64")]

use core::arch::asm;
use core::marker::PhantomData;
use x86::bits64::vmx::{vmclear, vmptrld};

use crate::memory::PhysicalAddress;

/// Intel-defined VMCS field encodings (partial)
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum VmcsField {
    GUEST_RIP          = 0x681E,
    GUEST_RSP          = 0x681C,
    GUEST_CR0          = 0x6800,
    GUEST_CR3          = 0x6802,
    GUEST_CR4          = 0x6804,
    GUEST_RAX          = 0x6806,
    GUEST_RBX          = 0x6808,
    GUEST_RCX          = 0x680A,
    GUEST_RDX          = 0x680C,
    HOST_CR0           = 0x6C00,
    HOST_CR3           = 0x6C02,
    HOST_CR4           = 0x6C04,
    HOST_RIP           = 0x6C16,
    EPT_POINTER        = 0x201A,
    EXIT_REASON        = 0x4402,
    EXIT_QUALIFICATION = 0x6400,
    GUEST_LINEAR_ADDR  = 0x640A,
    GUEST_PHYS_ADDR    = 0x2400,
}

/// Wrapper representing a loaded VMCS pointer.
pub struct ActiveVmcs<'a> {
    _phantom: PhantomData<&'a mut ()>,
}

impl<'a> ActiveVmcs<'a> {
    /// Perform `VMREAD` for the given field.
    #[inline]
    pub fn read(&self, field: VmcsField) -> u64 {
        let value: u64;
        unsafe {
            asm!(
                "vmread {field:e}, {value}",
                field = in(reg) field as u32,
                value = lateout(reg) value,
                options(nostack, preserves_flags),
            );
        }
        value
    }

    /// Perform `VMWRITE` for the given field.
    #[inline]
    pub fn write(&mut self, field: VmcsField, value: u64) {
        unsafe {
            asm!(
                "vmwrite {value}, {field:e}",
                field = in(reg) field as u32,
                value = in(reg) value,
                options(nostack, preserves_flags),
            );
        }
    }
}

/// Safe wrapper representing ownership of a VMCS region in physical memory.
pub struct Vmcs {
    phys_addr: PhysicalAddress,
}

impl Vmcs {
    /// Create a new wrapper from a 4-KiB-aligned physical address.
    pub const fn new(phys: PhysicalAddress) -> Self { Self { phys_addr: phys } }

    /// Clear VMCS state using `VMCLEAR`.
    pub fn clear(&self) -> Result<(), VmcsError> {
        unsafe { vmclear(self.phys_addr) }.map_err(|_| VmcsError::VmclearFailed)
    }

    /// Load this VMCS to current VMCS pointer with `VMPTRLD`, returning an
    /// `ActiveVmcs` token that allows VMREAD/VMWRITE.
    pub fn load(&self) -> Result<ActiveVmcs, VmcsError> {
        unsafe { vmptrld(self.phys_addr) }.map_err(|_| VmcsError::VmptrldFailed)?;
        Ok(ActiveVmcs { _phantom: PhantomData })
    }

    /// Physical address of VMCS region.
    pub fn phys_addr(&self) -> PhysicalAddress { self.phys_addr }
}

/// VMCS-related errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmcsError {
    VmclearFailed,
    VmptrldFailed,
} 