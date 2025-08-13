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

// IOMMU domain and mapping counters
pub static IOMMU_DOMAIN_CREATED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_ASSIGN_ADDED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_ASSIGN_REMOVED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_MAP_ADDED: AtomicU64 = AtomicU64::new(0);
pub static IOMMU_MAP_REMOVED: AtomicU64 = AtomicU64::new(0);

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
    print("metrics: iommu_domain_created=", IOMMU_DOMAIN_CREATED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_assign_added=", IOMMU_ASSIGN_ADDED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_assign_removed=", IOMMU_ASSIGN_REMOVED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_map_added=", IOMMU_MAP_ADDED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: iommu_map_removed=", IOMMU_MAP_REMOVED.load(core::sync::atomic::Ordering::Relaxed));
}


