#![allow(dead_code)]

//! Minimal IOMMU domain state management for early bootstrap.
//! This is a conservative, fixed-capacity registry to track domains and
//! device assignments before full hardware programming is implemented.

#[derive(Clone, Copy, Debug, Default)]
pub struct Domain { pub id: u16, pub used: bool }

#[derive(Clone, Copy, Debug, Default)]
pub struct DevAssign { pub used: bool, pub seg: u16, pub bus: u8, pub dev: u8, pub func: u8, pub domid: u16 }

pub const MAX_DOMAINS: usize = 16;
pub const MAX_ASSIGNMENTS: usize = 128;
pub const MAX_MAPPINGS: usize = 256;

use crate::util::spinlock::SpinLock;

static DOMAINS: SpinLock<[Domain; MAX_DOMAINS]> = SpinLock::new([Domain { id: 0, used: false }; MAX_DOMAINS]);
static ASSIGNS: SpinLock<[DevAssign; MAX_ASSIGNMENTS]> = SpinLock::new([DevAssign { used: false, seg: 0, bus: 0, dev: 0, func: 0, domid: 0 }; MAX_ASSIGNMENTS]);
static NEXT_DOMAIN_ID_LOCK: SpinLock<u16> = SpinLock::new(1);

#[derive(Clone, Copy, Debug, Default)]
pub struct Mapping { pub used: bool, pub domid: u16, pub iova: u64, pub pa: u64, pub len: u64, pub perm_r: bool, pub perm_w: bool, pub perm_x: bool }

static MAPPINGS: SpinLock<[Mapping; MAX_MAPPINGS]> = SpinLock::new([Mapping { used: false, domid: 0, iova: 0, pa: 0, len: 0, perm_r: false, perm_w: false, perm_x: false }; MAX_MAPPINGS]);

pub fn create_domain() -> Option<u16> {
    let id = NEXT_DOMAIN_ID_LOCK.lock(|n| { let id = *n; *n = n.wrapping_add(1); id });
    let created = DOMAINS.lock(|arr| {
        for i in 0..MAX_DOMAINS { if !arr[i].used { arr[i] = Domain { id, used: true }; return true; } }
        false
    });
    if created { crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_DOMAIN_CREATED).inc(); Some(id) } else { None }
}

pub fn domain_exists(id: u16) -> bool {
    DOMAINS.lock(|arr| arr.iter().any(|d| d.used && d.id == id))
}

pub fn list_domains(mut f: impl FnMut(u16)) {
    DOMAINS.lock(|arr| { for d in arr.iter() { if d.used { f(d.id); } } })
}

pub fn assign_device(seg: u16, bus: u8, dev: u8, func: u8, domid: u16) -> bool {
    if !domain_exists(domid) { return false; }
    let added = ASSIGNS.lock(|arr| {
        for i in 0..MAX_ASSIGNMENTS { if !arr[i].used { arr[i] = DevAssign { used: true, seg, bus, dev, func, domid }; return true; } }
        false
    });
    if added { crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_ASSIGN_ADDED).inc(); true } else { false }
}

pub fn list_assignments(mut f: impl FnMut(u16, u8, u8, u8, u16)) { ASSIGNS.lock(|arr| { for a in arr.iter() { if a.used { f(a.seg, a.bus, a.dev, a.func, a.domid); } } }) }

pub fn has_assignments() -> bool { ASSIGNS.lock(|arr| arr.iter().any(|a| a.used)) }

pub fn destroy_domain(id: u16) -> bool {
    // Clear domain slot, purge mappings and assignments of this domain
    let mut found = false;
    DOMAINS.lock(|arr| {
        for d in arr.iter_mut() { if d.used && d.id == id { d.used = false; found = true; break; } }
    });
    if !found { return false; }
    ASSIGNS.lock(|arr| { for a in arr.iter_mut() { if a.used && a.domid == id { a.used = false; } } });
    MAPPINGS.lock(|arr| { for m in arr.iter_mut() { if m.used && m.domid == id { m.used = false; } } });
    true
}

pub fn unassign_device(seg: u16, bus: u8, dev: u8, func: u8) -> bool {
    let removed = ASSIGNS.lock(|arr| {
        for a in arr.iter_mut() { if a.used && a.seg == seg && a.bus == bus && a.dev == dev && a.func == func { a.used = false; return true; } }
        false
    });
    if removed { crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_ASSIGN_REMOVED).inc(); true } else { false }
}

pub fn add_mapping(domid: u16, iova: u64, pa: u64, len: u64, r: bool, w: bool, x: bool) -> bool {
    if !domain_exists(domid) || len == 0 { return false; }
    let ok = MAPPINGS.lock(|arr| {
        for m in arr.iter_mut() { if !m.used { *m = Mapping { used: true, domid, iova, pa, len, perm_r: r, perm_w: w, perm_x: x }; return true; } }
        false
    });
    if ok { crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_MAP_ADDED).inc(); true } else { false }
}

pub fn remove_mapping(domid: u16, iova: u64, len: u64) -> bool {
    let removed = MAPPINGS.lock(|arr| {
        for m in arr.iter_mut() { if m.used && m.domid == domid && m.iova == iova && m.len == len { m.used = false; return true; } }
        false
    });
    if removed { crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_MAP_REMOVED).inc(); true } else { false }
}

pub fn list_mappings(mut f: impl FnMut(u16, u64, u64, u64, bool, bool, bool)) {
    MAPPINGS.lock(|arr| { for m in arr.iter() { if m.used { f(m.domid, m.iova, m.pa, m.len, m.perm_r, m.perm_w, m.perm_x); } } })
}

pub fn remove_mappings_for_domain(domid: u16) -> u32 {
    let mut removed: u32 = 0;
    MAPPINGS.lock(|arr| {
        for m in arr.iter_mut() {
            if m.used && m.domid == domid {
                m.used = false;
                removed = removed.saturating_add(1);
                crate::obs::metrics::Counter::new(&crate::obs::metrics::IOMMU_MAP_REMOVED).inc();
            }
        }
    });
    removed
}

pub fn find_domain_for_bdf(seg: u16, bus: u8, dev: u8, func: u8) -> Option<u16> {
    let mut out: Option<u16> = None;
    ASSIGNS.lock(|arr| { for a in arr.iter() { if a.used && a.seg == seg && a.bus == bus && a.dev == dev && a.func == func { out = Some(a.domid); break; } } });
    out
}


