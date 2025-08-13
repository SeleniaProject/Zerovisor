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

        let lang = i18n::detect_lang(&system_table);
        // Resolve ACPI headers before borrowing stdout to avoid borrow conflicts
        let dmar_hdr = if b_dmar { crate::firmware::acpi::find_dmar(&system_table) } else { None };
        let ivrs_hdr = if b_ivrs { crate::firmware::acpi::find_ivrs(&system_table) } else { None };

        let stdout = system_table.stdout();
        let _ = stdout.reset(false);
        let _ = stdout.write_str(i18n::t(lang, i18n::key::BANNER));
        let _ = stdout.write_str(i18n::t(lang, i18n::key::ENV));

        if b_vmx { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_VMX)); }
        if b_svm { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_SVM)); }
        if b_ept { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_EPT)); }
        if b_npt { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_NPT)); }
        if b_dmar { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_VTD)); }
        if let Some(h) = dmar_hdr {
            crate::firmware::acpi::dmar_summary(|s| { let _ = stdout.write_str(s); }, h);
            crate::firmware::acpi::dmar_list_structs_from(|s| { let _ = stdout.write_str(s); }, h);
        }
        if b_ivrs { let _ = stdout.write_str(i18n::t(lang, i18n::key::FEAT_AMDVI)); }
        if let Some(h) = ivrs_hdr {
            crate::firmware::acpi::ivrs_summary(|s| { let _ = stdout.write_str(s); }, h);
            crate::firmware::acpi::ivrs_list_entries_from(|s| { let _ = stdout.write_str(s); }, h);
        }
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
            // Enumerate CPUs via SMP module (MADT-based)
            if madt {
                crate::arch::x86::smp::enumerate_and_report(&system_table);
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
        // Report HPET presence and nominal frequency if available
        time::hpet::report_hpet(&system_table);

        // Detect invariant TSC and calibrate; cache the result
        let inv = arch::x86::cpuid::has_invariant_tsc();
        let hz = time::init_time(&system_table);
        let lang = i18n::detect_lang(&system_table);
        let stdout = system_table.stdout();
        let mut buf = [0u8; 64];
        let mut n = 0;
        for &b in b"TSC frequency (approx): " { buf[n] = b; n += 1; }
        n += firmware::acpi::u32_to_dec((hz / 1_000_000) as u32, &mut buf[n..]);
        for &b in b" MHz\r\n" { buf[n] = b; n += 1; }
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        // Log invariant TSC flag
        let _ = stdout.write_str(if inv { "TSC: invariant\r\n" } else { "TSC: not invariant\r\n" });

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
                    vmx::vmx_report_ept_vpid_cap(&mut system_table);
                    let vmx_ok = vmx::vmx_smoke_test(&system_table).is_ok();
                    let stdout = system_table.stdout();
                    if vmx_ok { let _ = stdout.write_str("VMX: VMXON/VMXOFF smoke test OK\r\n"); }
                    else { let _ = stdout.write_str("VMX: smoke test skipped/failed\r\n"); }

                    // VMCS pointer load/clear smoke test
                    let vmcs_ok = vmx::vmx_vmcs_smoke_test(&system_table).is_ok();
                    let stdout = system_table.stdout();
                    if vmcs_ok { let _ = stdout.write_str("VMX: VMCS VMPTRLD/VMCLEAR smoke test OK\r\n"); }
                    else { let _ = stdout.write_str("VMX: VMCS smoke test skipped/failed\r\n"); }

                    // Attempt to set EPTP in VMCS to verify EPT plumbing (non-launch)
                    let _ = vmx::vmx_ept_smoke_test(&system_table);
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

    // Minimal AP bring-up: prepare a real-mode trampoline and count AP wakeups.
    {
        if let Some(info) = crate::arch::x86::trampoline::prepare_real_mode_trampoline(&system_table) {
            // Prepare identity-mapped native paging for APs and write CR3 to mailbox area
            crate::arch::x86::smp::write_ap_cr3_mailbox(&system_table, info.phys_base, 1u64 << 30);
            // LAPIC base via MSR if possible; fall back to MADT
            let mut lapic_base = crate::arch::x86::lapic::apic_base_via_msr();
            if lapic_base.is_none() {
                if let Some(madt_hdr) = crate::firmware::acpi::find_madt(&system_table) {
                    let madt = madt_hdr as *const crate::firmware::acpi::SdtHeader as *const crate::firmware::acpi::MadtHeader;
                    lapic_base = Some(unsafe { (*madt).lapic_addr as usize });
                }
            }
            if let Some(lapic_base) = lapic_base {
                // Send INIT + SIPIs to APs
                crate::arch::x86::smp::start_aps_init_sipi(&system_table, lapic_base, info.phys_base);
                // Wait for APs to tick the mailbox with a timeout (~100ms)
                let mut waited_us: u64 = 0;
                let start_count = crate::arch::x86::trampoline::read_mailbox_count(info);
                loop {
                    let now = crate::arch::x86::trampoline::read_mailbox_count(info);
                    if now != start_count { break; }
                    if waited_us >= 100_000 { break; }
                    let _ = system_table.boot_services().stall(1000);
                    waited_us += 1000;
                }
                // Report mailbox count and expected CPUs
                let stdout = system_table.stdout();
                let mut buf = [0u8; 64];
                let mut n = 0;
                for &b in b"SMP: AP wakeups (mailbox count)=" { buf[n] = b; n += 1; }
                let cnt = crate::arch::x86::trampoline::read_mailbox_count(info) as u32;
                n += crate::firmware::acpi::u32_to_dec(cnt, &mut buf[n..]);
                buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                if let Some(madt_hdr2) = acpi::find_madt(&system_table) {
                    let expected = acpi::madt_count_logical_cpus_from(madt_hdr2);
                    let mut b2 = [0u8; 64];
                    let mut m2 = 0;
                    for &b in b"SMP: expected CPUs=" { b2[m2] = b; m2 += 1; }
                    m2 += crate::firmware::acpi::u32_to_dec(expected, &mut b2[m2..]);
                    b2[m2] = b'\r'; m2 += 1; b2[m2] = b'\n'; m2 += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&b2[..m2]).unwrap_or("\r\n"));
                }

                // Report PM/LM success flags
                let pm_ok = crate::arch::x86::trampoline::read_mailbox_pm_ok(info);
                let lm_ok = crate::arch::x86::trampoline::read_mailbox_lm_ok(info);
                let _ = stdout.write_str(if pm_ok { "SMP: AP PM-entry OK\r\n" } else { "SMP: AP PM-entry not observed\r\n" });
                let _ = stdout.write_str(if lm_ok { "SMP: AP LM-entry OK\r\n" } else { "SMP: AP LM-entry not observed\r\n" });
                // If LM reached, also print the LM entry hit count at mailbox+6..+7 and the APIC ID byte at +8
                if lm_ok {
                    let base = info.phys_base as usize + info.mailbox_offset as usize;
                    let cnt16 = unsafe { core::ptr::read_volatile((base + 6) as *const u16) } as u32;
                    let mut buf2 = [0u8; 64];
                    let mut m = 0;
                    for &b in b"SMP: AP LM-count=" { buf2[m] = b; m += 1; }
                    m += crate::firmware::acpi::u32_to_dec(cnt16, &mut buf2[m..]);
                    buf2[m] = b'\r'; m += 1; buf2[m] = b'\n'; m += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf2[..m]).unwrap_or("\r\n"));
                    let apic_byte = unsafe { core::ptr::read_volatile((base + 8) as *const u8) } as u32;
                    let mut buf3 = [0u8; 64];
                    let mut m3 = 0;
                    for &b in b"SMP: AP APIC-ID(byte)=" { buf3[m3] = b; m3 += 1; }
                    m3 += crate::firmware::acpi::u32_to_dec(apic_byte, &mut buf3[m3..]);
                    buf3[m3] = b'\r'; m3 += 1; buf3[m3] = b'\n'; m3 += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&buf3[..m3]).unwrap_or("\r\n"));
                    // Dump APIC ID list written by APs at mailbox+32 .. (byte array)
                    let mut listbuf = [0u8; 128];
                    let mut l = 0;
                    for &b in b"SMP: AP IDs=" { listbuf[l] = b; l += 1; }
                    for i in 0..16usize {
                        let idb = unsafe { core::ptr::read_volatile((base + 32 + i) as *const u8) } as u32;
                        if i > 0 { listbuf[l] = b','; l += 1; listbuf[l] = b' '; l += 1; }
                        l += crate::firmware::acpi::u32_to_dec(idb, &mut listbuf[l..]);
                    }
                    listbuf[l] = b'\r'; l += 1; listbuf[l] = b'\n'; l += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&listbuf[..l]).unwrap_or("\r\n"));
                }
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


