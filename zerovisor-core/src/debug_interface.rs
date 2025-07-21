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