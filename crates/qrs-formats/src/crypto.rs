//! The two AES-256 modes Rocksmith uses, written on the raw block cipher: CFB with 128-bit segments and a zero
//! IV (the PSARC table of contents) and CTR with a 128-bit big-endian counter (SNG charts).

use aes::Aes256;
use aes::cipher::{BlockCipherEncrypt, KeyInit};

fn cipher(key: &[u8; 32]) -> Aes256 {
    Aes256::new(key.into())
}

fn encrypt_block(c: &Aes256, block: &[u8; 16]) -> [u8; 16] {
    let mut b = (*block).into();
    c.encrypt_block(&mut b);
    b.into()
}

/// AES-256-CFB128 decryption, IV = 0. Works on any length (the last partial block uses a truncated keystream,
/// which is what padding with zeros and cutting the result back does).
pub fn cfb_decrypt(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let c = cipher(key);
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let ks = encrypt_block(&c, &prev);
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        for (i, b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
        prev = block;
    }
    out
}

/// AES-256-CFB128 encryption, IV = 0 (for building test archives and writing packages).
pub fn cfb_encrypt(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    let c = cipher(key);
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let ks = encrypt_block(&c, &prev);
        let mut block = [0u8; 16];
        for (i, b) in chunk.iter().enumerate() {
            block[i] = b ^ ks[i];
        }
        out.extend_from_slice(&block[..chunk.len()]);
        prev = block;
    }
    out
}

/// AES-256-CTR with a 128-bit big-endian counter starting at `iv` (same operation both ways).
pub fn ctr_apply(data: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let c = cipher(key);
    let mut counter = u128::from_be_bytes(*iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let ks = encrypt_block(&c, &counter.to_be_bytes());
        out.extend(chunk.iter().zip(ks.iter()).map(|(a, b)| a ^ b));
        counter = counter.wrapping_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfb_roundtrip_any_length() {
        let key = [7u8; 32];
        for n in [0, 1, 15, 16, 17, 100] {
            let data: Vec<u8> = (0..n as u8).collect();
            assert_eq!(cfb_decrypt(&cfb_encrypt(&data, &key), &key), data);
        }
    }

    #[test]
    fn ctr_is_an_involution() {
        let key = [3u8; 32];
        let iv = [0xffu8; 16]; // counter wraps
        let data: Vec<u8> = (0..50).collect();
        assert_eq!(ctr_apply(&ctr_apply(&data, &key, &iv), &key, &iv), data);
    }

    #[test]
    fn aes256_known_answer() {
        // FIPS-197 C.3
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let pt: [u8; 16] = core::array::from_fn(|i| (i as u8) * 0x11);
        let ct = encrypt_block(&cipher(&key), &pt);
        assert_eq!(
            ct,
            [
                0x8e, 0xa2, 0xb7, 0xca, 0x51, 0x67, 0x45, 0xbf, 0xea, 0xfc, 0x49, 0x90, 0x4b, 0x49, 0x60,
                0x89
            ]
        );
    }
}
