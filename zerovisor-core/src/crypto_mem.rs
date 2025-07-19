//! AES-256-XTS page encryption engine (Task 4.2)
//! Works in `no_std` + `alloc` environment.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use aes::Aes256;
use cipher::{
    generic_array::GenericArray,
    xts::{AesXts, get_tweak},
    StreamCipher,
};

pub const PAGE_SIZE: usize = 4096;

/// Encrypt a single 4 KiB page in place using AES-256-XTS.
/// `lba` is the logical block address (page index).
pub fn encrypt_page(page: &mut [u8; PAGE_SIZE], key1: &[u8; 32], key2: &[u8; 32], lba: u64) {
    let cipher = AesXts::<Aes256>::new(GenericArray::from_slice(key1), GenericArray::from_slice(key2));
    cipher.encrypt_sector_exact(page, lba, get_tweak);
}

/// Decrypt a single 4 KiB page in place.
pub fn decrypt_page(page: &mut [u8; PAGE_SIZE], key1: &[u8; 32], key2: &[u8; 32], lba: u64) {
    let cipher = AesXts::<Aes256>::new(GenericArray::from_slice(key1), GenericArray::from_slice(key2));
    cipher.decrypt_sector_exact(page, lba, get_tweak);
} 