#![allow(dead_code)]

//! HPET discovery and access helpers.
//!
//! This module parses the ACPI HPET table to locate the HPET MMIO base
//! and provides minimal read/write helpers to access the main counter and
//! configuration register for timing calibration.

use core::ptr::{read_volatile, write_volatile};

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;
use crate::util::format;

/// ACPI Generic Address Structure (GAS)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Gas {
    space_id: u8,
    bit_width: u8,
    bit_offset: u8,
    access_size: u8,
    address: u64,
}

/// Minimal HPET ACPI table (subset).
#[repr(C, packed)]
struct HpetTable {
    header: crate::firmware::acpi::SdtHeader,
    event_timer_block_id: u32,
    base_address: Gas,
    hpet_number: u8,
    minimum_tick: u16,
    page_protection: u8,
}

/// HPET address space identifiers (ACPI).
const GAS_SPACE_SYSTEM_MEMORY: u8 = 0x00;

/// HPET register offsets.
const HPET_GENERAL_CAP_ID: usize = 0x000; // 64-bit
const HPET_GENERAL_CONFIG: usize = 0x010; // 64-bit
const HPET_MAIN_COUNTER: usize = 0x0F0; // 64-bit

/// HPET info discovered from ACPI.
#[derive(Clone, Copy, Debug)]
pub struct HpetInfo {
    pub base_phys: u64,
    pub period_fs: u32, // femtoseconds per tick from capabilities
}

#[inline(always)]
unsafe fn read64(addr: *const u8, off: usize) -> u64 {
    let p = addr.add(off) as *const u64;
    read_volatile(p)
}

#[inline(always)]
unsafe fn write64(addr: *mut u8, off: usize, val: u64) {
    let p = addr.add(off) as *mut u64;
    write_volatile(p, val);
}

/// Locate HPET via ACPI and return its physical base and nominal period.
pub fn locate_hpet(system_table: &SystemTable<Boot>) -> Option<HpetInfo> {
    // Find HPET SDT from ACPI and validate GAS.
    let hdr = crate::firmware::acpi::find_table(system_table, *b"HPET")?;
    let hpet = hdr as *const crate::firmware::acpi::SdtHeader as *const HpetTable;
    let hpet = unsafe { &*hpet };
    if hpet.base_address.space_id != GAS_SPACE_SYSTEM_MEMORY { return None; }
    if hpet.base_address.address == 0 { return None; }
    // Read capabilities to extract period (fs per tick).
    let base = hpet.base_address.address as *mut u8; // identity mapping assumption
    let cap = unsafe { read64(base as *const u8, HPET_GENERAL_CAP_ID) };
    let period_fs = (cap >> 32) as u32;
    if period_fs == 0 { return None; }
    Some(HpetInfo { base_phys: hpet.base_address.address, period_fs })
}

/// Enable HPET main counter if not already enabled; returns previous config.
pub fn enable_hpet_counter(hpet_base_phys: u64) -> u64 {
    let base = hpet_base_phys as *mut u8;
    // Set enable bit (bit 0) in General Configuration Register.
    let prev = unsafe { read64(base as *const u8, HPET_GENERAL_CONFIG) };
    let newv = prev | 1;
    unsafe { write64(base, HPET_GENERAL_CONFIG, newv); }
    prev
}

/// Restore HPET general configuration register.
pub fn restore_hpet_config(hpet_base_phys: u64, prev_cfg: u64) {
    let base = hpet_base_phys as *mut u8;
    unsafe { write64(base, HPET_GENERAL_CONFIG, prev_cfg); }
}

/// Read HPET main counter value.
#[inline(always)]
pub fn read_hpet_main_counter(hpet_base_phys: u64) -> u64 {
    let base = hpet_base_phys as *const u8;
    unsafe { read64(base, HPET_MAIN_COUNTER) }
}

/// Compute HPET frequency in Hz from the period (fs per tick).
#[inline(always)]
pub fn hpet_hz_from_period(period_fs: u32) -> u64 {
    // 1e15 fs per second. Use 128-bit arithmetic via u128 to avoid overflow.
    if period_fs == 0 { return 0; }
    let num: u128 = 1_000_000_000_000_000u128;
    (num / (period_fs as u128)) as u64
}

/// Calibrate TSC using HPET as a reference over a sampling window.
/// Returns TSC frequency in Hz if successful.
pub fn calibrate_tsc_via_hpet(system_table: &SystemTable<Boot>, sample_hpet_ticks: u64) -> Option<u64> {
    let info = locate_hpet(system_table)?;
    if info.period_fs == 0 { return None; }
    let hpet_hz = hpet_hz_from_period(info.period_fs);
    if hpet_hz == 0 { return None; }

    // Ensure HPET enabled; save previous config.
    let prev = enable_hpet_counter(info.base_phys);
    // Synchronize to a change in HPET counter to reduce partial-tick error.
    let last = read_hpet_main_counter(info.base_phys);
    let mut now = last;
    while now == last { now = read_hpet_main_counter(info.base_phys); }

    // Record start.
    let tsc0 = super::rdtsc();
    let h0 = now;
    // Busy-wait until desired HPET delta elapses.
    loop {
        now = read_hpet_main_counter(info.base_phys);
        if now.wrapping_sub(h0) >= sample_hpet_ticks { break; }
        core::hint::spin_loop();
    }
    let tsc1 = super::rdtsc();
    let dh = now.wrapping_sub(h0);
    let dt = tsc1.wrapping_sub(tsc0);

    // Restore prior config to avoid altering firmware timers unexpectedly.
    restore_hpet_config(info.base_phys, prev);

    // TSC Hz = dt / (dh / hpet_hz) = dt * hpet_hz / dh
    if dh == 0 { return None; }
    let tsc_hz = ((dt as u128) * (hpet_hz as u128) / (dh as u128)) as u64;
    Some(tsc_hz)
}

/// Print a brief HPET presence line.
pub fn report_hpet(system_table: &mut SystemTable<Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    if let Some(info) = locate_hpet(system_table) {
        let hz = hpet_hz_from_period(info.period_fs);
        let stdout = system_table.stdout();
        let mut buf = [0u8; 96];
        let mut n = 0;
        // Prefix localized: "HPET: present, base=0x"
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::HPET_PRESENT));
        // Append hex base and frequency
        n += format::u64_hex(info.base_phys, &mut buf[n..]);
        for &b in b" freq=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec((hz / 1_000_000) as u32, &mut buf[n..]);
        for &b in b" MHz\r\n" { buf[n] = b; n += 1; }
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    } else {
        let stdout = system_table.stdout();
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::HPET_NOT_FOUND));
    }
}


