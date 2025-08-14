#![allow(dead_code)]

//! Intel VT-d (DMA Remapping) discovery and minimal initialization scaffolding.

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use crate::util::spinlock::SpinLock;
use core::fmt::Write as _;

// --- VT-d register offsets (subset) ---
const REG_VER: usize = 0x000;    // Version (R)
const REG_CAP: usize = 0x008;    // Capability (R)
const REG_ECAP: usize = 0x010;   // Extended Capability (R)
const REG_GCMD: usize = 0x018;   // Global Command (R/W)
const REG_GSTS: usize = 0x01C;   // Global Status (R)
const REG_RTADDR: usize = 0x020; // Root Table Address (64-bit) (R/W)
// const REG_CCMD: usize = 0x028; // Context Command (R/W)
const REG_FSTS: usize = 0x034;   // Fault Status (R/WC)

// GCMD bits (subset)
const GCMD_SRTP: u32 = 1 << 30; // Set Root Table Pointer
const GCMD_TE: u32 = 1 << 31;   // Translation Enable

// GSTS bits (subset)
const GSTS_RTPS: u32 = 1 << 30; // Root Table Pointer Status
const GSTS_TES: u32 = 1 << 31;  // Translation Enable Status

#[repr(C, packed)]
struct VtdRootEntry {
    lower: u64, // bit0: present, 63:12 context table pointer
    upper: u64,
}

#[repr(C, packed)]
struct VtdContextEntry {
    lower: u64,
    upper: u64,
}

#[derive(Clone, Copy)]
struct VtdUnit {
    seg: u16,
    reg_base: u64,
    root_tbl: u64,
}

static VTD_UNITS: SpinLock<[Option<VtdUnit>; 8]> = SpinLock::new([None, None, None, None, None, None, None, None]);

// Domain -> Second-level page table root (PML4) physical pointer holder
// Each domain uses a dedicated root page. This is a zero-initialized page for now.
static DOMAIN_SLPTPTR: SpinLock<[Option<u64>; 16]> = SpinLock::new([None; 16]);

fn register_unit(seg: u16, reg_base: u64, root_tbl: u64) {
    VTD_UNITS.lock(|arr| {
        for slot in arr.iter_mut() {
            if slot.is_none() { *slot = Some(VtdUnit { seg, reg_base, root_tbl }); break; }
        }
    });
}

fn for_each_unit(mut f: impl FnMut(VtdUnit)) {
    VTD_UNITS.lock(|arr| { for slot in arr.iter() { if let Some(u) = slot.as_ref() { f(*u); } } });
}

fn get_unit_by_index(index: usize) -> Option<VtdUnit> {
    let mut out: Option<VtdUnit> = None;
    VTD_UNITS.lock(|arr| {
        let mut i = 0usize;
        for slot in arr.iter() {
            if let Some(u) = slot.as_ref() {
                if i == index { out = Some(*u); break; }
                i += 1;
            }
        }
    });
    out
}

fn ensure_domain_slptptr(system_table: &SystemTable<Boot>, domid: u16) -> Option<u64> {
    // We only provision up to 16 domains in this early bootstrap path.
    let idx = (domid as usize) & 0xF;
    let mut ret: Option<u64> = None;
    DOMAIN_SLPTPTR.lock(|arr| {
        if arr[idx].is_none() {
            // Build an identity-mapped 2MiB page table up to 1GiB for early bootstrap DMA
            if let Some(cr3) = crate::mm::paging::build_identity_2m(system_table, 1u64 << 30) {
                arr[idx] = Some((cr3 as u64) & 0xFFFF_FFFF_FFFF_F000u64);
            }
        }
        if let Some(p) = arr[idx] { ret = Some(p); }
    });
    ret
}

fn get_domain_slptptr(domid: u16) -> Option<u64> {
    let mut out = None;
    DOMAIN_SLPTPTR.lock(|arr| { out = arr[(domid as usize) & 0xF]; });
    out
}

// --- Second-level page table helpers (IA-32e like) ---
const PTE_P: u64 = 1 << 0;
const PTE_RW: u64 = 1 << 1;
const PTE_PS: u64 = 1 << 7; // large (for PDE -> 2MiB)
const PTE_NX: u64 = 1u64 << 63;

unsafe fn ensure_table_entry(table: *mut u64, idx: usize, system_table: &SystemTable<Boot>) -> *mut u64 {
    let e = table.add(idx);
    let val = core::ptr::read_volatile(e);
    if (val & PTE_P) == 0 {
        if let Some(p) = alloc_zeroed_pages(system_table, 1) {
            let phys = (p as u64) & 0xFFFF_FFFF_FFFF_F000u64;
            core::ptr::write_volatile(e, phys | PTE_P | PTE_RW);
        }
    }
    let newv = core::ptr::read_volatile(e) & 0xFFFF_FFFF_FFFF_F000u64;
    newv as *mut u64
}

fn map_range_4k(system_table: &SystemTable<Boot>, cr3_phys: u64, iova: u64, pa: u64, len: u64, _r: bool, w: bool, x: bool) {
    if cr3_phys == 0 || len == 0 { return; }
    let mut off = 0u64;
    while off < len {
        let gpa = iova.wrapping_add(off);
        let hpa = pa.wrapping_add(off);
        unsafe {
            let pml4 = cr3_phys as *mut u64;
            let i4 = ((gpa >> 39) & 0x1FF) as usize;
            let i3 = ((gpa >> 30) & 0x1FF) as usize;
            let i2 = ((gpa >> 21) & 0x1FF) as usize;
            let i1 = ((gpa >> 12) & 0x1FF) as usize;
            let pdpt = ensure_table_entry(pml4, i4, system_table);
            let pd = ensure_table_entry(pdpt, i3, system_table);
            let pt = ensure_table_entry(pd, i2, system_table);
            let pte = pt.add(i1);
            let mut flags = PTE_P;
            if w { flags |= PTE_RW; }
            if !x { flags |= PTE_NX; }
            core::ptr::write_volatile(pte, (hpa & 0xFFFF_FFFF_FFFF_F000u64) | flags);
        }
        off = off.wrapping_add(4096);
    }
}

fn map_range_2m(system_table: &SystemTable<Boot>, cr3_phys: u64, iova: u64, pa: u64, len: u64, _r: bool, w: bool, x: bool) {
    if cr3_phys == 0 || len == 0 { return; }
    let mut off = 0u64;
    while off < len {
        let gpa = iova.wrapping_add(off);
        let hpa = pa.wrapping_add(off);
        unsafe {
            let pml4 = cr3_phys as *mut u64;
            let i4 = ((gpa >> 39) & 0x1FF) as usize;
            let i3 = ((gpa >> 30) & 0x1FF) as usize;
            let i2 = ((gpa >> 21) & 0x1FF) as usize;
            let pdpt = ensure_table_entry(pml4, i4, system_table);
            let pd = ensure_table_entry(pdpt, i3, system_table);
            let pde = pd.add(i2);
            let mut flags = PTE_P | PTE_PS; // 2MiB page
            if w { flags |= PTE_RW; }
            if !x { flags |= PTE_NX; }
            // For 2MiB large page, bits [20:0] are ignored; program [51:21]
            core::ptr::write_volatile(pde, (hpa & 0xFFFF_FFFF_FFE0_0000u64) | flags);
        }
        off = off.wrapping_add(2 * 1024 * 1024);
    }
}

fn unmap_range_4k(_system_table: &SystemTable<Boot>, cr3_phys: u64, iova: u64, len: u64) {
    if cr3_phys == 0 || len == 0 { return; }
    let mut off = 0u64;
    while off < len {
        let gpa = iova.wrapping_add(off);
        unsafe {
            let pml4 = cr3_phys as *mut u64;
            let i4 = ((gpa >> 39) & 0x1FF) as usize;
            let i3 = ((gpa >> 30) & 0x1FF) as usize;
            let i2 = ((gpa >> 21) & 0x1FF) as usize;
            let i1 = ((gpa >> 12) & 0x1FF) as usize;
            let e4 = pml4.add(i4); let v4 = core::ptr::read_volatile(e4);
            if (v4 & PTE_P) == 0 { off = off.wrapping_add(4096); continue; }
            let pdpt = (v4 & 0xFFFF_FFFF_FFFF_F000u64) as *mut u64;
            let e3 = pdpt.add(i3); let v3 = core::ptr::read_volatile(e3);
            if (v3 & PTE_P) == 0 { off = off.wrapping_add(4096); continue; }
            let pd = (v3 & 0xFFFF_FFFF_FFFF_F000u64) as *mut u64;
            let e2 = pd.add(i2); let v2 = core::ptr::read_volatile(e2);
            if (v2 & PTE_P) == 0 || (v2 & PTE_PS) != 0 { off = off.wrapping_add(4096); continue; }
            let pt = (v2 & 0xFFFF_FFFF_FFFF_F000u64) as *mut u64;
            let pte = pt.add(i1);
            core::ptr::write_volatile(pte, 0u64);
        }
        off = off.wrapping_add(4096);
    }
}

pub fn apply_mappings(system_table: &mut SystemTable<Boot>) {
    crate::iommu::state::list_mappings(|dom,iova,pa,len,r,w,x| {
        if let Some(cr3) = ensure_domain_slptptr(system_table, dom) {
            // If aligned for 2MiB and length multiple of 2MiB, use 2MiB large pages for efficiency
            if (iova | pa | len) & ((2 * 1024 * 1024) - 1) == 0 {
                map_range_2m(system_table, cr3, iova, pa, len, r, w, x);
            } else {
                map_range_4k(system_table, cr3, iova, pa, len, r, w, x);
            }
        }
    });
    let _ = system_table.stdout().write_str("iommu: second-level mappings applied\r\n");
    // Emit trace for mapping activity per domain (summary only)
    crate::obs::trace::emit(crate::obs::trace::Event::IommuMapAdded(0));
    // If translation is enabled, refresh caches conservatively
    maybe_refresh_after_updates(system_table);
}

pub fn unmap_range(system_table: &mut SystemTable<Boot>, dom: u16, iova: u64, len: u64) {
    if let Some(cr3) = get_domain_slptptr(dom) {
        unmap_range_4k(system_table, cr3, iova, len);
        let _ = system_table.stdout().write_str("iommu: unmapped from second-level tables\r\n");
    }
    maybe_refresh_after_updates(system_table);
    crate::obs::trace::emit(crate::obs::trace::Event::IommuMapRemoved(dom));
}

fn maybe_refresh_after_updates(system_table: &mut SystemTable<Boot>) {
    let mut needs = false;
    for_each_unit(|u| unsafe {
        let gsts = (u.reg_base as usize + REG_GSTS) as *const u32;
        if (core::ptr::read_volatile(gsts) & GSTS_TES) != 0 { needs = true; }
    });
    if needs { invalidate_all(system_table); }
}

fn get_cr3_for_bdf(system_table: &mut SystemTable<Boot>, seg: u16, bus: u8, dev: u8, func: u8) -> Option<u64> {
    unsafe {
        if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
            let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
            let root_ptr = u.root_tbl as *const VtdRootEntry;
            let re = root_ptr.add(ri);
            let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
            if (re_lo & CTX_PRESENT) == 0 { return None; }
            let ctx_ptr = (re_lo & 0xFFFF_FFFF_FFFF_F000u64) as *const VtdContextEntry;
            let ce = ctx_ptr.add(ci);
            let ce_lo = core::ptr::read_volatile(core::ptr::addr_of!((*ce).lower));
            if (ce_lo & CTX_PRESENT) == 0 { return None; }
            return Some(ce_lo & CTX_LO_PTR_MASK);
        }
    }
    None
}

fn walk_second_level(cr3: u64, iova: u64) -> (Option<u64>, [u64; 4]) {
    // Return (PA, entries[4]) where entries are raw PML4E, PDPTE, PDE, PTE (0 if not present)
    let mut ents = [0u64; 4];
    if cr3 == 0 { return (None, ents); }
    unsafe {
        let pml4 = cr3 as *const u64;
        let i4 = ((iova >> 39) & 0x1FF) as usize;
        let e4 = pml4.add(i4); let v4 = core::ptr::read_volatile(e4); ents[0] = v4;
        if (v4 & PTE_P) == 0 { return (None, ents); }
        let pdpt = (v4 & 0xFFFF_FFFF_FFFF_F000u64) as *const u64;
        let i3 = ((iova >> 30) & 0x1FF) as usize;
        let e3 = pdpt.add(i3); let v3 = core::ptr::read_volatile(e3); ents[1] = v3;
        if (v3 & PTE_P) == 0 { return (None, ents); }
        let pd = (v3 & 0xFFFF_FFFF_FFFF_F000u64) as *const u64;
        let i2 = ((iova >> 21) & 0x1FF) as usize;
        let e2 = pd.add(i2); let v2 = core::ptr::read_volatile(e2); ents[2] = v2;
        if (v2 & PTE_P) == 0 { return (None, ents); }
        if (v2 & PTE_PS) != 0 {
            let base = v2 & 0xFFFF_FFFF_FFE0_0000u64;
            let offs = iova & 0x1F_FFFFu64;
            return (Some(base | offs), ents);
        }
        let pt = (v2 & 0xFFFF_FFFF_FFFF_F000u64) as *const u64;
        let i1 = ((iova >> 12) & 0x1FF) as usize;
        let e1 = pt.add(i1); let v1 = core::ptr::read_volatile(e1); ents[3] = v1;
        if (v1 & PTE_P) == 0 { return (None, ents); }
        let pa = (v1 & 0xFFFF_FFFF_FFFF_F000u64) | (iova & 0xFFF);
        (Some(pa), ents)
    }
}

pub fn translate_bdf_iova(system_table: &mut SystemTable<Boot>, seg: u16, bus: u8, dev: u8, func: u8, iova: u64) {
    if let Some(cr3) = get_cr3_for_bdf(system_table, seg, bus, dev, func) {
        let (pa, _) = walk_second_level(cr3, iova);
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"xlate: iova=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(iova, &mut buf[n..]);
        for &b in b" -> pa=" { buf[n] = b; n += 1; }
        if let Some(pa) = pa { n += u64_to_hex(pa, &mut buf[n..]); } else { for &b in b"<none>" { buf[n] = b; n += 1; } }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    } else {
        // Try domain-level CR3 if BDF is not yet present in context
        if let Some(dom) = crate::iommu::state::find_domain_for_bdf(seg, bus, dev, func) {
            if let Some(cr3) = get_domain_slptptr(dom) {
                let (pa, _) = walk_second_level(cr3, iova);
                let mut buf = [0u8; 96]; let mut n = 0;
                for &b in b"xlate(dom): iova=0x" { buf[n] = b; n += 1; }
                n += u64_to_hex(iova, &mut buf[n..]);
                for &b in b" -> pa=" { buf[n] = b; n += 1; }
                if let Some(pa) = pa { n += u64_to_hex(pa, &mut buf[n..]); } else { for &b in b"<none>" { buf[n] = b; n += 1; } }
                buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                return;
            }
        }
        let _ = system_table.stdout().write_str("xlate: no cr3\r\n");
    }
}

pub fn walk_bdf_iova(system_table: &mut SystemTable<Boot>, seg: u16, bus: u8, dev: u8, func: u8, iova: u64) {
    if let Some(cr3) = get_cr3_for_bdf(system_table, seg, bus, dev, func) {
        let (pa, ents) = walk_second_level(cr3, iova);
        let mut buf = [0u8; 192]; let mut n = 0;
        for &b in b"walk: pml4e=" { buf[n] = b; n += 1; }
        n += u64_to_hex(ents[0], &mut buf[n..]);
        for &b in b" pdpte=" { buf[n] = b; n += 1; }
        n += u64_to_hex(ents[1], &mut buf[n..]);
        for &b in b" pde=" { buf[n] = b; n += 1; }
        n += u64_to_hex(ents[2], &mut buf[n..]);
        for &b in b" pte=" { buf[n] = b; n += 1; }
        n += u64_to_hex(ents[3], &mut buf[n..]);
        for &b in b" -> pa=" { buf[n] = b; n += 1; }
        if let Some(pa) = pa { n += u64_to_hex(pa, &mut buf[n..]); } else { for &b in b"<none>" { buf[n] = b; n += 1; } }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    } else {
        let _ = system_table.stdout().write_str("walk: no cr3\r\n");
    }
}

/// List registered VT-d units with segment, MMIO base and root table address.
pub fn list_units(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| {
        let mut buf = [0u8; 128]; let mut n = 0;
        for &b in b"VT-d: unit seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" reg=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(u.reg_base, &mut buf[n..]);
        for &b in b" root=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(u.root_tbl as u64, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout();
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Dump a single root/context slot from our in-memory tables (no HW reads).
pub fn dump_context(system_table: &mut SystemTable<Boot>, bus: u8, dev: u8, func: u8) {
    let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
    for_each_unit(|u| unsafe {
        let root_ptr = u.root_tbl as *mut VtdRootEntry;
        let re = root_ptr.add(ri);
        let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
        let _re_up = core::ptr::read_volatile(core::ptr::addr_of!((*re).upper));
        let ctx_ptr = re_lo & 0xFFFF_FFFF_FFFF_F000u64;
        let present = (re_lo & 1) != 0;
        let mut buf = [0u8; 192]; let mut n = 0;
        for &b in b"VT-d: dump seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" bus=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b" dev=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
        for &b in b" fn=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
        for &b in b" re.present=" { buf[n] = b; n += 1; }
        buf[n] = if present { b'1' } else { b'0' }; n += 1;
        for &b in b" ctx=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(ctx_ptr, &mut buf[n..]);
        // If present, peek context entry lower/upper
        if present && ctx_ptr != 0 {
            let ct = ctx_ptr as *const VtdContextEntry;
            let ce = ct.add(ci);
            let ce_lo = core::ptr::read_volatile(core::ptr::addr_of!((*ce).lower));
            let ce_up = core::ptr::read_volatile(core::ptr::addr_of!((*ce).upper));
            for &b in b" ce.lo=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(ce_lo, &mut buf[n..]);
            for &b in b" ce.hi=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(ce_up, &mut buf[n..]);
            // Decode fields (raw)
            for &b in b" present=" { buf[n] = b; n += 1; }
            buf[n] = if (ce_lo & CTX_PRESENT) != 0 { b'1' } else { b'0' }; n += 1;
            for &b in b" fpd=" { buf[n] = b; n += 1; }
            buf[n] = if (ce_lo & CTX_FPD) != 0 { b'1' } else { b'0' }; n += 1;
            for &b in b" tt=" { buf[n] = b; n += 1; }
            let tt = ((ce_lo >> CTX_TT_SHIFT) & 0x3) as u32; n += crate::firmware::acpi::u32_to_dec(tt, &mut buf[n..]);
            for &b in b" aw=" { buf[n] = b; n += 1; }
            let aw = ((ce_up >> CTXU_AW_SHIFT) & 0x7) as u32; n += crate::firmware::acpi::u32_to_dec(aw, &mut buf[n..]);
            for &b in b" did=" { buf[n] = b; n += 1; }
            let did = ((ce_up >> CTXU_DID_SHIFT) & 0xFFFF) as u32; n += crate::firmware::acpi::u32_to_dec(did, &mut buf[n..]);
        }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout();
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Dump a root entry for a given bus number across all units
pub fn dump_root(system_table: &mut SystemTable<Boot>, bus: u8) {
    let ri = bus as usize;
    for_each_unit(|u| unsafe {
        let root_ptr = u.root_tbl as *const VtdRootEntry;
        let re = root_ptr.add(ri);
        let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
        let re_up = core::ptr::read_volatile(core::ptr::addr_of!((*re).upper));
        let present = (re_lo & 1) != 0;
        let ctx_ptr = re_lo & 0xFFFF_FFFF_FFFF_F000u64;
        let mut buf = [0u8; 192]; let mut n = 0;
        for &b in b"VT-d: root seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" bus=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b" present=" { buf[n] = b; n += 1; }
        buf[n] = if present { b'1' } else { b'0' }; n += 1;
        for &b in b" ctx=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(ctx_ptr, &mut buf[n..]);
        for &b in b" re.up=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(re_up, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout();
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// List all present context entries on a given bus (dev.func) across units
pub fn list_bus_contexts(system_table: &mut SystemTable<Boot>, bus: u8) {
    let ri = bus as usize;
    for_each_unit(|u| unsafe {
        let root_ptr = u.root_tbl as *const VtdRootEntry;
        let re = root_ptr.add(ri);
        let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
        let ctx_ptr = re_lo & 0xFFFF_FFFF_FFFF_F000u64;
        if (re_lo & CTX_PRESENT) == 0 || ctx_ptr == 0 { return; }
        let ct = ctx_ptr as *const VtdContextEntry;
        let mut found = 0u32;
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let ci = ((dev as usize) << 3) | (func as usize);
                let ce = ct.add(ci);
                let ce_lo = core::ptr::read_volatile(core::ptr::addr_of!((*ce).lower));
                if (ce_lo & CTX_PRESENT) != 0 {
                    found = found.saturating_add(1);
                    let mut buf = [0u8; 96]; let mut n = 0;
                    for &b in b"VT-d: ctx seg=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
                    for &b in b" bus=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                    for &b in b" dev=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                    for &b in b" fn=" { buf[n] = b; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                    let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                }
            }
        }
        let mut buf = [0u8; 64]; let mut n = 0;
        for &b in b"VT-d: total present on bus=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b" cnt=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(found, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Validate current domain assignments against DMAR device scopes.
/// Prints any missing devices that are not covered by any DRHD scope.
pub fn validate_assignments(system_table: &mut SystemTable<Boot>) {
    // Collect all scoped devices into a small bitmap/set in a fixed array
    let mut scoped: [(u16,u8,u8,u8); 256] = [(0,0,0,0); 256];
    let mut cnt: usize = 0;
    if let Some(dmar) = crate::firmware::acpi::find_dmar(system_table) {
        crate::firmware::acpi::dmar_for_each_device_scope_from(|seg, _reg, bus, dev, func| {
            if cnt < scoped.len() { scoped[cnt] = (seg,bus,dev,func); cnt += 1; }
        }, dmar);
    }
    let stdout = system_table.stdout();
    let mut all_ok = true;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,_dom| {
        let mut found = false;
        for i in 0..cnt { let (s,b,d,f) = scoped[i]; if s==seg && b==bus && d==dev && f==func { found = true; break; } }
        if !found { all_ok = false; let _ = stdout.write_str("validate: missing in DMAR scope\r\n"); }
    });
    if all_ok { let _ = stdout.write_str("validate: OK\r\n"); }
}

// --- Context entry bit fields (based on widely used VT-d layout summaries) ---
// Lower 64-bit word fields
const CTX_PRESENT: u64 = 1 << 0;        // Present
const CTX_FPD: u64 = 1 << 1;            // Fault Processing Disable
const CTX_TT_SHIFT: u64 = 2;            // Translation Type (2 bits)
const CTX_TT_MULTI_LEVEL: u64 = 0x1;    // 01b = second-level paging
// SLPTPTR occupies lower[63:12]
const CTX_LO_PTR_MASK: u64 = 0xFFFF_FFFF_FFF0_000u64; // 4KiB aligned pointer mask in lower

// Upper 64-bit word fields
const CTXU_AW_SHIFT: u64 = 0;           // Address Width (bits 2:0 of upper)
const CTXU_DID_SHIFT: u64 = 8;          // Domain ID (bits 23:8 of upper)

fn find_unit_for_bdf(system_table: &mut SystemTable<Boot>, seg: u16, bus: u8, dev: u8, func: u8) -> Option<VtdUnit> {
    // Locate DRHD (reg_base) that owns this BDF via DMAR scopes, then find registered unit
    let mut match_reg: Option<u64> = None;
    if let Some(dmar) = crate::firmware::acpi::find_dmar(system_table) {
        crate::firmware::acpi::dmar_for_each_device_scope_from(|s, reg, b, d, f| {
            if match_reg.is_none() && s == seg && b == bus && d == dev && f == func { match_reg = Some(reg); }
        }, dmar);
    }
    let reg = match_reg.unwrap_or(0);
    let mut chosen: Option<VtdUnit> = None;
    for_each_unit(|u| { if chosen.is_none() { if u.seg == seg && (reg == 0 || u.reg_base == reg) { chosen = Some(u); } } });
    chosen
}

/// Apply domain assignments into in-memory context tables (no TE, no HW invalidates yet).
pub fn apply_assignments(system_table: &mut SystemTable<Boot>) {
    crate::iommu::state::list_assignments(|seg,bus,dev,func,domid| unsafe {
        if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
            let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
            let root_ptr = u.root_tbl as *mut VtdRootEntry;
            let re = root_ptr.add(ri);
            let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
            if (re_lo & CTX_PRESENT) == 0 || (re_lo & 0xFFFF_FFFF_FFFF_F000u64) == 0 { return; }
            let ctx_ptr = (re_lo & 0xFFFF_FFFF_FFFF_F000u64) as *mut VtdContextEntry;
            let ce = ctx_ptr.add(ci);
            // Compose context entry according to common VT-d layout references:
            // - lower: present, fpd (0), tt, slptptr (63:12)
            // - upper: aw (2:0), did (23:8)
            let tt = (CTX_TT_MULTI_LEVEL) << CTX_TT_SHIFT;
            let slpt = if let Some(p) = ensure_domain_slptptr(system_table, domid) { p & CTX_LO_PTR_MASK } else { 0 };
            let lo = CTX_PRESENT | tt | slpt;
            let did = ((domid as u64) & 0xFFFF) << CTXU_DID_SHIFT;
            let aw = 2u64 << CTXU_AW_SHIFT; // 48-bit
            let hi = aw | did;
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*ce).lower), lo);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*ce).upper), hi);
        }
    });
    let stdout = system_table.stdout();
    let _ = stdout.write_str("apply: context entries updated (in-memory, SLPTPTR provisioned)\r\n");
}

/// Convenience: apply assignments and perform a conservative refresh of VT-d caches.
pub fn apply_and_refresh(system_table: &mut SystemTable<Boot>) {
    apply_assignments(system_table);
    invalidate_all(system_table);
}

pub fn apply_safe(system_table: &mut SystemTable<Boot>) {
    // Disable translation on all units, apply assignments and second-level mappings, then re-enable
    disable_translation_all(system_table);
    apply_assignments(system_table);
    apply_mappings(system_table);
    enable_translation_all(system_table);
}

pub fn sync_contexts(system_table: &mut SystemTable<Boot>) {
    // Clear all context pages for each unit
    for_each_unit(|u| unsafe {
        for bus in 0u16..=255u16 {
            let root_ptr = u.root_tbl as *mut VtdRootEntry;
            let re = root_ptr.add(bus as usize);
            let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
            let present = (re_lo & CTX_PRESENT) != 0;
            let ctx_ptr = (re_lo & 0xFFFF_FFFF_FFFF_F000u64) as *mut u8;
            if present && !ctx_ptr.is_null() { core::ptr::write_bytes(ctx_ptr, 0, 4096); }
        }
    });
    // Re-apply assignments to contexts
    crate::iommu::state::list_assignments(|seg,bus,dev,func,domid| unsafe {
        if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
            let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
            let root_ptr = u.root_tbl as *mut VtdRootEntry;
            let re = root_ptr.add(ri);
            let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
            if (re_lo & CTX_PRESENT) == 0 || (re_lo & 0xFFFF_FFFF_FFFF_F000u64) == 0 { return; }
            let ctx_ptr = (re_lo & 0xFFFF_FFFF_FFFF_F000u64) as *mut VtdContextEntry;
            let ce = ctx_ptr.add(ci);
            let tt = (CTX_TT_MULTI_LEVEL) << CTX_TT_SHIFT;
            let slpt = if let Some(p) = ensure_domain_slptptr(system_table, domid) { p & CTX_LO_PTR_MASK } else { 0 };
            let lo = CTX_PRESENT | tt | slpt;
            let did = ((domid as u64) & 0xFFFF) << CTXU_DID_SHIFT;
            let aw = 2u64 << CTXU_AW_SHIFT; // 48-bit
            let hi = aw | did;
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*ce).lower), lo);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*ce).upper), hi);
        }
    });
    invalidate_all(system_table);
    let _ = system_table.stdout().write_str("iommu: contexts synchronized from assignments\r\n");
}

/// Stub for global invalidates (context/iotlb). Currently prints a message only.
pub fn invalidate_all(system_table: &mut SystemTable<Boot>) {
    // Re-issue SRTP with current RTADDR per unit to conservatively force hardware to
    // re-sample the root pointer and refresh associated caches.
    for_each_unit(|u| unsafe {
        let rtaddr = (u.reg_base as usize + REG_RTADDR) as *mut u64;
        let gcmd = (u.reg_base as usize + REG_GCMD) as *mut u32;
        let gsts = (u.reg_base as usize + REG_GSTS) as *const u32;
        let cur_rt = core::ptr::read_volatile(rtaddr);
        core::ptr::write_volatile(rtaddr, cur_rt);
        let cur = core::ptr::read_volatile(gcmd);
        core::ptr::write_volatile(gcmd, cur | GCMD_SRTP);
        let mut ok = false; let mut tries = 0u32;
        while tries < 5000 { if (core::ptr::read_volatile(gsts) & GSTS_RTPS) != 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"VT-d: invalidate seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" result=" { buf[n] = b; n += 1; }
        let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
        for &b in s { buf[n] = b; n += 1; }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout();
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
    // Emit metrics and generic trace for all-units invalidate (segment not tracked per loop here)
    crate::obs::metrics::IOMMU_INV_ALL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::obs::trace::emit(crate::obs::trace::Event::IommuInvalidateAll(0));
}

/// Perform a hard global invalidate by toggling TE off and on per unit.
pub fn hard_invalidate_all(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| unsafe {
        let gcmd = (u.reg_base as usize + REG_GCMD) as *mut u32;
        let gsts = (u.reg_base as usize + REG_GSTS) as *const u32;
        // If TE is set, clear it
        let mut s = core::ptr::read_volatile(gsts);
        if (s & GSTS_TES) != 0 {
            let cur = core::ptr::read_volatile(gcmd);
            core::ptr::write_volatile(gcmd, cur & !GCMD_TE);
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 { s = core::ptr::read_volatile(gsts); if (s & GSTS_TES) == 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
            let mut buf = [0u8; 96]; let mut n = 0;
            for &b in b"VT-d: hard-inv off seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let t: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in t { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1; let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
        // Set TE
        let cur = core::ptr::read_volatile(gcmd);
        core::ptr::write_volatile(gcmd, cur | GCMD_TE);
        let mut ok = false; let mut tries = 0u32;
        while tries < 5000 { let s2 = core::ptr::read_volatile(gsts); if (s2 & GSTS_TES) != 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"VT-d: hard-inv on seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" result=" { buf[n] = b; n += 1; }
        let t: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
        for &b in t { buf[n] = b; n += 1; }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1; let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

fn srtp_one_unit(system_table: &mut SystemTable<Boot>, seg: u16, reg_base: u64) {
    unsafe {
        let rtaddr = (reg_base as usize + REG_RTADDR) as *mut u64;
        let gcmd = (reg_base as usize + REG_GCMD) as *mut u32;
        let gsts = (reg_base as usize + REG_GSTS) as *const u32;
        let cur_rt = core::ptr::read_volatile(rtaddr);
        core::ptr::write_volatile(rtaddr, cur_rt);
        let cur = core::ptr::read_volatile(gcmd);
        core::ptr::write_volatile(gcmd, cur | GCMD_SRTP);
        let mut ok = false; let mut tries = 0u32;
        while tries < 5000 { if (core::ptr::read_volatile(gsts) & GSTS_RTPS) != 0 { ok = true; break; } tries += 1; let _ = system_table.boot_services().stall(100); }
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"VT-d: SRTP seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
        for &b in b" " { buf[n] = b; n += 1; }
        n += u64_to_hex(reg_base, &mut buf[n..]);
        for &b in b" result=" { buf[n] = b; n += 1; }
        let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
        for &b in s { buf[n] = b; n += 1; }
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1; let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}

pub fn invalidate_domain(system_table: &mut SystemTable<Boot>, domid: u16) {
    // Targeted SRTP to units that host any BDF assigned to this domain
    let mut regs: [u64; 8] = [0; 8];
    let mut segs: [u16; 8] = [0; 8];
    let mut cnt: usize = 0;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
        if dom != domid { return; }
        // find unit for this bdf
        if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
            // dedup by reg_base
            let mut found = false;
            for i in 0..cnt { if regs[i] == u.reg_base { found = true; break; } }
            if !found && cnt < regs.len() { regs[cnt] = u.reg_base; segs[cnt] = u.seg; cnt += 1; }
        }
    });
    for i in 0..cnt { srtp_one_unit(system_table, segs[i], regs[i]); }
    crate::obs::metrics::IOMMU_INV_DOMAIN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::obs::trace::emit(crate::obs::trace::Event::IommuInvalidateDomain(domid));
}

pub fn invalidate_bdf(system_table: &mut SystemTable<Boot>, seg: u16, bus: u8, dev: u8, func: u8) {
    if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
        srtp_one_unit(system_table, u.seg, u.reg_base);
    }
    crate::obs::metrics::IOMMU_INV_BDF.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::obs::trace::emit(crate::obs::trace::Event::IommuInvalidateBdf(seg, bus, dev, func));
}

fn alloc_zeroed_pages(system_table: &uefi::table::SystemTable<Boot>, pages: usize) -> Option<*mut u8> {
    let p = crate::mm::uefi::alloc_pages(system_table, pages, uefi::table::boot::MemoryType::LOADER_DATA)?;
    unsafe { core::ptr::write_bytes(p, 0, pages * 4096); }
    Some(p)
}

fn u64_to_hex(v: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut started = false;
    let mut n = 0usize;
    for i in (0..16).rev() {
        let nyb = ((v >> (i * 4)) & 0xF) as usize;
        if nyb != 0 || started || i == 0 {
            started = true;
            if n < out.len() { out[n] = HEX[nyb]; n += 1; }
        }
    }
    n
}

/// Perform a minimal, non-intrusive VT-d setup on all DRHD units found via ACPI DMAR:
/// - Allocate an empty Root Table (4KiB, 256 entries) and program RTADDR
/// - Issue SRTP and wait for RTPS per unit
/// - Do NOT enable translation (TE)
pub fn minimal_init(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    if dmar.is_none() { return; }
    let dmar = dmar.unwrap();
    // Iterate DRHDs
    crate::firmware::acpi::dmar_for_each_drhd_from(|seg, reg_base| {
        unsafe {
            // If translation is already enabled by firmware, do not touch this unit
            let gsts_pre = (reg_base as usize + REG_GSTS) as *const u32;
            let s_pre = core::ptr::read_volatile(gsts_pre);
            if (s_pre & GSTS_TES) != 0 {
                // Avoid holding stdout across boot services use; emit inside a short scope
                let mut buf = [0u8; 128];
                let mut n = 0;
                for &b in b"VT-d: DRHD seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                for &b in b" reg=0x" { buf[n] = b; n += 1; }
                n += u64_to_hex(reg_base, &mut buf[n..]);
                for &b in b" skip: TE=1\r\n" { buf[n] = b; n += 1; }
                let stdout = system_table.stdout();
                let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                return;
            }
            // Allocate a dedicated root table for this DRHD and link 256 empty context tables
            let root_tbl = match alloc_zeroed_pages(system_table, 1) { Some(p) => p as *mut VtdRootEntry, None => return };
            for bus in 0u16..=255u16 {
                let ctx_page = match alloc_zeroed_pages(system_table, 1) { Some(p) => p as *mut VtdContextEntry, None => return };
                let re = root_tbl.add(bus as usize);
                // Set present=1 and context-table pointer (bits 63:12)
                (*re).lower = ((ctx_page as u64) & 0xFFFF_FFFF_FFFF_F000u64) | 1u64;
                (*re).upper = 0u64;
            }
            // Program RTADDR
            let rtaddr = (reg_base as usize + REG_RTADDR) as *mut u64;
            let rtaddr_val = (root_tbl as u64) & 0xFFFF_FFFF_FFFF_F000u64;
            core::ptr::write_volatile(rtaddr, rtaddr_val);
            // Set SRTP in GCMD
            let gcmd = (reg_base as usize + REG_GCMD) as *mut u32;
            let gsts = (reg_base as usize + REG_GSTS) as *const u32;
            let cur = core::ptr::read_volatile(gcmd);
            core::ptr::write_volatile(gcmd, cur | GCMD_SRTP);
            // Poll RTPS
            let mut ok = false;
            let mut tries = 0u32;
            while tries < 1000 {
                let s = core::ptr::read_volatile(gsts);
                if (s & GSTS_RTPS) != 0 { ok = true; break; }
                tries += 1;
                let _ = system_table.boot_services().stall(100);
            }
            // Register this unit for later operations
            register_unit(seg, reg_base, (root_tbl as u64) & 0xFFFF_FFFF_FFFF_F000u64);
            // Print status line without capturing stdout across closure lifetime
            let mut buf = [0u8; 128];
            let mut n = 0;
            for &b in b"VT-d: DRHD seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
            for &b in b" reg=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(reg_base, &mut buf[n..]);
            for &b in b" SRTP=" { buf[n] = b; n += 1; }
            let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in s { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let stdout = system_table.stdout();
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
    }, dmar);
}

/// Probe for ACPI DMAR table and print a short summary.
pub fn probe_and_report(system_table: &mut SystemTable<Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    // Resolve header before borrowing stdout to avoid aliasing borrows
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    let stdout = system_table.stdout();
    if let Some(hdr) = dmar {
        crate::firmware::acpi::dmar_summary(|s| { let _ = stdout.write_str(s); }, hdr);
        crate::firmware::acpi::dmar_list_structs_from(|s| { let _ = stdout.write_str(s); }, hdr);
        minimal_init(system_table);
    } else {
        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::IOMMU_VTD_NONE));
    }
}

/// Report detailed VT-d DRHD MMIO register snapshot (safe subset):
/// - Version, CAP/ECAP, GSTS, RTADDR
pub fn report_details(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    if dmar.is_none() { return; }
    let dmar = dmar.unwrap();
    crate::firmware::acpi::dmar_for_each_drhd_from(|seg, reg_base| {
        unsafe {
            let ver = core::ptr::read_volatile((reg_base as usize + REG_VER) as *const u32) as u64;
            let cap = core::ptr::read_volatile((reg_base as usize + REG_CAP) as *const u64);
            let ecap = core::ptr::read_volatile((reg_base as usize + REG_ECAP) as *const u64);
            let gsts = core::ptr::read_volatile((reg_base as usize + REG_GSTS) as *const u32) as u64;
            let rtaddr = core::ptr::read_volatile((reg_base as usize + REG_RTADDR) as *const u64);
            let mut buf = [0u8; 192];
            let mut n = 0;
            for &b in b"VT-d: DRHD seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
            for &b in b" ver=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(ver, &mut buf[n..]);
            for &b in b" cap=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(cap, &mut buf[n..]);
            for &b in b" ecap=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(ecap, &mut buf[n..]);
            for &b in b" gsts=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(gsts, &mut buf[n..]);
            for &b in b" rtaddr=0x" { buf[n] = b; n += 1; }
            n += u64_to_hex(rtaddr, &mut buf[n..]);
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let stdout = system_table.stdout();
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
    }, dmar);
}

/// Report raw Fault Status (FSTS) per unit (hex). Write-clear requires a separate call.
pub fn report_faults(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| unsafe {
        let fsts = core::ptr::read_volatile((u.reg_base as usize + REG_FSTS) as *const u32) as u64;
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"VT-d: FSTS seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" fsts=0x" { buf[n] = b; n += 1; }
        n += u64_to_hex(fsts, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Clear Fault Status by write-1-to-clear semantics (write back read value).
pub fn clear_faults(system_table: &mut SystemTable<Boot>) {
    for_each_unit(|u| unsafe {
        let reg = (u.reg_base as usize + REG_FSTS) as *mut u32;
        let val = core::ptr::read_volatile(reg);
        core::ptr::write_volatile(reg, val);
        let mut buf = [0u8; 64]; let mut n = 0;
        for &b in b"VT-d: FSTS cleared seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

pub fn set_te_for_unit(system_table: &mut SystemTable<Boot>, index: usize, enable: bool) {
    if let Some(u) = get_unit_by_index(index) {
        unsafe {
            let gcmd = (u.reg_base as usize + REG_GCMD) as *mut u32;
            let gsts = (u.reg_base as usize + REG_GSTS) as *const u32;
            let cur = core::ptr::read_volatile(gcmd);
            if enable { core::ptr::write_volatile(gcmd, cur | GCMD_TE); } else { core::ptr::write_volatile(gcmd, cur & !GCMD_TE); }
            let _want = if enable { GSTS_TES } else { 0 };
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 {
                let s = core::ptr::read_volatile(gsts);
                if ((s & GSTS_TES) != 0) == enable { ok = true; break; }
                tries += 1; let _ = system_table.boot_services().stall(100);
            }
            let mut buf = [0u8; 96]; let mut n = 0;
            let prefix: &[u8] = if enable { b"VT-d: TE on idx=" } else { b"VT-d: TE off idx=" };
            for &b in prefix { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(index as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let t: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in t { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1; let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
    }
}

/// Enable translation (TE) on all DRHD units that are currently disabled.
/// Assumes RTADDR has been programmed. Polls TES and prints per-DRHD status.
pub fn enable_translation_all(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    if dmar.is_none() { return; }
    let dmar = dmar.unwrap();
    crate::firmware::acpi::dmar_for_each_drhd_from(|seg, reg_base| {
        unsafe {
            let gsts = (reg_base as usize + REG_GSTS) as *const u32;
            let gcmd = (reg_base as usize + REG_GCMD) as *mut u32;
            // Skip if already enabled
            if (core::ptr::read_volatile(gsts) & GSTS_TES) != 0 {
                let mut buf = [0u8; 96]; let mut n = 0;
                for &b in b"VT-d: DRHD seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                for &b in b" TE=1 (skip)\r\n" { buf[n] = b; n += 1; }
                let stdout = system_table.stdout();
                let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                return;
            }
            // Set TE
            let cur = core::ptr::read_volatile(gcmd);
            core::ptr::write_volatile(gcmd, cur | GCMD_TE);
            // Poll TES
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 {
                if (core::ptr::read_volatile(gsts) & GSTS_TES) != 0 { ok = true; break; }
                tries += 1; let _ = system_table.boot_services().stall(100);
            }
            let mut buf = [0u8; 96]; let mut n = 0;
            for &b in b"VT-d: enable seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in s { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let stdout = system_table.stdout();
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
    }, dmar);
}

/// Disable translation (TE) on all DRHD units that are currently enabled.
pub fn disable_translation_all(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    if dmar.is_none() { return; }
    let dmar = dmar.unwrap();
    crate::firmware::acpi::dmar_for_each_drhd_from(|seg, reg_base| {
        unsafe {
            let gsts = (reg_base as usize + REG_GSTS) as *const u32;
            let gcmd = (reg_base as usize + REG_GCMD) as *mut u32;
            // Skip if already disabled
            if (core::ptr::read_volatile(gsts) & GSTS_TES) == 0 {
                let mut buf = [0u8; 96]; let mut n = 0;
                for &b in b"VT-d: DRHD seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                for &b in b" TE=0 (skip)\r\n" { buf[n] = b; n += 1; }
                let stdout = system_table.stdout();
                let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
                return;
            }
            // Clear TE
            let cur = core::ptr::read_volatile(gcmd);
            core::ptr::write_volatile(gcmd, cur & !GCMD_TE);
            // Poll TES clear
            let mut ok = false; let mut tries = 0u32;
            while tries < 5000 {
                if (core::ptr::read_volatile(gsts) & GSTS_TES) == 0 { ok = true; break; }
                tries += 1; let _ = system_table.boot_services().stall(100);
            }
            let mut buf = [0u8; 96]; let mut n = 0;
            for &b in b"VT-d: disable seg=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
            for &b in b" result=" { buf[n] = b; n += 1; }
            let s: &[u8] = if ok { b"OK" } else { b"TIMEOUT" };
            for &b in s { buf[n] = b; n += 1; }
            buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let stdout = system_table.stdout();
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        }
    }, dmar);
}

/// Dump DMAR device scopes as BDF lines with owning DRHD.
pub fn dump_device_scopes(system_table: &mut SystemTable<Boot>) {
    let dmar = crate::firmware::acpi::find_dmar(system_table);
    if dmar.is_none() { return; }
    let dmar = dmar.unwrap();
    crate::firmware::acpi::dmar_for_each_device_scope_from(|seg, _reg, bus, dev, func| {
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"DMAR: scope " { buf[n] = b; n += 1; }
        // seg:bus:dev.func (hex)
        for &b in b"0000:" { buf[n] = b; n += 1; } // prefix; we will overwrite digits below
        // print seg as decimal to reuse helper, acceptable for now
        n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
        for &b in b":" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b":" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
        for &b in b"." { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout();
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }, dmar);
}

#[inline(always)]
fn vtd_indices_from_bdf(bus: u8, dev: u8, func: u8) -> (usize, usize) {
    let root_index = bus as usize;           // 0..=255
    let ctx_index = ((dev as usize) << 3) | (func as usize); // 0..=255 (dev0..31, func0..7)
    (root_index, ctx_index)
}

/// Print a plan for programming Root/Context entries derived from current domain assignments.
/// This does not touch hardware; it only reports root/context indices and domain ids.
pub fn plan_assignments(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let _ = stdout.write_str("VT-d plan:\r\n");
    crate::iommu::state::list_assignments(|seg, bus, dev, func, domid| {
        let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
        let mut buf = [0u8; 128]; let mut n = 0;
        for &b in b"  seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
        for &b in b" bus=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b" dev=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
        for &b in b" fn=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
        for &b in b" => root[" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(ri as u32, &mut buf[n..]);
        for &b in b"], ctx[" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(ci as u32, &mut buf[n..]);
        for &b in b"], dom=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(domid as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Print a plan for a specific domain id
pub fn plan_assignments_for_domain(system_table: &mut SystemTable<Boot>, domid_filter: u16) {
    let stdout = system_table.stdout();
    let _ = stdout.write_str("VT-d plan (domain):\r\n");
    crate::iommu::state::list_assignments(|seg, bus, dev, func, domid| {
        if domid != domid_filter { return; }
        let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
        let mut buf = [0u8; 128]; let mut n = 0;
        for &b in b"  seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
        for &b in b" bus=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
        for &b in b" dev=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
        for &b in b" fn=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
        for &b in b" => root[" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(ri as u32, &mut buf[n..]);
        for &b in b"], ctx[" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(ci as u32, &mut buf[n..]);
        for &b in b"], dom=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(domid as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
}

/// Print a short summary: number of VT-d units, TE status per unit, and domain/assignment/mapping counts.
pub fn report_summary(system_table: &mut SystemTable<Boot>) {
    // Count units and report TE state per unit
    let mut unit_count = 0u32;
    for_each_unit(|u| unsafe {
        unit_count = unit_count.saturating_add(1);
        let gsts = (u.reg_base as usize + REG_GSTS) as *const u32;
        let s = core::ptr::read_volatile(gsts);
        let te = (s & GSTS_TES) != 0;
        let mut buf = [0u8; 96]; let mut n = 0;
        for &b in b"VT-d: seg=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(u.seg as u32, &mut buf[n..]);
        for &b in b" TE=" { buf[n] = b; n += 1; }
        buf[n] = if te { b'1' } else { b'0' }; n += 1;
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let stdout = system_table.stdout(); let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    });
    // Print unit count and domain/assign/map counters (from metrics)
    {
        let stdout = system_table.stdout();
        let mut buf = [0u8; 128]; let mut n = 0;
        for &b in b"VT-d: units=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(unit_count, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
    crate::obs::metrics::dump(system_table);
}

pub fn report_stats(system_table: &mut SystemTable<Boot>) {
    let mut doms = 0u32; let mut assigns = 0u32; let mut maps = 0u32;
    crate::iommu::state::list_domains(|_| { doms = doms.saturating_add(1); });
    crate::iommu::state::list_assignments(|_,_,_,_,_| { assigns = assigns.saturating_add(1); });
    crate::iommu::state::list_mappings(|_,_,_,_,_,_,_| { maps = maps.saturating_add(1); });
    let stdout = system_table.stdout();
    let mut buf = [0u8; 96]; let mut n = 0;
    for &b in b"VT-d: doms=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(doms, &mut buf[n..]);
    for &b in b" assigns=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(assigns, &mut buf[n..]);
    for &b in b" maps=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(maps, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

/// Verify that in-memory context entries reflect current domain assignments and SLPTPTR provision
pub fn verify_state(system_table: &mut SystemTable<Boot>) {
    let mut issues = 0u32;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,domid| unsafe {
        if let Some(u) = find_unit_for_bdf(system_table, seg, bus, dev, func) {
            let (ri, ci) = vtd_indices_from_bdf(bus, dev, func);
            let root_ptr = u.root_tbl as *mut VtdRootEntry;
            let re = root_ptr.add(ri);
            let re_lo = core::ptr::read_volatile(core::ptr::addr_of!((*re).lower));
            if (re_lo & CTX_PRESENT) == 0 || (re_lo & 0xFFFF_FFFF_FFFF_F000u64) == 0 {
                issues = issues.saturating_add(1);
                let _ = system_table.stdout().write_str("verify: root entry missing or null ctx\r\n");
                return;
            }
            let ctx_ptr = (re_lo & 0xFFFF_FFFF_FFFF_F000u64) as *const VtdContextEntry;
            let ce = ctx_ptr.add(ci);
            let ce_lo = core::ptr::read_volatile(core::ptr::addr_of!((*ce).lower));
            let ce_up = core::ptr::read_volatile(core::ptr::addr_of!((*ce).upper));
            let ok_present = (ce_lo & CTX_PRESENT) != 0;
            let tt = ((ce_lo >> CTX_TT_SHIFT) & 0x3) as u64;
            let aw = ((ce_up >> CTXU_AW_SHIFT) & 0x7) as u64;
            let did = ((ce_up >> CTXU_DID_SHIFT) & 0xFFFF) as u64;
            let slpt = (ce_lo & CTX_LO_PTR_MASK) != 0;
            let ok = ok_present && tt == CTX_TT_MULTI_LEVEL && aw == 2 && did == (domid as u64) && slpt;
            if !ok {
                issues = issues.saturating_add(1);
                let mut buf = [0u8; 160]; let mut n = 0;
                for &b in b"verify: mismatch seg=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(seg as u32, &mut buf[n..]);
                for &b in b" bus=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(bus as u32, &mut buf[n..]);
                for &b in b" dev=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(dev as u32, &mut buf[n..]);
                for &b in b" fn=" { buf[n] = b; n += 1; }
                n += crate::firmware::acpi::u32_to_dec(func as u32, &mut buf[n..]);
                buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
                let _ = system_table.stdout().write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
            }
        }
    });
    if issues == 0 { let _ = system_table.stdout().write_str("verify: OK\r\n"); }
}

pub fn verify_mappings(system_table: &mut SystemTable<Boot>) {
    let mut issues = 0u32;
    crate::iommu::state::list_mappings(|dom,iova,_pa,len,_r,_w,_x| {
        if let Some(cr3) = get_cr3_for_bdf(system_table, 0, 0, 0, 0) { let _ = cr3; }
        // Verify first and last page
        let cr3 = match get_domain_slptptr(dom) { Some(v) => v, None => { issues = issues.saturating_add(1); return; } };
        let (pa0, _) = walk_second_level(cr3, iova);
        let (pal, _) = walk_second_level(cr3, iova.wrapping_add(len.saturating_sub(1)) & !0xFFFu64);
        if pa0.is_none() || pal.is_none() {
            issues = issues.saturating_add(1);
            let _ = system_table.stdout().write_str("verify-map: missing\r\n");
        }
        // Note: deeper range walk omitted for performance
    });
    if issues == 0 { let _ = system_table.stdout().write_str("verify-map: OK\r\n"); }
}



// --- Self-test helpers ---

#[derive(Clone, Copy)]
pub struct SelfTestConfig {
    pub quick: bool,
    pub do_apply: bool,
    pub do_invalidate: bool,
    pub test_domain: Option<u16>,
    pub walk_samples: u32,
    pub xlate_samples: u32,
}

impl Default for SelfTestConfig {
    fn default() -> Self {
        Self {
            quick: false,
            do_apply: true,
            do_invalidate: true,
            test_domain: None,
            walk_samples: 2,
            xlate_samples: 2,
        }
    }
}

/// Run a conservative VT-d self-test: plan→apply→verify→(invalidate)→stats/summary.
/// Optionally sample translate/walk on first mapped IOVA for a BDF within the domain.
pub fn selftest(system_table: &mut SystemTable<Boot>, cfg: SelfTestConfig) {
    let _ = system_table.stdout().write_str("VT-d: selftest start\r\n");

    if cfg.do_apply {
        plan_assignments(system_table);
        if cfg.quick {
            apply_and_refresh(system_table);
        } else {
            apply_safe(system_table);
        }
    }

    verify_state(system_table);
    verify_mappings(system_table);

    if cfg.do_invalidate { invalidate_all(system_table); }

    // Optional: sample a walk/xlate on first domain assignment + mapping
    let mut sampled = false;
    let mut sample_seg: u16 = 0; let mut sample_bus: u8 = 0; let mut sample_dev: u8 = 0; let mut sample_func: u8 = 0; let mut sample_dom: u16 = 0;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
        if sampled { return; }
        if let Some(filter) = cfg.test_domain { if filter != dom { return; } }
        sampled = true; sample_seg = seg; sample_bus = bus; sample_dev = dev; sample_func = func; sample_dom = dom;
    });
    if sampled {
        let mut iova_sample: Option<u64> = None;
        crate::iommu::state::list_mappings(|dom,iova,_pa,_len,_r,_w,_x| {
            if iova_sample.is_none() && dom == sample_dom { iova_sample = Some(iova); }
        });
        if let Some(iova) = iova_sample {
            // Perform a few translate/walk samples around the chosen IOVA
            let mut n = 0u32;
            while n < cfg.xlate_samples { translate_bdf_iova(system_table, sample_seg, sample_bus, sample_dev, sample_func, iova); n = n.saturating_add(1); }
            n = 0;
            while n < cfg.walk_samples { walk_bdf_iova(system_table, sample_seg, sample_bus, sample_dev, sample_func, iova); n = n.saturating_add(1); }
        } else {
            let _ = system_table.stdout().write_str("selftest: no mapping found for sampled domain\r\n");
        }
    } else {
        let _ = system_table.stdout().write_str("selftest: no assignment found\r\n");
    }

    report_stats(system_table);
    report_summary(system_table);
    let _ = system_table.stdout().write_str("VT-d: selftest done\r\n");
}


/// Sample translate/walk for all BDFs assigned to a given domain id.
/// Parameters:
/// - domid: target domain id
/// - iova: IOVA to translate/walk
/// - count: repeat count per BDF (minimum 1)
/// - do_walk/do_xlate: control which operations to run
pub fn sample_walk_xlate_for_domain(
    system_table: &mut SystemTable<Boot>,
    domid: u16,
    iova: u64,
    count: usize,
    do_walk: bool,
    do_xlate: bool,
) {
    let mut ran_any = false;
    crate::iommu::state::list_assignments(|seg,bus,dev,func,dom| {
        if dom != domid { return; }
        ran_any = true;
        let mut n = 0usize;
        let reps = if count == 0 { 1 } else { count };
        while n < reps {
            if do_xlate { translate_bdf_iova(system_table, seg, bus, dev, func, iova); }
            if do_walk { walk_bdf_iova(system_table, seg, bus, dev, func, iova); }
            n = n.saturating_add(1);
        }
    });
    if !ran_any { let _ = system_table.stdout().write_str("sample: no BDFs in domain\r\n"); }
}