//! Minimal GDB Remote-Serial-Protocol stub (Task 12.2)
//! 現状は no_std + シリアル I/O が未定義のため、インターフェースのみ用意。
//! 実際のパケット送受信はボード依存層に実装予定。

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::fmt::Write;

/// Simple trait for byte-wise I/O (e.g., UART).
pub trait ByteIo {
    fn read_byte(&self) -> Option<u8>;
    fn write_byte(&self, byte: u8);
}

/// Result type for debug stub operations.
pub type DebugResult<T> = Result<T, DebugError>;

#[derive(Debug)]
pub enum DebugError { Io, Protocol }

/// Guest execution trace (PC addresses). Fixed-size ring buffer.
const TRACE_LEN: usize = 1024;
static TRACE_BUF: Mutex<[u64; TRACE_LEN]> = Mutex::new([0; TRACE_LEN]);
static TRACE_IDX: Mutex<usize> = Mutex::new(0);

/// Simple breakpoint representation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Breakpoint { pub addr: u64 }

/// Max breakpoints supported.
const MAX_BP: usize = 256;
static BREAKPOINTS: Mutex<[Option<Breakpoint>; MAX_BP]> = Mutex::new([None; MAX_BP]);

/// Global stub instance (single-threaded environment)
static mut DEBUG_STUB: Option<DebugStub<'static>> = None;
// Current selected thread id (GDB uses 1-based ids by default). Zerovisor maps each VCPU to a
// thread id equal to its handle value. 0 means "all threads".
static SELECTED_THREAD: Mutex<u32> = Mutex::new(0);

pub struct DebugStub<'a> {
    io: &'a dyn ByteIo,
    packet_buf: Vec<u8>,
}

impl<'a> DebugStub<'a> {
    pub fn init(io: &'a dyn ByteIo) {
        unsafe { DEBUG_STUB = Some(DebugStub { io, packet_buf: Vec::with_capacity(128) }); }
    }

    fn checksum(buf: &[u8]) -> u8 { buf.iter().fold(0u8, |acc, b| acc.wrapping_add(*b)) }

    /// Poll for incoming packets and respond with OK packet.
    pub fn poll() {
        let stub_opt = unsafe { DEBUG_STUB.as_mut() };
        if stub_opt.is_none() { return; }
        let stub = stub_opt.unwrap();

        while let Some(b) = stub.io.read_byte() {
            if b == b'$' {
                stub.packet_buf.clear();
                continue;
            }
            if b == b'#' {
                // checksum bytes
                let _ = stub.io.read_byte();
                let _ = stub.io.read_byte();

                // process packet
                let reply = process_packet(core::str::from_utf8(&stub.packet_buf).unwrap_or(""));
                stub.io.write_byte(b'+'); // ack
                stub.send_str(&reply);
                continue;
            }
            stub.packet_buf.push(b);
        }
    }

    fn send_str(&mut self, s: &str) {
        self.io.write_byte(b'$');
        for b in s.as_bytes() { self.io.write_byte(*b); }
        self.io.write_byte(b'#');
        let chk = Self::checksum(s.as_bytes());
        let hi = nibble_to_hex((chk >> 4) & 0xF);
        let lo = nibble_to_hex(chk & 0xF);
        self.io.write_byte(hi);
        self.io.write_byte(lo);
    }
}

fn nibble_to_hex(n: u8) -> u8 { if n < 10 { b'0' + n } else { b'a' + (n - 10) } }
fn hex_val(c: u8) -> u8 { if c >= b'0' && c <= b'9' { c - b'0' } else { c - b'a' + 10 } }
fn hex_to_byte(c1: u8, c2: u8) -> u8 { (hex_val(c1) << 4) | hex_val(c2) } 

// ---------------------------------------------------------------------------
// Packet handler
// ---------------------------------------------------------------------------

fn process_packet(pkt: &str) -> String {
    if pkt.starts_with("qSupported") { return "PacketSize=4000".into(); }

    // Stop reply – return SIGTRAP on initial attach
    if pkt == "?" { return "S05".into(); }

    // Read registers – return 16 × 64-bit GPRs as zeroes (placeholder)
    if pkt == "g" {
        return encode_regs();
    }

    // Write registers – accept but ignore content
    if pkt.starts_with("G") { return "OK".into(); }

    // Continue / single-step acknowledgements
    if pkt == "c" || pkt.starts_with("c") { return "OK".into(); }
    if pkt == "s" { return "OK".into(); }

    // Breakpoint set/clear: Z0 / z0 (software breakpoint)
    if pkt.starts_with("Z0,") {
        if let Some(addr) = parse_hex_u64(&pkt[3..]) { add_breakpoint(addr); }
        return "OK".into();
    }
    if pkt.starts_with("z0,") {
        if let Some(addr) = parse_hex_u64(&pkt[3..]) { remove_breakpoint(addr); }
        return "OK".into();
    }

    // Memory read: mADDR,LEN
    if pkt.starts_with("m") {
        if let Some(comma) = pkt.find(',') {
            let addr_str = &pkt[1..comma];
            let len_str  = &pkt[comma + 1..];
            if let (Ok(addr), Ok(len)) = (u64::from_str_radix(addr_str, 16), usize::from_str_radix(len_str, 16)) {
                unsafe {
                    let slice = core::slice::from_raw_parts(addr as *const u8, len);
                    return hex_encode(slice);
                }
            }
        }
        return "E01".into();
    }

    // Memory write: MADDR,LEN:DATA
    if pkt.starts_with("M") {
        if let Some(colon) = pkt.find(':') {
            let header = &pkt[1..colon];
            if let Some(comma) = header.find(',') {
                let addr_str = &header[..comma];
                let len_str = &header[comma + 1..];
                if let (Ok(addr), Ok(len)) = (u64::from_str_radix(addr_str, 16), usize::from_str_radix(len_str, 16)) {
                    let data_str = &pkt[colon + 1..];
                    let bytes = hex_decode(data_str);
                    if bytes.len() == len {
                        unsafe {
                            let dst = core::slice::from_raw_parts_mut(addr as *mut u8, len);
                            dst.copy_from_slice(&bytes);
                            return "OK".into();
                        }
                    }
                }
            }
        }
        return "E02".into();
    }

    // Switch active thread (Hc thread-id / Hg thread-id). Only acknowledge selection.
    if pkt.starts_with("Hc") || pkt.starts_with("Hg") {
        if let Ok(tid) = u32::from_str_radix(&pkt[2..], 16) {
            *SELECTED_THREAD.lock() = tid;
        }
        return "OK".into();
    }

    // Thread list – single chunk (no pagination)
    if pkt == "qfThreadInfo" {
        // For now expose one dummy thread id 1; real implementation will iterate VCPUs.
        return "m1".into();
    }
    if pkt == "qsThreadInfo" { return "l".into(); }

    // Thread alive check (Ttid)
    if pkt.starts_with("T") {
        return "OK".into();
    }

    // Query attached flag
    if pkt.starts_with("qAttached") { return "1".into(); }

    // Default – unsupported
    String::from("")
}

// Hex helpers --------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xF) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hi = hex_val(bytes[i]);
        let lo = hex_val(bytes[i + 1]);
        v.push((hi << 4) | lo);
        i += 2;
    }
    v
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let mut end = s.len();
    for (i, ch) in s.bytes().enumerate() {
        if ch == b',' || ch == b'#' { end = i; break; }
    }
    u64::from_str_radix(&s[..end], 16).ok()
}

// ---------------------------------------------------------------------------
// Breakpoint management
// ---------------------------------------------------------------------------

pub fn add_breakpoint(addr: u64) {
    let mut bps = BREAKPOINTS.lock();
    for slot in bps.iter_mut() {
        if slot.is_none() { *slot = Some(Breakpoint { addr }); return; }
    }
}

pub fn remove_breakpoint(addr: u64) {
    let mut bps = BREAKPOINTS.lock();
    for slot in bps.iter_mut() {
        if let Some(bp) = slot { if bp.addr == addr { *slot = None; return; } }
    }
}

/// Check if the current PC hits a breakpoint.
pub fn check_breakpoint(pc: u64) -> bool {
    let bps = BREAKPOINTS.lock();
    bps.iter().any(|b| b.map(|bp| bp.addr == pc).unwrap_or(false))
}

// ---------------------------------------------------------------------------
// Trace utilities
// ---------------------------------------------------------------------------

/// Record program counter into ring buffer for offline analysis.
pub fn trace_pc(pc: u64) {
    let mut buf = TRACE_BUF.lock();
    let mut idx = TRACE_IDX.lock();
    buf[*idx] = pc;
    *idx = (*idx + 1) % TRACE_LEN;
}

/// Retrieve snapshot of trace buffer.
pub fn get_trace_snapshot(out: &mut [u64]) -> usize {
    let buf = TRACE_BUF.lock();
    let idx = TRACE_IDX.lock();
    let mut out_idx = 0;
    for i in 0..TRACE_LEN {
        let j = (*idx + i) % TRACE_LEN;
        if out_idx < out.len() { out[out_idx] = buf[j]; out_idx += 1; }
    }
    out_idx
} 

fn encode_regs() -> String {
    // Provide a realistic register dump for x86_64 guests. The hypervisor core will need to fill
    // actual values; until then we return zeroes to satisfy GDB protocol.
    const REG_COUNT: usize = 27; // 16 GPR + RIP + RFLAGS + segment + misc
    let mut s = String::with_capacity(REG_COUNT * 16);
    for _ in 0..REG_COUNT { s.push_str("0000000000000000"); }
    s
} 