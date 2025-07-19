//! Post-quantum cryptography primitives (Task 4.1)
//!
//! Provides Kyber KEM wrappers for `no_std` + `alloc` environments.
//! Further algorithms (Dilithium, SPHINCS+) will be added in subsequent commits.

#![cfg_attr(not(test), no_std)]

extern crate alloc;
use alloc::vec::Vec;

use pqcrypto_kyber::{kyber512, kyber768};
use pqcrypto_dilithium::{dilithium3, dilithium5};

/// Kyber keypair (public + secret)
#[derive(Clone)]
pub struct KyberKeypair {
    pub public: Vec<u8>,
    pub secret: Vec<u8>,
}

/// Kyber encapsulated ciphertext and shared key
pub struct KyberCiphertext {
    pub cipher: Vec<u8>,
    pub shared_key: Vec<u8>,
}

/// Generate a Kyber keypair (default to kyber768)
pub fn kyber_generate() -> KyberKeypair {
    let (pk, sk) = kyber768::keypair();
    KyberKeypair { public: pk.as_bytes().to_vec(), secret: sk.as_bytes().to_vec() }
}

/// Encapsulate to produce ciphertext + shared secret
pub fn kyber_encapsulate(public_key: &[u8]) -> KyberCiphertext {
    let pk = kyber768::PublicKey::from_bytes(public_key).expect("invalid pk");
    let (ct, ss) = kyber768::encapsulate(&pk);
    KyberCiphertext { cipher: ct.as_bytes().to_vec(), shared_key: ss.as_bytes().to_vec() }
}

/// Decapsulate using secret key; returns shared secret
pub fn kyber_decapsulate(ciphertext: &[u8], secret_key: &[u8]) -> Vec<u8> {
    let ct = kyber768::Ciphertext::from_bytes(ciphertext).expect("invalid ct");
    let sk = kyber768::SecretKey::from_bytes(secret_key).expect("invalid sk");
    let ss = kyber768::decapsulate(&ct, &sk);
    ss.as_bytes().to_vec()
}

// ---------------------------------------------------------------------------
// Dilithium signature scheme (dilithium5 default)
// ---------------------------------------------------------------------------

/// Dilithium keypair (public, secret)
#[derive(Clone)]
pub struct DilithiumKeypair {
    pub public: Vec<u8>,
    pub secret: Vec<u8>,
}

/// Generate Dilithium keypair (dilithium5)
pub fn dilithium_generate() -> DilithiumKeypair {
    let (pk, sk) = dilithium5::keypair();
    DilithiumKeypair { public: pk.as_bytes().to_vec(), secret: sk.as_bytes().to_vec() }
}

/// Sign message with Dilithium secret key
pub fn dilithium_sign(secret_key: &[u8], message: &[u8]) -> Vec<u8> {
    let sk = dilithium5::SecretKey::from_bytes(secret_key).expect("invalid sk");
    let sig = dilithium5::sign(message, &sk);
    sig.as_bytes().to_vec()
}

/// Verify Dilithium signature, returns true if valid
pub fn dilithium_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let pk = dilithium5::PublicKey::from_bytes(public_key).expect("invalid pk");
    let sig = dilithium5::DetachedSignature::from_bytes(signature).expect("invalid sig");
    dilithium5::verify_detached_signature(&sig, message, &pk).is_ok()
} 