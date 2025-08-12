#![no_std]
#![no_main]

// Entry point and UEFI types
use uefi::prelude::*;

mod arch;
mod i18n;
mod firmware;
mod time;
mod mm;

// For formatted writes to UEFI text output
use core::fmt::Write as _;

/// UEFI application entry point.
///
/// This function is discovered via the `#[entry]` attribute provided by the
/// `uefi` crate and serves as the dynamic library entry used by UEFI firmware.
#[entry]
fn efi_main(_image: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // Print a minimal initialization banner to the UEFI console using i18n.
    {
        // Detect features first without borrowing stdout, to satisfy the borrow checker.
        use arch::x86::cpuid;
        let b_vmx = cpuid::has_vmx();
        let b_svm = cpuid::has_svm();
        let b_ept = cpuid::may_support_ept();
        let b_npt = cpuid::has_npt();
        let b_dmar = crate::firmware::acpi::find_dmar(&system_table).is_some();
        let b_ivrs = crate::firmware::acpi::find_ivrs(&system_table).is_some();

        let stdout = system_table.stdout();
        let _ = stdout.reset(false);
        let lang = i18n::detect_lang();
        let _ = stdout.write_str(i18n::t(lang, i18n::key::BANNER));
        let _ = stdout.write_str(i18n::t(lang, i18n::key::ENV));

        if b_vmx { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_VMX)); }
        if b_svm { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_SVM)); }
        if b_ept { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_EPT)); }
        if b_npt { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_NPT)); }
        if b_dmar { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_VTD)); }
        if b_ivrs { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_AMDVI)); }
    }

    // ACPI discovery: Check presence of RSDP and core tables
    {
        use firmware::acpi;
        if let Some(rsdp) = acpi::find_rsdp(&system_table) {
            {
                let stdout = system_table.stdout();
                let _ = stdout.write_str("ACPI: RSDP found\r\n");
            }
            let fadt = acpi::find_fadt(&system_table).is_some();
            let madt = acpi::find_madt(&system_table).is_some();
            let mcfg = acpi::find_mcfg(&system_table).is_some();
            {
                let stdout = system_table.stdout();
                if fadt { let _ = stdout.write_str("ACPI: FADT found\r\n"); }
                if madt { let _ = stdout.write_str("ACPI: MADT found\r\n"); }
                if mcfg { let _ = stdout.write_str("ACPI: MCFG found\r\n"); }
            }
            // Enumerate CPUs from MADT
            if madt {
                if let Some(madt_hdr) = acpi::find_madt(&system_table) {
                    let stdout = system_table.stdout();
                    acpi::madt_list_cpus_from(madt_hdr, |s| { let _ = stdout.write_str(s); });
                }
            }
            // Enumerate PCIe ECAM segments from MCFG
            if mcfg {
                if let Some(mcfg_hdr) = acpi::find_mcfg(&system_table) {
                    let stdout = system_table.stdout();
                    acpi::mcfg_list_segments_from(mcfg_hdr, |s| { let _ = stdout.write_str(s); });
                }
            }
            let _ = rsdp; // suppress unused warning
        } else {
            let stdout = system_table.stdout();
            let _ = stdout.write_str("ACPI: RSDP not found\r\n");
        }
    }

    {
        // Calibrate TSC and print rough frequency
        let hz = time::calibrate_tsc(&system_table);
        let stdout = system_table.stdout();
        let mut buf = [0u8; 64];
        let mut n = 0;
        for &b in b"TSC frequency (approx): " { buf[n] = b; n += 1; }
        n += firmware::acpi::u32_to_dec((hz / 1_000_000) as u32, &mut buf[n..]);
        for &b in b" MHz\r\n" { buf[n] = b; n += 1; }
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));

        let lang = i18n::detect_lang();
        let _ = stdout.write_str(i18n::t(lang, i18n::key::READY));
    }

    // Virtualization preflight summary (non-intrusive)
    {
        use arch::x86::vm::{self, vmx, svm};
        match vm::detect_vendor() {
            vm::Vendor::Intel => {
                if vmx::vmx_preflight_available() {
                    {
                        let stdout = system_table.stdout();
                        let _ = stdout.write_str("VMX: available (preflight)\r\n");
                    }
                    // Report VMX control MSRs
                    vmx::vmx_report_controls(&mut system_table);
                    let vmx_ok = vmx::vmx_smoke_test(&system_table).is_ok();
                    let stdout = system_table.stdout();
                    if vmx_ok { let _ = stdout.write_str("VMX: VMXON/VMXOFF smoke test OK\r\n"); }
                    else { let _ = stdout.write_str("VMX: smoke test skipped/failed\r\n"); }

                    // VMCS pointer load/clear smoke test
                    let vmcs_ok = vmx::vmx_vmcs_smoke_test(&system_table).is_ok();
                    let stdout = system_table.stdout();
                    if vmcs_ok { let _ = stdout.write_str("VMX: VMCS VMPTRLD/VMCLEAR smoke test OK\r\n"); }
                    else { let _ = stdout.write_str("VMX: VMCS smoke test skipped/failed\r\n"); }
                }
            }
            vm::Vendor::Amd => {
                if svm::svm_preflight_available() {
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str("SVM: available (preflight)\r\n");
                }
            }
            vm::Vendor::Unknown => {
                let stdout = system_table.stdout();
                let _ = stdout.write_str("CPU vendor: unknown\r\n");
            }
        }
    }

    Status::SUCCESS
}

/// Panic handler for `no_std` environment.
///
/// We keep this extremely conservative: in case of a panic before the text
/// console is fully usable, just halt in a loop to avoid returning control with
/// an undefined state. Environments with a working console will still show the
/// last printed banner above.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // Spin forever to signal a terminal failure state.
    }
}


