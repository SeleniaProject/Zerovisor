//! High-level debug interface wrapping `debug_stub` (Task 12.2)
//! Provides UART-backed GDB RSP service.

#![allow(dead_code)]

use crate::debug_stub::{ByteIo, DebugStub};

/// Simple MMIO UART implementation of ByteIo.
struct UartIo;

impl ByteIo for UartIo {
    fn read_byte(&self) -> Option<u8> {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            const BASE: u16 = 0x3F8;
            const LSR: u16 = BASE + 5;
            const DATA_READY: u8 = 1 << 0;
            let mut status: u8;
            core::arch::asm!("in al, dx", in("dx") LSR, out("al") status, options(nomem, nostack, preserves_flags));
            if status & DATA_READY != 0 {
                let mut byte: u8;
                core::arch::asm!("in al, dx", in("dx") BASE, out("al") byte, options(nomem, nostack, preserves_flags));
                Some(byte)
            } else { None }
        }
        #[cfg(not(target_arch = "x86_64"))]
        { None }
    }

    fn write_byte(&self, byte: u8) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            const BASE: u16 = 0x3F8;
            core::arch::asm!("out dx, al", in("dx") BASE, in("al") byte, options(nomem, nostack, preserves_flags));
        }
    }
}

static UART: UartIo = UartIo;

pub fn init() { DebugStub::init(&UART); }

pub fn poll() { DebugStub::poll(); }

/// Program a software breakpoint at virtual address `addr`.
pub fn set_breakpoint(addr: u64) { crate::debug_stub::add_breakpoint(addr); }

/// Request single-step execution via GDB stub.
pub fn single_step() {
    // Send a GDB 's' packet over UART I/O.
    // For simplicity we craft minimum packet directly.
    const CMD: &str = "s";
    unsafe {
        if let Some(stub) = crate::debug_stub::DEBUG_STUB.as_mut() {
            stub.io.write_byte(b'$');
            for b in CMD.as_bytes() { stub.io.write_byte(*b); }
            stub.io.write_byte(b'#');
            let chk = (CMD.bytes().fold(0u8, |a, v| a.wrapping_add(v)));
            let hi = ((chk >> 4) & 0xF) as u8; let lo = (chk & 0xF) as u8;
            let nib = |n: u8| if n < 10 { b'0' + n } else { b'a' + (n-10) };
            stub.io.write_byte(nib(hi));
            stub.io.write_byte(nib(lo));
        }
    }
} 