//! Post-quantum cryptography primitives (Task 4.1)
//!
//! Provides Kyber KEM wrappers for `no_std` + `alloc` environments.
//! Further algorithms (Dilithium, SPHINCS+) will be added in subsequent commits.

#![cfg_attr(not(test), no_std)]

extern crate alloc;
use alloc::vec::Vec;

use pqcrypto_kyber::{kyber512, kyber768};

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