#![allow(dead_code)]

//! VMCS capability helpers and region management (Intel VMX).

/// IA32_VMX_* control MSR indices
pub const IA32_VMX_PINBASED_CTLS: u32 = 0x481;
pub const IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
pub const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
pub const IA32_VMX_EXIT_CTLS: u32 = 0x483;
pub const IA32_VMX_ENTRY_CTLS: u32 = 0x484;

/// Extract allowed-0 and allowed-1 masks from a VMX control MSR value.
#[inline(always)]
pub fn control_msrs_masks(msr_val: u64) -> (u32, u32) {
    let allowed_0 = msr_val as u32;             // bits that must be zero -> if 1, allowed to set
    let allowed_1 = (msr_val >> 32) as u32;     // bits that must be one
    (allowed_0, allowed_1)
}

/// Compute a control value that satisfies allowed-0/allowed-1 constraints given a desired mask.
#[inline(always)]
pub fn satisfy_controls(desired: u32, allowed_0: u32, allowed_1: u32) -> u32 {
    // Intel SDM: low 32 = allowed-0 (bit=1 means 0 allowed), high 32 = allowed-1 (bit=1 means 1 allowed)
    // A safe composition is: (desired | must_be_one) & allowed_one
    let must_be_one: u32 = !allowed_0;
    let allowed_one: u32 = allowed_1;
    (desired | must_be_one) & allowed_one
}

/// Allocate a 4KiB VMCS region and write the revision ID at the first 31 bits.
pub fn alloc_vmcs_region(system_table: &uefi::table::SystemTable<uefi::prelude::Boot>) -> Option<*mut u8> {
    let page = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)?;
    // Write VMCS revision ID from IA32_VMX_BASIC[30:0]
    let vmx_basic = unsafe { crate::arch::x86::msr::rdmsr(0x480) };
    let rev_id: u32 = (vmx_basic & 0x7FFF_FFFF) as u32;
    unsafe {
        core::ptr::write_bytes(page, 0, 4096);
        core::ptr::write_unaligned(page as *mut u32, rev_id);
    }
    Some(page)
}

/// Free a previously allocated VMCS region.
pub fn free_vmcs_region(system_table: &uefi::table::SystemTable<uefi::prelude::Boot>, ptr: *mut u8) {
    crate::mm::uefi::free_pages(system_table, ptr, 1);
}

// --- VMCS field encodings (subset) ---

/// Primary processor-based VM-execution controls
pub const VMCS_PROCBASED_CTLS: u64 = 0x0000_4002;
/// Secondary processor-based VM-execution controls
pub const VMCS_SECONDARY_CTLS: u64 = 0x0000_401E;
/// EPT pointer (EPTP), 64-bit field
pub const VMCS_EPT_POINTER: u64 = 0x0000_201A;

/// Write a VMCS field; returns Ok if VMwrite succeeds (no CF/ZF).
#[inline(always)]
pub fn vmwrite(field: u64, value: u64) -> Result<(), &'static str> {
    let mut rflags: u64;
    unsafe {
        core::arch::asm!(
            "vmwrite {val}, {fld}"
            , fld = in(reg) field
            , val = in(reg) value
            , options(nostack, preserves_flags)
        );
        // Read back RFLAGS to check CF/ZF. Use pushfq/pop to a register.
        core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nostack, preserves_flags));
    }
    let cf = (rflags & 0x1) != 0;
    let zf = (rflags & 0x40) != 0;
    if cf || zf { return Err("vmwrite failed"); }
    Ok(())
}



