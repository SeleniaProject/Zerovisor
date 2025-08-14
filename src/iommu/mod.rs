#![allow(dead_code)]

pub mod vtd;
pub mod amdv;
pub mod state;

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use uefi::table::runtime::VariableVendor;
use uefi::cstr16;
use core::fmt::Write as _;

// --- Minimal PCI ECAM helpers (shared by iommu reporting) ---

#[inline(always)]
pub fn mmio_read32(addr: usize) -> u32 { unsafe { core::ptr::read_volatile(addr as *const u32) } }
#[inline(always)]
pub fn mmio_read16(addr: usize) -> u16 { unsafe { core::ptr::read_volatile(addr as *const u16) } }
#[inline(always)]
pub fn mmio_read8(addr: usize) -> u8 { unsafe { core::ptr::read_volatile(addr as *const u8) } }

#[inline(always)]
pub fn ecam_fn_base(seg_base: u64, start_bus: u8, bus: u8, dev: u8, func: u8) -> usize {
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


/// Enumerate endpoints filtered by PCI class/subclass and print compact lines.
pub fn report_pci_by_class(system_table: &mut SystemTable<Boot>, class_code: u8, subclass: u8) {
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            let mut bus = a.start_bus;
            while bus <= a.end_bus {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(a.base_address, a.start_bus, bus, dev, func);
                        let vid = mmio_read16(cfg + 0x00);
                        if vid == 0xFFFF { continue; }
                        let did = mmio_read16(cfg + 0x02);
                        let cls = mmio_read8(cfg + 0x0B);
                        let sc = mmio_read8(cfg + 0x0A);
                        if cls != class_code || sc != subclass { continue; }
                        let stdout = system_table.stdout();
                        let mut buf = [0u8; 128]; let mut n = 0;
                        for &b in b"PCI(cls): seg=" { buf[n] = b; n += 1; }
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
                        n += crate::firmware::acpi::u32_to_dec(cls as u32, &mut buf[n..]);
                        for &b in b"/" { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(sc as u32, &mut buf[n..]);
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

// ---- Persist IOMMU assignments (UEFI variable) ----

const VAR_NS: VariableVendor = VariableVendor::GLOBAL_VARIABLE;

pub fn cfg_save(system_table: &SystemTable<Boot>) {
    let rs = system_table.runtime_services();
    // fixed buffer: u16 count + N * 8B entries
    let mut buf = [0u8; 2048];
    let mut n: usize = 2; // reserve for count
    let mut count: u16 = 0;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
        if n + 8 <= buf.len() {
            buf[n + 0] = (seg & 0xFF) as u8; buf[n + 1] = ((seg >> 8) & 0xFF) as u8;
            buf[n + 2] = bus; buf[n + 3] = dev; buf[n + 4] = func; buf[n + 5] = 0;
            buf[n + 6] = (dom & 0xFF) as u8; buf[n + 7] = ((dom >> 8) & 0xFF) as u8;
            n += 8; count = count.saturating_add(1);
        }
    });
    buf[0] = (count & 0xFF) as u8; buf[1] = ((count >> 8) & 0xFF) as u8;
    let _ = rs.set_variable(cstr16!("ZerovisorIommuAssign"), &VAR_NS, uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS, &buf[..n]);
}

pub fn cfg_load(system_table: &mut SystemTable<Boot>) {
    let rs = system_table.runtime_services();
    let mut buf = [0u8; 2048];
    if let Ok((data, _attrs)) = rs.get_variable(cstr16!("ZerovisorIommuAssign"), &VAR_NS, &mut buf) {
        if data.len() < 2 { return; }
        let count = (data[0] as usize) | ((data[1] as usize) << 8);
        let mut map_old_new: [(u16,u16); 16] = [(0,0); 16];
        let mut map_cnt: usize = 0;
        let mut off = 2usize;
        for _ in 0..count {
            if off + 8 > data.len() { break; }
            let seg = (data[off + 0] as u16) | ((data[off + 1] as u16) << 8);
            let bus = data[off + 2]; let dev = data[off + 3]; let func = data[off + 4];
            let odom = (data[off + 6] as u16) | ((data[off + 7] as u16) << 8);
            off += 8;
            // map old dom id to a new/current one
            let mut ndom: Option<u16> = None;
            for i in 0..map_cnt { let (o,n) = map_old_new[i]; if o == odom { ndom = Some(n); break; } }
            if ndom.is_none() {
                // create a new domain id
                if let Some(newid) = crate::iommu::state::create_domain() {
                    if map_cnt < map_old_new.len() { map_old_new[map_cnt] = (odom, newid); map_cnt += 1; }
                    ndom = Some(newid);
                }
            }
            if let Some(new_dom) = ndom { let _ = crate::iommu::state::assign_device(seg, bus, dev, func, new_dom); }
        }
        // Apply contexts and refresh caches for safety (both vendors conservatively)
        crate::iommu::vtd::apply_and_refresh(system_table);
        crate::iommu::amdv::minimal_init(system_table);
        crate::iommu::amdv::enable_translation_all(system_table);
    }
}

