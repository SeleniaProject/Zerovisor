#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use core::fmt::Write as _;

pub struct Counter(&'static AtomicU64);

impl Counter {
    pub const fn new(cell: &'static AtomicU64) -> Self { Self(cell) }
    pub fn inc(&self) { self.0.fetch_add(1, Ordering::Relaxed); }
    pub fn add(&self, v: u64) { self.0.fetch_add(v, Ordering::Relaxed); }
    pub fn get(&self) -> u64 { self.0.load(Ordering::Relaxed) }
}

pub static VM_CREATED: AtomicU64 = AtomicU64::new(0);
pub static VM_STARTED: AtomicU64 = AtomicU64::new(0);
pub static VCPU_STARTED: AtomicU64 = AtomicU64::new(0);
pub static VCPU_STOPPED: AtomicU64 = AtomicU64::new(0);

// IOMMU domain and mapping counters
pub static IOMMU_DOMAIN_CREATED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_ASSIGN_ADDED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_ASSIGN_REMOVED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_MAP_ADDED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_MAP_REMOVED: AtomicU64 = AtomicU64::new(0);

// IOMMU invalidation counters
pub static IOMMU_INV_ALL: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_INV_DOMAIN: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_INV_BDF: AtomicU64 = AtomicU64::new(0);

// Simple fixed-bucket histogram for microsecond durations
const VMX_SMOKE_BUCKET_EDGES_US: [u64; 8] = [1, 5, 10, 25, 50, 100, 250, 1000];
pub static VMX_SMOKE_HIST_US: [AtomicU64; 9] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)
];

pub fn observe_vmx_smoke_us(us: u64) {
    // Find bucket index
    let mut idx = VMX_SMOKE_BUCKET_EDGES_US.len();
    for (i, edge) in VMX_SMOKE_BUCKET_EDGES_US.iter().enumerate() {
        if us <= *edge { idx = i; break; }
    }
    VMX_SMOKE_HIST_US[idx].fetch_add(1, Ordering::Relaxed);
}

pub fn dump(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let stdout = system_table.stdout();
    let mut buf = [0u8; 128];
    let mut print = |label: &str, val: u64| {
        let mut n = 0;
        for &b in label.as_bytes() { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(val as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    };
    print("metrics: vm_created=", VM_CREATED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: vm_started=", VM_STARTED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: vcpu_started=", VCPU_STARTED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: vcpu_stopped=", VCPU_STOPPED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_domain_created=", IOMMU_DOMAIN_CREATED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_assign_added=", IOMMU_ASSIGN_ADDED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_assign_removed=", IOMMU_ASSIGN_REMOVED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_map_added=", IOMMU_MAP_ADDED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_map_removed=", IOMMU_MAP_REMOVED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_inval_all=", IOMMU_INV_ALL.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_inval_domain=", IOMMU_INV_DOMAIN.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_inval_bdf=", IOMMU_INV_BDF.load(core::sync::atomic::Ordering::Relaxed));
    // Dump histogram (compact)
    {
        let mut n = 0;
        for &b in b"metrics: vmx_smoke_us=" { buf[n] = b; n += 1; }
        // Print buckets as [<=edge:count,...,>last:count]
        for (i, edge) in VMX_SMOKE_BUCKET_EDGES_US.iter().enumerate() {
            if i > 0 { buf[n] = b','; n += 1; }
            buf[n] = b'['; n += 1; buf[n] = b'<'; n += 1; buf[n] = b'='; n += 1;
            n += crate::firmware::acpi::u32_to_dec(*edge as u32, &mut buf[n..]);
            buf[n] = b':'; n += 1;
            n += crate::firmware::acpi::u32_to_dec(VMX_SMOKE_HIST_US[i].load(Ordering::Relaxed) as u32, &mut buf[n..]);
            buf[n] = b']'; n += 1;
        }
        // Last bucket '>'
        buf[n] = b','; n += 1; buf[n] = b'['; n += 1; buf[n] = b'>'; n += 1;
        n += crate::firmware::acpi::u32_to_dec(*VMX_SMOKE_BUCKET_EDGES_US.last().unwrap() as u32, &mut buf[n..]);
        buf[n] = b':'; n += 1;
        n += crate::firmware::acpi::u32_to_dec(VMX_SMOKE_HIST_US[VMX_SMOKE_BUCKET_EDGES_US.len()].load(Ordering::Relaxed) as u32, &mut buf[n..]);
        buf[n] = b']'; n += 1; buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    }
}

pub fn reset() {
    VM_CREATED.store(0, Ordering::Relaxed);
    VM_STARTED.store(0, Ordering::Relaxed);
    VCPU_STARTED.store(0, Ordering::Relaxed);
    VCPU_STOPPED.store(0, Ordering::Relaxed);
    IOMMU_DOMAIN_CREATED.store(0, Ordering::Relaxed);
    IOMMU_ASSIGN_ADDED.store(0, Ordering::Relaxed);
    IOMMU_ASSIGN_REMOVED.store(0, Ordering::Relaxed);
    IOMMU_MAP_ADDED.store(0, Ordering::Relaxed);
    IOMMU_MAP_REMOVED.store(0, Ordering::Relaxed);
    IOMMU_INV_ALL.store(0, Ordering::Relaxed);
    IOMMU_INV_DOMAIN.store(0, Ordering::Relaxed);
    IOMMU_INV_BDF.store(0, Ordering::Relaxed);
    for b in &VMX_SMOKE_HIST_US { b.store(0, Ordering::Relaxed); }
}


