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
/// DMAR (Intel VT-d) signature
const SIG_DMAR: [u8; 4] = *b"DMAR";
/// IVRS (AMD-Vi) signature
const SIG_IVRS: [u8; 4] = *b"IVRS";

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
pub(crate) fn find_rsdp(system_table: &SystemTable<Boot>) -> Option<Rsdp20> {
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
pub(crate) fn iter_xsdt(xsdt_phys: u64) -> impl Iterator<Item = &'static SdtHeader> {
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
pub(crate) fn find_table(system_table: &SystemTable<Boot>, sig: [u8; 4]) -> Option<&'static SdtHeader> {
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

pub(crate) fn find_madt(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_MADT)
}

pub(crate) fn find_fadt(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_FADT)
}

pub(crate) fn find_mcfg(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_MCFG)
}

/// Find Intel VT-d remapping table (DMAR) if present.
pub(crate) fn find_dmar(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_DMAR)
}

/// Find AMD I/O virtualization reporting table (IVRS) if present.
pub(crate) fn find_ivrs(system_table: &SystemTable<Boot>) -> Option<&'static SdtHeader> {
    find_table(system_table, SIG_IVRS)
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
pub(crate) fn madt_list_cpus_from<F>(hdr: &'static SdtHeader, mut writer: F)
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
    while v > 0 && i < tmp.len() { tmp[i] = b'0' + (v % 10) as u8; v /= 10; i += 1; }
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
pub(crate) fn mcfg_list_segments_from<F>(hdr: &'static SdtHeader, mut writer: F)
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

fn u64_to_hex(v: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut started = false;
    let mut n = 0;
    for i in (0..16).rev() {
        let nyb = ((v >> (i * 4)) & 0xF) as usize;
        if nyb != 0 || started || i == 0 { started = true; if n < out.len() { out[n] = HEX[nyb]; n += 1; } }
    }
    n
}

// --- DMAR/IVRS minimal summaries (header-only, safe) ---

fn write_ascii_trim(src: &[u8], out: &mut [u8]) -> usize {
    // Copy printable ASCII, trim trailing spaces
    let mut end = src.len();
    while end > 0 && src[end - 1] == b' ' { end -= 1; }
    let mut n = 0;
    for &b in &src[..end] {
        let ch = if b >= 0x20 && b <= 0x7E { b } else { b'.' };
        if n < out.len() { out[n] = ch; n += 1; } else { break; }
    }
    n
}

fn sdt_header_summary(label: &[u8; 4], hdr: &'static SdtHeader, out_line: &mut [u8]) -> usize {
    let mut n = 0;
    // e.g., "DMAR: len=xxxx rev=x oem=XXXXXX table=XXXXXXXX"
    for &b in label.iter() { if n < out_line.len() { out_line[n] = b; n += 1; } }
    if n + 2 <= out_line.len() { out_line[n] = b':'; n += 1; out_line[n] = b' '; n += 1; }
    for &b in b"len=" { if n < out_line.len() { out_line[n] = b; n += 1; } }
    n += u32_to_dec(hdr.length, &mut out_line[n..]);
    for &b in b" rev=" { if n < out_line.len() { out_line[n] = b; n += 1; } }
    n += u32_to_dec(hdr.revision as u32, &mut out_line[n..]);
    for &b in b" oem=" { if n < out_line.len() { out_line[n] = b; n += 1; } }
    n += write_ascii_trim(&hdr.oem_id, &mut out_line[n..]);
    for &b in b" table=" { if n < out_line.len() { out_line[n] = b; n += 1; } }
    n += write_ascii_trim(&hdr.oem_table_id, &mut out_line[n..]);
    if n + 2 <= out_line.len() { out_line[n] = b'\r'; n += 1; out_line[n] = b'\n'; n += 1; }
    n
}

/// Print a one-line summary for the DMAR table header.
pub(crate) fn dmar_summary(mut writer: impl FnMut(&str), hdr: &'static SdtHeader) {
    let mut buf = [0u8; 96];
    let n = sdt_header_summary(b"DMAR", hdr, &mut buf);
    let _ = writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

/// Print a one-line summary for the IVRS table header.
pub(crate) fn ivrs_summary(mut writer: impl FnMut(&str), hdr: &'static SdtHeader) {
    let mut buf = [0u8; 96];
    let n = sdt_header_summary(b"IVRS", hdr, &mut buf);
    let _ = writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

/// Enumerate DMAR remapping structures (DRHD/RMRR/ATSR) with minimal fields.
pub(crate) fn dmar_list_structs_from(mut writer: impl FnMut(&str), hdr: &'static SdtHeader) {
    #[repr(C, packed)]
    struct DmarTableHeader { header: SdtHeader, host_addr_width: u8, flags: u8, _rsvd: [u8; 10] }
    let base = hdr as *const SdtHeader as usize;
    let total_len = hdr.length as usize;
    let off0 = core::mem::size_of::<DmarTableHeader>();
    let mut off = off0;
    while off + 4 <= total_len {
        let p = (base + off) as *const u8;
        let t0 = unsafe { p.read() } as u16;
        let t1 = unsafe { p.add(1).read() } as u16;
        let typ = t0 | ((t1 as u16) << 8);
        let l0 = unsafe { p.add(2).read() } as u16;
        let l1 = unsafe { p.add(3).read() } as u16;
        let len = (l0 | ((l1 as u16) << 8)) as usize;
        if len < 4 || off + len > total_len { break; }
        // Format a short line: type/len and key fields if known
        let mut buf = [0u8; 128];
        let mut n = 0;
        for &b in b"DMAR: struct type=" { buf[n] = b; n += 1; }
        n += u32_to_dec(typ as u32, &mut buf[n..]);
        for &b in b" len=" { buf[n] = b; n += 1; }
        n += u32_to_dec(len as u32, &mut buf[n..]);
        // Determine header size to locate optional device scope list
        let mut header_size_for_scopes: usize = 0;
        match typ {
            0 => {
                // DRHD: flags(1), rsvd(1), seg(2), reg_base(8)
                if len >= 4 + 12 {
                    let seg_lo = unsafe { p.add(6).read() } as u16;
                    let seg_hi = unsafe { p.add(7).read() } as u16;
                    let seg = (seg_lo | (seg_hi << 8)) as u32;
                    let mut addr: u64 = 0;
                    for i in 0..8 {
                        addr |= (unsafe { p.add(8 + i).read() } as u64) << (i * 8);
                    }
                    for &b in b" seg=" { buf[n] = b; n += 1; }
                    n += u32_to_dec(seg, &mut buf[n..]);
                    for &b in b" reg=0x" { buf[n] = b; n += 1; }
                    n += u64_to_hex(addr, &mut buf[n..]);
                    header_size_for_scopes = 4 + 12;
                }
            }
            1 => {
                // RMRR: rsvd(2), seg(2), base(8), limit(8)
                if len >= 4 + 20 {
                    let seg_lo = unsafe { p.add(4).read() } as u16;
                    let seg_hi = unsafe { p.add(5).read() } as u16;
                    let seg = (seg_lo | (seg_hi << 8)) as u32;
                    let mut base64: u64 = 0; let mut limit64: u64 = 0;
                    for i in 0..8 { base64 |= (unsafe { p.add(6 + i).read() } as u64) << (i * 8); }
                    for i in 0..8 { limit64 |= (unsafe { p.add(14 + i).read() } as u64) << (i * 8); }
                    for &b in b" seg=" { buf[n] = b; n += 1; }
                    n += u32_to_dec(seg, &mut buf[n..]);
                    for &b in b" range=0x" { buf[n] = b; n += 1; }
                    n += u64_to_hex(base64, &mut buf[n..]);
                    for &b in b"-0x" { buf[n] = b; n += 1; }
                    n += u64_to_hex(limit64, &mut buf[n..]);
                    header_size_for_scopes = 4 + 20;
                }
            }
            2 => {
                // ATSR: flags(1) at +4, reserved(1) at +5, segment(2) at +6..+7
                if len >= 8 {
                    let flags = unsafe { p.add(4).read() } as u32;
                    let seg_lo = unsafe { p.add(6).read() } as u16;
                    let seg_hi = unsafe { p.add(7).read() } as u16;
                    let seg = (seg_lo | (seg_hi << 8)) as u32;
                    for &b in b" seg=" { buf[n] = b; n += 1; }
                    n += u32_to_dec(seg, &mut buf[n..]);
                    for &b in b" flags=0x" { buf[n] = b; n += 1; }
                    n += u64_to_hex(flags as u64, &mut buf[n..]);
                    header_size_for_scopes = 8;
                }
            }
            _ => {}
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        // If this structure carries a device scope list, enumerate shallow info
        if header_size_for_scopes > 0 && header_size_for_scopes < len {
            let mut s_off = off + header_size_for_scopes;
            let end = off + len;
            while s_off + 6 <= end {
                let sp = (base + s_off) as *const u8;
                let s_type = unsafe { sp.read() } as u32;
                let s_len = (unsafe { sp.add(1).read() } as u32) | ((unsafe { sp.add(2).read() } as u32) << 8);
                if s_len < 6 || s_off + (s_len as usize) > end { break; }
                let bus = unsafe { sp.add(4).read() } as u32; // start bus number
                let mut lbuf = [0u8; 96];
                let mut m = 0;
                for &b in b"DMAR:   scope type=" { lbuf[m] = b; m += 1; }
                m += u32_to_dec(s_type, &mut lbuf[m..]);
                for &b in b" len=" { lbuf[m] = b; m += 1; }
                m += u32_to_dec(s_len, &mut lbuf[m..]);
                for &b in b" bus=" { lbuf[m] = b; m += 1; }
                m += u32_to_dec(bus, &mut lbuf[m..]);
                lbuf[m] = b'\r'; m += 1; lbuf[m] = b'\n'; m += 1;
                writer(core::str::from_utf8(&lbuf[..m]).unwrap_or("\r\n"));
                s_off += s_len as usize;
            }
        }
        off += len;
    }
}

/// Enumerate IVRS entries (type and length only, safe header walk).
pub(crate) fn ivrs_list_entries_from(mut writer: impl FnMut(&str), hdr: &'static SdtHeader) {
    #[repr(C, packed)] struct IvrsTableHeader { header: SdtHeader, iv_info: u32 }
    let base = hdr as *const SdtHeader as usize;
    let total = hdr.length as usize;
    let mut off = core::mem::size_of::<IvrsTableHeader>();
    while off + 4 <= total {
        let p = (base + off) as *const u8;
        let entry_type = unsafe { p.read() } as u32;
        let len_lo = unsafe { p.add(2).read() } as u16;
        let len_hi = unsafe { p.add(3).read() } as u16;
        let len = (len_lo | (len_hi << 8)) as usize;
        if len < 4 || off + len > total { break; }
        let mut buf = [0u8; 96];
        let mut n = 0;
        for &b in b"IVRS: entry type=" { buf[n] = b; n += 1; }
        n += u32_to_dec(entry_type, &mut buf[n..]);
        for &b in b" len=" { buf[n] = b; n += 1; }
        n += u32_to_dec(len as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        writer(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        off += len;
    }
}


