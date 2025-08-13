#![allow(dead_code)]

pub mod vtd;
pub mod amdv;
pub mod state;

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

// --- Minimal PCI ECAM helpers (shared by iommu reporting) ---

#[inline(always)]
fn mmio_read32(addr: usize) -> u32 { unsafe { core::ptr::read_volatile(addr as *const u32) } }
#[inline(always)]
fn mmio_read16(addr: usize) -> u16 { unsafe { core::ptr::read_volatile(addr as *const u16) } }
#[inline(always)]
fn mmio_read8(addr: usize) -> u8 { unsafe { core::ptr::read_volatile(addr as *const u8) } }

#[inline(always)]
fn ecam_fn_base(seg_base: u64, start_bus: u8, bus: u8, dev: u8, func: u8) -> usize {
    // ECAM: Base + (Bus-Start)*1MB + Dev*32KB + Func*4KB
    (seg_base as usize)
        .wrapping_add(((bus as usize).saturating_sub(start_bus as usize)) << 20)
        .wrapping_add((dev as usize) << 15)
        .wrapping_add((func as usize) << 12)
}

const PCI_VENDOR_ID: usize = 0x00;
const PCI_DEVICE_ID: usize = 0x02;
const PCI_CLASS: usize = 0x0B; // class at [0x0B], subclass at [0x0A]

/// Enumerate PCI devices from ACPI MCFG and print compact BDF/ID lines.
pub fn report_pci_endpoints(system_table: &mut SystemTable<Boot>) {
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            let mut bus = a.start_bus;
            while bus <= a.end_bus {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(a.base_address, a.start_bus, bus, dev, func);
                        let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                        if vid == 0xFFFF { continue; }
                        let did = mmio_read16(cfg + PCI_DEVICE_ID);
                        let class_code = mmio_read8(cfg + PCI_CLASS);
                        let subclass = mmio_read8(cfg + PCI_CLASS - 1);
                        // Print line
                        let stdout = system_table.stdout();
                        let mut buf = [0u8; 128];
                        let mut n = 0;
                        for &b in b"IOMMU: dev seg=" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(a.pci_segment as u32, &mut buf[n..]);
                        for &b in b" bus=" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                        for &b in b" dev=" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                        for &b in b" fn=" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                        for &b in b" vid=0x" { buf[n] = b; n += 1; }
                        n += crate::util::format::u64_hex(vid as u64, &mut buf[n..]);
                        for &b in b" did=0x" { buf[n] = b; n += 1; }
                        n += crate::util::format::u64_hex(did as u64, &mut buf[n..]);
                        for &b in b" class=" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(class_code as u32, &mut buf[n..]);
                        for &b in b"/" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(subclass as u32, &mut buf[n..]);
                        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                    }
                }
                if bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
    }
}

fn find_ecam_for_segment(seg: u16, bus: u8, hdr: &'static crate::firmware::acpi::SdtHeader) -> Option<(u64, u8)> {
    let mut found: Option<(u64, u8)> = None;
    crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
        if a.pci_segment == seg && bus >= a.start_bus && bus <= a.end_bus { found = Some((a.base_address, a.start_bus)); }
    }, hdr);
    found
}

/// Cross-join DMAR Device Scopes with ECAM to print BDF + VID/DID for devices covered by remapping.
pub fn report_dmar_scoped_devices_with_ids(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    let mcfg = crate::firmware::acpi::find_mcfg(system_table);
    if dmar.is_none() || mcfg.is_none() { return; }
    let dmar = dmar.unwrap();
    let mcfg = mcfg.unwrap();
    crate::firmware::acpi::dmar_for_each_device_scope_from(|seg, _reg, bus, dev, func| {
        if let Some((base, start_bus)) = find_ecam_for_segment(seg, bus, mcfg) {
            let cfg = ecam_fn_base(base, start_bus, bus, dev, func);
            let vid = mmio_read16(cfg + PCI_VENDOR_ID);
            if vid != 0xFFFF {
                let did = mmio_read16(cfg + PCI_DEVICE_ID);
                let stdout = system_table.stdout();
                let mut buf = [0u8; 128]; let mut n = 0;
                for &b in b"DMAR dev: seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                for &b in b" bus=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                for &b in b" dev=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                for &b in b" fn=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                for &b in b" vid=0x" { buf[n] = b; n += 1; }
                n += crate::util::format::u64_hex(vid as u64, &mut buf[n..]);
                for &b in b" did=0x" { buf[n] = b; n += 1; }
                n += crate::util::format::u64_hex(did as u64, &mut buf[n..]);
                buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
            }
        }
    }, dmar);
}


