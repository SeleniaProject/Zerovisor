//! Post-quantum cryptography primitives (Task 4.1)
//!
//! Provides Kyber KEM wrappers for `no_std` + `alloc` environments.
//! Further algorithms (Dilithium, SPHINCS+) will be added in subsequent commits.

#![cfg_attr(not(test), no_std)]

extern crate alloc;
use alloc::vec::Vec;

use pqcrypto_kyber::{kyber512, kyber768};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use pqcrypto_dilithium::{dilithium3, dilithium5};
use pqcrypto_sphincsplus::sphincssha256128ssimple as sphincs128s;
// Bring trait methods (as_bytes/from_bytes) into scope.
use pqcrypto_traits::kem::{Ciphertext as KemCiphertext, PublicKey as KemPublicKey, SecretKey as KemSecretKey, SharedSecret};
use pqcrypto_traits::sign::{PublicKey as SigPublicKey, SecretKey as SigSecretKey, DetachedSignature as SigDetachedSignature, SignedMessage};

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

// ---------------------------------------------------------------------------
// SPHINCS+ SHA2-128s signature scheme
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SphincsKeypair {
    pub public: Vec<u8>,
    pub secret: Vec<u8>,
}

pub fn sphincs_generate() -> SphincsKeypair {
    let (pk, sk) = sphincs128s::keypair();
    SphincsKeypair { public: pk.as_bytes().to_vec(), secret: sk.as_bytes().to_vec() }
}

pub fn sphincs_sign(secret_key: &[u8], message: &[u8]) -> Vec<u8> {
    let sk = sphincs128s::SecretKey::from_bytes(secret_key).expect("invalid sk");
    let sig = sphincs128s::detached_sign(message, &sk);
    sig.as_bytes().to_vec()
}

pub fn sphincs_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let pk = sphincs128s::PublicKey::from_bytes(public_key).expect("invalid pk");
    let sig = sphincs128s::DetachedSignature::from_bytes(signature).expect("invalid sig");
    sphincs128s::verify_detached_signature(&sig, message, &pk).is_ok()
} 

/// Unified quantum-resistant crypto bundle used by SecurityEngine and plugins.
#[derive(Clone)]
pub struct QuantumCrypto {
    kyber: KyberKeypair,
    dilithium: DilithiumKeypair,
    sphincs: SphincsKeypair,
}

impl QuantumCrypto {
    /// Generate fresh keypairs for Kyber-768, Dilithium-5 and SPHINCS+-128s.
    pub fn generate_keypairs() -> Self {
        Self { kyber: kyber_generate(), dilithium: dilithium_generate(), sphincs: sphincs_generate() }
    }

    /// Encrypt data buffer using AES-256-GCM, key derived from Kyber shared secret.
    /// Output format: [KyberCipher (1088B)] [12B nonce] [ciphertext+tag]
    pub fn encrypt_memory(&self, data: &[u8]) -> Result<Vec<u8>, ()> {
        let ctxt = kyber_encapsulate(&self.kyber.public);

        // Derive 256-bit key via SHA-256 of shared secret.
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&ctxt.shared_key);
        let key_bytes = hasher.finalize();
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        // Random 96-bit nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, data).map_err(|_| ())?;

        let mut out = Vec::with_capacity(ctxt.cipher.len() + 12 + ciphertext.len());
        out.extend_from_slice(&ctxt.cipher);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt buffer created by `encrypt_memory`.
    pub fn decrypt_memory(&self, blob: &[u8]) -> Result<Vec<u8>, ()> {
        // 1. Extract Kyber ciphertext (length from kyber768::ciphertext_bytes())
        let ct_len = kyber768::ciphertext_bytes();
        if blob.len() < ct_len + 12 { return Err(()); }
        let kyber_ct = &blob[..ct_len];
        let nonce_bytes = &blob[ct_len..ct_len+12];
        let ciphertext = &blob[ct_len+12..];

        // 2. Decapsulate to derive shared secret
        let ss = kyber_decapsulate(kyber_ct, &self.kyber.secret);

        // 3. Derive AES key
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&ss);
        let key_bytes = hasher.finalize();
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher.decrypt(nonce, ciphertext).map_err(|_| ())
    }

    /// Sign arbitrary blob with Dilithium and return detached signature.
    pub fn sign_attestation(&self, report: &[u8]) -> Result<Vec<u8>, ()> {
        Ok(dilithium_sign(&self.dilithium.secret, report))
    }

    /// Verify detached signature using SPHINCS+ public key as secondary path.
    /// Returns true if either Dilithium or SPHINCS+ signature validates.
    pub fn verify_signature(&self, msg: &[u8], sig: &[u8]) -> bool {
        dilithium_verify(&self.dilithium.public, msg, sig) || sphincs_verify(&self.sphincs.public, msg, sig)
    }

    /// Accessors
    pub fn kyber(&self) -> &KyberKeypair { &self.kyber }
    pub fn dilithium(&self) -> &DilithiumKeypair { &self.dilithium }
    pub fn sphincs(&self) -> &SphincsKeypair { &self.sphincs }
} 

pub mod crypto_mem;
pub mod quantum_crypto;
pub use quantum_crypto::*; 