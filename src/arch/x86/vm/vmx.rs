#![allow(dead_code)]

//! Minimal Intel VMX capability checks and VMXON preparation stubs.

use crate::arch::x86::cpuid;
use crate::obs::metrics;
use core::fmt::Write as _;
use crate::util::format;

// Control MSR indices
const IA32_FEATURE_CONTROL: u32 = 0x3A;
const IA32_VMX_CR0_FIXED0: u32 = 0x486;
const IA32_VMX_CR0_FIXED1: u32 = 0x487;
const IA32_VMX_CR4_FIXED0: u32 = 0x488;
const IA32_VMX_CR4_FIXED1: u32 = 0x489;

/// VMX initialization preflight checks (read-only).
pub fn vmx_preflight_available() -> bool {
    if !cpuid::has_vmx() { return false; }
    true
}

/// Compute legal CR0/CR4 values according to VMX fixed bits.
pub fn vmx_adjust_cr0_cr4(cr0: u64, cr4: u64) -> (u64, u64) {
    use crate::arch::x86::msr::rdmsr;
    let (mut cr0v, mut cr4v) = (cr0, cr4);
    unsafe {
        let cr0_f0 = rdmsr(IA32_VMX_CR0_FIXED0);
        let cr0_f1 = rdmsr(IA32_VMX_CR0_FIXED1);
        let cr4_f0 = rdmsr(IA32_VMX_CR4_FIXED0);
        let cr4_f1 = rdmsr(IA32_VMX_CR4_FIXED1);
        cr0v |= cr0_f0; cr0v &= cr0_f1;
        cr4v |= cr4_f0; cr4v &= cr4_f1;
    }
    (cr0v, cr4v)
}

/// Allocate VMXON region and attempt VMXON (stub; not executed yet).
pub fn vmx_try_enable() -> Result<(), &'static str> {
    if !vmx_preflight_available() { return Err("VMX not available"); }
    // Enabling requires setting CR4.VMXE and executing VMXON with a properly
    // aligned (4KB) region containing VMCS revision id. We defer the actual
    // VMXON until memory management is prepared. This stub just reports ready.
    Ok(())
}

/// Check IA32_FEATURE_CONTROL for VMX permission outside SMX.
fn feature_control_allows_vmx() -> Result<(), &'static str> {
    let fc = unsafe { crate::arch::x86::msr::rdmsr(IA32_FEATURE_CONTROL) };
    let lock = (fc & 1) != 0;
    let vmx_outside_smx = (fc & (1 << 2)) != 0;
    if lock && !vmx_outside_smx {
        return Err("IA32_FEATURE_CONTROL locked without VMX outside SMX");
    }
    Ok(())
}

#[inline(always)]
fn read_rflags() -> u64 {
    let r: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) r); }
    r
}

#[inline(always)]
fn u64_to_hex_buf(v: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut started = false; let mut n = 0;
    for i in (0..16).rev() {
        let nyb = ((v >> (i * 4)) & 0xF) as usize;
        if nyb != 0 || started || i == 0 { started = true; if n < out.len() { out[n] = HEX[nyb]; n += 1; } }
    }
    n
}

/// Read VMX control MSRs and print allowed masks.
pub fn vmx_report_controls(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let pin = unsafe { crate::arch::x86::msr::rdmsr(0x481) };
    let pri = unsafe { crate::arch::x86::msr::rdmsr(0x482) };
    let sec = unsafe { crate::arch::x86::msr::rdmsr(0x48B) };
    let exit = unsafe { crate::arch::x86::msr::rdmsr(0x483) };
    let entry = unsafe { crate::arch::x86::msr::rdmsr(0x484) };

    let stdout = system_table.stdout();
    let mut buf = [0u8; 96];
    for (label, val) in [
        (b"VMX MSR PinCtl=0x".as_ref(), pin),
        (b"VMX MSR PriCtl=0x".as_ref(), pri),
        (b"VMX MSR SecCtl=0x".as_ref(), sec),
        (b"VMX MSR ExitCtl=0x".as_ref(), exit),
        (b"VMX MSR EntryCtl=0x".as_ref(), entry),
    ] {
        let mut n = 0;
        for &b in label { buf[n] = b; n += 1; }
        n += u64_to_hex_buf(val, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}

/// Read IA32_VMX_EPT_VPID_CAP and print a brief capability summary.
pub fn vmx_report_ept_vpid_cap(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let cap = unsafe { crate::arch::x86::msr::rdmsr(0x48C) };
    let stdout = system_table.stdout();
    // Print raw value
    let mut buf = [0u8; 96];
    let mut n = 0;
    for &b in b"VMX MSR EPT_VPID_CAP=0x" { buf[n] = b; n += 1; }
    n += format::u64_hex(cap, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));

    // Minimal decode: large page support flags commonly used (bit positions from SDM)
    let ept_2m = (cap & (1 << 16)) != 0;
    let ept_1g = (cap & (1 << 17)) != 0;
    let ept_ad = (cap & (1 << 21)) != 0; // Accessed/Dirty bits for EPT
    if ept_2m { let _ = stdout.write_str("EPT: 2MiB pages supported\r\n"); }
    if ept_1g { let _ = stdout.write_str("EPT: 1GiB pages supported\r\n"); }
    if ept_ad { let _ = stdout.write_str("EPT: Accessed/Dirty bits supported\r\n"); }
}

/// Attempt VMXON then VMXOFF for a smoke test using UEFI page allocation.
pub fn vmx_smoke_test(system_table: &uefi::table::SystemTable<uefi::prelude::Boot>) -> Result<(), &'static str> {
    if !vmx_preflight_available() { return Err("VMX not available"); }
    if let Err(e) = feature_control_allows_vmx() { return Err(e); }

    // Read VMCS revision id from IA32_VMX_BASIC MSR
    let vmx_basic = unsafe { crate::arch::x86::msr::rdmsr(0x480) };
    let rev_id: u32 = vmx_basic as u32;

    // Allocate one page for VMXON region
    let mem = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)
        .ok_or("alloc_pages failed")?;
    // Ensure 4KB alignment (AllocatePages should already align). Zero and write revision id.
    unsafe {
        core::ptr::write_bytes(mem, 0, 4096);
        core::ptr::write_unaligned(mem as *mut u32, rev_id);
    }

    // Adjust CR0/CR4 and set CR4.VMXE
    let mut cr0: u64; let mut cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
    }
    let (cr0a, mut cr4a) = vmx_adjust_cr0_cr4(cr0, cr4);
    cr4a |= 1 << 13; // CR4.VMXE
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0a, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4a, options(nostack, preserves_flags));
    }

    // Execute VMXON and capture flags
    let phys = mem as u64; // identity mapping assumption
    let _before = read_rflags();
    let t0 = crate::time::rdtsc();
    unsafe { core::arch::asm!("vmxon [{}]", in(reg) &phys); }
    let after = read_rflags();
    let t1 = crate::time::rdtsc();
    let cf = (after & 0x1) != 0;
    let zf = (after & 0x40) != 0;
    if cf || zf {
        // VMXON failed; no need to vmxoff
        crate::mm::uefi::free_pages(system_table, mem, 1);
        return Err("VMXON failed (CF/ZF)");
    }

    // Immediately VMXOFF to avoid staying in VMX root mode
    unsafe { core::arch::asm!("vmxoff"); }

    // Restore original CR0/CR4 best-effort
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }

    // Free memory
    crate::mm::uefi::free_pages(system_table, mem, 1);
    // Observe duration in microseconds using approximate TSC Hz
    let hz = crate::time::tsc_hz();
    if hz != 0 {
        let dt = t1.wrapping_sub(t0);
        let us = (dt as u128) * 1_000_000u128 / (hz as u128);
        metrics::observe_vmx_smoke_us(us as u64);
    }
    Ok(())
}

/// VMCS pointer load/clear smoke test under VMX root (no VMLAUNCH).
pub fn vmx_vmcs_smoke_test(system_table: &uefi::table::SystemTable<uefi::prelude::Boot>) -> Result<(), &'static str> {
    if !vmx_preflight_available() { return Err("VMX not available"); }
    if let Err(e) = feature_control_allows_vmx() { return Err(e); }

    // Read VMCS revision id and prepare VMXON/VMCS regions
    let vmx_basic = unsafe { crate::arch::x86::msr::rdmsr(0x480) };
    let rev_id: u32 = (vmx_basic & 0x7FFF_FFFF) as u32;
    let vmxon = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)
        .ok_or("alloc_pages VMXON failed")?;
    unsafe { core::ptr::write_bytes(vmxon, 0, 4096); core::ptr::write_unaligned(vmxon as *mut u32, rev_id); }
    let vmcs = crate::arch::x86::vm::vmcs::alloc_vmcs_region(system_table).ok_or("alloc VMCS failed")?;

    // Save and adjust control registers
    let mut cr0: u64; let mut cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
    }
    let (cr0a, mut cr4a) = vmx_adjust_cr0_cr4(cr0, cr4);
    cr4a |= 1 << 13; // CR4.VMXE
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0a, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4a, options(nostack, preserves_flags));
    }

    // Enter VMX root
    let vmxon_phys = vmxon as u64;
    unsafe { core::arch::asm!("vmxon [{}]", in(reg) &vmxon_phys); }

    // VMPTRLD
    let vmcs_phys = vmcs as u64;
    unsafe { core::arch::asm!("vmptrld [{}]", in(reg) &vmcs_phys); }

    // VMCLEAR and leave VMX root
    unsafe { core::arch::asm!("vmclear [{}]", in(reg) &vmcs_phys); }
    unsafe { core::arch::asm!("vmxoff"); }

    // Restore CRs
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }

    // Free pages
    crate::mm::uefi::free_pages(system_table, vmcs, 1);
    crate::mm::uefi::free_pages(system_table, vmxon, 1);
    Ok(())
}

/// Configure VMCS controls to enable secondary controls and EPT, and set EPTP.
pub fn vmx_ept_smoke_test(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) -> Result<(), &'static str> {
    if !vmx_preflight_available() { return Err("VMX not available"); }
    if let Err(e) = feature_control_allows_vmx() { return Err(e); }

    // Allocate VMXON and VMCS regions
    let vmx_basic = unsafe { crate::arch::x86::msr::rdmsr(0x480) };
    let rev_id: u32 = (vmx_basic & 0x7FFF_FFFF) as u32;
    let vmxon = crate::mm::uefi::alloc_pages(system_table, 1, uefi::table::boot::MemoryType::LOADER_DATA)
        .ok_or("alloc VMXON failed")?;
    let vmcs = crate::arch::x86::vm::vmcs::alloc_vmcs_region(system_table).ok_or("alloc VMCS failed")?;
    unsafe { core::ptr::write_bytes(vmxon, 0, 4096); core::ptr::write_unaligned(vmxon as *mut u32, rev_id); }

    // Save and adjust CR0/CR4 and set CR4.VMXE
    let mut cr0: u64; let mut cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, preserves_flags));
    }
    let (cr0a, mut cr4a) = vmx_adjust_cr0_cr4(cr0, cr4);
    cr4a |= 1 << 13; // CR4.VMXE
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0a, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4a, options(nostack, preserves_flags));
    }

    // Enter VMX root and load VMCS
    let vmxon_phys = vmxon as u64;
    unsafe { core::arch::asm!("vmxon [{}]", in(reg) &vmxon_phys); }
    let vmcs_phys = vmcs as u64;
    unsafe { core::arch::asm!("vmptrld [{}]", in(reg) &vmcs_phys); }

    // Read control MSRs
    let pri_ctl_msr = unsafe { crate::arch::x86::msr::rdmsr(0x482) };
    let sec_ctl_msr = unsafe { crate::arch::x86::msr::rdmsr(0x48B) };
    let (pri_allowed0, pri_allowed1) = ((pri_ctl_msr as u32), (pri_ctl_msr >> 32) as u32);
    let (sec_allowed0, sec_allowed1) = ((sec_ctl_msr as u32), (sec_ctl_msr >> 32) as u32);

    // Desired: activate secondary controls (bit 31) in primary controls
    let desired_pri: u32 = 1u32 << 31;
    let adj_pri = crate::arch::x86::vm::vmcs::satisfy_controls(desired_pri, pri_allowed0, pri_allowed1);
    crate::arch::x86::vm::vmcs::vmwrite(crate::arch::x86::vm::vmcs::VMCS_PROCBASED_CTLS, adj_pri as u64)?;

    // Desired: enable EPT (bit 1) in secondary controls
    let desired_sec: u32 = 1u32 << 1;
    let adj_sec = crate::arch::x86::vm::vmcs::satisfy_controls(desired_sec, sec_allowed0, sec_allowed1);
    crate::arch::x86::vm::vmcs::vmwrite(crate::arch::x86::vm::vmcs::VMCS_SECONDARY_CTLS, adj_sec as u64)?;

    // Build minimal EPT and set EPTP (pick 1GiB if supported)
    let ept_caps = unsafe { crate::arch::x86::msr::rdmsr(0x48C) };
    let caps = crate::mm::ept::EptCaps {
        large_page_2m: (ept_caps & (1 << 16)) != 0,
        large_page_1g: (ept_caps & (1 << 17)) != 0,
    };
    let ept_ad = (ept_caps & (1 << 21)) != 0;
    if let Some(pml4) = crate::mm::ept::build_identity_best(system_table, 1u64 << 30, caps) {
        let eptp = if ept_ad {
            crate::mm::ept::eptp_from_pml4_with_opts(pml4 as u64, crate::mm::ept::EptOptions { allow_execute: true, enable_ad: true })
        } else {
            crate::mm::ept::eptp_from_pml4(pml4 as u64)
        };
        crate::arch::x86::vm::vmcs::vmwrite(crate::arch::x86::vm::vmcs::VMCS_EPT_POINTER, eptp)?;
        let stdout = system_table.stdout();
        let _ = stdout.write_str("VMX: EPTP set (identity mapping)\r\n");
    } else {
        let stdout = system_table.stdout();
        let _ = stdout.write_str("VMX: EPT build failed\r\n");
    }

    // Cleanup: VMCLEAR and VMXOFF
    unsafe { core::arch::asm!("vmclear [{}]", in(reg) &vmcs_phys); }
    unsafe { core::arch::asm!("vmxoff"); }
    // Restore CRs
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nostack, preserves_flags));
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
    }
    // Free memory
    crate::mm::uefi::free_pages(system_table, vmcs, 1);
    crate::mm::uefi::free_pages(system_table, vmxon, 1);
    Ok(())
}


