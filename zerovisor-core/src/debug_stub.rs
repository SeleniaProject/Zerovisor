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
    if pkt.starts_with("qSupported") { return String::from(""); }
    if pkt == "?" { return String::from("S05"); } // dummy signal 5

    // Read registers
    if pkt == "g" { return String::from("00000000"); }

    // Continue / single-step
    if pkt == "c" || pkt.starts_with("c") { return String::from("OK"); }
    if pkt == "s" { return String::from("OK"); }

    // Breakpoint set/clear: Z0 / z0
    if pkt.starts_with("Z0,") {
        if let Some(addr) = parse_hex_u64(&pkt[3..]) { add_breakpoint(addr); }
        return String::from("OK");
    }
    if pkt.starts_with("z0,") {
        if let Some(addr) = parse_hex_u64(&pkt[3..]) { remove_breakpoint(addr); }
        return String::from("OK");
    }

    // Memory read: mADDR,LEN
    if pkt.starts_with("m") {
        return String::from("E01"); // not implemented
    }

    // Default
    String::from("")
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

fn add_breakpoint(addr: u64) {
    let mut bps = BREAKPOINTS.lock();
    for slot in bps.iter_mut() {
        if slot.is_none() { *slot = Some(Breakpoint { addr }); return; }
    }
}

fn remove_breakpoint(addr: u64) {
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