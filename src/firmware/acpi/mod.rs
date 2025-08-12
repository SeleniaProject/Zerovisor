#![allow(dead_code)]

//! Minimal ACPI discovery for UEFI environment.
//!
//! This module locates the RSDP via UEFI Configuration Table and then provides
//! safe wrappers to walk RSDT/XSDT to find a few essential system description
//! tables (FADT, MADT, MCFG). Parsing is intentionally shallow at this stage
//! to avoid complex dependencies while enabling SMP and PCIe groundwork.

use core::mem::size_of;
use core::ptr::NonNull;

use uefi::table::cfg::{ACPI2_GUID, ACPI_GUID};
use uefi::table::SystemTable;
use uefi::prelude::Boot;

/// Root System Description Pointer (RSDP) for ACPI 2.0+
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct Rsdp20 {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    _reserved: [u8; 3],
}

/// ACPI System Description Table header
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[repr(C, packed)]
struct Rsdt {
    header: SdtHeader,
    entries: [u32; 0],
}

#[repr(C, packed)]
struct Xsdt {
    header: SdtHeader,
    entries: [u64; 0],
}

/// MADT (APIC) signature
const SIG_MADT: [u8; 4] = *b"APIC";
/// FADT signature
const SIG_FADT: [u8; 4] = *b"FACP";
/// MCFG signature
const SIG_MCFG: [u8; 4] = *b"MCFG";

fn calc_checksum(bytes: &[u8]) -> u8 {
    let mut sum: u8 = 0;
    for &b in bytes { sum = sum.wrapping_add(b); }
    sum
}

fn validate_sdt(h: &SdtHeader) -> bool {
    let len = h.length as usize;
    if len < size_of::<SdtHeader>() { return false; }
    let p = h as *const _ as *const u8;
    let data = unsafe { core::slice::from_raw_parts(p, len) };
    calc_checksum(data) == 0
}

fn slice_from_phys<T>(phys: u64, _len: usize) -> Option<&'static T> {
    // In UEFI identity-mapped firmware context, physical == virtual for low
    // memory regions where ACPI tables reside. This is a pragmatic assumption
    // for bootstrap; a robust implementation should use memory map services.
    if phys == 0 { return None; }
    let p = phys as *const T;
    NonNull::new(p as *mut T).map(|nn| unsafe { &*nn.as_ptr() })
}

/// Locate RSDP via UEFI Configuration Table. Prefers ACPI 2.0+ GUID.
pub fn find_rsdp(system_table: &SystemTable<Boot>) -> Option<Rsdp20> {
    for entry in system_table.config_table() {
        if entry.guid == ACPI2_GUID || entry.guid == ACPI_GUID {
            let phys = entry.address as u64;
            let rsdp = slice_from_phys::<Rsdp20>(phys, size_of::<Rsdp20>())?;
            // Check 8-byte signature "RSD PTR "
            if &rsdp.signature == b"RSD PTR " {
                // Validate checksum according to revision
                let len = if rsdp.revision >= 2 { rsdp.length as usize } else { 20 };
                let ptr = rsdp as *const _ as *const u8;
                let data = unsafe { core::slice::from_raw_parts(ptr, len) };
                if calc_checksum(data) == 0 { return Some(*rsdp); }
            }
        }
    }
    None
}

/// Iterate XSDT entries and yield SDT headers.
pub fn iter_xsdt(xsdt_phys: u64) -> impl Iterator<Item = &'static SdtHeader> {
    struct Iter { base: &'static Xsdt, count: usize, idx: usize }
    impl Iterator for Iter {
        type Item = &'static SdtHeader;
        fn next(&mut self) -> Option<Self::Item> {
            if self.idx >= self.count { return None; }
            // Compute entries pointer without referencing packed fields
            let entries_ptr = (self.base as *const Xsdt as *const u8)
                .wrapping_add(size_of::<SdtHeader>()) as *const u64;
            let ptrs = unsafe { core::slice::from_raw_parts(entries_ptr, self.count) };
            let phys = unsafe { *ptrs.get_unchecked(self.idx) };
            self.idx += 1;
            let hdr = slice_from_phys::<SdtHeader>(phys, 0)?;
            if validate_sdt(hdr) { Some(hdr) } else { None }
        }
    }
    let xsdt = slice_from_phys::<Xsdt>(xsdt_phys, 0).expect("XSDT address invalid");
    let bytes = xsdt.header.length as usize;
    let count = (bytes - size_of::<SdtHeader>()) / size_of::<u64>();
    Iter { base: xsdt, count, idx: 0 }
}

/// Finds first table by 4-byte signature in XSDT, falling back to RSDT.
pub fn find_table(system_table: &SystemTable<Boot>, sig: [u8; 4]) -> Option<&'static SdtHeader> {
    let rsdp = find_rsdp(system_table)?;
    if rsdp.xsdt_address != 0 {
        for hdr in iter_xsdt(rsdp.xsdt_address) {
            if hdr.signature == sig { return Some(hdr); }
        }
    }
    // RSDT fallback (32-bit entries)
    if rsdp.rsdt_address != 0 {
        let rsdt = slice_from_phys::<Rsdt>(rsdp.rsdt_address as u64, 0)?;
        if !validate_sdt(&rsdt.header) { return None; }
        let bytes = rsdt.header.length as usize;
        let count = (bytes - size_of::<SdtHeader>()) / size_of::<u32>();
        // Compute entries pointer without referencing packed fields
        let entries_ptr = (rsdt as *const Rsdt as *const u8)
            .wrapping_add(size_of::<SdtHeader>()) as *const u32;
        let ptrs = unsafe { core::slice::from_raw_parts(entries_ptr, count) };
        for &phys in ptrs.iter() {
            let hdr = slice_from_phys::<SdtHeader>(phys as u64, 0)?;
            if validate_sdt(hdr) && hdr.signature == sig { return Some(hdr); }
        }
    }
    None
}

pub fn find_madt(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_MADT)
}

pub fn find_fadt(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_FADT)
}

pub fn find_mcfg(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_MCFG)
}

/// Minimal MADT header for iterating APIC structures.
#[repr(C, packed)]
pub(crate) struct MadtHeader {
    header: SdtHeader,
    lapic_addr: u32,
    flags: u32,
    // followed by variable-length APIC structures
}

/// Enumerate CPU APIC IDs from MADT and print to the provided writer function.
pub fn madt_list_cpus_from<F>(hdr: &'static SdtHeader, mut writer: F)
where
    F: FnMut(&str),
{
    let madt = hdr as *const SdtHeader as *const MadtHeader;
    let madt = unsafe { &*madt };
    let base = madt as *const MadtHeader as usize;
    let total_len = madt.header.length as usize;
    let mut off = core::mem::size_of::<MadtHeader>();
    let mut count: u32 = 0;
    while off + 2 <= total_len {
        let p = (base + off) as *const u8;
        let etype = unsafe { p.read() };
        let elen = unsafe { p.add(1).read() } as usize;
        if elen < 2 || off + elen > total_len { break; }
        match etype {
            0 => {
                // Processor Local APIC
                if elen >= 8 {
                    let apic_id = unsafe { p.add(3).read() } as u32;
                    let _flags = u32::from_le_bytes([
                        unsafe { p.add(4).read() },
                        unsafe { p.add(5).read() },
                        unsafe { p.add(6).read() },
                        unsafe { p.add(7).read() },
                    ]);
                    count += 1;
                    // Small fixed buffer formatting without allocation
                    let mut buf = [0u8; 64];
                    let mut n = 0;
                    for &b in b"CPU: Local APIC ID=" { buf[n] = b; n += 1; }
                    // decimal formatting
                    n += u32_to_dec(apic_id, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                }
            }
            9 => {
                // Processor Local x2APIC
                if elen >= 16 {
                    let apic_id = u32::from_le_bytes([
                        unsafe { p.add(4).read() },
                        unsafe { p.add(5).read() },
                        unsafe { p.add(6).read() },
                        unsafe { p.add(7).read() },
                    ]);
                    count += 1;
                    let mut buf = [0u8; 64];
                    let mut n = 0;
                    for &b in b"CPU: x2APIC ID=" { buf[n] = b; n += 1; }
                    n += u32_to_dec(apic_id, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                }
            }
            _ => {}
        }
        off += elen;
    }
    // Print total count
    let mut buf = [0u8; 64];
    let mut n = 0;
    for &b in b"ACPI: CPU count=" { buf[n] = b; n += 1; }
    n += u32_to_dec(count, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub(crate) fn u32_to_dec(mut v: u32, out: &mut [u8]) -> usize {
    // write decimal to buffer; returns number of bytes written
    if v == 0 {
        if !out.is_empty() { out[0] = b'0'; return 1; }
        return 0;
    }
    let mut tmp = [0u8; 10];
    let mut i = 0;
    while v > 0 && i < tmp.len() { tmp[i] = (b'0' + (v % 10) as u8); v /= 10; i += 1; }
    let mut n = 0;
    while i > 0 && n < out.len() { i -= 1; out[n] = tmp[i]; n += 1; }
    n
}

/// Minimal MCFG structures
#[repr(C, packed)]
pub(crate) struct McfgHeader {
    header: SdtHeader,
    _reserved: u64,
    // followed by allocations
}

#[repr(C, packed)]
pub(crate) struct McfgAllocation {
    pub base_address: u64,
    pub pci_segment: u16,
    pub start_bus: u8,
    pub end_bus: u8,
    _reserved: [u8; 4],
}

/// Enumerate PCIe ECAM segments from MCFG and print via writer
pub fn mcfg_list_segments_from<F>(hdr: &'static SdtHeader, mut writer: F)
where
    F: FnMut(&str),
{
    let mcfg = hdr as *const SdtHeader as *const McfgHeader;
    let mcfg = unsafe { &*mcfg };
    let base = mcfg as *const McfgHeader as usize;
    let total = mcfg.header.length as usize;
    let mut off = core::mem::size_of::<McfgHeader>();
    while off + core::mem::size_of::<McfgAllocation>() <= total {
        let p = (base + off) as *const McfgAllocation;
        let a = unsafe { &*p };
        // format
        let mut buf = [0u8; 96];
        let mut n = 0;
        for &b in b"PCIe ECAM: seg=" { buf[n] = b; n += 1; }
        n += u32_to_dec(a.pci_segment as u32, &mut buf[n..]);
        for &b in b" bus=" { buf[n] = b; n += 1; }
        n += u32_to_dec(a.start_bus as u32, &mut buf[n..]);
        buf[n] = b'-'; n += 1;
        n += u32_to_dec(a.end_bus as u32, &mut buf[n..]);
        for &b in b" base=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(a.base_address, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        off += core::mem::size_of::<McfgAllocation>();
    }
}

fn u64_to_hex(mut v: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut started = false;
    let mut n = 0;
    for i in (0..16).rev() {
        let nyb = ((v >> (i * 4)) & 0xF) as usize;
        if nyb != 0 || started || i == 0 { started = true; if n < out.len() { out[n] = HEX[nyb]; n += 1; } }
    }
    n
}


