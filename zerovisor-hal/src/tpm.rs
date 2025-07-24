//! TPM 2.0 interface utilities used for measured boot (root of trust)
//! Supports two transport mechanisms:
//!  • x86 TIS (I/O‐port 0xFED4* range)
//!  • ACPI CRB (MMIO 0xFED40000+ by default)
//! Only the PCR Extend (SHA-256) command is implemented as it is sufficient
//! for Zerovisor's early measured boot.
//!
//! This module chooses the appropriate backend at runtime on the first call
//! and caches the implementation via a `Once`.

#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
use crate::Box;
use spin::Once;
use sha2::{Sha256, Digest};

/// Public error type for TPM operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmError { Unsupported, Timeout, Io, InvalidParam }

/// Back-end trait implemented by transport-specific drivers.
trait Backend {
    fn pcr_extend(&self, pcr: u8, digest: &[u8]) -> Result<(), TpmError>;
}

// ------------------------------------------------------------------------------------------------
// x86 TIS backend (port-mapped I/O, 32-bit access)
// ------------------------------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
mod tis {
    use super::*;
    const BASE: u16 = 0xFED4; // TI spec default (PNP0C31)
    const ACCESS: u16 = 0x0000;
    const STS: u16 = 0x0018;
    const DATA: u16 = 0x0024;

    #[inline(always)]
    unsafe fn inb(port: u16) -> u8 { 
        let v: u8; 
        unsafe {
            core::arch::asm!("in al, dx", in("dx") port, out("al") v, options(nomem, nostack, preserves_flags)); 
        }
        v 
    }
    #[inline(always)]
    unsafe fn outb(port: u16, v: u8) { 
        unsafe {
            core::arch::asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags)); 
        }
    }

    fn wait(flags: u8) -> Result<(), TpmError> {
        for _ in 0..10000 {
            let s = unsafe { inb(BASE + STS) };
            if (s & flags) == flags { return Ok(()); }
            core::hint::spin_loop();
        }
        Err(TpmError::Timeout)
    }

    #[derive(Default)]
    pub struct TisBackend;
    impl Backend for TisBackend {
        fn pcr_extend(&self, pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
            if digest.len() != 32 { return Err(TpmError::InvalidParam); }
            unsafe {
                wait(0x20)?; // valid+ready
                outb(BASE + STS, 0x40); // command ready
                wait(0x20)?;
            }
            // Build command header
            const CMD_SIZE: u16 = 34 + 32;
            const HEADER: [u8; 10] = [0x80,0x01, 0,0, // size later
                                       0x00,0x00,0x01,0x82, // PCR_Extend
                                       0x00,0x00]; // handle count Hi
            unsafe {
                // write header with size
                for (i,b) in HEADER.iter().enumerate() {
                    let val = if i==3 { (CMD_SIZE & 0xFF) as u8 } else if i==2 { (CMD_SIZE>>8) as u8 } else {*b};
                    outb(BASE+DATA, val);
                }
                outb(BASE+DATA, 0x01); // handle count lo
                outb(BASE+DATA, 0x00);
                outb(BASE+DATA, pcr);
                // authSize=0
                outb(BASE+DATA,0);
                outb(BASE+DATA,0);
                // digest count =1, hashAlg=0x000B
                outb(BASE+DATA,0);
                outb(BASE+DATA,1);
                outb(BASE+DATA,0);
                outb(BASE+DATA,0x0B);
                for b in digest { outb(BASE+DATA,*b); }
                // GO
                outb(BASE+STS,0x20);
            }
            unsafe { wait(0x80)?; }
            Ok(())
        }
    }
}

// ------------------------------------------------------------------------------------------------
// CRB MMIO backend (commonly used on UEFI systems, incl. ARM & RISC-V)
// ------------------------------------------------------------------------------------------------
mod crb {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};

    const DEFAULT_CRB_BASE: usize = 0xFED4_0000;
    const CRB_CTRL_START: usize = 0x40;
    const CRB_CTRL_CMD_SIZE: usize = 0x18;
    const CRB_CTRL_CMD_ADDR: usize = 0x18;
    const CRB_CTRL_STS: usize = 0x0C;

    const STS_TPM_IDLE: u32 = 0x0000_0002;

    static mut CRB_BASE: usize = DEFAULT_CRB_BASE;

    #[inline(always)]
    unsafe fn mmio32(off: usize) -> *mut u32 { 
        unsafe { (CRB_BASE + off) as *mut u32 }
    }

    fn wait_idle() -> Result<(), TpmError> {
        for _ in 0..10000 {
            let v = unsafe { read_volatile(mmio32(CRB_CTRL_STS)) };
            if (v & STS_TPM_IDLE) != 0 { return Ok(()); }
            core::hint::spin_loop();
        }
        Err(TpmError::Timeout)
    }

    pub struct CrbBackend;
    impl Backend for CrbBackend {
        fn pcr_extend(&self, pcr: u8, digest: &[u8]) -> Result<(), TpmError> {
            if digest.len() != 32 { return Err(TpmError::Unsupported); }
            wait_idle()?;
            // Allocate temporary buffer on stack (small) for command
            const HEADER_LEN: usize = 14;
            const CMD_SIZE: u32 = (HEADER_LEN + 2 + 2 + 32) as u32;
            let mut buf = [0u8; HEADER_LEN + 2 + 2 + 32];
            buf[..4].copy_from_slice(&[0x80,0x01,(CMD_SIZE>>8) as u8,(CMD_SIZE&0xFF) as u8]);
            buf[4..8].copy_from_slice(&[0,0,0x01,0x82]);
            buf[8..12].copy_from_slice(&[0,0,0,1]);
            buf[12..14].copy_from_slice(&[0,0]);
            buf[14] = pcr; buf[15]=0; // handle (little-endian)
            buf[16..18].copy_from_slice(&[0,0]); // authSize
            buf[18..22].copy_from_slice(&[0,1,0,0x0B]);
            buf[22..54].copy_from_slice(digest);

            unsafe {
                // Program command address/size
                write_volatile(mmio32(CRB_CTRL_CMD_ADDR), buf.as_ptr() as u32);
                write_volatile(mmio32(CRB_CTRL_CMD_ADDR+4), 0);
                write_volatile(mmio32(CRB_CTRL_CMD_SIZE), CMD_SIZE);
                // Start
                write_volatile(mmio32(CRB_CTRL_START), 1);
            }
            wait_idle()?;
            Ok(())
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Runtime backend selection
// ------------------------------------------------------------------------------------------------
static BACKEND: Once<Box<dyn Backend + Send + Sync>> = Once::new();

fn backend() -> Option<&'static (dyn Backend + Send + Sync)> {
    BACKEND.get().map(|b| b.as_ref())
}

/// Initialize TPM backend
pub fn init_tpm() {
    BACKEND.call_once(|| {
        #[cfg(target_arch = "x86_64")]
        {
            Box::new(tis::TisBackend::default())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Box::new(SoftwareTpm::new())
        }
    });
}

// ------------------------------------------------------------------------------------------------
// Public helpers
// ------------------------------------------------------------------------------------------------

/// Extend PCR `index` with `data` (SHA-256) – returns `Ok` on success.
pub fn pcr_extend(index: u8, data: &[u8]) -> Result<(), TpmError> {
    // Compute digest on-stack to guarantee constant size.
    let mut h = Sha256::new();
    h.update(data);
    let digest = h.finalize();
    let be = backend().ok_or(TpmError::Unsupported)?;
    be.pcr_extend(index, &digest)
}

// ------------------------------------------------------------------
// Endorsement key (EK) stub – real TPM exposes RSA/ECC key. Here we use
// SHA-256 hash of a fixed identifier to derive a 32-byte public key and
// emulate signing by hashing (EK || data) for constant-time placeholder.
// ------------------------------------------------------------------

static EK_PUB_ONCE: Once<[u8;32]> = Once::new();

fn ek_pub() -> &'static [u8;32] {
    EK_PUB_ONCE.call_once(|| {
        let mut hasher = Sha256::new();
        hasher.update(b"Zerovisor-TPM-EK");
        let digest = hasher.finalize();
        let mut out = [0u8;32];
        out.copy_from_slice(&digest);
        out
    })
}

/// Return TPM endorsement public key (stub – 32-byte hash).
pub fn endorsement_key() -> &'static [u8] { ek_pub() }

/// Sign arbitrary data with the endorsement key (stub: SHA-256(EK || data)).
pub fn sign_with_ek(data: &[u8]) -> [u8;32] {
    let mut hasher = Sha256::new();
    hasher.update(ek_pub());
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = [0u8;32];
    out.copy_from_slice(&digest);
    out
} 