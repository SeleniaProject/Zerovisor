#![allow(dead_code)]

use core::sync::atomic::{AtomicUsize, Ordering};
use core::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    VmCreate(u64),
    VmStart(u64),
    VmStop(u64),
    VmDestroy(u64),
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
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}


