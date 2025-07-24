//! I/O port emulation layer (x86_64)
//! Handles legacy devices for full I/O interception per requirement 4.3.
#![cfg(target_arch="x86_64")]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;

pub fn handle(port: u16, size: u8, write: bool, value: &mut u64) -> bool {
    match port {
        0x3F8..=0x3FF => serial(port, size, write, value),
        0x0040..=0x0043 => pit(port, size, write, value),
        0x0020 | 0x00A0 | 0x0021 | 0x00A1 => pic(port, size, write, value),
        0x0060 => keyboard(port, size, write, value),
        _ => false,
    }
}

fn serial(_port: u16, _size: u8, write: bool, value: &mut u64) -> bool {
    if write {
        let byte = (*value & 0xFF) as u8;
        crate::console::write_byte(byte);
    } else {
        *value = 0xFF; // no data
    }
    true
}

static PIT_CH0: Mutex<u16> = Mutex::new(0);
fn pit(_port: u16, size: u8, write: bool, value: &mut u64) -> bool {
    if write && size == 1 {
        *PIT_CH0.lock() = *value as u16;
    } else if !write {
        *value = *PIT_CH0.lock() as u64;
    }
    true
}

fn pic(port: u16, _size: u8, write: bool, value: &mut u64) -> bool {
    if write && port == 0x0020 && (*value & 0x20) == 0x20 {
        // EOI
    }
    true
}

fn keyboard(_port: u16, _size: u8, write: bool, value: &mut u64) -> bool {
    if write {
        // ignore writes
    } else {
        *value = 0;
    }
    true
} 