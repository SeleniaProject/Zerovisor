//! Homomorphic memory encryption engine (Task: Homomorphic Memory Encryption)
//!
//! This module provides page‐level encryption based on TFHE‐rs.  When the
//! `homomorphic_encryption` feature is enabled, real fully homomorphic
//! ciphertexts are produced and can be processed by the hypervisor without
//! ever decrypting guest memory.  In `no_std` builds where the TFHE
//! dependency is disabled, stub implementations are provided so that the
//! rest of the hypervisor can still compile.
//!
//! NOTE: The current implementation demonstrates encryption/decryption and
//! simple homomorphic addition to verify correctness.  Production use would
//! integrate block RAM backing and stream processing to avoid the `Vec`
//! allocations seen here.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;

/// A single page is 4 KiB.
pub const PAGE_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Public API (independent of TFHE)
// ---------------------------------------------------------------------------

/// Opaque ciphertext for a 4 KiB page.
#[derive(Clone)]
pub struct EncryptedPage {
    /// Underlying ciphertext bytes (serialization format depends on backend).
    pub(crate) data: Vec<u8>,
}

/// Errors returned by the FHE engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FheError {
    BackendUnavailable,
    InvalidCiphertext,
    InvalidPlaintext,
}

/// Engine trait shared by the real implementation and the stub.
pub trait FheEngine {
    /// Encrypt a plaintext page.
    fn encrypt_page(&self, plaintext: &[u8; PAGE_SIZE]) -> Result<EncryptedPage, FheError>;
    /// Decrypt a ciphertext page.
    fn decrypt_page(&self, ciphertext: &EncryptedPage) -> Result<[u8; PAGE_SIZE], FheError>;
    /// Add two encrypted pages element‐wise (for demo). Returns new ciphertext.
    fn add_pages(&self, a: &EncryptedPage, b: &EncryptedPage) -> Result<EncryptedPage, FheError>;
}

// ---------------------------------------------------------------------------
// Real TFHE implementation – only compiled when std + feature enabled.
// ---------------------------------------------------------------------------

#[cfg(all(feature = "homomorphic_encryption", feature = "std"))]
mod tfhe_impl {
    use super::*;
    use once_cell::sync::Lazy;
    use tfhe::integer::{gen_keys_radix, RadixCiphertext, U256};
    use tfhe::shortint::parameters::PARAM_MESSAGE_2_CARRY_2_KS_PBS;
    use tfhe::integer::ServerKey;

    // 4096 bytes = 512 * u32 (assuming 8-bit blocks but for demo we pack two bits per block)
    const NUM_BLOCKS: usize = 2048; // Each block holds 2 bits (see parameters above)

    /// Internal shared context – generated once.
    struct TfheContext {
        client_key: tfhe::integer::ClientKey,
        server_key: ServerKey,
    }

    static CONTEXT: Lazy<TfheContext> = Lazy::new(|| {
        let (ck, sk) = gen_keys_radix(PARAM_MESSAGE_2_CARRY_2_KS_PBS, NUM_BLOCKS as u64);
        TfheContext { client_key: ck, server_key: sk }
    });

    /// Convert 8-bit chunk into U256 for encryption (packs up to 256 bits).  Here
    /// we simply sign‐extend into lower bits.
    fn byte_to_u256(b: u8) -> U256 {
        U256::from(b as u64)
    }

    fn u256_to_byte(v: U256) -> u8 {
        (v.iter_u64_digits().next().unwrap_or(0) & 0xFF) as u8
    }

    pub struct TfheEngine;

    impl super::FheEngine for TfheEngine {
        fn encrypt_page(&self, plaintext: &[u8; PAGE_SIZE]) -> Result<EncryptedPage, FheError> {
            // Encrypt each byte independently for demo.  Production would use
            // vectorized shortint packing.
            let mut blocks: Vec<RadixCiphertext> = Vec::with_capacity(PAGE_SIZE);
            for &byte in plaintext.iter() {
                let ct = CONTEXT.client_key.encrypt_radix(byte_to_u256(byte), NUM_BLOCKS as usize);
                blocks.push(ct);
            }
            // Serialize ciphertexts (bincode)
            let data = bincode::serialize(&blocks).map_err(|_| FheError::InvalidPlaintext)?;
            Ok(EncryptedPage { data })
        }

        fn decrypt_page(&self, ciphertext: &EncryptedPage) -> Result<[u8; PAGE_SIZE], FheError> {
            let blocks: Vec<RadixCiphertext> =
                bincode::deserialize(&ciphertext.data).map_err(|_| FheError::InvalidCiphertext)?;
            if blocks.len() != PAGE_SIZE { return Err(FheError::InvalidCiphertext); }
            let mut out = [0u8; PAGE_SIZE];
            for (i, ct) in blocks.iter().enumerate() {
                let plain: U256 = CONTEXT.client_key.decrypt_radix(ct);
                out[i] = u256_to_byte(plain);
            }
            Ok(out)
        }

        fn add_pages(&self, a: &EncryptedPage, b: &EncryptedPage) -> Result<EncryptedPage, FheError> {
            let mut blocks_a: Vec<RadixCiphertext> =
                bincode::deserialize(&a.data).map_err(|_| FheError::InvalidCiphertext)?;
            let blocks_b: Vec<RadixCiphertext> =
                bincode::deserialize(&b.data).map_err(|_| FheError::InvalidCiphertext)?;
            if blocks_a.len() != PAGE_SIZE || blocks_b.len() != PAGE_SIZE { return Err(FheError::InvalidCiphertext); }
            for (a_ct, b_ct) in blocks_a.iter_mut().zip(blocks_b.iter()) {
                *a_ct = CONTEXT.server_key.add_parallelized(a_ct, b_ct);
            }
            let data = bincode::serialize(&blocks_a).map_err(|_| FheError::InvalidCiphertext)?;
            Ok(EncryptedPage { data })
        }
    }

    pub fn engine() -> impl super::FheEngine {
        TfheEngine
    }
}

// ---------------------------------------------------------------------------
// Stub implementation – builds in `no_std` or when feature disabled.
// ---------------------------------------------------------------------------

#[cfg(any(not(feature = "homomorphic_encryption"), not(feature = "std")))]
mod tfhe_impl {
    use super::*;

    #[derive(Debug)]
    pub struct StubEngine;

    impl FheEngine for StubEngine {
        fn encrypt_page(&self, _plaintext: &[u8; PAGE_SIZE]) -> Result<EncryptedPage, FheError> {
            Err(FheError::BackendUnavailable)
        }
        fn decrypt_page(&self, _ciphertext: &EncryptedPage) -> Result<[u8; PAGE_SIZE], FheError> {
            Err(FheError::BackendUnavailable)
        }
        fn add_pages(&self, _a: &EncryptedPage, _b: &EncryptedPage) -> Result<EncryptedPage, FheError> {
            Err(FheError::BackendUnavailable)
        }
    }

    pub fn engine() -> impl FheEngine {
        StubEngine
    }
}

// Re-export default engine getter.
pub use tfhe_impl::engine;

// ---------------------------------------------------------------------------
// Tests (behind std + feature)
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "homomorphic_encryption", feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let engine = engine();
        let mut page = [0u8; PAGE_SIZE];
        for i in 0..PAGE_SIZE { page[i] = (i % 256) as u8; }
        let ct = engine.encrypt_page(&page).unwrap();
        let dec = engine.decrypt_page(&ct).unwrap();
        assert_eq!(page, dec);
    }
} 