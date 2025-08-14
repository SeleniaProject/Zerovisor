#![allow(dead_code)]

//! VirtIO minimal scaffolding: PCIe ECAM scan and basic device reporting.
//!
//! This module implements a conservative PCIe ECAM scanner to locate VirtIO
//! devices (legacy and modern) using ACPI MCFG information discovered earlier.
//! It does not program devices yet; the goal is to validate enumeration and
//! provide a foundation for queue setup in subsequent milestones.

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

mod console;
mod block;
pub mod net;

/// Read a 32-bit little-endian value from an MMIO address safely.
#[inline(always)]
pub(super) fn mmio_read32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline(always)]
pub(super) fn mmio_read16(addr: usize) -> u16 {
    unsafe { core::ptr::read_volatile(addr as *const u16) }
}

#[inline(always)]
pub(super) fn mmio_read8(addr: usize) -> u8 {
    unsafe { core::ptr::read_volatile(addr as *const u8) }
}

#[inline(always)]
pub(super) fn mmio_write8(addr: usize, val: u8) { unsafe { core::ptr::write_volatile(addr as *mut u8, val) } }
#[inline(always)]
pub(super) fn mmio_write16(addr: usize, val: u16) { unsafe { core::ptr::write_volatile(addr as *mut u16, val) } }
#[inline(always)]
pub(super) fn mmio_write32(addr: usize, val: u32) { unsafe { core::ptr::write_volatile(addr as *mut u32, val) } }
#[inline(always)]
pub(super) fn mmio_write64(addr: usize, val: u64) { unsafe { core::ptr::write_volatile(addr as *mut u64, val) } }

#[inline(always)]
pub(super) fn ecam_fn_base(seg_base: u64, start_bus: u8, bus: u8, dev: u8, func: u8) -> usize {
    // ECAM address = Base + (Bus-Start)*1MB + Dev*32KB + Func*4KB
    (seg_base as usize)
        .wrapping_add(((bus as usize).saturating_sub(start_bus as usize)) << 20)
        .wrapping_add((dev as usize) << 15)
        .wrapping_add((func as usize) << 12)
}

/// Vendor ID values used for VirtIO PCI devices.
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;

/// Minimal PCI configuration offsets.
const PCI_VENDOR_ID: usize = 0x00;
const PCI_DEVICE_ID: usize = 0x02;
const PCI_REVISION_ID: usize = 0x08; // low byte
const PCI_PROG_IF: usize = 0x09;
const PCI_SUBCLASS: usize = 0x0A;
const PCI_CLASS: usize = 0x0B;
const PCI_CAP_PTR: usize = 0x34;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;

// virtio_pci_cap.cfg_type values (virtio 1.0+)
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

// Device status bits (virtio 1.0)
const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;

/// Scan all ECAM segments from MCFG for VirtIO devices and print brief lines.
pub fn scan_and_report(system_table: &mut SystemTable<Boot>) {
    // Try to locate MCFG and iterate segments
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        let mut found = 0u32;
        let lang = crate::i18n::detect_lang(system_table);
        let stdout = system_table.stdout();
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::VIRTIO_SCAN));
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            let ecam_base = a.base_address;
            let bus_start = a.start_bus;
            let bus_end = a.end_bus;
            // Enumerate bus/dev/func within this segment conservatively
            let mut bus = bus_start;
            while bus <= bus_end {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(ecam_base, bus_start, bus, dev, func);
                        let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                        if vid == 0xFFFF { continue; }
                        let did = mmio_read16(cfg + PCI_DEVICE_ID);
                        let cls = (mmio_read32(cfg + PCI_CLASS & !0x3).to_le() >> 24) as u8;
                        let scls = (mmio_read32(cfg + PCI_CLASS & !0x3).to_le() >> 16) as u8;
                        if vid == VIRTIO_PCI_VENDOR || (cls == 0x02 || cls == 0x01) && (vid == VIRTIO_PCI_VENDOR) {
                            // Print a compact line with location and IDs
                            let mut buf = [0u8; 128];
                            let mut n = 0;
                            for &b in b"VirtIO: seg=" { buf[n] = b; n += 1; }
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
                            n += crate::firmware::acpi::u32_to_dec(scls as u32, &mut buf[n..]);
                            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                            found = found.saturating_add(1);

                            // Parse PCI capability list for virtio modern caps
                            let cap_ptr = mmio_read8(cfg + PCI_CAP_PTR) as usize;
                            let mut p = cap_ptr;
                            let mut have_common = false;
                            let mut have_notify = false;
                            let mut have_isr = false;
                            let mut have_device = false;
                            // Remember common cfg location to attempt status handshake
                            let mut common_bar: u8 = 0;
                            let mut common_off: u32 = 0;
                            let mut iter_guard = 0u32;
                            while p >= 0x40 && p < 0x100 && iter_guard < 64 {
                                let cap_id = mmio_read8(cfg + p);
                                let next = mmio_read8(cfg + p + 1) as usize;
                                let cap_len = mmio_read8(cfg + p + 2);
                                if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC && (cap_len as usize) >= 16 {
                                    let cfg_type = mmio_read8(cfg + p + 3);
                                    let bar = mmio_read8(cfg + p + 4);
                                    let off = mmio_read32(cfg + p + 8);
                                    let len = mmio_read32(cfg + p + 12);
                                    // Report a short line per capability
                                    let mut lbuf = [0u8; 128];
                                    let mut m = 0;
                                    for &b in b"  cap: type=" { lbuf[m] = b; m += 1; }
                                    m += crate::firmware::acpi::u32_to_dec(cfg_type as u32, &mut lbuf[m..]);
                                    for &b in b" bar=" { lbuf[m] = b; m += 1; }
                                    m += crate::firmware::acpi::u32_to_dec(bar as u32, &mut lbuf[m..]);
                                    for &b in b" off=0x" { lbuf[m] = b; m += 1; }
                                    m += crate::util::format::u64_hex(off as u64, &mut lbuf[m..]);
                                    for &b in b" len=0x" { lbuf[m] = b; m += 1; }
                                    m += crate::util::format::u64_hex(len as u64, &mut lbuf[m..]);
                                    lbuf[m] = b'\r'; m += 1; lbuf[m] = b'\n'; m += 1;
                                    let _ = stdout.write_str(core::str::from_utf8(&lbuf[..m]).unwrap_or("\r\n"));
                                    match cfg_type {
                                        VIRTIO_PCI_CAP_COMMON_CFG => { have_common = true; common_bar = bar; common_off = off; }
                                        VIRTIO_PCI_CAP_NOTIFY_CFG => { have_notify = true; }
                                        VIRTIO_PCI_CAP_ISR_CFG => { have_isr = true; }
                                        VIRTIO_PCI_CAP_DEVICE_CFG => { have_device = true; }
                                        _ => {}
                                    }
                                }
                                if next == 0 || next == p { break; }
                                p = next;
                                iter_guard += 1;
                            }
                            // Summary line for capabilities
                            let mut sbuf = [0u8; 96];
                            let mut s = 0;
                            for &b in b"  caps: common=" { sbuf[s] = b; s += 1; }
                            sbuf[s] = if have_common { b'1' } else { b'0' }; s += 1;
                            for &b in b" notify=" { sbuf[s] = b; s += 1; }
                            sbuf[s] = if have_notify { b'1' } else { b'0' }; s += 1;
                            for &b in b" isr=" { sbuf[s] = b; s += 1; }
                            sbuf[s] = if have_isr { b'1' } else { b'0' }; s += 1;
                            for &b in b" device=" { sbuf[s] = b; s += 1; }
                            sbuf[s] = if have_device { b'1' } else { b'0' }; s += 1;
                            sbuf[s] = b'\r'; s += 1; sbuf[s] = b'\n'; s += 1;
                            let _ = stdout.write_str(core::str::from_utf8(&sbuf[..s]).unwrap_or("\r\n"));

                            // Try a minimal modern status handshake (ACK+DRIVER)
                            if have_common {
                                // Read BAR base (supports 32/64-bit MMIO BAR types for BAR0..5)
                                let bar_index = common_bar as usize;
                                if bar_index < 6 {
                                    let bar_off = 0x10 + bar_index * 4;
                                    let bar_lo = mmio_read32(cfg + bar_off);
                                    // Mem BAR if bit0==0
                                    if (bar_lo & 0x1) == 0 {
                                        let mem_type = (bar_lo >> 1) & 0x3;
                                        let mut base: u64 = (bar_lo as u64) & 0xFFFF_FFF0u64;
                                        let is_64 = mem_type == 0x2;
                                        if is_64 && bar_index < 5 {
                                            let bar_hi = mmio_read32(cfg + bar_off + 4);
                                            base |= (bar_hi as u64) << 32;
                                        }
                                        let common_base = (base as usize).wrapping_add(common_off as usize);
                                        // Offsets per virtio_pci_common_cfg
                                        let device_status = 0x14usize;
                                        // Write ACK|DRIVER
                                        let st = mmio_read8(common_base + device_status);
                                        mmio_write8(common_base + device_status, st | VIRTIO_STATUS_ACKNOWLEDGE);
                                        let st2 = mmio_read8(common_base + device_status);
                                        mmio_write8(common_base + device_status, st2 | VIRTIO_STATUS_DRIVER);
                                        let _ = stdout.write_str("  handshake: ACK|DRIVER set\r\n");
                                    }
                                }
                            }
                        }
                    }
                }
                if bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
        if found == 0 {
            let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::VIRTIO_NONE));
        }
    }
}

/// Initialize the first detected virtio-console device minimally and transmit a hello line.
pub fn console_init_minimal(system_table: &mut SystemTable<Boot>) {
    console::init_and_write_hello(system_table);
}

/// Report minimal info for virtio-blk/virtio-net devices.
pub fn devices_report_minimal(system_table: &mut SystemTable<Boot>) {
    block::report_first(system_table);
    net::report_first(system_table);
}


