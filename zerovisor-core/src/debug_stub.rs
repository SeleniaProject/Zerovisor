//! Minimal GDB Remote-Serial-Protocol stub (Task 12.2)
//! 現状は no_std + シリアル I/O が未定義のため、インターフェースのみ用意。
//! 実際のパケット送受信はボード依存層に実装予定。

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
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
        let stub = unsafe { DEBUG_STUB.as_mut() };
        if stub.is_none() { return; }
        let stub = stub.unwrap();
        // Very naive implementation: read until '#', assume one full packet.
        while let Some(b) = stub.io.read_byte() {
            if b == b'$' { stub.packet_buf.clear(); continue; }
            if b == b'#' {
                // read checksum (2 hex chars)
                let c1 = stub.io.read_byte().unwrap_or(0);
                let c2 = stub.io.read_byte().unwrap_or(0);
                let _recv_chk = hex_to_byte(c1, c2);
                // Send generic OK
                stub.io.write_byte(b'+');
                stub.send_str("OK");
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