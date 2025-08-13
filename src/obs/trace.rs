#![allow(dead_code)]

use core::sync::atomic::{AtomicUsize, Ordering};
use core::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    VmCreate(u64),
    VmStart(u64),
    VmStop(u64),
    VmDestroy(u64),
    IommuInvalidateAll(u16),
    IommuInvalidateDomain(u16),
    IommuInvalidateBdf(u16, u8, u8, u8),
    IommuMapAdded(u16),
    IommuMapRemoved(u16),
}

const TRACE_CAP: usize = 64;
static TRACE_WIDX: AtomicUsize = AtomicUsize::new(0);
static mut TRACE_BUF: [Event; TRACE_CAP] = [Event::VmCreate(0); TRACE_CAP];

pub fn emit(e: Event) {
    let i = TRACE_WIDX.fetch_add(1, Ordering::Relaxed) % TRACE_CAP;
    unsafe { core::ptr::write_volatile(&mut TRACE_BUF[i], e); }
}

pub fn dump(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let stdout = system_table.stdout();
    let mut buf = [0u8; 96];
    // Print last TRACE_CAP events
    let cur = TRACE_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(TRACE_CAP);
    for idx in start..cur {
        let ev = unsafe { core::ptr::read_volatile(&TRACE_BUF[idx % TRACE_CAP]) };
        let mut n = 0;
        match ev {
            Event::VmCreate(id) => {
                for &b in b"trace: vm_create id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            Event::VmStart(id) => {
                for &b in b"trace: vm_start id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            Event::VmStop(id) => {
                for &b in b"trace: vm_stop id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            Event::VmDestroy(id) => {
                for &b in b"trace: vm_destroy id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]);
            }
            Event::IommuInvalidateAll(seg) => {
                for &b in b"trace: vtd_inval_all seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
            }
            Event::IommuInvalidateDomain(dom) => {
                for &b in b"trace: vtd_inval_dom id=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
            Event::IommuInvalidateBdf(seg, bus, dev, func) => {
                for &b in b"trace: vtd_inval_bdf " { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                buf[n] = b'.'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
            }
            Event::IommuMapAdded(dom) => {
                for &b in b"trace: vtd_map_add dom=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
            Event::IommuMapRemoved(dom) => {
                for &b in b"trace: vtd_map_del dom=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]);
            }
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}

pub fn dump_with_writer(mut write_bytes: impl FnMut(&[u8])) {
    let cur = TRACE_WIDX.load(Ordering::Relaxed);
    let start = cur.saturating_sub(TRACE_CAP);
    let mut buf = [0u8; 96];
    for idx in start..cur {
        let ev = unsafe { core::ptr::read_volatile(&TRACE_BUF[idx % TRACE_CAP]) };
        let mut n = 0;
        match ev {
            Event::VmCreate(id) => { for &b in b"trace: vm_create id=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]); }
            Event::VmStart(id) => { for &b in b"trace: vm_start id=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]); }
            Event::VmStop(id) => { for &b in b"trace: vm_stop id=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]); }
            Event::VmDestroy(id) => { for &b in b"trace: vm_destroy id=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(id as u32, &mut buf[n..]); }
            Event::IommuInvalidateAll(seg) => { for &b in b"trace: vtd_inval_all seg=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]); }
            Event::IommuInvalidateDomain(dom) => { for &b in b"trace: vtd_inval_dom id=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]); }
            Event::IommuInvalidateBdf(seg, bus, dev, func) => {
                for &b in b"trace: vtd_inval_bdf " { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]); buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]); buf[n] = b':'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]); buf[n] = b'.'; n += 1;
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
            }
            Event::IommuMapAdded(dom) => { for &b in b"trace: vtd_map_add dom=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]); }
            Event::IommuMapRemoved(dom) => { for &b in b"trace: vtd_map_del dom=" { buf[n] = b; n += 1; } n += crate::firmware::acpi::u32_to_dec(dom as u32, &mut buf[n..]); }
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        write_bytes(&buf[..n]);
    }
}

pub fn clear() {
    // Reset write index and wipe buffer best-effort
    TRACE_WIDX.store(0, Ordering::Relaxed);
    unsafe {
        for i in 0..TRACE_CAP { core::ptr::write_volatile(&mut TRACE_BUF[i], Event::VmCreate(0)); }
    }
}


