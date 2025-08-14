#![allow(dead_code)]

//! Live migration groundwork: dirty-page tracking and control plane hooks.
//!
//! This module provides a minimal, allocator-free implementation to build and
//! manage a dirty-page bitmap for a VM using identity-mapped EPT/NPT tables that
//! Zerovisor already creates for smoke tests. It purposefully avoids dynamic
//! allocation by using UEFI page allocation and a compact bitset layout.
//!
//! Notes:
//! - On Intel, we rely on EPT A/D flags (when available) and scan leaf entries
//!   at 1GiB/2MiB/4KiB granularity. Bits are cleared after sampling to enable
//!   delta rounds. The EPTP A/D enable bit (bit 6) must be set by the caller
//!   when entering VMX with EPT to have the CPU set A/D flags.
//! - On AMD, Nested Page Tables (NPT) entries also expose Accessed/Dirty bits in
//!   the same bit positions as the standard page tables (A=bit 5, D=bit 6) per
//!   AMD64 architecture. We read and (optionally) clear those bits.
//! - In the current prototype, guest execution is not sustained; therefore A/D
//!   flags will not be toggled by hardware yet. The tracker and scanner are
//!   designed to be correct and ready for future long-running guests.
//!
//! All code paths are `no_std` and safe for early-boot usage.

use core::ptr::read_volatile;
use core::ptr::write_volatile;
use core::fmt::Write as _; // enable write_str on UEFI text output
use uefi::prelude::Boot;
use uefi::table::SystemTable;
use uefi::table::boot::MemoryType;
use core::mem::size_of;
use uefi::table::runtime::VariableVendor;

/// Kind of nested translation used by the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackerKind { IntelEpt, AmdNpt, Unknown }

/// Live migration tracker instance bound to a VM identity map.
#[derive(Debug)]
pub struct DirtyTracker {
    pub vm_id: u64,
    pub root_phys: u64,     // PML4 physical address of EPT/NPT
    pub memory_limit: u64,  // Bytes of guest-physical to consider
    pub kind: TrackerKind,
}

/// Compact bitset stored in UEFI-allocated pages.
pub struct DirtyBitmap {
    base: *mut u8,
    bytes: usize,
    pages: usize,
}

impl DirtyBitmap {
    /// Allocate bitmap that can represent `num_pages` bits (rounded up to 4KiB pages).
    pub fn allocate(system_table: &SystemTable<Boot>, num_pages: u64) -> Option<Self> {
        let bytes = ((num_pages as usize) + 7) / 8;
        let pages = (bytes + 4095) / 4096;
        let ptr = crate::mm::uefi::alloc_pages(system_table, pages, uefi::table::boot::MemoryType::LOADER_DATA)?;
        unsafe { core::ptr::write_bytes(ptr, 0, pages * 4096); }
        Some(Self { base: ptr, bytes, pages })
    }

    /// Free underlying storage.
    pub fn free(self, system_table: &SystemTable<Boot>) {
        unsafe {
            crate::mm::uefi::free_pages(system_table, self.base, self.pages);
        }
    }

    #[inline(always)]
    pub fn clear_all(&mut self) {
        unsafe { core::ptr::write_bytes(self.base, 0, self.bytes); }
    }

    #[inline(always)]
    pub fn set_bit(&mut self, index: u64) {
        let i = index as usize;
        let byte = i >> 3;
        let bit = i & 7;
        if byte < self.bytes {
            unsafe {
                let p = self.base.add(byte);
                let v = read_volatile(p);
                write_volatile(p, v | (1u8 << bit));
            }
        }
    }

    /// Count set bits (population count). Runs in O(n) over the bitmap.
    pub fn count_set(&self) -> u64 {
        let mut total: u64 = 0;
        let mut i = 0;
        while i < self.bytes {
            let v = unsafe { read_volatile(self.base.add(i)) } as u64;
            total += v.count_ones() as u64;
            i += 1;
        }
        total
    }

    /// Iterate all set bits and call the closure with page index.
    pub fn for_each_set<F: FnMut(u64)>(&self, mut f: F) {
        let mut byte_index = 0usize;
        let mut base_bit: u64 = 0;
        while byte_index < self.bytes {
            let v = unsafe { read_volatile(self.base.add(byte_index)) };
            if v != 0 {
                let mut mask = v;
                let mut bit = 0u8;
                while mask != 0 {
                    if (mask & 1) != 0 { f(base_bit + bit as u64); }
                    mask >>= 1;
                    bit = bit.wrapping_add(1);
                }
            }
            base_bit += 8;
            byte_index += 1;
        }
    }
}

/// Global tracker state for the simple CLI control plane.
struct TrackerState {
    tracker: DirtyTracker,
    bitmap: DirtyBitmap,
}

static mut G_TRACKER: Option<TrackerState> = None;
static mut G_SEQ: u32 = 1;
static mut G_CHUNK: usize = 1500; // default MTU-like chunk size for writers
static mut SESSION_START_TSC: u64 = 0;
// Transmit log for resend operations
#[derive(Clone, Copy)]
struct TxEntry { kind: u8, seq: u32, page_index: u64 }
const TX_LOG_CAP: usize = 1024;
static mut TX_LOG: [TxEntry; TX_LOG_CAP] = [TxEntry { kind: 0, seq: 0, page_index: 0 }; TX_LOG_CAP];
static mut TX_WIDX: usize = 0;
#[inline(always)]
unsafe fn tx_log_append(kind: u8, seq: u32, page_index: u64) {
    let i = TX_WIDX % TX_LOG_CAP; TX_LOG[i] = TxEntry { kind, seq, page_index }; TX_WIDX = TX_WIDX.wrapping_add(1);
}

/// Create a tracker for the given VM with identity map already built.
pub fn create_tracker_for_vm(vm: &crate::hv::vm::Vm) -> Option<DirtyTracker> {
    let kind = match vm.vendor {
        crate::hv::vm::HvVendor::Intel => TrackerKind::IntelEpt,
        crate::hv::vm::HvVendor::Amd => TrackerKind::AmdNpt,
        crate::hv::vm::HvVendor::Unknown => TrackerKind::Unknown,
    };
    if kind == TrackerKind::Unknown { return None; }
    if vm.pml4_phys == 0 { return None; }
    Some(DirtyTracker { vm_id: vm.id.0, root_phys: vm.pml4_phys, memory_limit: vm.config.memory_bytes.max(1u64 << 30), kind })
}

/// Begin tracking: allocate bitmap and install the global state.
pub fn start_tracking(system_table: &SystemTable<Boot>, vm: &crate::hv::vm::Vm) -> bool {
    let tracker = match create_tracker_for_vm(vm) { Some(t) => t, None => return false };
    let pages = (tracker.memory_limit + 4095) / 4096; // 4KiB pages in scope
    let bitmap = match DirtyBitmap::allocate(system_table, pages) { Some(b) => b, None => return false };
    unsafe { G_TRACKER = Some(TrackerState { tracker, bitmap }); }
    crate::diag::audit::record(crate::diag::audit::AuditKind::MigrateStart(vm.id.0));
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_SESSIONS).inc();
    true
}

pub fn start_tracking_by_id(system_table: &SystemTable<Boot>, id: u64) -> bool {
    if let Some(info) = crate::hv::vm::find_vm(id) {
        let vm = crate::hv::vm::Vm { id: crate::hv::vm::VmId(info.id), config: crate::hv::vm::VmConfig { memory_bytes: info.memory_bytes, vcpu_count: 1 }, vendor: info.vendor, pml4_phys: info.pml4_phys };
        return start_tracking(system_table, &vm);
    }
    false
}

/// Stop tracking and free resources if any.
pub fn stop_tracking(system_table: &SystemTable<Boot>) -> bool {
    let st = unsafe { G_TRACKER.take() };
    if let Some(state) = st {
        state.bitmap.free(system_table);
        crate::diag::audit::record(crate::diag::audit::AuditKind::MigrateStop(state.tracker.vm_id));
        return true;
    }
    false
}

/// Perform one scan round. Returns number of dirty pages observed in this round.
pub fn scan_round(clear_ad: bool) -> u64 {
    let st = unsafe { G_TRACKER.as_mut() };
    if st.is_none() { return 0; }
    let state = st.unwrap();
    let dirty = match state.tracker.kind {
        TrackerKind::IntelEpt => scan_ept(state.tracker.root_phys, state.tracker.memory_limit, &mut state.bitmap, clear_ad),
        TrackerKind::AmdNpt => scan_npt(state.tracker.root_phys, state.tracker.memory_limit, &mut state.bitmap, clear_ad),
        TrackerKind::Unknown => 0,
    };
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_SCAN_ROUNDS).inc();
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_DIRTY_PAGES).add(dirty);
    crate::obs::trace::emit(crate::obs::trace::Event::MigrateScanRound(state.tracker.vm_id as u64, dirty));
    crate::diag::audit::record(crate::diag::audit::AuditKind::MigrateScan(state.tracker.vm_id as u64, dirty));
    dirty
}

/// Dump tracker stats to console.
pub fn dump_stats(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let mut buf = [0u8; 128];
    if let Some(st) = unsafe { G_TRACKER.as_ref() } {
        let mut n = 0;
        for &b in b"migrate: vm_id=" { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(st.tracker.vm_id as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
        // Dirty pages total (bitmap popcount)
        let total = st.bitmap.count_set();
        let mut n2 = 0;
        for &b in b"migrate: dirty_pages_total=" { buf[n2] = b; n2 += 1; }
        n2 += crate::firmware::acpi::u32_to_dec(total as u32, &mut buf[n2..]);
        buf[n2] = b'\r'; n2 += 1; buf[n2] = b'\n'; n2 += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n2]).unwrap_or("\r\n"));
    } else {
        let _ = stdout.write_str("migrate: no active tracker\r\n");
    }
}

/// Data sink for migration export operations.
#[derive(Clone, Copy, Debug)]
pub enum ExportSink { Console, Null, Buffer, Snp, Virtio }
/// Abstract writer for migration. Future implementations can add network or storage sinks.
pub trait MigrWriter {
    /// Write bytes; returns number written.
    fn write(&mut self, buf: &[u8]) -> usize;
}

#[cfg(feature = "virtio-net")]
pub struct VirtioNetWriter<'a> { pub system_table: &'a mut SystemTable<Boot> }
#[cfg(feature = "virtio-net")]
impl<'a> MigrWriter for VirtioNetWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> usize {
        let wrote = crate::virtio::net::tx_send(self.system_table, buf);
        if wrote > 0 {
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_BYTES).add(wrote as u64);
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_FRAMES).inc();
        } else {
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_ERRS).inc();
        }
        wrote
    }
}

/// Console-backed writer (UEFI text; printable hex only). For binary pages we rely on `export_range`.
pub struct ConsoleWriter<'a> { pub system_table: &'a mut SystemTable<Boot> }
impl<'a> MigrWriter for ConsoleWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> usize {
        // Hex-dump bytes in lines of up to 16 bytes for console safety
        let stdout = self.system_table.stdout();
        let mut i = 0usize;
        let mut line: [u8; 96] = [0; 96];
        while i < buf.len() {
            let take = core::cmp::min(16, buf.len() - i);
            let mut n = 0usize;
            for j in 0..take {
                let b = buf[i + j] as u64;
                n += crate::util::format::u64_hex(b, &mut line[n..]);
                line[n] = b' '; n += 1;
            }
            line[n] = b'\r'; n += 1; line[n] = b'\n'; n += 1;
            let _ = stdout.write_str(core::str::from_utf8(&line[..n]).unwrap_or("\r\n"));
            i += take;
        }
        buf.len()
    }
}

pub struct NullWriter;
impl MigrWriter for NullWriter {
    fn write(&mut self, _buf: &[u8]) -> usize { 0 }
}

struct Buffer {
    ptr: *mut u8,
    cap: usize,
    wpos: usize,
    len: usize,
}

static mut G_BUF: Option<Buffer> = None;
static mut G_DEST_MAC: [u8; 6] = [0; 6];
static mut G_MTU: usize = 1500; // network MTU hint (payload chunking uses G_CHUNK by default)
static mut G_ETHER_TYPE: u16 = 0x88B5; // experimental EtherType for migration frames
static mut G_CTRL_RESEND_SINK: ExportSink = ExportSink::Buffer; // default resend target for ctrl NAK
static mut G_CTRL_AUTO_ACK: bool = false;
static mut G_CTRL_AUTO_NAK: bool = false;
static mut G_DEFAULT_SINK: ExportSink = ExportSink::Buffer;
#[cfg(feature = "snp")]
const SNP_MAX: usize = 16;
#[cfg(feature = "snp")]
static mut G_SNP_HANDLES: [uefi::Handle; SNP_MAX] = [core::ptr::null_mut(); SNP_MAX];
#[cfg(feature = "snp")]
static mut G_SNP_LEN: usize = 0;
#[cfg(feature = "snp")]
static mut G_SNP_SEL_IDX: Option<usize> = None;

#[inline(always)]
pub fn net_get_dest_mac() -> [u8; 6] { unsafe { G_DEST_MAC } }
#[inline(always)]
pub fn net_set_dest_mac(mac: [u8; 6]) { unsafe { G_DEST_MAC = mac; } }
#[inline(always)]
pub fn net_get_mtu() -> usize { unsafe { if G_MTU == 0 { 1500 } else { G_MTU } } }
#[inline(always)]
pub fn net_set_mtu(mtu: usize) { unsafe { G_MTU = if mtu < 576 { 576 } else { mtu }; } }
#[inline(always)]
pub fn net_get_ethertype() -> u16 { unsafe { G_ETHER_TYPE } }
#[inline(always)]
pub fn net_set_ethertype(et: u16) { unsafe { G_ETHER_TYPE = et; } }
#[inline(always)]
pub fn ctrl_get_resend_sink() -> ExportSink { unsafe { G_CTRL_RESEND_SINK } }
#[inline(always)]
pub fn ctrl_set_resend_sink(s: ExportSink) { unsafe { G_CTRL_RESEND_SINK = s; } }
#[inline(always)]
pub fn ctrl_get_auto_ack() -> bool { unsafe { G_CTRL_AUTO_ACK } }
#[inline(always)]
pub fn ctrl_set_auto_ack(v: bool) { unsafe { G_CTRL_AUTO_ACK = v; } }
#[inline(always)]
pub fn ctrl_get_auto_nak() -> bool { unsafe { G_CTRL_AUTO_NAK } }
#[inline(always)]
pub fn ctrl_set_auto_nak(v: bool) { unsafe { G_CTRL_AUTO_NAK = v; } }
#[inline(always)]
pub fn get_default_sink() -> ExportSink { unsafe { G_DEFAULT_SINK } }
#[inline(always)]
pub fn set_default_sink(s: ExportSink) { unsafe { G_DEFAULT_SINK = s; } }

#[inline(always)]
fn sink_to_u8(s: ExportSink) -> u8 {
    match s {
        ExportSink::Console => 0,
        ExportSink::Null => 1,
        ExportSink::Buffer => 2,
        ExportSink::Snp => 3,
        ExportSink::Virtio => 4,
    }
}
#[inline(always)]
fn u8_to_sink(v: u8) -> ExportSink {
    match v {
        0 => ExportSink::Console,
        1 => ExportSink::Null,
        2 => ExportSink::Buffer,
        3 => ExportSink::Snp,
        4 => ExportSink::Virtio,
        _ => ExportSink::Buffer,
    }
}

// ---- SNP discovery/control (feature-gated) ----
#[cfg(feature = "snp")]
pub fn snp_discover(system_table: &mut SystemTable<Boot>) {
    use uefi::table::boot::SearchType;
    let bs = system_table.boot_services();
    match bs.locate_handle_buffer(SearchType::ByProtocol(&uefi::proto::network::snp::SimpleNetwork::GUID)) {
        Ok(handles) => {
            let count = handles.len();
            // Copy handles into our static store to avoid lifetime issues
            let mut copied = 0usize;
            unsafe {
                while copied < count && copied < SNP_MAX { G_SNP_HANDLES[copied] = handles[copied]; copied += 1; }
                G_SNP_LEN = copied;
            }
            let stdout = system_table.stdout();
            let mut buf = [0u8; 64]; let mut n = 0; for &b in b"snp: handles=" { buf[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(copied as u32, &mut buf[n..]); buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
            for i in 0..copied {
                let h = unsafe { G_SNP_HANDLES[i] };
                let mut line = [0u8; 64]; let mut m = 0; for &b in b"  idx=" { line[m] = b; m += 1; }
                m += crate::firmware::acpi::u32_to_dec(i as u32, &mut line[m..]);
                for &b in b" handle=0x" { line[m] = b; m += 1; }
                m += crate::util::format::u64_hex(h as u64, &mut line[m..]);
                line[m] = b'\r'; m += 1; line[m] = b'\n'; m += 1;
                let _ = stdout.write_str(core::str::from_utf8(&line[..m]).unwrap_or("\r\n"));
            }
        }
        Err(_) => { let _ = system_table.stdout().write_str("snp: no devices\r\n"); }
    }
}

#[cfg(not(feature = "snp"))]
pub fn snp_discover(system_table: &mut SystemTable<Boot>) { let _ = system_table.stdout().write_str("snp: feature disabled\r\n"); }

#[cfg(feature = "snp")]
pub fn snp_use(system_table: &mut SystemTable<Boot>, idx: usize) {
    let len = unsafe { G_SNP_LEN };
    if idx < len { unsafe { G_SNP_SEL_IDX = Some(idx); } let _ = system_table.stdout().write_str("snp: selected\r\n"); return; }
    let _ = system_table.stdout().write_str("snp: invalid index\r\n");
}

#[cfg(not(feature = "snp"))]
pub fn snp_use(system_table: &mut SystemTable<Boot>, _idx: usize) { let _ = system_table.stdout().write_str("snp: feature disabled\r\n"); }

#[cfg(feature = "snp")]
pub fn snp_info(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    if let Some(sel) = unsafe { G_SNP_SEL_IDX } {
        let h = unsafe { G_SNP_HANDLES[sel] };
        // Try open protocol and print current station address
        let bs = system_table.boot_services();
        if let Ok(mut snp) = unsafe { bs.open_protocol_exclusive::<uefi::proto::network::snp::SimpleNetwork>(h) } {
            let mode = snp.mode();
            let mac = mode.current_address;
            let mut out = [0u8; 96]; let mut n = 0; for &b in b"snp: mac=" { out[n] = b; n += 1; }
            for i in 0..6 { n += crate::util::format::u64_hex(mac.addr[i] as u64, &mut out[n..]); if i != 5 { out[n] = b':'; n += 1; } }
            for &b in b" mtu=" { out[n] = b; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(mode.max_packet_size as u32, &mut out[n..]);
            out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1; let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
            return;
        }
    }
    let _ = stdout.write_str("snp: not selected\r\n");
}

#[cfg(not(feature = "snp"))]
pub fn snp_info(system_table: &mut SystemTable<Boot>) { let _ = system_table.stdout().write_str("snp: feature disabled\r\n"); }

#[cfg(feature = "snp")]
pub fn snp_pump(system_table: &mut SystemTable<Boot>, limit: usize) {
    let stdout = system_table.stdout();
    let sel = unsafe { G_SNP_SEL_IDX };
    if sel.is_none() { let _ = stdout.write_str("snp: not selected\r\n"); return; }
    let h = unsafe { G_SNP_HANDLES[sel.unwrap()] };
    let bs = system_table.boot_services();
    let mut opened = match unsafe { bs.open_protocol_exclusive::<uefi::proto::network::snp::SimpleNetwork>(h) } {
        Ok(p) => p,
        Err(_) => { let _ = stdout.write_str("snp: open fail\r\n"); return; }
    };
    // Ensure started and initialized
    if opened.state() == uefi::proto::network::snp::State::Stopped {
        if opened.start().is_err() { let _ = stdout.write_str("snp: start fail\r\n"); return; }
    }
    if opened.state() == uefi::proto::network::snp::State::Started {
        if opened.initialize(0, 0).is_err() { let _ = stdout.write_str("snp: init fail\r\n"); return; }
    }
    let mut pumped = 0usize;
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_CALLS).inc();
    let mut pkt = [0u8; 2048];
    // Expected sequence tracking using global last seq
    let mut expected_seq = crate::obs::metrics::MIG_LAST_SEQ.load(core::sync::atomic::Ordering::Relaxed) as u32;
    let hdr_len = core::mem::size_of::<FrameHeader>();
    while limit == 0 || pumped < limit {
        let res = unsafe { opened.receive(None, &mut pkt) };
        let data = match res { Ok((_h, d)) => d, Err(_) => { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_EMPTY).inc(); break } };
        let mut pos = 0usize;
        while pos + hdr_len <= data.len() {
            if &data[pos..pos+4] != &MAGIC { pos += 1; continue; }
            if pos + hdr_len > data.len() { break; }
            let ver = data[pos+4]; let _typ = data[pos+5];
            if ver != 1 { pos += 1; continue; }
            let payload_len = le_u32(&data[pos+20..pos+24]) as usize;
            let crc_hdr = le_u32(&data[pos+24..pos+28]);
            if pos + hdr_len + payload_len > data.len() { break; }
            let payload = &data[pos+hdr_len .. pos+hdr_len+payload_len];
            let crc_calc = crate::util::crc32::crc32(payload);
            let seq = le_u32(&data[pos+8..pos+12]);
            let good = crc_calc == crc_hdr;
            if good {
                // Write header+payload into channel buffer
                let _ = chan_write(&data[pos .. pos+hdr_len]);
                let _ = chan_write(payload);
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_OK).inc();
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_BYTES).add((hdr_len + payload_len) as u64);
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_FRAMES).inc();
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_BYTES).add((hdr_len + payload_len) as u64);
                // Ordering diagnostics
                if expected_seq != 0 {
                    let next = expected_seq.wrapping_add(1);
                    if seq == next { /* in order */ }
                    else if seq < next { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_DUP_FRAMES).inc(); }
                    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_MISSING_FRAMES).inc(); }
                }
                expected_seq = seq;
                crate::obs::metrics::MIG_LAST_SEQ.store(seq as u64, core::sync::atomic::Ordering::Relaxed);
                pumped += 1;
            } else {
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_BAD).inc();
            }
            pos += hdr_len + payload_len;
        }
    }
}

#[cfg(not(feature = "snp"))]
pub fn snp_pump(system_table: &mut SystemTable<Boot>, _limit: usize) { let _ = system_table.stdout().write_str("snp: feature disabled\r\n"); }

#[cfg(feature = "snp")]
pub fn snp_poll(system_table: &mut SystemTable<Boot>, cycles: usize, sleep_us: usize, do_ctrl: bool, do_verify: bool) {
    snp_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, 0);
}

pub fn snp_poll_ex(system_table: &mut SystemTable<Boot>, mut cycles: usize, sleep_us: usize, do_ctrl: bool, do_verify: bool, empty_limit: usize) {
    let mut empty_runs = 0usize;
    loop {
        let before = crate::obs::metrics::MIG_PUMP_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
        snp_pump(system_table, 0);
        let after = crate::obs::metrics::MIG_PUMP_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
        if after == before { empty_runs = empty_runs.saturating_add(1); } else { empty_runs = 0; }
        if do_ctrl { chan_handle_ctrl(system_table, 0); }
        if do_verify { chan_verify_ex(system_table, 0, true, true); }
        if sleep_us > 0 { let _ = system_table.boot_services().stall(sleep_us); }
        if cycles > 0 { cycles -= 1; if cycles == 0 { break; } }
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_POLL_CYCLES).inc();
        if empty_limit > 0 && empty_runs >= empty_limit { break; }
        if cycles == 0 && sleep_us == 0 && !do_ctrl && !do_verify { break; }
    }
}

pub fn virtio_poll(system_table: &mut SystemTable<Boot>, cycles: usize, sleep_us: usize, do_ctrl: bool, do_verify: bool, empty_limit: usize) {
    virtio_poll_ex(system_table, cycles, sleep_us, do_ctrl, do_verify, empty_limit);
}

pub fn virtio_poll_ex(system_table: &mut SystemTable<Boot>, mut cycles: usize, sleep_us: usize, do_ctrl: bool, do_verify: bool, empty_limit: usize) {
    let mut empty_runs = 0usize;
    loop {
        let before = crate::obs::metrics::MIG_PUMP_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
        crate::virtio::net::rx_pump(system_table, 0);
        let after = crate::obs::metrics::MIG_PUMP_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
        if after == before { empty_runs = empty_runs.saturating_add(1); } else { empty_runs = 0; }
        if do_ctrl { chan_handle_ctrl(system_table, 0); }
        if do_verify { chan_verify_ex(system_table, 0, true, true); }
        if sleep_us > 0 { let _ = system_table.boot_services().stall(sleep_us); }
        if cycles > 0 { cycles -= 1; if cycles == 0 { break; } }
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_POLL_CYCLES).inc();
        if empty_limit > 0 && empty_runs >= empty_limit { break; }
        if cycles == 0 && sleep_us == 0 && !do_ctrl && !do_verify { break; }
    }
}

#[cfg(not(feature = "snp"))]
pub fn snp_poll(system_table: &mut SystemTable<Boot>, _cycles: usize, _sleep_us: usize, _do_ctrl: bool, _do_verify: bool) { let _ = system_table.stdout().write_str("snp: feature disabled\r\n"); }

fn chan_write(buf: &[u8]) -> usize {
    unsafe {
        if let Some(b) = G_BUF.as_mut() {
            let mut written = 0usize;
            let mut src_off = 0usize;
            while src_off < buf.len() {
                if b.cap == 0 { break; }
                let space = b.cap - (if b.len < b.cap { b.len } else { b.cap });
                let to_write = core::cmp::min(buf.len() - src_off, if space == 0 { b.cap } else { space });
                // Overwrite oldest when full
                if b.len + to_write > b.cap { b.len = b.cap; }
                else { b.len += to_write; }
                let end = core::cmp::min(b.cap - b.wpos, to_write);
                core::ptr::copy_nonoverlapping(buf.as_ptr().add(src_off), b.ptr.add(b.wpos), end);
                b.wpos = (b.wpos + end) % b.cap;
                let rem = to_write - end;
                if rem > 0 {
                    core::ptr::copy_nonoverlapping(buf.as_ptr().add(src_off + end), b.ptr, rem);
                    b.wpos = rem;
                }
                written += to_write;
                src_off += to_write;
            }
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CB_WRITTEN_BYTES).add(written as u64);
            return written;
        }
    }
    0
}

pub struct BufferWriter;
impl MigrWriter for BufferWriter {
    fn write(&mut self, buf: &[u8]) -> usize { chan_write(buf) }
}

/// Public helper to allow other modules to write into the migration channel buffer.
pub fn chan_write_bytes(buf: &[u8]) -> usize { chan_write(buf) }

// SNP-backed writer (UEFI Simple Network Protocol)
// The real implementation is enabled with the "snp" feature. Without it, this writer is unavailable.
#[cfg(feature = "snp")]
pub struct SnpWriter<'a> {
    pub system_table: &'a mut SystemTable<Boot>,
    snp: Option<uefi::table::boot::ScopedProtocol<'a, uefi::proto::network::snp::SimpleNetwork>>,
}
#[cfg(feature = "snp")]
impl<'a> SnpWriter<'a> {
    pub fn new(system_table: &'a mut SystemTable<Boot>) -> Self { SnpWriter { system_table, snp: None } }
    fn ensure_open(&'a mut self) -> Option<&'a mut uefi::proto::network::snp::SimpleNetwork> {
        if self.snp.is_none() {
            let sel = unsafe { G_SNP_SEL_IDX }?;
            let h = unsafe { G_SNP_HANDLES[sel] };
            let bs = self.system_table.boot_services();
            match unsafe { bs.open_protocol_exclusive::<uefi::proto::network::snp::SimpleNetwork>(h) } {
                Ok(s) => { self.snp = Some(s); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_OPEN_OK).inc(); }
                Err(_) => { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_OPEN_FAIL).inc(); return None; }
            }
        }
        self.snp.as_mut().map(|p| &mut **p)
    }
}
#[cfg(feature = "snp")]
impl<'a> MigrWriter for SnpWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> usize {
        // Attempt to open selected SNP handle lazily and transmit MTU-sized frames.
        let snp = match self.ensure_open() { Some(s) => s, None => return 0 };
        // Ensure started/initialized
        let state = snp.state();
        if state == uefi::proto::network::snp::State::Stopped {
            if snp.start().is_ok() { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_START_OK).inc(); } else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_START_FAIL).inc(); return 0; }
        }
        if snp.state() == uefi::proto::network::snp::State::Started {
            if snp.initialize(0, 0).is_ok() { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_INIT_OK).inc(); } else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_INIT_FAIL).inc(); return 0; }
        }
        let mtu = core::cmp::min(net_get_mtu(), snp.mode().max_packet_size as usize);
        let cfg_dest = net_get_dest_mac();
        let ether = net_get_ethertype();
        // Build a MacAddress typed destination; default to broadcast if not configured
        let mut d = snp.mode().current_address;
        let use_bcast = cfg_dest.iter().all(|&b| b == 0);
        for i in 0..6 { d.addr[i] = if use_bcast { 0xFF } else { cfg_dest[i] }; }
        let mut off = 0usize; let mut frames = 0u64; let mut bytes = 0u64;
        while off < buf.len() {
            let take = core::cmp::min(buf.len() - off, mtu);
            let slice = &buf[off..off+take];
            // Transmit: HeaderSize=0, SNP provides header; pass dest/protocol.
            let res = unsafe { snp.transmit(None, None, slice, Some(&d), None, Some(ether)) };
            if res.is_err() { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_ERRS).inc(); break; }
            frames += 1; bytes += take as u64; off += take;
        }
        if frames > 0 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_FRAMES).add(frames); }
        if bytes > 0 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_BYTES).add(bytes); }
        off
    }
}

#[cfg(not(feature = "snp"))]
pub struct SnpWriter;
#[cfg(not(feature = "snp"))]
impl SnpWriter { pub fn new(_system_table: &mut SystemTable<Boot>) -> Self { SnpWriter } }
#[cfg(not(feature = "snp"))]
impl MigrWriter for SnpWriter {
    fn write(&mut self, buf: &[u8]) -> usize {
        // In non-SNP builds, simulate segmentation and account metrics for planning purposes.
        let mtu = net_get_mtu();
        let mut off = 0usize; let mut frames = 0u64;
        while off < buf.len() {
            let take = if buf.len() - off > mtu { mtu } else { buf.len() - off };
            frames += 1; crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_BYTES).add(take as u64);
            off += take;
        }
        if frames > 0 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_FRAMES).add(frames); }
        buf.len()
    }
}

pub fn chan_new(system_table: &SystemTable<Boot>, pages: usize) -> bool {
    let bytes = pages.saturating_mul(4096);
    if bytes == 0 { return false; }
    if let Some(p) = crate::mm::uefi::alloc_pages(system_table, pages, MemoryType::LOADER_DATA) {
        unsafe {
            core::ptr::write_bytes(p, 0, bytes);
            G_BUF = Some(Buffer { ptr: p, cap: bytes, wpos: 0, len: 0 });
        }
        return true;
    }
    false
}

pub fn chan_clear() {
    unsafe { if let Some(b) = G_BUF.as_mut() { b.wpos = 0; b.len = 0; } }
}

pub fn chan_stats() -> (usize, usize) {
    unsafe { if let Some(b) = G_BUF.as_ref() { return (b.len, b.cap); } }
    (0, 0)
}

pub fn chan_consume(mut bytes: usize) {
    unsafe {
        if let Some(b) = G_BUF.as_mut() {
            if bytes > b.len { bytes = b.len; }
            // Advance head by reducing length; start position is derived from wpos and len
            b.len -= bytes;
        }
    }
}

pub fn chan_dump(system_table: &mut SystemTable<Boot>, mut want: usize, hex: bool) {
    let stdout = system_table.stdout();
    unsafe {
        if let Some(b) = G_BUF.as_ref() {
            if want == 0 || want > b.len { want = b.len; }
            let start = if b.len < b.cap { (b.wpos + b.cap - b.len) % b.cap } else { (b.wpos + b.cap - b.len) % b.cap };
            let mut remaining = want;
            let mut pos = start;
            let mut line: [u8; 96] = [0; 96];
            while remaining > 0 {
                let take = core::cmp::min(remaining, b.cap - pos);
                let mut i = 0usize;
                while i < take {
                    if hex {
                        let mut n = 0usize;
                        let sub = core::cmp::min(16, take - i);
                        let chunk_ptr = b.ptr.add(pos + i);
                        for j in 0..sub { let v = core::ptr::read_volatile(chunk_ptr.add(j)) as u64; n += crate::util::format::u64_hex(v, &mut line[n..]); line[n] = b' '; n += 1; }
                        line[n] = b'\r'; n += 1; line[n] = b'\n'; n += 1;
                        let _ = stdout.write_str(core::str::from_utf8(&line[..n]).unwrap_or("\r\n"));
                        i += sub;
                    } else {
                        let sub = core::cmp::min(64, take - i);
                        let s = core::slice::from_raw_parts(b.ptr.add(pos + i), sub);
                        let _ = stdout.write_str(core::str::from_utf8(s).unwrap_or(""));
                        i += sub;
                    }
                }
                pos = (pos + take) % b.cap; remaining -= take;
            }
            return;
        }
    }
    let lang = crate::i18n::detect_lang(&*system_table);
    let stdout2 = system_table.stdout();
    let _ = stdout2.write_str(crate::i18n::t(lang, crate::i18n::key::MIG_NO_BUFFER));
}

/// Export a contiguous guest-physical range using identity mapping assumption.
/// For Console sink, prints hex lines; for Null sink, discards while counting bytes.
pub fn export_range(system_table: &mut SystemTable<Boot>, start_pa: u64, len: u64, sink: ExportSink) -> u64 {
    if len == 0 { return 0; }
    let mut remaining = len;
    let mut addr = start_pa;
    let stdout = system_table.stdout();
    let mut line: [u8; 96] = [0; 96];
    let mut total: u64 = 0;
    unsafe {
        while remaining > 0 {
            let chunk = if remaining > 16 { 16 } else { remaining as usize };
            match sink {
                ExportSink::Console => {
                    let mut n = 0;
                    // address prefix
                    for &b in b"0x" { line[n] = b; n += 1; }
                    n += crate::util::format::u64_hex(addr, &mut line[n..]);
                    for &b in b": " { line[n] = b; n += 1; }
                    // hex bytes
                    let mut i = 0;
                    while i < chunk { let v = read_volatile((addr as *const u8).add(i)); n += crate::util::format::u64_hex(v as u64, &mut line[n..]); line[n] = b' '; n += 1; i += 1; }
                    line[n] = b'\r'; n += 1; line[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&line[..n]).unwrap_or("\r\n"));
                }
                ExportSink::Buffer => {
                    // Emit raw bytes into channel buffer
                    let mut tmp = [0u8; 64];
                    let mut left = chunk;
                    let mut off = 0usize;
                    while left > 0 {
                        let take = core::cmp::min(left, tmp.len());
                        for j in 0..take { tmp[j] = core::ptr::read_volatile((addr as *const u8).add(off + j)); }
                        let _ = chan_write(&tmp[..take]);
                        off += take; left -= take;
                    }
                }
                ExportSink::Null => {
                    // Touch memory to simulate read side-effect without output
                    let mut i = 0usize; while i < chunk { let _ = read_volatile((addr as *const u8).add(i)); i += 1; }
                }
                ExportSink::Snp => {
                    // Treat as null for raw export path; framed network path is via send_dirty_pages.
                    let mut i = 0usize; while i < chunk { let _ = read_volatile((addr as *const u8).add(i)); i += 1; }
                }
            }
            addr = addr.wrapping_add(chunk as u64);
            remaining -= chunk as u64;
            total += chunk as u64;
        }
    }
    total
}

/// Run a pre-copy loop: scan dirty, copy pages, repeat. Returns stats.
pub fn precopy(system_table: &mut SystemTable<Boot>, max_rounds: u32, clear_each_round: bool, sink: ExportSink) -> (u32, u64, u64) {
    let st = unsafe { G_TRACKER.as_mut() };
    if st.is_none() { return (0, 0, 0); }
    let state = st.unwrap();
    let mut rounds_done = 0u32;
    let mut pages_copied = 0u64;
    let mut bytes_copied = 0u64;
    while rounds_done < max_rounds {
        state.bitmap.clear_all();
        let dirty = scan_round(clear_each_round);
        if dirty == 0 { rounds_done += 1; break; }
        // Copy pages marked dirty in this round
        state.bitmap.for_each_set(|page_idx| {
            let pa = page_idx << 12;
            // Simple zero-page elision
            let mut all_zero = true;
            unsafe {
                let mut off = 0usize;
                while off < 4096 {
                    if read_volatile((pa as *const u64).add(off / 8)) != 0 { all_zero = false; break; }
                    off += 8;
                }
            }
            if all_zero {
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc();
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096);
            } else {
                // Optional content hash elision to skip duplicates (lightweight rolling hash)
                let mut h: u64 = 1469598103934665603u64; // FNV offset basis
                unsafe {
                    let mut off = 0usize;
                    while off < 4096 {
                        let v = read_volatile((pa as *const u64).add(off / 8));
                        h ^= v; h = h.wrapping_mul(1099511628211u64);
                        off += 8;
                    }
                }
                // For prototype, treat zero hash only as special skip case
                if h == 0 {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc();
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096);
                } else {
                    bytes_copied += export_range(system_table, pa, 4096, sink);
                    pages_copied += 1;
                }
            }
        });
        rounds_done += 1;
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PRECOPY_ROUNDS).inc();
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PRECOPY_PAGES).add(dirty);
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_BYTES_TX).add(dirty * 4096);
        if dirty as u64 == 0 { break; }
    }
    (rounds_done, pages_copied, bytes_copied)
}

/// Plan only: run scan rounds without copying, reporting tentative metrics.
pub fn plan_dirty_runs(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let st = unsafe { G_TRACKER.as_mut() };
    if st.is_none() { let _ = stdout.write_str("migrate: no active tracker\r\n"); return; }
    let state = st.unwrap();
    state.bitmap.clear_all();
    let dirty = scan_round(false);
    let mut buf = [0u8; 64]; let mut n = 0;
    for &b in b"plan: dirty_pages=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(dirty as u32, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

/// Export dirty-set bytes for the current bitmap without framing, to selected sink.
pub fn export_dirty_runs(system_table: &mut SystemTable<Boot>, sink: ExportSink) -> (u32, u64, u64) {
    let st = unsafe { G_TRACKER.as_mut() };
    if st.is_none() { return (0, 0, 0); }
    let state = st.unwrap();
    // Do one non-clearing scan then export
    state.bitmap.clear_all();
    let dirty = scan_round(false);
    let mut pages = 0u64; let mut bytes = 0u64;
    state.bitmap.for_each_set(|page_idx| {
        let pa = page_idx << 12;
        pages += 1; bytes += export_range(system_table, pa, 4096, sink);
    });
    (1, pages, bytes)
}

fn stall_for_rate(system_table: &mut SystemTable<Boot>, bytes: usize, rate_kbps: u32) {
    if rate_kbps == 0 { return; }
    // microseconds = bytes * 1e6 / (rate_kbps * 1000)
    let us = ((bytes as u64) * 1000u64) / (rate_kbps as u64);
    if us > 0 { let _ = system_table.boot_services().stall(us as usize); }
}

/// Throttled variant of precopy with approximate rate control in KB/s.
pub fn precopy_throttled(system_table: &mut SystemTable<Boot>, max_rounds: u32, clear_each_round: bool, sink: ExportSink, rate_kbps: u32) -> (u32, u64, u64) {
    let st = unsafe { G_TRACKER.as_mut() };
    if st.is_none() { return (0, 0, 0); }
    let state = st.unwrap();
    let mut rounds_done = 0u32;
    let mut pages_copied = 0u64;
    let mut bytes_copied = 0u64;
    while rounds_done < max_rounds {
        state.bitmap.clear_all();
        let dirty = scan_round(clear_each_round);
        if dirty == 0 { rounds_done += 1; break; }
        state.bitmap.for_each_set(|page_idx| {
            let pa = page_idx << 12;
            let mut all_zero = true;
            unsafe {
                let mut off = 0usize;
                while off < 4096 {
                    if read_volatile((pa as *const u64).add(off / 8)) != 0 { all_zero = false; break; }
                    off += 8;
                }
            }
            if all_zero {
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc();
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096);
            } else {
                let mut h: u64 = 1469598103934665603u64;
                unsafe {
                    let mut off = 0usize;
                    while off < 4096 {
                        let v = read_volatile((pa as *const u64).add(off / 8));
                        h ^= v; h = h.wrapping_mul(1099511628211u64);
                        off += 8;
                    }
                }
                if h == 0 {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc();
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096);
                } else {
                    let wrote = export_range(system_table, pa, 4096, sink) as usize;
                    bytes_copied += wrote as u64;
                    pages_copied += 1;
                    stall_for_rate(system_table, wrote + core::mem::size_of::<FrameHeader>(), rate_kbps);
                }
            }
        });
        rounds_done += 1;
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PRECOPY_ROUNDS).inc();
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PRECOPY_PAGES).add(dirty);
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_BYTES_TX).add(dirty * 4096);
        if dirty as u64 == 0 { break; }
    }
    (rounds_done, pages_copied, bytes_copied)
}

pub fn txlog_dump(system_table: &mut SystemTable<Boot>, count: usize) {
    let stdout = system_table.stdout();
    unsafe {
        let total = if TX_WIDX > TX_LOG_CAP { TX_LOG_CAP } else { TX_WIDX };
        let n = if count == 0 || count > total { total } else { count };
        let start = TX_WIDX.saturating_sub(n);
        for idx in start..TX_WIDX {
            let e = TX_LOG[idx % TX_LOG_CAP];
            let mut buf = [0u8; 96]; let mut i = 0;
            for &b in b"txlog: kind=" { buf[i] = b; i += 1; }
            let k: &[u8] = match e.kind { TYP_PAGE => b"page", TYP_MANIFEST => b"manifest", TYP_CTRL => b"ctrl", _ => b"?" };
            for &b in k { buf[i] = b; i += 1; }
            for &b in b" seq=" { buf[i] = b; i += 1; }
            i += crate::firmware::acpi::u32_to_dec(e.seq, &mut buf[i..]);
            if e.kind == TYP_PAGE { for &b in b" page=" { buf[i] = b; i += 1; } i += crate::firmware::acpi::u32_to_dec(e.page_index as u32, &mut buf[i..]); }
            buf[i] = b'\r'; i += 1; buf[i] = b'\n'; i += 1;
            let _ = stdout.write_str(core::str::from_utf8(&buf[..i]).unwrap_or("\r\n"));
        }
    }
}

pub fn reset(system_table: &mut SystemTable<Boot>) {
    unsafe {
        G_SEQ = 1;
        TX_WIDX = 0;
        for i in 0..TX_LOG_CAP { TX_LOG[i] = TxEntry { kind: 0, seq: 0, page_index: 0 }; }
    }
    chan_clear();
    let _ = system_table; // placeholder to keep signature uniform
}

pub fn session_start(system_table: &SystemTable<Boot>) {
    let _ = crate::time::init_time(system_table);
    unsafe { SESSION_START_TSC = crate::time::rdtsc(); }
}

fn elapsed_us_since(start_tsc: u64, system_table: &SystemTable<Boot>) -> u64 {
    let hz = crate::time::tsc_hz();
    if hz == 0 || start_tsc == 0 { return 0; }
    let now = crate::time::rdtsc();
    let dt = now.wrapping_sub(start_tsc);
    (dt.saturating_mul(1_000_000)) / hz
}

pub fn session_elapsed(system_table: &mut SystemTable<Boot>) {
    let us = unsafe { elapsed_us_since(SESSION_START_TSC, system_table) };
    let stdout = system_table.stdout();
    let mut buf = [0u8; 64]; let mut n = 0;
    for &b in b"migrate: elapsed_us=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(us as u32, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn session_bw(system_table: &mut SystemTable<Boot>) {
    let us = unsafe { elapsed_us_since(SESSION_START_TSC, system_table) };
    let bytes = crate::obs::metrics::MIG_CB_WRITTEN_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let stdout = system_table.stdout();
    if us == 0 { let _ = stdout.write_str("migrate: bw unavailable\r\n"); return; }
    let kbps = (bytes.saturating_mul(1_000) / us) as u64; // KB/s approx (1KB=1000B)
    let mut buf = [0u8; 64]; let mut n = 0;
    for &b in b"migrate: kbps=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(kbps as u32, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn session_bw_net(system_table: &mut SystemTable<Boot>) {
    let us = unsafe { elapsed_us_since(SESSION_START_TSC, system_table) };
    let bytes = crate::obs::metrics::MIG_NET_TX_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let stdout = system_table.stdout();
    if us == 0 { let _ = stdout.write_str("migrate: bw_net unavailable\r\n"); return; }
    let kbps = (bytes.saturating_mul(1_000) / us) as u64; // KB/s approx (1KB=1000B)
    let mut buf = [0u8; 64]; let mut n = 0;
    for &b in b"migrate: kbps_net=" { buf[n] = b; n += 1; }
    n += crate::firmware::acpi::u32_to_dec(kbps as u32, &mut buf[n..]);
    buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
    let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
}

pub fn summary(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    // Collect counters
    let frames = crate::obs::metrics::MIG_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
    let raw_pages = crate::obs::metrics::MIG_RAW_PAGES.load(core::sync::atomic::Ordering::Relaxed);
    let comp_pages = crate::obs::metrics::MIG_COMPRESSED_PAGES.load(core::sync::atomic::Ordering::Relaxed);
    let manifests = crate::obs::metrics::MIG_MANIFESTS.load(core::sync::atomic::Ordering::Relaxed);
    let ctrl = crate::obs::metrics::MIG_CTRL_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
    let acks = crate::obs::metrics::MIG_ACKS.load(core::sync::atomic::Ordering::Relaxed);
    let naks = crate::obs::metrics::MIG_NAKS.load(core::sync::atomic::Ordering::Relaxed);
    let resent = crate::obs::metrics::MIG_RESEND_TRIGGERS.load(core::sync::atomic::Ordering::Relaxed);
    let bytes_tx = crate::obs::metrics::MIG_BYTES_TX.load(core::sync::atomic::Ordering::Relaxed);
    let cb_written = crate::obs::metrics::MIG_CB_WRITTEN_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let zero_saved = crate::obs::metrics::MIG_ZERO_BYTES_SAVED.load(core::sync::atomic::Ordering::Relaxed);
    let hash_saved = crate::obs::metrics::MIG_HASH_BYTES_SAVED.load(core::sync::atomic::Ordering::Relaxed);
    let rx_ok = crate::obs::metrics::MIG_RX_FRAMES_OK.load(core::sync::atomic::Ordering::Relaxed);
    let rx_bad = crate::obs::metrics::MIG_RX_FRAMES_BAD.load(core::sync::atomic::Ordering::Relaxed);
    let rx_bytes = crate::obs::metrics::MIG_RX_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let net_tx_bytes = crate::obs::metrics::MIG_NET_TX_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let net_tx_frames = crate::obs::metrics::MIG_NET_TX_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
    let net_open_ok = crate::obs::metrics::MIG_NET_OPEN_OK.load(core::sync::atomic::Ordering::Relaxed);
    let net_open_fail = crate::obs::metrics::MIG_NET_OPEN_FAIL.load(core::sync::atomic::Ordering::Relaxed);
    let net_start_ok = crate::obs::metrics::MIG_NET_START_OK.load(core::sync::atomic::Ordering::Relaxed);
    let net_start_fail = crate::obs::metrics::MIG_NET_START_FAIL.load(core::sync::atomic::Ordering::Relaxed);
    let net_init_ok = crate::obs::metrics::MIG_NET_INIT_OK.load(core::sync::atomic::Ordering::Relaxed);
    let net_init_fail = crate::obs::metrics::MIG_NET_INIT_FAIL.load(core::sync::atomic::Ordering::Relaxed);
    let net_tx_errs = crate::obs::metrics::MIG_NET_TX_ERRS.load(core::sync::atomic::Ordering::Relaxed);
    let dup = crate::obs::metrics::MIG_DUP_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
    let missing = crate::obs::metrics::MIG_MISSING_FRAMES.load(core::sync::atomic::Ordering::Relaxed);
    let last_seq = crate::obs::metrics::MIG_LAST_SEQ.load(core::sync::atomic::Ordering::Relaxed);
    let mut buf = [0u8; 160];
    let mut print = |label: &str, val: u64| {
        let mut n = 0;
        for &b in label.as_bytes() { buf[n] = b; n += 1; }
        n += crate::firmware::acpi::u32_to_dec(val as u32, &mut buf[n..]);
        buf[n] = b'\r'; n += 1; buf[n] = b'\n'; n += 1;
        let _ = stdout.write_str(core::str::from_utf8(&buf[..n]).unwrap_or("\r\n"));
    };
    print("summary: frames=", frames);
    print("summary: pages_raw=", raw_pages);
    print("summary: pages_comp=", comp_pages);
    print("summary: manifests=", manifests);
    print("summary: ctrl=", ctrl);
    print("summary: acks=", acks);
    print("summary: naks=", naks);
    print("summary: resend_triggers=", resent);
    print("summary: bytes_tx=", bytes_tx);
    print("summary: cb_written=", cb_written);
    print("summary: zero_saved=", zero_saved);
    print("summary: hash_saved=", hash_saved);
    print("summary: rx_ok=", rx_ok);
    print("summary: rx_bad=", rx_bad);
    print("summary: rx_bytes=", rx_bytes);
    print("summary: net_tx_bytes=", net_tx_bytes);
    print("summary: net_tx_frames=", net_tx_frames);
    print("summary: net_open_ok=", net_open_ok);
    print("summary: net_open_fail=", net_open_fail);
    print("summary: net_start_ok=", net_start_ok);
    print("summary: net_start_fail=", net_start_fail);
    print("summary: net_init_ok=", net_init_ok);
    print("summary: net_init_fail=", net_init_fail);
    print("summary: net_tx_errs=", net_tx_errs);
    print("summary: dup=", dup);
    print("summary: missing=", missing);
    print("summary: last_seq=", last_seq);
}

// ---- Simple framing and compression ----

#[repr(C, packed)]
struct FrameHeader {
    magic: [u8;4],   // 'Z','M','I','G'
    ver: u8,         // 1
    typ: u8,         // 1=page, 2=manifest
    flags: u16,      // bit0=compressed
    seq: u32,
    page_index: u64,
    payload_len: u32,
    crc32: u32,
}

const MAGIC: [u8;4] = *b"ZMIG";
const TYP_PAGE: u8 = 1;
const TYP_MANIFEST: u8 = 2;
const TYP_CTRL: u8 = 3;
const CTRL_ACK: u8 = 1;
const CTRL_NAK: u8 = 2;
const FLAG_COMP: u16 = 1u16 << 0;

fn rle_compress_page(pa: u64, out: &mut [u8]) -> Option<usize> {
    // Very simple RLE: (value:1, run_len:1) pairs per byte, 4096 -> worst 8192, but we bound using out.len()
    let mut w = 0usize;
    let mut i = 0usize;
    unsafe {
        while i < 4096 {
            let v = read_volatile((pa as *const u8).add(i));
            let mut run = 1usize;
            while i + run < 4096 && run < 255 {
                let nv = read_volatile((pa as *const u8).add(i + run));
                if nv != v { break; }
                run += 1;
            }
            if w + 2 > out.len() { return None; }
            out[w] = v; out[w+1] = run as u8; w += 2;
            i += run;
        }
    }
    Some(w)
}

fn frame_and_send_page(writer: &mut impl MigrWriter, page_index: u64, pa: u64, compress: bool, chunked: bool) -> (bool, usize) {
    // Try compression if requested
    let mut flags: u16 = 0;
    let mut payload_len: usize = 4096;
    let mut comp_buf_storage = [0u8; 8192];
    let payload_ptr: *const u8;
    if compress {
        if let Some(n) = rle_compress_page(pa, &mut comp_buf_storage) {
            if n < 4096 { flags |= FLAG_COMP; payload_len = n; payload_ptr = comp_buf_storage.as_ptr();
            } else { payload_ptr = pa as *const u8; }
        } else { payload_ptr = pa as *const u8; }
    } else {
        payload_ptr = pa as *const u8;
    }
    // Build header
    let mut hdr = FrameHeader { magic: MAGIC, ver: 1, typ: TYP_PAGE, flags, seq: 0, page_index, payload_len: payload_len as u32, crc32: 0 };
    let seq = unsafe { let s = G_SEQ; G_SEQ = G_SEQ.wrapping_add(1); s };
    hdr.seq = seq;
    hdr.crc32 = crate::util::crc32::crc32_ptr(payload_ptr, payload_len);
    // Send header then payload
    let hdr_bytes: &[u8] = unsafe { core::slice::from_raw_parts((&hdr as *const FrameHeader) as *const u8, core::mem::size_of::<FrameHeader>()) };
    if chunked { write_chunked(writer, hdr_bytes); } else { let _ = writer.write(hdr_bytes); }
    let payload_bytes: &[u8] = unsafe { core::slice::from_raw_parts(payload_ptr, payload_len) };
    if chunked { write_chunked(writer, payload_bytes); } else { let _ = writer.write(payload_bytes); }
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_FRAMES).inc();
    if (flags & FLAG_COMP) != 0 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_COMPRESSED_PAGES).inc(); }
    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RAW_PAGES).inc(); }
    unsafe { tx_log_append(TYP_PAGE, seq, page_index); }
    ((flags & FLAG_COMP) != 0, payload_len)
}

fn frame_and_send_manifest(writer: &mut impl MigrWriter, pages: u64, bytes: u64, chunked: bool) {
    let mut body = [0u8; 16];
    // pages (8) + bytes (8) little-endian
    body[0] = (pages & 0xFF) as u8; body[1] = ((pages >> 8) & 0xFF) as u8; body[2] = ((pages >> 16) & 0xFF) as u8; body[3] = ((pages >> 24) & 0xFF) as u8;
    body[4] = ((pages >> 32) & 0xFF) as u8; body[5] = ((pages >> 40) & 0xFF) as u8; body[6] = ((pages >> 48) & 0xFF) as u8; body[7] = ((pages >> 56) & 0xFF) as u8;
    body[8] = (bytes & 0xFF) as u8; body[9] = ((bytes >> 8) & 0xFF) as u8; body[10] = ((bytes >> 16) & 0xFF) as u8; body[11] = ((bytes >> 24) & 0xFF) as u8;
    body[12] = ((bytes >> 32) & 0xFF) as u8; body[13] = ((bytes >> 40) & 0xFF) as u8; body[14] = ((bytes >> 48) & 0xFF) as u8; body[15] = ((bytes >> 56) & 0xFF) as u8;
    let mut hdr = FrameHeader { magic: MAGIC, ver: 1, typ: TYP_MANIFEST, flags: 0, seq: 0, page_index: 0, payload_len: 16, crc32: 0 };
    let seq = unsafe { let s = G_SEQ; G_SEQ = G_SEQ.wrapping_add(1); s };
    hdr.seq = seq;
    hdr.crc32 = crate::util::crc32::crc32(&body);
    let hdr_bytes: &[u8] = unsafe { core::slice::from_raw_parts((&hdr as *const FrameHeader) as *const u8, core::mem::size_of::<FrameHeader>()) };
    if chunked { write_chunked(writer, hdr_bytes); } else { let _ = writer.write(hdr_bytes); }
    if chunked { write_chunked(writer, &body); } else { let _ = writer.write(&body); }
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_MANIFESTS).inc();
    unsafe { tx_log_append(TYP_MANIFEST, seq, 0); }
}

#[inline(always)]
fn page_skip_reason(pa: u64) -> Option<u8> {
    let mut all_zero = true;
    unsafe {
        let mut off = 0usize;
        while off < 4096 {
            if read_volatile((pa as *const u64).add(off / 8)) != 0 { all_zero = false; break; }
            off += 8;
        }
    }
    if all_zero { return Some(1); }
    let mut h: u64 = 1469598103934665603u64;
    unsafe {
        let mut off = 0usize;
        while off < 4096 {
            let v = read_volatile((pa as *const u64).add(off / 8));
            h ^= v; h = h.wrapping_mul(1099511628211u64);
            off += 8;
        }
    }
    if h == 0 { return Some(2); }
    None
}

pub fn send_dirty_pages(system_table: &mut SystemTable<Boot>, compress: bool, sink: ExportSink) -> (u64, u64, u64) {
    let st = unsafe { G_TRACKER.as_ref() };
    if st.is_none() { return (0, 0, 0); }
    let state = st.unwrap();
    let mut frames = 0u64; let mut pages = 0u64; let mut bytes = 0u64;
    // Choose writer
    match sink {
        ExportSink::Console => {
            let mut w = ConsoleWriter { system_table };
            state.bitmap.for_each_set(|page_idx| {
                let pa = page_idx << 12;
                if let Some(r) = page_skip_reason(pa) {
                    if r == 1 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096); }
                    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096); }
                    return;
                }
            let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, true);
                frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
            });
            // Trailer manifest
            frame_and_send_manifest(&mut w, pages, bytes, true);
        }
        ExportSink::Buffer => {
            let mut w = BufferWriter;
            state.bitmap.for_each_set(|page_idx| {
                let pa = page_idx << 12;
                if let Some(r) = page_skip_reason(pa) {
                    if r == 1 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096); }
                    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096); }
                    return;
                }
                let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, true);
                frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
            });
            frame_and_send_manifest(&mut w, pages, bytes, true);
        }
        ExportSink::Null => {
            let mut w = NullWriter;
            state.bitmap.for_each_set(|page_idx| {
                let pa = page_idx << 12;
                if let Some(r) = page_skip_reason(pa) {
                    if r == 1 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096); }
                    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096); }
                    return;
                }
                let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, true);
                frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
            });
            frame_and_send_manifest(&mut w, pages, bytes, true);
        }
        ExportSink::Snp => {
            let mut w = SnpWriter::new(system_table);
            state.bitmap.for_each_set(|page_idx| {
                let pa = page_idx << 12;
                if let Some(r) = page_skip_reason(pa) {
                    if r == 1 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096); }
                    else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096); }
                    return;
                }
                // Do not chunk at MIG frame level. Let SnpWriter segment into L2 frames internally.
                let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, false);
                frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
            });
            frame_and_send_manifest(&mut w, pages, bytes, false);
        }
        ExportSink::Virtio => {
            #[cfg(feature = "virtio-net")]
            {
                let mut w = VirtioNetWriter { system_table };
                state.bitmap.for_each_set(|page_idx| {
                    let pa = page_idx << 12;
                    if let Some(r) = page_skip_reason(pa) {
                        if r == 1 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ZERO_BYTES_SAVED).add(4096); }
                        else { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_SKIPPED).inc(); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_HASH_BYTES_SAVED).add(4096); }
                        return;
                    }
                    let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, false);
                    frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                });
                frame_and_send_manifest(&mut w, pages, bytes, false);
            }
            #[cfg(not(feature = "virtio-net"))]
            {
                let mut w = NullWriter;
                state.bitmap.for_each_set(|page_idx| {
                    let pa = page_idx << 12;
                    let (_comp, plen) = frame_and_send_page(&mut w, page_idx, pa, compress, true);
                    frames += 1; pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                });
                frame_and_send_manifest(&mut w, pages, bytes, true);
            }
        }
    }
    bytes
        .checked_add(0)
        .unwrap_or(bytes);
    (frames, pages, bytes)
}

pub fn resend_from(system_table: &mut SystemTable<Boot>, from_seq: u32, max_count: usize, compress: bool, sink: ExportSink) -> (u64, u64) {
    let mut frames = 0u64; let mut bytes = 0u64; let mut sent_pages = 0u64;
    match sink {
        ExportSink::Console => {
            let mut w = ConsoleWriter { system_table };
            unsafe {
                let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                let end = TX_WIDX;
                while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                    let e = TX_LOG[idx % TX_LOG_CAP];
                    idx += 1;
                    if e.seq < from_seq { continue; }
                    if e.kind == TYP_PAGE {
                        let pa = e.page_index << 12;
                        let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, true);
                        frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                    }
                }
                // send a trailing manifest for the resend window
                frame_and_send_manifest(&mut w, sent_pages, bytes, true);
            }
        }
        ExportSink::Buffer => {
            let mut w = BufferWriter;
            unsafe {
                let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                let end = TX_WIDX;
                while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                    let e = TX_LOG[idx % TX_LOG_CAP];
                    idx += 1;
                    if e.seq < from_seq { continue; }
                    if e.kind == TYP_PAGE {
                        let pa = e.page_index << 12;
                        let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, true);
                        frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                    }
                }
                frame_and_send_manifest(&mut w, sent_pages, bytes, true);
            }
        }
        ExportSink::Null => {
            let mut w = NullWriter;
            unsafe {
                let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                let end = TX_WIDX;
                while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                    let e = TX_LOG[idx % TX_LOG_CAP];
                    idx += 1;
                    if e.seq < from_seq { continue; }
                    if e.kind == TYP_PAGE {
                        let pa = e.page_index << 12;
                        let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, true);
                        frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                    }
                }
                frame_and_send_manifest(&mut w, sent_pages, bytes, true);
            }
        }
        ExportSink::Snp => {
            let mut w = SnpWriter::new(system_table);
            unsafe {
                let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                let end = TX_WIDX;
                while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                    let e = TX_LOG[idx % TX_LOG_CAP];
                    idx += 1;
                    if e.seq < from_seq { continue; }
                    if e.kind == TYP_PAGE {
                        let pa = e.page_index << 12;
                        let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, false);
                        frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                    }
                }
                frame_and_send_manifest(&mut w, sent_pages, bytes, false);
            }
        }
        ExportSink::Virtio => {
            #[cfg(feature = "virtio-net")]
            {
                let mut w = VirtioNetWriter { system_table };
                unsafe {
                    let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                    let end = TX_WIDX;
                    while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                        let e = TX_LOG[idx % TX_LOG_CAP];
                        idx += 1;
                        if e.seq < from_seq { continue; }
                        if e.kind == TYP_PAGE {
                            let pa = e.page_index << 12;
                            let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, false);
                            frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                        }
                    }
                    frame_and_send_manifest(&mut w, sent_pages, bytes, false);
                }
            }
            #[cfg(not(feature = "virtio-net"))]
            {
                let mut w = NullWriter;
                unsafe {
                    let mut idx = if TX_WIDX > TX_LOG_CAP { TX_WIDX - TX_LOG_CAP } else { 0 };
                    let end = TX_WIDX;
                    while idx < end && (max_count == 0 || (frames as usize) < max_count) {
                        let e = TX_LOG[idx % TX_LOG_CAP];
                        idx += 1;
                        if e.seq < from_seq { continue; }
                        if e.kind == TYP_PAGE {
                            let pa = e.page_index << 12;
                            let (_comp, plen) = frame_and_send_page(&mut w, e.page_index, pa, compress, true);
                            frames += 1; sent_pages += 1; bytes += (core::mem::size_of::<FrameHeader>() + plen) as u64;
                        }
                    }
                    frame_and_send_manifest(&mut w, sent_pages, bytes, true);
                }
            }
        }
    }
    (frames, bytes)
}

fn frame_and_send_ctrl(writer: &mut impl MigrWriter, code: u8, seq_to_ref: u32) {
    let body = [code, (seq_to_ref & 0xFF) as u8, ((seq_to_ref >> 8) & 0xFF) as u8, ((seq_to_ref >> 16) & 0xFF) as u8, ((seq_to_ref >> 24) & 0xFF) as u8];
    let mut hdr = FrameHeader { magic: MAGIC, ver: 1, typ: TYP_CTRL, flags: 0, seq: 0, page_index: 0, payload_len: body.len() as u32, crc32: 0 };
    let seq = unsafe { let s = G_SEQ; G_SEQ = G_SEQ.wrapping_add(1); s };
    hdr.seq = seq;
    hdr.crc32 = crate::util::crc32::crc32(&body);
    let hdr_bytes: &[u8] = unsafe { core::slice::from_raw_parts((&hdr as *const FrameHeader) as *const u8, core::mem::size_of::<FrameHeader>()) };
    write_chunked(writer, hdr_bytes);
    write_chunked(writer, &body);
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CTRL_FRAMES).inc();
    if code == CTRL_ACK { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_ACKS).inc(); }
    if code == CTRL_NAK { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NAKS).inc(); }
}

pub fn send_ctrl(system_table: &mut SystemTable<Boot>, ack: bool, seq_to_ref: u32, sink: ExportSink) {
    match sink {
        ExportSink::Console => { let mut w = ConsoleWriter { system_table }; frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
        ExportSink::Buffer => { let mut w = BufferWriter; frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
        ExportSink::Null => { let mut w = NullWriter; frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
        ExportSink::Snp => { let mut w = SnpWriter::new(system_table); frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
        ExportSink::Virtio => {
            #[cfg(feature = "virtio-net")]
            { let mut w = VirtioNetWriter { system_table }; frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
            #[cfg(not(feature = "virtio-net"))]
            { let mut w = NullWriter; frame_and_send_ctrl(&mut w, if ack { CTRL_ACK } else { CTRL_NAK }, seq_to_ref); }
        }
    }
}

pub fn chan_handle_ctrl(system_table: &mut SystemTable<Boot>, limit: usize) {
    unsafe {
        if let Some(b) = G_BUF.as_ref() {
            let start = if b.len == 0 { 0 } else { (b.wpos + b.cap - b.len) % b.cap };
            let mut cur = ChanCursor { ptr: b.ptr as *const u8, cap: b.cap, pos: start, remaining: b.len };
            let mut handled = 0usize;
            let mut hb = [0u8; 32];
            while cur.remaining >= size_of::<FrameHeader>() && (limit == 0 || handled < limit) {
                let mut hdr_bytes = [0u8; 32];
                let mut tmp = cur;
                if !tmp.read_into(&mut hdr_bytes) { break; }
                if &hdr_bytes[0..4] != &MAGIC { let _ = cur.skip(1); continue; }
                let typ = hdr_bytes[5];
                let payload_len = le_u32(&hdr_bytes[20..24]) as usize;
                let _ = cur.read_into(&mut hb[..size_of::<FrameHeader>()]);
                if cur.remaining < payload_len { break; }
                if typ == TYP_CTRL {
                    let mut body = [0u8; 8];
                    let take = if payload_len <= body.len() { payload_len } else { body.len() };
                    if !cur.read_into(&mut body[..take]) { break; }
                    if payload_len > take { let _ = cur.skip(payload_len - take); }
                    let code = body[0];
                    let seq = le_u32(&body[1..5]);
                // Action on NAK: trigger resend from seq to configured sink
                if code == CTRL_NAK {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RESEND_TRIGGERS).inc();
                    let sink = ctrl_get_resend_sink();
                    let (_f,_b) = resend_from(system_table, seq, 0, false, sink);
                    if ctrl_get_auto_nak() { send_ctrl(system_table, false, seq, sink); }
                }
                    if code == CTRL_ACK {
                    if ctrl_get_auto_ack() { let sink = ctrl_get_resend_sink(); send_ctrl(system_table, true, seq, sink); }
                    }
                    handled += 1;
                    let mut out = [0u8; 64]; let mut n = 0;
                    for &bch in b"ctrl: " { out[n] = bch; n += 1; }
                    let s = if code == CTRL_ACK { b"ack" } else { b"nak" };
                    for &bch in s { out[n] = bch; n += 1; }
                    for &bch in b" seq=" { out[n] = bch; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(seq, &mut out[n..]);
                    out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                    let stdout = system_table.stdout();
                    let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                } else {
                    let _ = cur.skip(payload_len);
                }
            }
            return;
        }
    }
    let lang = crate::i18n::detect_lang(&*system_table);
    let stdout = system_table.stdout();
    let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::MIG_NO_BUFFER));
}

#[inline(always)]
fn write_chunked(writer: &mut impl MigrWriter, buf: &[u8]) -> usize {
    let mut written = 0usize;
    let chunk = unsafe { if G_CHUNK == 0 { 1500 } else { G_CHUNK } };
    let mut off = 0usize;
    while off < buf.len() {
        let take = core::cmp::min(chunk, buf.len() - off);
        written += writer.write(&buf[off..off+take]);
        off += take;
    }
    written
}

pub fn set_chunk_size(bytes: usize) { unsafe { G_CHUNK = if bytes == 0 { 1500 } else { bytes }; } }
pub fn get_chunk_size() -> usize { unsafe { if G_CHUNK == 0 { 1500 } else { G_CHUNK } } }

// ---- Persist simple migration configuration in UEFI variables ----

const VAR_NS: VariableVendor = VariableVendor::GLOBAL_VARIABLE; // Use EFI_GLOBAL for simplicity

pub fn cfg_save(system_table: &SystemTable<Boot>) {
    let rs = system_table.runtime_services();
    // Save chunk size and next seq
    let chunk = get_chunk_size() as u32;
    let seq = unsafe { G_SEQ };
    let mut buf = [0u8; 8];
    buf[0] = (chunk & 0xFF) as u8; buf[1] = ((chunk >> 8) & 0xFF) as u8; buf[2] = ((chunk >> 16) & 0xFF) as u8; buf[3] = ((chunk >> 24) & 0xFF) as u8;
    buf[4] = (seq & 0xFF) as u8; buf[5] = ((seq >> 8) & 0xFF) as u8; buf[6] = ((seq >> 16) & 0xFF) as u8; buf[7] = ((seq >> 24) & 0xFF) as u8;
    let _ = rs.set_variable(uefi::cstr16!("ZerovisorMigCfg"), &VAR_NS, uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS, &buf);
    // Save network config separately: dest MAC (6) + MTU (4) + EtherType (2) + resend sink (1) + auto flags (2) + default sink (1)
    let mac = net_get_dest_mac();
    let mtu = net_get_mtu() as u32;
    let et = net_get_ethertype() as u16;
    let rsink = sink_to_u8(ctrl_get_resend_sink());
    let aack = if ctrl_get_auto_ack() { 1u8 } else { 0u8 };
    let anak = if ctrl_get_auto_nak() { 1u8 } else { 0u8 };
    let def_sink = sink_to_u8(get_default_sink());
    let mut nbuf = [0u8; 16];
    nbuf[0..6].copy_from_slice(&mac);
    nbuf[6] = (mtu & 0xFF) as u8; nbuf[7] = ((mtu >> 8) & 0xFF) as u8; nbuf[8] = ((mtu >> 16) & 0xFF) as u8; nbuf[9] = ((mtu >> 24) & 0xFF) as u8;
    nbuf[10] = (et & 0xFF) as u8; nbuf[11] = ((et >> 8) & 0xFF) as u8;
    nbuf[12] = rsink;
    nbuf[13] = aack; nbuf[14] = anak;
    nbuf[15] = def_sink;
    let _ = rs.set_variable(uefi::cstr16!("ZerovisorMigNet"), &VAR_NS, uefi::table::runtime::VariableAttributes::BOOTSERVICE_ACCESS, &nbuf);
    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CFG_SAVES).inc();
}

pub fn cfg_load(system_table: &SystemTable<Boot>) {
    let rs = system_table.runtime_services();
    let mut buf = [0u8; 16];
    if let Ok((data, _attrs)) = rs.get_variable(uefi::cstr16!("ZerovisorMigCfg"), &VAR_NS, &mut buf) {
        if data.len() >= 8 {
            let chunk = (data[0] as u32) | ((data[1] as u32) << 8) | ((data[2] as u32) << 16) | ((data[3] as u32) << 24);
            let seq = (data[4] as u32) | ((data[5] as u32) << 8) | ((data[6] as u32) << 16) | ((data[7] as u32) << 24);
            set_chunk_size(chunk as usize);
            unsafe { G_SEQ = seq; }
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CFG_LOADS).inc();
        }
    }
    // Load network config if present
    let mut nbuf = [0u8; 16];
    if let Ok((data, _attrs)) = rs.get_variable(uefi::cstr16!("ZerovisorMigNet"), &VAR_NS, &mut nbuf) {
        if data.len() >= 10 {
            let mut mac = [0u8;6]; mac.copy_from_slice(&data[0..6]);
            let mtu = (data[6] as u32) | ((data[7] as u32) << 8) | ((data[8] as u32) << 16) | ((data[9] as u32) << 24);
            net_set_dest_mac(mac);
            net_set_mtu(mtu as usize);
            if data.len() >= 12 {
                let et = (data[10] as u16) | ((data[11] as u16) << 8);
                net_set_ethertype(et);
            }
            if data.len() >= 13 { ctrl_set_resend_sink(u8_to_sink(data[12])); }
            if data.len() >= 14 { ctrl_set_auto_ack(data[13] != 0); }
            if data.len() >= 15 { ctrl_set_auto_nak(data[14] != 0); }
            if data.len() >= 16 { set_default_sink(u8_to_sink(data[15])); }
        }
    }
}

// ---- Channel frame verification ----

#[derive(Clone, Copy)]
struct ChanCursor {
    ptr: *const u8,
    cap: usize,
    pos: usize,
    remaining: usize,
}

impl ChanCursor {
    unsafe fn read_into(&mut self, dst: &mut [u8]) -> bool {
        if self.remaining < dst.len() { return false; }
        let first = core::cmp::min(dst.len(), self.cap - self.pos);
        core::ptr::copy_nonoverlapping(self.ptr.add(self.pos), dst.as_mut_ptr(), first);
        self.pos = (self.pos + first) % self.cap; self.remaining -= first;
        if first < dst.len() {
            let rest = dst.len() - first;
            core::ptr::copy_nonoverlapping(self.ptr.add(self.pos), dst.as_mut_ptr().add(first), rest);
            self.pos = (self.pos + rest) % self.cap; self.remaining -= rest;
        }
        true
    }
    unsafe fn skip(&mut self, n: usize) -> bool {
        if self.remaining < n { return false; }
        let adv = n % self.cap;
        self.pos = (self.pos + adv) % self.cap; self.remaining -= n; true
    }
    unsafe fn checksum(&self, mut len: usize) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        let mut pos = self.pos; let mut rem = self.remaining;
        let mut l = len;
        let mut tmp = [0u8; 64];
        while l > 0 && rem > 0 {
            let take = core::cmp::min(core::cmp::min(l, tmp.len()), self.cap - pos);
            core::ptr::copy_nonoverlapping(self.ptr.add(pos), tmp.as_mut_ptr(), take);
            c = crate::util::crc32::crc32_update(!c, &tmp[..take]);
            c = !c;
            pos = (pos + take) % self.cap; rem -= take; l -= take;
        }
        !c
    }
}

fn le_u32(b: &[u8]) -> u32 { (b[0] as u32) | ((b[1] as u32) << 8) | ((b[2] as u32) << 16) | ((b[3] as u32) << 24) }
fn le_u64(b: &[u8]) -> u64 { (le_u32(&b[0..4]) as u64) | ((le_u32(&b[4..8]) as u64) << 32) }

pub fn chan_verify(system_table: &mut SystemTable<Boot>, limit: usize, quiet: bool) {
    chan_verify_ex(system_table, limit, quiet, false);
}

pub fn chan_verify_ex(system_table: &mut SystemTable<Boot>, limit: usize, quiet: bool, auto_ctrl: bool) {
    let stdout = system_table.stdout();
    unsafe {
        if let Some(b) = G_BUF.as_ref() {
            let start = if b.len == 0 { 0 } else { (b.wpos + b.cap - b.len) % b.cap };
            let mut cur = ChanCursor { ptr: b.ptr as *const u8, cap: b.cap, pos: start, remaining: b.len };
            let mut frames = 0usize; let mut ok = 0usize; let mut bad = 0usize;
            let mut expected_seq: u32 = 0;
            let mut hb = [0u8; 32];
            while cur.remaining >= size_of::<FrameHeader>() && (limit == 0 || frames < limit) {
                // Peek header
                let mut hdr_bytes = [0u8; 32];
                let mut tmp = cur; // copy
                if !tmp.read_into(&mut hdr_bytes) { break; }
                if &hdr_bytes[0..4] != &MAGIC {
                    // realign by one byte
                    if !cur.skip(1) { break; }
                    continue;
                }
                let ver = hdr_bytes[4]; let typ = hdr_bytes[5];
                let flags = (hdr_bytes[6] as u16) | ((hdr_bytes[7] as u16) << 8);
                let seq = le_u32(&hdr_bytes[8..12]);
                let page_index = le_u64(&hdr_bytes[12..20]);
                let payload_len = le_u32(&hdr_bytes[20..24]) as usize;
                let crc = le_u32(&hdr_bytes[24..28]);
                // Consume header
                let _ = cur.read_into(&mut hb[..size_of::<FrameHeader>()]);
                if cur.remaining < payload_len { break; }
                let ccalc = cur.checksum(payload_len);
                let _ = cur.skip(payload_len);
                let good = ccalc == crc;
                frames += 1; if good { ok += 1; } else { bad += 1; }
                // Track simple ordering diagnostics
                if expected_seq != 0 && seq == expected_seq { /* in order */ }
                else if expected_seq != 0 && seq < expected_seq {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_DUP_FRAMES).inc();
                    if auto_ctrl { send_ctrl(system_table, true, seq, ctrl_get_resend_sink()); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CTRL_AUTO_ACK_SENT).inc(); }
                } else if expected_seq != 0 && seq > expected_seq {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_MISSING_FRAMES).inc();
                    if auto_ctrl { send_ctrl(system_table, false, expected_seq, ctrl_get_resend_sink()); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CTRL_AUTO_NAK_SENT).inc(); }
                }
                expected_seq = seq.wrapping_add(1);
                crate::obs::metrics::MIG_LAST_SEQ.store(seq as u64, core::sync::atomic::Ordering::Relaxed);
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_BYTES).add((size_of::<FrameHeader>() + payload_len) as u64);
                if good { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_OK).inc(); }
                else {
                    crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_BAD).inc();
                    if auto_ctrl { send_ctrl(system_table, false, seq, ctrl_get_resend_sink()); crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_CTRL_AUTO_NAK_SENT).inc(); }
                }
                if !quiet {
                    let mut out = [0u8; 128]; let mut n = 0;
                    for &bch in b"verify: typ=" { out[n] = bch; n += 1; }
            let t: &[u8] = if typ == TYP_MANIFEST { b"manifest" } else { b"page" };
                    for &bch in t { out[n] = bch; n += 1; }
                    for &bch in b" seq=" { out[n] = bch; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(seq, &mut out[n..]);
                    if typ != TYP_MANIFEST {
                        for &bch in b" page=" { out[n] = bch; n += 1; }
                        n += crate::firmware::acpi::u32_to_dec(page_index as u32, &mut out[n..]);
                    }
                    for &bch in b" len=" { out[n] = bch; n += 1; }
                    n += crate::firmware::acpi::u32_to_dec(payload_len as u32, &mut out[n..]);
                    for &bch in b" " { out[n] = bch; n += 1; }
            let s: &[u8] = if good { b"ok" } else { b"bad" };
                    for &bch in s { out[n] = bch; n += 1; }
                    out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
                    let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
                }
            }
            let mut out = [0u8; 96]; let mut n = 0;
            for &bch in b"verify: frames=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(frames as u32, &mut out[n..]);
            for &bch in b" ok=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(ok as u32, &mut out[n..]);
            for &bch in b" bad=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(bad as u32, &mut out[n..]);
            out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
            let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
            return;
        }
    }
    let _ = system_table.stdout().write_str("migrate: no buffer\r\n");
}

// ---- Replay (decompress and reconstruct) to a scratch buffer ----

pub fn replay_to_buffer(system_table: &mut SystemTable<Boot>, max_pages: usize) {
    let stdout = system_table.stdout();
    unsafe {
        if let Some(b) = G_BUF.as_ref() {
            // Allocate a scratch page for reconstructed data
            let scratch = crate::mm::uefi::alloc_pages(system_table, 1, MemoryType::LOADER_DATA);
            if scratch.is_none() { let _ = stdout.write_str("replay: alloc failed\r\n"); return; }
            let scratch = scratch.unwrap();
            let start = if b.len == 0 { 0 } else { (b.wpos + b.cap - b.len) % b.cap };
            let mut cur = ChanCursor { ptr: b.ptr as *const u8, cap: b.cap, pos: start, remaining: b.len };
            let mut pages_done = 0usize; let mut bytes_done = 0usize; let mut errors = 0usize;
            let mut hdr = [0u8; 32];
            while cur.remaining >= size_of::<FrameHeader>() && (max_pages == 0 || pages_done < max_pages) {
                // Peek alignment
                    let mut tmp = cur; if !tmp.read_into(&mut hdr) { break; }
                    if &hdr[0..4] != &MAGIC { let _ = cur.skip(1); continue; }
                let payload_len = le_u32(&hdr[20..24]) as usize;
                let flags = (hdr[6] as u16) | ((hdr[7] as u16) << 8);
                    let _ = cur.read_into(&mut hdr);
                // Bounds
                if cur.remaining < payload_len { break; }
                // Reconstruct into scratch: either raw 4KiB or RLE expand
                if (flags & FLAG_COMP) == 0 {
                    // Raw; copy up to 4KiB
                    let to_read = core::cmp::min(4096, payload_len);
                    let mut copied = 0usize;
                    while copied < to_read {
                        let take = core::cmp::min(to_read - copied, 64);
                        let mut buf = [0u8; 64];
                        if !cur.read_into(&mut buf[..take]) { errors += 1; break; }
                        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), scratch.add(copied), take); }
                        copied += take;
                    }
                        if payload_len > to_read { let _ = cur.skip(payload_len - to_read); }
                } else {
                    // RLE decompress
                    let mut wrote = 0usize;
                    while wrote < 4096 {
                        if cur.remaining < 2 { errors += 1; break; }
                        let mut pair = [0u8; 2];
                        if !cur.read_into(&mut pair) { errors += 1; break; }
                        let v = pair[0]; let run = pair[1] as usize;
                        if wrote + run > 4096 { errors += 1; break; }
                        unsafe { core::ptr::write_bytes(scratch.add(wrote), v, run); }
                        wrote += run;
                    }
                    // If extra payload due to over-provision, skip remainder
                    if cur.remaining > 0 && wrote == 4096 {
                        // No-op; we consumed exactly the page payload encoded
                    }
                }
                pages_done += 1; bytes_done += 4096;
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_REPLAY_PAGES).inc();
                crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_REPLAY_BYTES).add(4096);
            }
            crate::mm::uefi::free_pages(system_table, scratch, 1);
            let mut out = [0u8; 96]; let mut n = 0;
            for &bch in b"replay: pages=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(pages_done as u32, &mut out[n..]);
            for &bch in b" bytes=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(bytes_done as u32, &mut out[n..]);
            for &bch in b" errors=" { out[n] = bch; n += 1; }
            n += crate::firmware::acpi::u32_to_dec(errors as u32, &mut out[n..]);
            out[n] = b'\r'; n += 1; out[n] = b'\n'; n += 1;
            let _ = stdout.write_str(core::str::from_utf8(&out[..n]).unwrap_or("\r\n"));
            if errors > 0 { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_REPLAY_ERRORS).add(errors as u64); }
            return;
        }
    }
    let _ = system_table.stdout().write_str("replay: no buffer\r\n");
}

// ---- EPT/NPT scanning helpers ----

// EPT common bit definitions (subset) – includes A/D flags when supported.
const EPT_R: u64 = 1 << 0;
const EPT_W: u64 = 1 << 1;
const EPT_X: u64 = 1 << 2;
const EPT_IGNORE_PAT: u64 = 1 << 6;
const EPT_PAGE_SIZE: u64 = 1 << 7;
const EPT_ACCESSED: u64 = 1 << 8; // A flag (requires EPT A/D enable)
const EPT_DIRTY: u64 = 1 << 9;    // D flag (requires EPT A/D enable)

fn scan_ept(pml4_phys: u64, limit_bytes: u64, bitmap: &mut DirtyBitmap, clear_ad: bool) -> u64 {
    if pml4_phys == 0 { return 0; }
    let mut dirty_pages: u64 = 0;
    let pml4 = (pml4_phys & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
    let mut addr: u64 = 0;
    unsafe {
        // Walk top-down, honoring large page leaves and mapping sizes.
        while addr < limit_bytes {
            let l4 = ((addr >> 39) & 0x1FF) as isize;
            let pml4e = read_volatile(pml4.offset(l4));
            if pml4e & EPT_R == 0 { addr = addr.saturating_add(1u64 << 39); continue; }
            let pdpt = (pml4e & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l3i = ((addr >> 30) & 0x1FF) as isize;
            let pdpte = read_volatile(pdpt.offset(l3i));
            if pdpte & EPT_R == 0 { addr = addr.saturating_add(1u64 << 30); continue; }
            // 1GiB leaf
            if (pdpte & EPT_PAGE_SIZE) != 0 {
                let page_count = 1u64 << (30 - 12); // 1GiB / 4KiB
                if (pdpte & EPT_DIRTY) != 0 { // treat as fully dirty when D is set
                    for i in 0..page_count { bitmap.set_bit(((addr >> 12) + i) as u64); }
                    dirty_pages += page_count;
                    if clear_ad { write_volatile(pdpt.offset(l3i), pdpte & !(EPT_DIRTY | EPT_ACCESSED)); }
                }
                addr = ((addr >> 30) + 1) << 30;
                continue;
            }
            let pd = (pdpte & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l2i = ((addr >> 21) & 0x1FF) as isize;
            let pde = read_volatile(pd.offset(l2i));
            if pde & EPT_R == 0 { addr = addr.saturating_add(1u64 << 21); continue; }
            // 2MiB leaf
            if (pde & EPT_PAGE_SIZE) != 0 {
                let page_count = 1u64 << (21 - 12);
                if (pde & EPT_DIRTY) != 0 {
                    for i in 0..page_count { bitmap.set_bit(((addr >> 12) + i) as u64); }
                    dirty_pages += page_count;
                    if clear_ad { write_volatile(pd.offset(l2i), pde & !(EPT_DIRTY | EPT_ACCESSED)); }
                }
                addr = ((addr >> 21) + 1) << 21;
                continue;
            }
            let pt = (pde & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let mut l1i = ((addr >> 12) & 0x1FF) as isize;
            while addr < limit_bytes && l1i < 512 {
                let pte = read_volatile(pt.offset(l1i));
                if (pte & EPT_R) != 0 {
                    if (pte & EPT_DIRTY) != 0 {
                        let page_index = (addr >> 12) as u64;
                        bitmap.set_bit(page_index);
                        dirty_pages += 1;
                        if clear_ad { write_volatile(pt.offset(l1i), pte & !(EPT_DIRTY | EPT_ACCESSED)); }
                    }
                }
                addr = addr.saturating_add(4096);
                l1i += 1;
                if (addr & ((1u64 << 21) - 1)) == 0 { break; }
            }
        }
    }
    dirty_pages
}

// AMD NPT bits (subset) – Accessed (A) = bit 5, Dirty (D) = bit 6 in PTEs and large leaves.
const NPT_P: u64 = 1 << 0;       // Present/Read
const NPT_W: u64 = 1 << 1;       // Write
const NPT_X: u64 = 1 << 2;       // Execute
const NPT_PS: u64 = 1 << 7;      // Page Size at PDE/PDPTE
const NPT_A: u64 = 1 << 5;       // Accessed
const NPT_D: u64 = 1 << 6;       // Dirty

fn scan_npt(pml4_phys: u64, limit_bytes: u64, bitmap: &mut DirtyBitmap, clear_ad: bool) -> u64 {
    if pml4_phys == 0 { return 0; }
    let mut dirty_pages: u64 = 0;
    let pml4 = (pml4_phys & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
    let mut addr: u64 = 0;
    unsafe {
        while addr < limit_bytes {
            let l4 = ((addr >> 39) & 0x1FF) as isize;
            let pml4e = read_volatile(pml4.offset(l4));
            if (pml4e & NPT_P) == 0 { addr = addr.saturating_add(1u64 << 39); continue; }
            let pdpt = (pml4e & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l3i = ((addr >> 30) & 0x1FF) as isize;
            let pdpte = read_volatile(pdpt.offset(l3i));
            if (pdpte & NPT_P) == 0 { addr = addr.saturating_add(1u64 << 30); continue; }
            if (pdpte & NPT_PS) != 0 {
                let page_count = 1u64 << (30 - 12);
                if (pdpte & NPT_D) != 0 {
                    for i in 0..page_count { bitmap.set_bit(((addr >> 12) + i) as u64); }
                    dirty_pages += page_count;
                    if clear_ad { write_volatile(pdpt.offset(l3i), pdpte & !(NPT_D | NPT_A)); }
                }
                addr = ((addr >> 30) + 1) << 30;
                continue;
            }
            let pd = (pdpte & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let l2i = ((addr >> 21) & 0x1FF) as isize;
            let pde = read_volatile(pd.offset(l2i));
            if (pde & NPT_P) == 0 { addr = addr.saturating_add(1u64 << 21); continue; }
            if (pde & NPT_PS) != 0 {
                let page_count = 1u64 << (21 - 12);
                if (pde & NPT_D) != 0 {
                    for i in 0..page_count { bitmap.set_bit(((addr >> 12) + i) as u64); }
                    dirty_pages += page_count;
                    if clear_ad { write_volatile(pd.offset(l2i), pde & !(NPT_D | NPT_A)); }
                }
                addr = ((addr >> 21) + 1) << 21;
                continue;
            }
            let pt = (pde & 0x000F_FFFF_FFFF_F000u64) as *mut u64;
            let mut l1i = ((addr >> 12) & 0x1FF) as isize;
            while addr < limit_bytes && l1i < 512 {
                let pte = read_volatile(pt.offset(l1i));
                if (pte & NPT_P) != 0 {
                    if (pte & NPT_D) != 0 {
                        let page_index = (addr >> 12) as u64;
                        bitmap.set_bit(page_index);
                        dirty_pages += 1;
                        if clear_ad { write_volatile(pt.offset(l1i), pte & !(NPT_D | NPT_A)); }
                    }
                }
                addr = addr.saturating_add(4096);
                l1i += 1;
                if (addr & ((1u64 << 21) - 1)) == 0 { break; }
            }
        }
    }
    dirty_pages
}


