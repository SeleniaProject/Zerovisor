#![allow(dead_code)]

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

use super::{mmio_read8, mmio_read16, mmio_read32, mmio_write8, ecam_fn_base};

const PCI_VENDOR_ID: usize = 0x00;
const PCI_DEVICE_ID: usize = 0x02;
const PCI_CAP_PTR: usize = 0x34;
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER: u8 = 2;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

/// Very small virtio-console init: set status, leave queues unconfigured, only demonstrate status.
pub fn init_and_write_hello(system_table: &mut SystemTable<Boot>) {
    // Find MCFG and scan for a virtio device with console-like class hints (not strictly needed)
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        let stdout = system_table.stdout();
        let mut initialized = false;
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            if initialized { return; }
            let ecam_base = a.base_address;
            let bus_start = a.start_bus;
            let bus_end = a.end_bus;
            let mut bus = bus_start;
            while bus <= bus_end {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(ecam_base, bus_start, bus, dev, func);
                        let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                        if vid == 0xFFFF { continue; }
                        if vid != VIRTIO_PCI_VENDOR { continue; }
                        let _did = mmio_read16(cfg + PCI_DEVICE_ID);
                        // Walk vendor-specific capabilities to get common cfg
                        let cap_ptr = mmio_read8(cfg + PCI_CAP_PTR) as usize;
                        let mut p = cap_ptr;
                        let mut common_bar: u8 = 0; let mut common_off: u32 = 0;
                        let mut have_common = false;
                        let mut guard = 0u32;
                        while p >= 0x40 && p < 0x100 && guard < 64 {
                            let cap_id = mmio_read8(cfg + p);
                            let next = mmio_read8(cfg + p + 1) as usize;
                            let cap_len = mmio_read8(cfg + p + 2);
                            if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC && (cap_len as usize) >= 16 {
                                let cfg_type = mmio_read8(cfg + p + 3);
                                let bar = mmio_read8(cfg + p + 4);
                                let off = mmio_read32(cfg + p + 8);
                                if cfg_type == VIRTIO_PCI_CAP_COMMON_CFG {
                                    common_bar = bar; common_off = off; have_common = true; break;
                                }
                            }
                            if next == 0 || next == p { break; }
                            p = next; guard += 1;
                        }
                        if !have_common { continue; }
                        // BAR base
                        let bar_index = common_bar as usize;
                        if bar_index >= 6 { continue; }
                        let bar_off = 0x10 + bar_index * 4;
                        let bar_lo = mmio_read32(cfg + bar_off);
                        if (bar_lo & 0x1) != 0 { continue; } // not memory BAR
                        let mem_type = (bar_lo >> 1) & 0x3;
                        let mut base: u64 = (bar_lo as u64) & 0xFFFF_FFF0u64;
                        if mem_type == 0x2 && bar_index < 5 {
                            let bar_hi = mmio_read32(cfg + bar_off + 4);
                            base |= (bar_hi as u64) << 32;
                        }
                        let common_base = (base as usize).wrapping_add(common_off as usize);
                        // Offsets in virtio_pci_common_cfg
                        let device_status = common_base + 0x14;
                        // Basic status handshake
                        mmio_write8(device_status, 0);
                        mmio_write8(device_status, VIRTIO_STATUS_ACKNOWLEDGE);
                        mmio_write8(device_status, VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
                        // Features negotiation would go here; skip and set DRIVER_OK for demo
                        let st = mmio_read8(device_status);
                        mmio_write8(device_status, st | VIRTIO_STATUS_DRIVER_OK);
                        let _ = stdout.write_str("virtio-console: minimal init (status set)\r\n");
                        initialized = true;
                        break;
                    }
                    if initialized { break; }
                }
                if initialized || bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
        if !initialized {
            let _ = stdout.write_str("virtio-console: device not found\r\n");
        }
    }
}


