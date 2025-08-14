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

// Migration counters
pub static MIG_SESSIONS: AtomicU64 = AtomicU64::new(0);
pub static MIG_SCAN_ROUNDS: AtomicU64 = AtomicU64::new(0);
pub static MIG_DIRTY_PAGES: AtomicU64 = AtomicU64::new(0);
pub static MIG_PRECOPY_ROUNDS: AtomicU64 = AtomicU64::new(0);
pub static MIG_PRECOPY_PAGES: AtomicU64 = AtomicU64::new(0);
pub static MIG_BYTES_TX: AtomicU64 = AtomicU64::new(0);
pub static MIG_ZERO_SKIPPED: AtomicU64 = AtomicU64::new(0);
pub static MIG_HASH_SKIPPED: AtomicU64 = AtomicU64::new(0);
pub static MIG_ZERO_BYTES_SAVED: AtomicU64 = AtomicU64::new(0);
pub static MIG_HASH_BYTES_SAVED: AtomicU64 = AtomicU64::new(0);
pub static MIG_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_RAW_PAGES: AtomicU64 = AtomicU64::new(0);
pub static MIG_COMPRESSED_PAGES: AtomicU64 = AtomicU64::new(0);
pub static MIG_MANIFESTS: AtomicU64 = AtomicU64::new(0);
pub static MIG_CTRL_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_ACKS: AtomicU64 = AtomicU64::new(0);
pub static MIG_NAKS: AtomicU64 = AtomicU64::new(0);
pub static MIG_RESEND_TRIGGERS: AtomicU64 = AtomicU64::new(0);
pub static MIG_CB_WRITTEN_BYTES: AtomicU64 = AtomicU64::new(0);
pub static MIG_CFG_SAVES: AtomicU64 = AtomicU64::new(0);
pub static MIG_CFG_LOADS: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_TX_BYTES: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_CFG_SET: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_TX_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_OPEN_OK: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_OPEN_FAIL: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_START_OK: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_START_FAIL: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_INIT_OK: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_INIT_FAIL: AtomicU64 = AtomicU64::new(0);
pub static MIG_NET_TX_ERRS: AtomicU64 = AtomicU64::new(0);
pub static MIG_PUMP_CALLS: AtomicU64 = AtomicU64::new(0);
pub static MIG_PUMP_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_PUMP_BYTES: AtomicU64 = AtomicU64::new(0);
pub static MIG_PUMP_EMPTY: AtomicU64 = AtomicU64::new(0);
pub static MIG_POLL_CYCLES: AtomicU64 = AtomicU64::new(0);
pub static MIG_CTRL_AUTO_ACK_SENT: AtomicU64 = AtomicU64::new(0);
pub static MIG_CTRL_AUTO_NAK_SENT: AtomicU64 = AtomicU64::new(0);
pub static MIG_RX_FRAMES_OK: AtomicU64 = AtomicU64::new(0);
pub static MIG_RX_FRAMES_BAD: AtomicU64 = AtomicU64::new(0);
pub static MIG_RX_BYTES: AtomicU64 = AtomicU64::new(0);
pub static MIG_REPLAY_PAGES: AtomicU64 = AtomicU64::new(0);
pub static MIG_REPLAY_BYTES: AtomicU64 = AtomicU64::new(0);
pub static MIG_REPLAY_ERRORS: AtomicU64 = AtomicU64::new(0);
pub static MIG_DUP_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_MISSING_FRAMES: AtomicU64 = AtomicU64::new(0);
pub static MIG_LAST_SEQ: AtomicU64 = AtomicU64::new(0);

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
    print("metrics: mig_sessions=", MIG_SESSIONS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_scan_rounds=", MIG_SCAN_ROUNDS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_dirty_pages=", MIG_DIRTY_PAGES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_precopy_rounds=", MIG_PRECOPY_ROUNDS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_precopy_pages=", MIG_PRECOPY_PAGES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_bytes_tx=", MIG_BYTES_TX.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_zero_skipped=", MIG_ZERO_SKIPPED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_hash_skipped=", MIG_HASH_SKIPPED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_zero_bytes_saved=", MIG_ZERO_BYTES_SAVED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_hash_bytes_saved=", MIG_HASH_BYTES_SAVED.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_frames=", MIG_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_raw_pages=", MIG_RAW_PAGES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_compressed_pages=", MIG_COMPRESSED_PAGES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_manifests=", MIG_MANIFESTS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_ctrl_frames=", MIG_CTRL_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_acks=", MIG_ACKS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_naks=", MIG_NAKS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_resend_triggers=", MIG_RESEND_TRIGGERS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_cb_written_bytes=", MIG_CB_WRITTEN_BYTES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_cfg_saves=", MIG_CFG_SAVES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_cfg_loads=", MIG_CFG_LOADS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_tx_bytes=", MIG_NET_TX_BYTES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_cfg_set=", MIG_NET_CFG_SET.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_tx_frames=", MIG_NET_TX_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_open_ok=", MIG_NET_OPEN_OK.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_open_fail=", MIG_NET_OPEN_FAIL.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_start_ok=", MIG_NET_START_OK.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_start_fail=", MIG_NET_START_FAIL.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_init_ok=", MIG_NET_INIT_OK.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_init_fail=", MIG_NET_INIT_FAIL.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_net_tx_errs=", MIG_NET_TX_ERRS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_pump_calls=", MIG_PUMP_CALLS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_pump_frames=", MIG_PUMP_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_pump_bytes=", MIG_PUMP_BYTES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_pump_empty=", MIG_PUMP_EMPTY.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_poll_cycles=", MIG_POLL_CYCLES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_ctrl_auto_ack=", MIG_CTRL_AUTO_ACK_SENT.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_ctrl_auto_nak=", MIG_CTRL_AUTO_NAK_SENT.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_rx_frames_ok=", MIG_RX_FRAMES_OK.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_rx_frames_bad=", MIG_RX_FRAMES_BAD.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_rx_bytes=", MIG_RX_BYTES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_replay_pages=", MIG_REPLAY_PAGES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_replay_bytes=", MIG_REPLAY_BYTES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_replay_errors=", MIG_REPLAY_ERRORS.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_dup_frames=", MIG_DUP_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_missing_frames=", MIG_MISSING_FRAMES.load(core::sync::atomic::Ordering::Relaxed));
    print("metrics: mig_last_seq=", MIG_LAST_SEQ.load(core::sync::atomic::Ordering::Relaxed));
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


