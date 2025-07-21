//! Security engine implementation

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::ZerovisorError;
use crate::crypto::{QuantumCrypto, kyber_encapsulate};
use crate::crypto_mem::{encrypt_page, decrypt_page, PAGE_SIZE};
use crate::attestation::{RemoteAttestation, AttestationReport};
use sha2::{Sha256, Digest};
use spin::Once;

/// Maximum number of security events stored in memory.
const MAX_EVENTS: usize = 1024;

/// Descriptor for a security-related hypervisor event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityEvent {
    /// Extended Page Table violation by guest.
    EptViolation {
        guest_pa: u64,
        guest_va: u64,
        error: u64,
    },
    /// VMEXIT latency exceeded target threshold (10 ns)
    PerfWarning {
        avg_latency_ns: u64,
        wcet_ns: Option<u64>,
    },
    /// Real-time deadline miss detected by scheduler.
    RealTimeDeadlineMiss {
        vm: u32,
        vcpu: u32,
        deadline_ns: u64,
        now_ns: u64,
    },
    /// Interrupt latency exceeded 1 microsecond target.
    InterruptLatencyViolation {
        vector: u8,
        latency_ns: u64,
    },
    /// Memory integrity verification failed (encrypted page tampering)
    MemoryIntegrityViolation {
        phys_addr: u64,
        expected_hash: [u8; 32],
        actual_hash: [u8; 32],
    },
    // Future event types will follow here.
}

/// Fixed-size ring buffer of security events (lock-free single producer).
static mut EVENT_BUF: [Option<SecurityEvent>; MAX_EVENTS] = [None; MAX_EVENTS];
static WRITE_IDX: AtomicUsize = AtomicUsize::new(0);

/// Global instance holder for security engine.
static SECURITY_ENGINE: Once<SecurityEngine> = Once::new();

/// Comprehensive security engine aggregating cryptography, attestation
/// and memory-encryption capabilities.
pub struct SecurityEngine {
    /// Unified PQ crypto material
    crypto: QuantumCrypto,
    /// Remote attestation subsystem (Dilithium based).
    attestation: RemoteAttestation,
    /// First half of 256-bit AES-XTS master key.
    enc_key1: [u8; 32],
    /// Second half of 256-bit AES-XTS master key.
    enc_key2: [u8; 32],
}

impl SecurityEngine {
    /// Instantiate the engine and derive all necessary key material.
    fn new() -> Self {
        // Generate PQ key material
        let crypto = QuantumCrypto::generate_keypairs();

        // Derive shared secret using Kyber self-encapsulation
        let ct = kyber_encapsulate(&crypto.kyber().public);

        // Derive 64-byte key material via SHA-256 (HKDF would be stronger, but
        // SHA-256 suffices for deterministic derivation here).
        let mut hasher = Sha256::new();
        hasher.update(&ct.shared_key);
        let digest1 = hasher.finalize_reset();
        hasher.update(&digest1);
        let digest2 = hasher.finalize();

        let mut enc_key1 = [0u8; 32];
        let mut enc_key2 = [0u8; 32];
        enc_key1.copy_from_slice(&digest1);
        enc_key2.copy_from_slice(&digest2);

        Self { crypto, attestation: RemoteAttestation::new(), enc_key1, enc_key2 }
    }

    /// Encrypt a guest memory page in-place using master keys.
    pub fn encrypt_page(&self, page: &mut [u8; PAGE_SIZE], lba: u64) {
        encrypt_page(page, &self.enc_key1, &self.enc_key2, lba);
    }

    /// Decrypt a guest memory page in-place.
    pub fn decrypt_page(&self, page: &mut [u8; PAGE_SIZE], lba: u64) {
        decrypt_page(page, &self.enc_key1, &self.enc_key2, lba);
    }

    /// Produce a fresh attestation report for the provided verifier nonce.
    pub fn attestation_report(&self, nonce: Option<[u8; 32]>) -> AttestationReport {
        self.attestation.generate_report(nonce)
    }

    /// Expose the attestation public key (Dilithium).
    pub fn attestation_pk(&self) -> &[u8] { self.attestation.public_key() }

    /// Sign attestation report using unified crypto wrapper.
    pub fn sign_report(&self, report: &[u8]) -> Vec<u8> { self.crypto.sign_attestation(report).unwrap() }
}

/// Record a security event into the global ring buffer.
pub fn record_event(ev: SecurityEvent) {
    let idx = WRITE_IDX.fetch_add(1, Ordering::Relaxed) % MAX_EVENTS;
    unsafe { EVENT_BUF[idx] = Some(ev); }
}

/// Initialize security engine (quantum-resistant crypto, attestation, memory encryption).
pub fn init() -> Result<(), ZerovisorError> {
    SECURITY_ENGINE.call_once(|| SecurityEngine::new());
    Ok(())
}

/// Access global security engine reference. Panics if `init()` not invoked.
pub fn engine() -> &'static SecurityEngine {
    SECURITY_ENGINE.get().expect("SecurityEngine not initialized")
}

/// Expose immutable slice of stored events for diagnostics.
pub fn events() -> &'static [Option<SecurityEvent>; MAX_EVENTS] {
    unsafe { &EVENT_BUF }
}