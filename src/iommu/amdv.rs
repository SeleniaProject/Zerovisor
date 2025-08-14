#![allow(dead_code)]

//! AMD-Vi (IOMMU) minimal discovery and early initialization.

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;
use crate::util::spinlock::SpinLock;

// AMD IOMMU register offsets (subset per common references)
const REG_MMIO_BASE: usize = 0x0; // placeholder per-unit base mapped via IVRS device table
const REG_STATUS: usize = 0x18; // Status (R)
const REG_CONTROL: usize = 0x18; // Control (W)

// Control bits (subset)
const CTRL_TE: u32 = 1 << 0; // Translation Enable

#[derive(Clone, Copy)]
struct AmdViUnit { seg: u16, reg_base: u64 }

static AMDVI_UNITS: SpinLock<[Option<AmdViUnit>; 8]> = SpinLock::new([None; 8]);

fn register_unit(seg: u16, reg_base: u64) {
    AMDVI_UNITS.lock(|arr| {
        for slot in arr.iter_mut() { if slot.is_none() { *slot = Some(AmdViUnit { seg, reg_base }); break; } }
    });
}

fn for_each_unit(mut f: impl FnMut(AmdViUnit)) { AMDVI_UNITS.lock(|arr| { for o in arr.iter() { if let Some(u) = *o { f(u); } } }) }

/// Early minimal init: discover IVRS and remember units (no TE enable here).
pub fn minimal_init(system_table: &mut SystemTable<Boot>) {
    if let Some(ivrs) = crate::firmware::acpi::find_ivrs(system_table) {
        crate::firmware::acpi::ivrs_for_each_ivhd_from(|seg, base| { register_unit(seg, base); }, ivrs);
        let stdout = system_table.stdout();
        let _ = stdout.write_str("AMD-Vi: units registered from IVRS\r\n");
    }
}

pub fn enable_translation_all(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| {
            let ctrl = (u.reg_base as usize + REG_CONTROL) as *mut u32;
            let stat = (u.reg_base as usize + REG_STATUS) as *const u32;
            let cur = unsafe { core::ptr::read_volatile(ctrl) };
            unsafe { core::ptr::write_volatile(ctrl, cur | CTRL_TE); }
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 { if (unsafe { core::ptr::read_volatile(stat) } & CTRL_TE) != 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
            let mut buf = [0u8; 96]; let mut n = 0;
            for &b in b"AMD-Vi: enable seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in s { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        });
}

pub fn disable_translation_all(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| {
            let ctrl = (u.reg_base as usize + REG_CONTROL) as *mut u32;
            let stat = (u.reg_base as usize + REG_STATUS) as *const u32;
            let cur = unsafe { core::ptr::read_volatile(ctrl) };
            unsafe { core::ptr::write_volatile(ctrl, cur & !CTRL_TE); }
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 { if (unsafe { core::ptr::read_volatile(stat) } & CTRL_TE) == 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
            let mut buf = [0u8; 96]; let mut n = 0;
            for &b in b"AMD-Vi: disable seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in s { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        });
}

pub fn report_units(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| {
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"AMD-Vi: seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" reg=0x" { buf[n] = b; n += 1; }
        n += crate::util::format::u64_hex(u.reg_base, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Probe for ACPI IVRS table and print a short summary.
pub fn probe_and_report(system_table: &mut SystemTable<Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    // Resolve header before borrowing stdout to avoid aliasing borrows
    let ivrs = crate::firmware::acpi::find_ivrs(system_table);
    let stdout = system_table.stdout();
    if let Some(hdr) = ivrs {
        crate::firmware::acpi::ivrs_summary(|s| { let _ = stdout.write_str(s); }, hdr);
        crate::firmware::acpi::ivrs_list_entries_from(|s| { let _ = stdout.write_str(s); }, hdr);
    } else {
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::IOMMU_AMDV_NONE));
    }
}


