#![allow(dead_code)]

/// Write u64 hex into a buffer without allocation, returns bytes written.
pub fn u64_hex(v: u64, out: &mut [u8]) -> usize {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut started = false; let mut n = 0;
    for i in (0..16).rev() {
        let nyb = ((v >> (i * 4)) & 0xF) as usize;
        if nyb != 0 || started || i == 0 { started = true; if n < out.len() { out[n] = HEX[nyb]; n += 1; } }
    }
    n
}



