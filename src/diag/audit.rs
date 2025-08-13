#![allow(dead_code)]

use core::sync::atomic::{AtomicUsize, Ordering};
use core::fmt::Write as _;
use uefi::prelude::Boot;
use uefi::table::SystemTable;

/// Audit event kinds recorded for security and operational visibility.
#[derive(Clone, Copy, Debug)]
pub enum AuditKind {
    BootStart,
    BootReady,
    VmCreate(u64),
    VmStart(u64),
    VmStop(u64),
    VmDestroy(u64),
    IommuDomainCreate(u16),
    IommuAssignAdded { seg: u16, bus: u8, dev: u8, func: u8, dom: u16 },
    IommuAssignRemoved { seg: u16, bus: u8, dev: u8, func: u8, dom: u16 },
}

const AUDIT_CAP: usize = 256;
static AUDIT_WIDX: AtomicUsize = AtomicUsize::new(0);
static mut AUDIT_BUF: [AuditKind; AUDIT_CAP] = [AuditKind::BootStart; AUDIT_CAP];

/// Append an audit event to the ring buffer.
pub fn record(event: AuditKind) {
    let i = AUDIT_WIDX.fetch_add(1, Ordering::Relaxed) % AUDIT_CAP;
    unsafe { core::ptr::write_volatile(&mut AUDIT_BUF[i], event); }
}

/// Dump recent audit events to the UEFI text console.
pub fn dump(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let mut buf = [0u8; 160];
    let cur = AUDIT_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(AUDIT_CAP);
    for idx in start..cur {
        let ev = unsafe { core::ptr::read_volatile(&AUDIT_BUF[idx % AUDIT_CAP]) };
        let mut n = 0;
        match ev {
            AuditKind::BootStart => { for &b in b"audit: boot_start" { buf[n] = b; n += 1; } }
            AuditKind::BootReady => { for &b in b"audit: boot_ready" { buf[n] = b; n += 1; } }
            AuditKind::VmCreate(id) => {
                for &b in b"audit: vm_create id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            AuditKind::VmStart(id) => {
                for &b in b"audit: vm_start id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            AuditKind::VmStop(id) => {
                for &b in b"audit: vm_stop id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            AuditKind::VmDestroy(id) => {
                for &b in b"audit: vm_destroy id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            AuditKind::IommuDomainCreate(dom) => {
                for &b in b"audit: iommu_domain_create id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
            AuditKind::IommuAssignAdded { seg, bus, dev, func, dom } => {
                for &b in b"audit: iommu_assign_add bdf=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                buf[n] = b'.'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                for &b in b" dom=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
            AuditKind::IommuAssignRemoved { seg, bus, dev, func, dom } => {
                for &b in b"audit: iommu_assign_del bdf=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                buf[n] = b'.'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                for &b in b" dom=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}


