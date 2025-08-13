#![allow(dead_code)]

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

use super::{mmio_read8, mmio_read16, mmio_read32, ecam_fn_base};

const PCI_VENDOR_ID: usize = 0x00;
const PCI_DEVICE_ID: usize = 0x02;
const PCI_CLASS_OFF: usize = 0x08; // 0x0B: class, 0x0A: subclass
const PCI_CAP_PTR: usize = 0x34;
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

/// Report minimal info for the first detected virtio-blk device (capacity).
pub fn report_first(system_table: &mut SystemTable<Boot>) {
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        let lang = crate::i18n::detect_lang(system_table);
        let stdout = system_table.stdout();
        let mut reported = false;
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            if reported { return; }
            let ecam_base = a.base_address;
            let bus_start = a.start_bus; let bus_end = a.end_bus;
            let mut bus = bus_start;
            while bus <= bus_end {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(ecam_base, bus_start, bus, dev, func);
                        let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                        if vid == 0xFFFF { continue; }
                        if vid != VIRTIO_PCI_VENDOR { continue; }
                        // Prefer class check: mass storage (0x01)
                        let classreg = mmio_read32(cfg + (PCI_CLASS_OFF & !0x3));
                        let class = (classreg >> 24) as u8; let _subclass = (classreg >> 16) as u8;
                        if class != 0x01 { continue; }
                        // Walk vendor-specific caps for common/device cfg
                        let mut _common_bar: u8 = 0; let mut _common_off: u32 = 0;
                        let mut device_bar: u8 = 0; let mut device_off: u32 = 0;
                        let mut p = mmio_read8(cfg + PCI_CAP_PTR) as usize; let mut guard = 0u32;
                        while p >= 0x40 && p < 0x100 && guard < 64 {
                            let cap_id = mmio_read8(cfg + p);
                            let next = mmio_read8(cfg + p + 1) as usize;
                            let cap_len = mmio_read8(cfg + p + 2);
                            if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC && (cap_len as usize) >= 16 {
                                let cfg_type = mmio_read8(cfg + p + 3);
                                let bar = mmio_read8(cfg + p + 4);
                                let off = mmio_read32(cfg + p + 8);
                                match cfg_type {
                                    VIRTIO_PCI_CAP_COMMON_CFG => { _common_bar = bar; _common_off = off; }
                                    VIRTIO_PCI_CAP_DEVICE_CFG => { device_bar = bar; device_off = off; }
                                    _ => {}
                                }
                            }
                            if next == 0 || next == p { break; }
                            p = next; guard += 1;
                        }
                        if device_bar >= 6 { continue; }
                        // Read BAR base
                        let bar_off = 0x10 + (device_bar as usize) * 4;
                        let bar_lo = mmio_read32(cfg + bar_off);
                        if (bar_lo & 1) != 0 { continue; }
                        let mem_type = (bar_lo >> 1) & 0x3;
                        let mut base: u64 = (bar_lo as u64) & 0xFFFF_FFF0u64;
                        if mem_type == 0x2 && (device_bar as usize) < 5 {
                            let bar_hi = mmio_read32(cfg + bar_off + 4);
                            base |= (bar_hi as u64) << 32;
                        }
                        let devcfg = (base as usize).wrapping_add(device_off as usize);
                        // virtio-blk config: capacity (u64) at offset 0
                        let cap_lo = mmio_read32(devcfg + 0) as u64;
                        let cap_hi = mmio_read32(devcfg + 4) as u64;
                        let capacity_sectors = (cap_hi << 32) | cap_lo;
                        let capacity_mb = (capacity_sectors.saturating_mul(512) / 1_048_576) as u32;
                        // Print
                        let mut buf = [0u8; 96];
                        let mut n = 0;
                        for &b in crate::i18n::t(lang, crate::i18n::key::VIRTIO_BLK).as_bytes() { buf[n] = b; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(capacity_mb, &mut buf[n..]);
                        for &b in b" MiB\r\n" { buf[n] = b; n += 1; }
                        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                        reported = true; break;
                    }
                    if reported { break; }
                }
                if reported || bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
        if !reported {
            let lang2 = crate::i18n::detect_lang(system_table);
            let stdout2 = system_table.stdout();
            let _ = stdout2.write_str(crate::i18n::t(lang2, crate::i18n::key::VIRTIO_BLK_NONE));
        }
    }
}


