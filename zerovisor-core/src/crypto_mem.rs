//! AES-256-XTS page encryption engine (Task 4.2)
//! Works in `no_std` + `alloc` environment.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use aes::Aes256;
use aes::cipher::{KeyInit, generic_array::GenericArray};
use xts_mode::{Xts128, get_tweak_default};

pub const PAGE_SIZE: usize = 4096;

/// Encrypt a single 4 KiB page in place using AES-256-XTS.
/// `lba` is the logical block address (page index).
pub fn encrypt_page(page: &mut [u8; PAGE_SIZE], key1: &[u8; 32], key2: &[u8; 32], lba: u64) {
    // Build XTS cipher with two AES-256 keys.
    let cipher1 = Aes256::new(GenericArray::from_slice(key1));
    let cipher2 = Aes256::new(GenericArray::from_slice(key2));
    let xts = Xts128::<Aes256>::new(cipher1, cipher2);
    let tweak = get_tweak_default(lba as u128);
    xts.encrypt_sector(page, tweak);
}

/// Decrypt a single 4 KiB page in place.
pub fn decrypt_page(page: &mut [u8; PAGE_SIZE], key1: &[u8; 32], key2: &[u8; 32], lba: u64) {
    let cipher1 = Aes256::new(GenericArray::from_slice(key1));
    let cipher2 = Aes256::new(GenericArray::from_slice(key2));
    let xts = Xts128::<Aes256>::new(cipher1, cipher2);
    let tweak = get_tweak_default(lba as u128);
    xts.decrypt_sector(page, tweak);
} 