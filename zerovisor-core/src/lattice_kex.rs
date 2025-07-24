//! Lattice-based (Kyber) key exchange for inter-VM communication
//!
//! Each VM/node owns a Kyber keypair generated at boot.  To establish a shared
//! symmetric key, the initiator sends its public key in a ClusterMsg::Custom
//! with discriminator 0xCC.  The responder encapsulates, returns ciphertext &
//! its own public key.  Both sides derive a 32-byte shared secret that can be
//! used for AEAD channels (not implemented here).

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use pqcrypto_kyber::kyber768 as kyber;
use crate::cluster::{ClusterManager, NodeId};
use crate::fault::Msg;

static SESSION_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone)]
pub struct KeyPair { pub pk: Vec<u8>, pub sk: Vec<u8> }
static mut KEYPAIR: Option<KeyPair> = None;

pub fn init() {
    let (pk, sk) = kyber::keypair();
    unsafe { KEYPAIR = Some(KeyPair { pk: pk.as_bytes().to_vec(), sk: sk.as_bytes().to_vec() }); }
}

fn kp() -> &'static KeyPair { unsafe { KEYPAIR.as_ref().expect("kex init") } }

/// Initiate key exchange with `dest` node. Returns session id.
pub fn initiate(dest: NodeId) -> u32 {
    let sid = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let mut payload = Vec::with_capacity(4 + kp().pk.len());
    payload.extend_from_slice(&sid.to_le_bytes());
    payload.extend_from_slice(&kp().pk);
    let msg = Msg::Custom(0xCC, Box::leak(payload.into_boxed_slice()));
    ClusterManager::global().transport.send_msg(dest, &msg, &mut [0u8;256]).ok();
    sid
}

/// Handle inbound KEX message.
pub fn on_msg(src: NodeId, payload: &[u8]) {
    if payload.len() < 4 { return; }
    let sid = u32::from_le_bytes([payload[0],payload[1],payload[2],payload[3]]);
    let peer_pk = &payload[4..];
    // If length equals Kyber768 public key size, act as responder.
    if peer_pk.len() == kyber::public_key_length() {
        let (ct, ss) = kyber::encapsulate(kyber::PublicKey::from_bytes(peer_pk).unwrap());
        // send back ct || my_pk
        let mut resp = Vec::with_capacity(4 + ct.as_bytes().len() + kp().pk.len());
        resp.extend_from_slice(&sid.to_le_bytes());
        resp.extend_from_slice(ct.as_bytes());
        resp.extend_from_slice(&kp().pk);
        let msg = Msg::Custom(0xCC, Box::leak(resp.into_boxed_slice()));
        ClusterManager::global().transport.send_msg(src, &msg, &mut [0u8;512]).ok();
        store_secret(sid, ss.as_bytes());
    } else {
        // initiator receives response: ct || pk_responder
        let ct_len = kyber::ciphertext_length();
        if payload.len() < 4 + ct_len { return; }
        let ct = &payload[4..4+ct_len];
        let _peer_pk = &payload[4+ct_len..];
        let ss = kyber::decapsulate(
            kyber::Ciphertext::from_bytes(ct).unwrap(),
            kyber::SecretKey::from_bytes(&kp().sk).unwrap(),
        );
        store_secret(sid, ss.as_bytes());
    }
}

fn store_secret(id: u32, secret: &[u8]) {
    // For demo, just log first 8 bytes.
    let mut arr = [0u8;8];
    arr.copy_from_slice(&secret[..8]);
    crate::log!("[kex] session {} shared {:#x}", id, u64::from_le_bytes(arr));
} 