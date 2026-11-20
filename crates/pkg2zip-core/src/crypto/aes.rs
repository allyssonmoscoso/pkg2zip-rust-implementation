//! AES-128 en modo ECB y CTR, sobre los crates `aes`/`ctr` de RustCrypto.
//!
//! Equivalencias con `legacy/pkg2zip_aes.c`:
//!
//! * ECB de un bloque  -> [`Aes128Ecb`]
//! * `aes128_ctr_xor`  -> [`ctr_xor`] (contador big-endian de 128 bits)
//!
//! La detección de AES-NI es interna a `aes`: en x86_64 el crate usa instrucciones AES cuando la
//! CPU las soporta y cae a una implementación en Rust si no.

use aes::Aes128;
use cipher::block::{BlockCipherDecrypt, BlockCipherEncrypt};
use cipher::{Block, KeyInit};
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};

use crate::error::{Error, Result};

/// Contexto AES-128 para cifrar/descifrar bloques sueltos (ECB de un bloque).
#[derive(Clone)]
pub struct Aes128Ecb {
    cipher: Aes128,
}

impl std::fmt::Debug for Aes128Ecb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Aes128Ecb")
    }
}

impl Aes128Ecb {
    /// Crea el contexto a partir de una clave de 16 bytes.
    #[must_use]
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            cipher: Aes128::new_from_slice(key).expect("AES-128 siempre acepta 16 bytes"),
        }
    }

    /// Cifra un bloque en sitio (equivalente a `aes128_ecb_encrypt`).
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        let mut b = Block::<Aes128>::from(*block);
        self.cipher.encrypt_block(&mut b);
        *block = b.into();
    }

    /// Descifra un bloque en sitio (equivalente a `aes128_ecb_decrypt`).
    pub fn decrypt_block(&self, block: &mut [u8; 16]) {
        let mut b = Block::<Aes128>::from(*block);
        self.cipher.decrypt_block(&mut b);
        *block = b.into();
    }
}

/// Aplica AES-128-CTR al buffer, arrancando en el bloque `block_offset`.
///
/// El contador es de 128 bits big-endian, igual que en el C: primero se inicializa con `iv` y luego
/// se le suma el número de bloque. `block_offset` se expresa en bloques de 16 bytes, no en bytes.
///
/// # Errores
///
/// Devuelve [`Error::Corrupt`] si `block_offset * 16` desborda `u64` (imposible para un PKG real,
/// porque se valida contra el tamaño del archivo) o si key/iv no miden 16 bytes.
pub fn ctr_xor(key: &[u8; 16], iv: &[u8; 16], block_offset: u64, data: &mut [u8]) -> Result<()> {
    let stream_pos = block_offset
        .checked_mul(16)
        .ok_or(Error::Corrupt("CTR block offset overflow"))?;

    let mut cipher = Ctr128BE::<Aes128>::new_from_slices(key, iv)
        .map_err(|_| Error::Corrupt("invalid AES-CTR key/iv length"))?;
    cipher.seek(stream_pos);
    cipher.apply_keystream(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex<const N: usize>(s: &str) -> [u8; N] {
        let mut out = [0u8; N];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex válido");
        }
        out
    }

    #[test]
    fn fips197_ecb_encrypt() {
        let key: [u8; 16] = hex("000102030405060708090a0b0c0d0e0f");
        let mut block: [u8; 16] = hex("00112233445566778899aabbccddeeff");
        let ctx = Aes128Ecb::new(&key);
        ctx.encrypt_block(&mut block);
        assert_eq!(block, hex::<16>("69c4e0d86a7b0430d8cdb78070b4c55a"));

        ctx.decrypt_block(&mut block);
        assert_eq!(block, hex::<16>("00112233445566778899aabbccddeeff"));
    }

    #[test]
    fn nist_sp800_38a_ctr_128() {
        // NIST SP 800-38A, F.5.1 (AES-128-CTR), primer bloque.
        let key: [u8; 16] = hex("2b7e151628aed2a6abf7158809cf4f3c");
        let iv: [u8; 16] = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let plain: [u8; 16] = hex("6bc1bee22e409f96e93d7e117393172a");
        let expected: [u8; 16] = hex("874d6191b620e3261bef6864990db6ce");

        let mut buf = plain;
        ctr_xor(&key, &iv, 0, &mut buf).expect("CTR válido");
        assert_eq!(buf, expected);

        // El mismo bloque vía offset explícito debe dar el mismo keystream que avanzar 32 bytes.
        let mut stream = [0u8; 32];
        ctr_xor(&key, &iv, 0, &mut stream).expect("CTR válido");
        let mut second = [0u8; 16];
        ctr_xor(&key, &iv, 1, &mut second).expect("CTR válido");
        assert_eq!(&stream[16..], &second);
    }

    #[test]
    fn ctr_xor_roundtrip_and_partial_block() {
        let key: [u8; 16] = hex("00112233445566778899aabbccddeeff");
        let iv: [u8; 16] = hex("0f0e0d0c0b0a09080706050403020100");
        let original = b"pkg2zip: bloque parcial de 37 bytes".to_vec();
        assert!(
            !original.len().is_multiple_of(16),
            "el buffer debe acabar en bloque parcial"
        );

        let mut data = original.clone();
        ctr_xor(&key, &iv, 3, &mut data).expect("CTR válido");
        assert_ne!(data, original);

        ctr_xor(&key, &iv, 3, &mut data).expect("CTR válido");
        assert_eq!(data, original);
    }
}
