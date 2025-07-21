//! Unified Quantum-resistant cryptography API
//! Supports Kyber (KEM), Dilithium (signature) and SPHINCS+ (fallback signature).
//! This high-level wrapper allows the rest of Zerovisor to stay algorithm-agnostic.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy)]
pub enum AlgoKind { Kyber512, Dilithium3, Sphincs128f }

#[derive(Debug, Clone)]
pub struct QuantumKeypair { pub pk: Vec<u8>, pub sk: Vec<u8> }

#[derive(Debug, Clone)]
pub struct CipherText(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct SharedSecret(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct Signature(pub Vec<u8>);

// ---------------- KEM operations ----------------

pub fn generate_keypair(algo: AlgoKind) -> QuantumKeypair {
    match algo {
        #[cfg(feature="pqcrypto")] AlgoKind::Kyber512 => {
            use pqcrypto_kyber::kyber512;
            let (pk, sk) = kyber512::keypair();
            QuantumKeypair { pk: pk.to_vec(), sk: sk.to_vec() }
        }
        _ => QuantumKeypair { pk: Vec::new(), sk: Vec::new() },
    }
}

pub fn encapsulate(algo: AlgoKind, pk: &[u8]) -> (CipherText, SharedSecret) {
    match algo {
        #[cfg(feature="pqcrypto")] AlgoKind::Kyber512 => {
            use pqcrypto_kyber::kyber512;
            let (ct, ss) = kyber512::encapsulate(kyber512::PublicKey::from_bytes(pk).unwrap());
            (CipherText(ct.to_vec()), SharedSecret(ss.to_vec()))
        }
        _ => (CipherText(Vec::new()), SharedSecret(Vec::new())),
    }
}

pub fn decapsulate(algo: AlgoKind, ct: &[u8], sk: &[u8]) -> SharedSecret {
    match algo {
        #[cfg(feature="pqcrypto")] AlgoKind::Kyber512 => {
            use pqcrypto_kyber::kyber512;
            let ss = kyber512::decapsulate(
                kyber512::Ciphertext::from_bytes(ct).unwrap(),
                kyber512::SecretKey::from_bytes(sk).unwrap(),
            );
            SharedSecret(ss.to_vec())
        }
        _ => SharedSecret(Vec::new()),
    }
}

// ---------------- Sign/Verify ----------------

pub fn sign(algo: AlgoKind, sk: &[u8], msg: &[u8]) -> Signature {
    match algo {
        #[cfg(feature="pqcrypto")] AlgoKind::Dilithium3 => {
            use pqcrypto_dilithium::dilithium3;
            let sig = dilithium3::sign_detached(msg, &dilithium3::SecretKey::from_bytes(sk).unwrap());
            Signature(sig.to_vec())
        }
        #[cfg(feature="pqcrypto")] AlgoKind::Sphincs128f => {
            use pqcrypto_sphincsplus::shake256s128f;
            let sig = shake256s128f::sign_detached(msg, &shake256s128f::SecretKey::from_bytes(sk).unwrap());
            Signature(sig.to_vec())
        }
        _ => Signature(Vec::new()),
    }
}

pub fn verify(algo: AlgoKind, pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    match algo {
        #[cfg(feature="pqcrypto")] AlgoKind::Dilithium3 => {
            use pqcrypto_dilithium::dilithium3;
            dilithium3::verify_detached(
                &dilithium3::Signature::from_bytes(sig).unwrap(),
                msg,
                &dilithium3::PublicKey::from_bytes(pk).unwrap(),
            ).is_ok()
        }
        #[cfg(feature="pqcrypto")] AlgoKind::Sphincs128f => {
            use pqcrypto_sphincsplus::shake256s128f;
            shake256s128f::verify_detached(
                &shake256s128f::Signature::from_bytes(sig).unwrap(),
                msg,
                &shake256s128f::PublicKey::from_bytes(pk).unwrap(),
            ).is_ok()
        }
        _ => false,
    }
} 