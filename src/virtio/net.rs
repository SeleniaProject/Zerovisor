#![allow(dead_code)]

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

use super::{mmio_read8, mmio_read16, mmio_read32, ecam_fn_base};

const PCI_VENDOR_ID: usize = 0x00;
const PCI_DEVICE_ID: usize = 0x02;
const PCI_CLASS_OFF: usize = 0x08; // 0x0B: class (0x02 net), 0x0A: subclass
const PCI_CAP_PTR: usize = 0x34;
const VIRTIO_PCI_VENDOR: u16 = 0x1AF4;
const PCI_CAP_ID_VENDOR_SPECIFIC: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;

/// Report minimal info for the first detected virtio-net device (presence only).
pub fn report_first(system_table: &mut SystemTable<Boot>) {
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        let lang = crate::i18n::detect_lang(system_table);
        let stdout = system_table.stdout();
        let mut reported = false;
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            if reported { return; }
            let ecam_base = a.base_address;
            let bus_start = a.start_bus; let bus_end = a.end_bus;
            let mut bus = bus_start;
            while bus <= bus_end {
                for dev in 0u8..32u8 {
                    for func in 0u8..8u8 {
                        let cfg = ecam_fn_base(ecam_base, bus_start, bus, dev, func);
                        let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                        if vid == 0xFFFF { continue; }
                        if vid != VIRTIO_PCI_VENDOR { continue; }
                        let classreg = mmio_read32(cfg + (PCI_CLASS_OFF & !0x3));
                        let class = (classreg >> 24) as u8;
                        if class != 0x02 { continue; }
                        // Ensure it has common cfg cap minimally
                        let mut p = mmio_read8(cfg + PCI_CAP_PTR) as usize; let mut ok = false; let mut guard = 0u32;
                        while p >= 0x40 && p < 0x100 && guard < 64 {
                            let cap_id = mmio_read8(cfg + p);
                            let next = mmio_read8(cfg + p + 1) as usize;
                            let cap_len = mmio_read8(cfg + p + 2);
                            if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC && (cap_len as usize) >= 16 {
                                let cfg_type = mmio_read8(cfg + p + 3);
                                if cfg_type == VIRTIO_PCI_CAP_COMMON_CFG { ok = true; break; }
                            }
                            if next == 0 || next == p { break; }
                            p = next; guard += 1;
                        }
                        if !ok { continue; }
                        let _ = stdout.write_str(crate::i18n::t(lang, crate::i18n::key::VIRTIO_NET));
                        reported = true; break;
                    }
                    if reported { break; }
                }
                if reported || bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
        if !reported {
            let lang2 = crate::i18n::detect_lang(system_table);
            let stdout2 = system_table.stdout();
            let _ = stdout2.write_str(crate::i18n::t(lang2, crate::i18n::key::VIRTIO_NET_NONE));
        }
    }
}


// ---- Minimal virtio-net modern (1.0+) TX initialization and transmit ----

#[repr(C)]
struct VirtqDesc { addr: u64, len: u32, flags: u16, next: u16 }
#[repr(C)]
struct VirtqAvail { flags: u16, idx: u16, ring: [u16; 0] }
#[repr(C)]
struct VirtqUsedElem { id: u32, len: u32 }
#[repr(C)]
struct VirtqUsed { flags: u16, idx: u16, ring: [VirtqUsedElem; 0] }

struct TxState {
    cfg_base: usize,          // common cfg MMIO base
    notify_base: usize,       // notify MMIO base
    notify_off_mul: u32,      // notify multiplier
    queue_index: u16,
    queue_size: u16,
    q_desc: *mut VirtqDesc,
    q_avail: *mut u16,        // points to avail.ring[0]
    q_avail_hdr: *mut VirtqAvail,
    q_used: *mut VirtqUsed,
    desc_data: *mut u8,       // data buffer for tx packet (hdr + payload)
    desc_data_cap: usize,
    desc_index: u16,
    queue_notify_addr: usize,
    inited: bool,
    used_last: u16,
}

static mut TX: TxState = TxState {
    cfg_base: 0,
    notify_base: 0,
    notify_off_mul: 0,
    queue_index: 0,
    queue_size: 0,
    q_desc: core::ptr::null_mut(),
    q_avail: core::ptr::null_mut(),
    q_avail_hdr: core::ptr::null_mut(),
    q_used: core::ptr::null_mut(),
    desc_data: core::ptr::null_mut(),
    desc_data_cap: 0,
    desc_index: 0,
    queue_notify_addr: 0,
    inited: false,
    used_last: 0,
};

// ---- RX queue state (virtio-net queue 0) ----
struct RxState {
    queue_index: u16,
    queue_size: u16,
    q_desc: *mut VirtqDesc,
    q_avail: *mut u16,
    q_avail_hdr: *mut VirtqAvail,
    q_used: *mut VirtqUsed,
    slab: *mut u8,
    slab_bytes: usize,
    used_last: u16,
    inited: bool,
}

static mut RX: RxState = RxState {
    queue_index: 0,
    queue_size: 0,
    q_desc: core::ptr::null_mut(),
    q_avail: core::ptr::null_mut(),
    q_avail_hdr: core::ptr::null_mut(),
    q_used: core::ptr::null_mut(),
    slab: core::ptr::null_mut(),
    slab_bytes: 0,
    used_last: 0,
    inited: false,
};

const VIRTQ_DESC_F_WRITE: u16 = 1 << 1;

unsafe fn mmio_write8(addr: usize, val: u8) { core::ptr::write_volatile(addr as *mut u8, val) }
unsafe fn mmio_write16(addr: usize, val: u16) { core::ptr::write_volatile(addr as *mut u16, val) }
unsafe fn mmio_write32(addr: usize, val: u32) { core::ptr::write_volatile(addr as *mut u32, val) }
unsafe fn mmio_write64(addr: usize, val: u64) { core::ptr::write_volatile(addr as *mut u64, val) }
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;
const VIRTIO_STATUS_DRIVER_OK: u8 = 4;

fn find_first_virtio_net(system_table: &mut SystemTable<Boot>) -> Option<(usize, u32, usize, usize)> {
    // returns (common_base, notify_mul, notify_base, cfg)
    if let Some(mcfg_hdr) = crate::firmware::acpi::find_mcfg(system_table) {
        let mut found: Option<(usize, u32, usize, usize)> = None;
        crate::firmware::acpi::mcfg_for_each_allocation_from(|a| {
            if found.is_some() { return; }
            let ecam_base = a.base_address; let bus_start = a.start_bus; let bus_end = a.end_bus;
            let mut bus = bus_start;
            while bus <= bus_end {
                for dev in 0u8..32u8 { for func in 0u8..8u8 {
                    let cfg = ecam_fn_base(ecam_base, bus_start, bus, dev, func);
                    let vid = mmio_read16(cfg + PCI_VENDOR_ID);
                    if vid == 0xFFFF { continue; }
                    let classreg = mmio_read32(cfg + (PCI_CLASS_OFF & !0x3));
                    let class = (classreg >> 24) as u8;
                    if vid != VIRTIO_PCI_VENDOR || class != 0x02 { continue; }
                    // scan caps
                    let mut p = mmio_read8(cfg + PCI_CAP_PTR) as usize; let mut guard = 0u32;
                    let mut common_off: u32 = 0; let mut common_bar: u8 = 0;
                    let mut notify_off: u32 = 0; let mut notify_bar: u8 = 0; let mut notify_mul: u32 = 0;
                    while p >= 0x40 && p < 0x100 && guard < 64 {
                        let cap_id = mmio_read8(cfg + p);
                        let next = mmio_read8(cfg + p + 1) as usize;
                        let cap_len = mmio_read8(cfg + p + 2);
                        if cap_id == PCI_CAP_ID_VENDOR_SPECIFIC && (cap_len as usize) >= 16 {
                            let cfg_type = mmio_read8(cfg + p + 3);
                            let bar = mmio_read8(cfg + p + 4);
                            let off = mmio_read32(cfg + p + 8);
                            if cfg_type == VIRTIO_PCI_CAP_COMMON_CFG { common_bar = bar; common_off = off; }
                            if cfg_type == VIRTIO_PCI_CAP_NOTIFY_CFG { notify_bar = bar; notify_off = off; notify_mul = mmio_read32(cfg + p + 16); }
                        }
                        if next == 0 || next == p { break; }
                        p = next; guard += 1;
                    }
                    if common_bar == 0 && common_off == 0 { continue; }
                    // BAR base resolve
                    let bar_index = common_bar as usize; if bar_index >= 6 { continue; }
                    let bar_off = 0x10 + bar_index * 4;
                    let bar_lo = mmio_read32(cfg + bar_off);
                    if (bar_lo & 0x1) != 0 { continue; }
                    let mem_type = (bar_lo >> 1) & 0x3; let mut base: u64 = (bar_lo as u64) & 0xFFFF_FFF0u64;
                    let is_64 = mem_type == 0x2; if is_64 && bar_index < 5 { let bar_hi = mmio_read32(cfg + bar_off + 4); base |= (bar_hi as u64) << 32; }
                    let common_base = (base as usize).wrapping_add(common_off as usize);
                    // notify base
                    if notify_bar as usize >= 6 { continue; }
                    let nbar_lo = mmio_read32(cfg + (0x10 + (notify_bar as usize)*4));
                    if (nbar_lo & 1) != 0 { continue; }
                    let ntype = (nbar_lo >> 1) & 0x3; let mut nbase: u64 = (nbar_lo as u64) & 0xFFFF_FFF0u64;
                    let n64 = ntype == 0x2; if n64 && (notify_bar as usize) < 5 { let hi = mmio_read32(cfg + (0x10 + (notify_bar as usize)*4 + 4)); nbase |= (hi as u64) << 32; }
                    let notify_base = (nbase as usize).wrapping_add(notify_off as usize);
                    found = Some((common_base, notify_mul, notify_base, cfg));
                    break;
                }}
                if found.is_some() || bus == 0xFF { break; }
                bus = bus.saturating_add(1);
            }
        }, mcfg_hdr);
        return found;
    }
    None
}

pub fn init_tx(system_table: &mut SystemTable<Boot>) -> bool {
    unsafe {
        if TX.inited { return true; }
        if let Some((common_base, notify_mul_u8, notify_base, cfg)) = find_first_virtio_net(system_table) {
            TX.cfg_base = common_base; TX.notify_base = notify_base; TX.notify_off_mul = notify_mul_u8 as u32; TX.queue_index = 1; // virtio-net: queue 1 is TX
            // device_status at 0x14
            let device_status = TX.cfg_base + 0x14;
            let st = mmio_read8(device_status);
            mmio_write8(device_status, st | 1); // ACKNOWLEDGE
            let st2 = mmio_read8(device_status);
            mmio_write8(device_status, st2 | 2); // DRIVER
            // Clear driver features (select 0/1 and write 0), then FEATURES_OK
            mmio_write32(TX.cfg_base + 0x08, 0); // driver_feature_select = 0
            mmio_write32(TX.cfg_base + 0x0C, 0); // driver_feature = 0
            mmio_write32(TX.cfg_base + 0x08, 1); // select upper 32
            mmio_write32(TX.cfg_base + 0x0C, 0);
            let st3 = mmio_read8(device_status);
            mmio_write8(device_status, st3 | VIRTIO_STATUS_FEATURES_OK);
            let chk = mmio_read8(device_status);
            if (chk & VIRTIO_STATUS_FEATURES_OK) == 0 { return false; }
            // select queue 0 and read size
            mmio_write16(TX.cfg_base + 0x16, TX.queue_index);
            let qsz = mmio_read16(TX.cfg_base + 0x18);
            if qsz == 0 { return false; }
            TX.queue_size = qsz;
            // allocate tables
            let desc_bytes = (core::mem::size_of::<VirtqDesc>() as usize).saturating_mul(qsz as usize);
            let avail_bytes = (core::mem::size_of::<u16>() * (3 + qsz as usize));
            let used_bytes = (core::mem::size_of::<u16>() * 3) + (core::mem::size_of::<VirtqUsedElem>() * qsz as usize);
            let total = desc_bytes + avail_bytes + used_bytes + 4096; // padding
            let pages = (total + 4095) / 4096;
            if let Some(mem) = crate::mm::uefi::alloc_pages(system_table, pages, uefi::table::boot::MemoryType::LOADER_DATA) {
                core::ptr::write_bytes(mem, 0, pages * 4096);
                TX.q_desc = mem as *mut VirtqDesc;
                TX.q_avail_hdr = (mem as usize + desc_bytes) as *mut VirtqAvail;
                TX.q_avail = (mem as usize + desc_bytes + 4) as *mut u16; // skip flags+idx
                TX.q_used = (mem as usize + desc_bytes + avail_bytes) as *mut VirtqUsed;
                // program addresses
                mmio_write64(TX.cfg_base + 0x20, TX.q_desc as u64);
                mmio_write64(TX.cfg_base + 0x28, TX.q_avail_hdr as u64);
                mmio_write64(TX.cfg_base + 0x30, TX.q_used as u64);
                // notify address
                mmio_write16(TX.cfg_base + 0x16, TX.queue_index);
                let qnoff = mmio_read16(TX.cfg_base + 0x1E) as u32;
                TX.queue_notify_addr = TX.notify_base.wrapping_add((qnoff.saturating_mul(TX.notify_off_mul)) as usize);
                // enable queue
                mmio_write16(TX.cfg_base + 0x1C, 1);
                // allocate tx data buffer
                TX.desc_data_cap = 4096 + 2048; // hdr + payload approx
                let dpages = (TX.desc_data_cap + 4095) / 4096;
                if let Some(dp) = crate::mm::uefi::alloc_pages(system_table, dpages, uefi::table::boot::MemoryType::LOADER_DATA) {
                    core::ptr::write_bytes(dp, 0, dpages * 4096);
                    TX.desc_data = dp;
                }
                if TX.desc_data.is_null() { return false; }
                // DRIVER_OK
                let st4 = mmio_read8(device_status);
                mmio_write8(device_status, st4 | VIRTIO_STATUS_DRIVER_OK);
                // Initialize last used index
                TX.used_last = core::ptr::read_volatile((TX.q_used as usize + 2) as *const u16);
                TX.inited = true;
                return TX.inited;
            }
        }
        false
    }
}

pub fn init_rx(system_table: &mut SystemTable<Boot>) -> bool {
    unsafe {
        if RX.inited { return true; }
        // Ensure TX common bases are initialized to reuse BARs and notify
        if !TX.inited { if !init_tx(system_table) { return false; } }
        RX.queue_index = 0;
        // select RX queue and read size
        mmio_write16(TX.cfg_base + 0x16, RX.queue_index);
        let qsz = mmio_read16(TX.cfg_base + 0x18);
        if qsz == 0 { return false; }
        RX.queue_size = qsz;
        // allocate rings and a slab for RX buffers (per-desc 2048 + 10 header margin)
        let desc_bytes = (core::mem::size_of::<VirtqDesc>() as usize).saturating_mul(qsz as usize);
        let avail_bytes = (core::mem::size_of::<u16>() * (3 + qsz as usize));
        let used_bytes = (core::mem::size_of::<u16>() * 3) + (core::mem::size_of::<VirtqUsedElem>() * qsz as usize);
        let ring_total = desc_bytes + avail_bytes + used_bytes + 4096;
        let slab_per = 2048 + 64; // allow some headroom
        RX.slab_bytes = (slab_per as usize) * (qsz as usize);
        let alloc_total = ring_total + RX.slab_bytes;
        let pages = (alloc_total + 4095) / 4096;
        if let Some(mem) = crate::mm::uefi::alloc_pages(system_table, pages, uefi::table::boot::MemoryType::LOADER_DATA) {
            core::ptr::write_bytes(mem, 0, pages * 4096);
            RX.q_desc = mem as *mut VirtqDesc;
            RX.q_avail_hdr = (mem as usize + desc_bytes) as *mut VirtqAvail;
            RX.q_avail = (mem as usize + desc_bytes + 4) as *mut u16;
            RX.q_used = (mem as usize + desc_bytes + avail_bytes) as *mut VirtqUsed;
            RX.slab = (mem as usize + ring_total) as *mut u8;
            // program addresses for RX queue
            mmio_write16(TX.cfg_base + 0x16, RX.queue_index);
            mmio_write64(TX.cfg_base + 0x20, RX.q_desc as u64);
            mmio_write64(TX.cfg_base + 0x28, RX.q_avail_hdr as u64);
            mmio_write64(TX.cfg_base + 0x30, RX.q_used as u64);
            mmio_write16(TX.cfg_base + 0x1C, 1); // enable queue
            // populate descriptors
            for i in 0..(RX.queue_size as usize) {
                let d = &mut *RX.q_desc.add(i);
                d.addr = RX.slab.add(i * slab_per as usize) as u64;
                d.len = slab_per as u32;
                d.flags = VIRTQ_DESC_F_WRITE;
                d.next = 0;
                core::ptr::write_volatile(RX.q_avail.add(i), i as u16);
            }
            // publish avail idx
            let avail_idx_ptr = (RX.q_avail_hdr as usize + 2) as *mut u16;
            core::ptr::write_volatile(avail_idx_ptr, RX.queue_size);
            // notify RX queue (queue_notify_addr computed for TX; recompute with RX qnoff)
            mmio_write16(TX.cfg_base + 0x16, RX.queue_index);
            let qnoff = mmio_read16(TX.cfg_base + 0x1E) as u32;
            let rx_notify_addr = TX.notify_base.wrapping_add((qnoff.saturating_mul(TX.notify_off_mul)) as usize);
            mmio_write16(rx_notify_addr, RX.queue_index);
            RX.used_last = core::ptr::read_volatile((RX.q_used as usize + 2) as *const u16);
            RX.inited = true;
            return true;
        }
        false
    }
}

pub fn rx_pump(system_table: &mut SystemTable<Boot>, limit: usize) {
    unsafe {
        if !RX.inited { if !init_rx(system_table) { return; } }
        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_CALLS).inc();
        let used_idx_ptr = (RX.q_used as usize + 2) as *const u16;
        let mut processed = 0usize;
        let hdr_len = 10usize;
        let hdr_mig = *b"ZMIG";
        loop {
            let used_idx = core::ptr::read_volatile(used_idx_ptr);
            if RX.used_last == used_idx { break; }
            let slot = (RX.used_last as usize) % (RX.queue_size as usize);
            // read used elem
            let ue_ptr = (RX.q_used as usize + 4 + slot * core::mem::size_of::<VirtqUsedElem>()) as *const VirtqUsedElem;
            let ue = core::ptr::read_volatile(ue_ptr);
            let len = ue.len as usize;
            let buf_ptr = RX.slab.add((ue.id as usize) * (2048 + 64));
            if len > hdr_len {
                let payload = core::slice::from_raw_parts(buf_ptr.add(hdr_len), len - hdr_len);
                // search for MIG magic and CRC-validate like SNP pump
                let mut pos = 0usize;
                let mut wrote_any = false;
                while pos + 28 <= payload.len() { // header size
                    if &payload[pos..pos+4] != &hdr_mig { pos += 1; continue; }
                    let payload_len = {
                        let b = &payload[pos+20..pos+24]; (b[0] as usize) | ((b[1] as usize) << 8) | ((b[2] as usize) << 16) | ((b[3] as usize) << 24)
                    };
                    if pos + 28 + payload_len > payload.len() { break; }
                    let crc_hdr = {
                        let b = &payload[pos+24..pos+28]; (b[0] as u32) | ((b[1] as u32) << 8) | ((b[2] as u32) << 16) | ((b[3] as u32) << 24)
                    };
                    let body = &payload[pos+28 .. pos+28+payload_len];
                    let crc_calc = crate::util::crc32::crc32(body);
                    if crc_calc == crc_hdr {
                        let _ = crate::migrate::chan_write_bytes(&payload[pos .. pos+28]);
                        let _ = crate::migrate::chan_write_bytes(body);
                        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_OK).inc();
                        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_FRAMES).inc();
                        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_BYTES).add((28 + payload_len) as u64);
                        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_BYTES).add((28 + payload_len) as u64);
                        wrote_any = true;
                        pos += 28 + payload_len;
                    } else {
                        crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_RX_FRAMES_BAD).inc();
                        pos += 1;
                    }
                }
                if !wrote_any { crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_PUMP_EMPTY).inc(); }
            }
            RX.used_last = RX.used_last.wrapping_add(1);
            processed += 1;
            if limit != 0 && processed >= limit { break; }
            // recycle descriptor back to avail
            let avail_idx_ptr = (RX.q_avail_hdr as usize + 2) as *mut u16;
            let avail_idx = core::ptr::read_volatile(avail_idx_ptr);
            let a_slot = (avail_idx as usize) % (RX.queue_size as usize);
            core::ptr::write_volatile(RX.q_avail.add(a_slot), ue.id as u16);
            core::ptr::write_volatile(avail_idx_ptr, avail_idx.wrapping_add(1));
        }
    }
}

#[inline(always)]
fn fence() { core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst) }

unsafe fn reclaim_used() {
    if !TX.inited || TX.q_used.is_null() { return; }
    let used_idx_ptr = (TX.q_used as usize + 2) as *const u16;
    let used_idx = core::ptr::read_volatile(used_idx_ptr);
    // Consume all completed used entries between TX.used_last..used_idx
    let mut cnt = TX.used_last;
    while cnt != used_idx {
        // read used element (optional)
        // let ring_mask = (TX.queue_size as u16).wrapping_sub(1);
        // let slot = (cnt as usize) % (TX.queue_size as usize);
        // let ue_ptr = (TX.q_used as usize + 4 + slot * core::mem::size_of::<VirtqUsedElem>()) as *const VirtqUsedElem;
        // let _ue = core::ptr::read_volatile(ue_ptr);
        cnt = cnt.wrapping_add(1);
    }
    TX.used_last = used_idx;
}

pub fn tx_send(system_table: &mut SystemTable<Boot>, data: &[u8]) -> usize {
    unsafe {
        if !TX.inited { if !init_tx(system_table) { return 0; } }
        if TX.desc_data.is_null() || TX.q_desc.is_null() { return 0; }
        // Reclaim any completed buffers before attempting to enqueue
        reclaim_used();
        let hdr_len = 10usize;
        let total = hdr_len + data.len();
        if total > TX.desc_data_cap { return 0; }
        // Zero header and copy payload
        core::ptr::write_bytes(TX.desc_data, 0, hdr_len);
        core::ptr::copy_nonoverlapping(data.as_ptr(), TX.desc_data.add(hdr_len), data.len());
        // Compute ring indices and check space
        let avail_idx_ptr = (TX.q_avail_hdr as usize + 2) as *mut u16; // idx field after flags
        let used_idx_ptr = (TX.q_used as usize + 2) as *const u16; // used.idx
        let avail_idx = core::ptr::read_volatile(avail_idx_ptr);
        let used_idx = core::ptr::read_volatile(used_idx_ptr);
        let pending = avail_idx.wrapping_sub(used_idx);
        if pending as u16 >= TX.queue_size.wrapping_sub(1) {
            crate::obs::metrics::Counter::new(&crate::obs::metrics::MIG_NET_TX_ERRS).inc();
            return 0;
        }
        let slot = (avail_idx as usize) % (TX.queue_size as usize);
        // Build descriptor at slot
        let d = &mut *TX.q_desc.add(slot);
        d.addr = TX.desc_data as u64; d.len = total as u32; d.flags = 0; d.next = 0;
        fence();
        // Push to avail ring
        core::ptr::write_volatile(TX.q_avail.add(slot), slot as u16);
        core::ptr::write_volatile(avail_idx_ptr, avail_idx.wrapping_add(1));
        fence();
        // Notify
        mmio_write16(TX.queue_notify_addr, TX.queue_index);
        total
    }
}


