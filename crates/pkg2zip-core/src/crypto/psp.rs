//! Variante de cifrado AES usada por PSP ("kirk").
//!
//! Equivalente a `aes128_psp_decrypt` de `legacy/pkg2zip_aes.c`: es un CBC sin padding donde el
//! "bloque" es `iv` con un contador de 32 bits little-endian en los últimos 4 bytes, y el IV inicial
//! es `iv` (index != 0) o ceros (index == 0).

use crate::crypto::aes::Aes128Ecb;
use crate::error::{Error, Result};

/// Descifra `data` en sitio con la variante PSP.
///
/// `index` es el índice de bloque (se suma en little-endian a los últimos 4 bytes del IV).
///
/// # Errores
///
/// [`Error::Corrupt`] si `data.len()` no es múltiplo de 16.
pub fn psp_decrypt(cipher: &Aes128Ecb, iv: &[u8; 16], index: u32, data: &mut [u8]) -> Result<()> {
    if !data.len().is_multiple_of(16) {
        return Err(Error::Corrupt("PSP AES data is not 16-byte aligned"));
    }

    let mut prev = [0u8; 16];
    if index != 0 {
        prev[..12].copy_from_slice(&iv[..12]);
        prev[12..].copy_from_slice(&index.to_le_bytes());
    }

    let mut block = *iv;
    block[12..].copy_from_slice(&index.to_le_bytes());

    let (blocks, _rest) = data.as_chunks_mut::<16>();
    for chunk in blocks {
        let counter = u32::from_le_bytes(block[12..].try_into().expect("4 bytes")) + 1;
        block[12..].copy_from_slice(&counter.to_le_bytes());

        let mut keystream = block;
        cipher.decrypt_block(&mut keystream);

        for (byte, (prev_byte, key_byte)) in chunk.iter_mut().zip(prev.iter().zip(keystream.iter()))
        {
            *byte ^= prev_byte ^ key_byte;
        }

        prev = block;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectores generados con el binario C de `legacy/` (MSVC x64, ruta AES-NI):
    /// `aes128_psp_decrypt` con clave `0x40..0x4f`, IV `0x80..0x8f`.
    const GOLDEN_INDEX_0: &str = "b48c54e0d985d36a6a0a5ab189adf772438c2508a571576948578c8285536a9f\
                                  c437a6f51bb165595d489c704875232dcc4629cd6239b671a4f4db5d3c4e9781";
    const GOLDEN_INDEX_5: &str = "e51536e0a4850f36bda7c35755dd0ea27b4666c6a46ecf1db2fe79a1e30cf24e";

    fn hex(s: &str) -> Vec<u8> {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..clean.len() / 2)
            .map(|i| u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).expect("hex válido"))
            .collect()
    }

    fn key() -> [u8; 16] {
        let mut k = [0u8; 16];
        for (i, b) in k.iter_mut().enumerate() {
            *b = (0x40 + i) as u8;
        }
        k
    }

    fn iv() -> [u8; 16] {
        let mut v = [0u8; 16];
        for (i, b) in v.iter_mut().enumerate() {
            *b = (0x80 + i) as u8;
        }
        v
    }

    #[test]
    fn golden_index_zero() {
        let cipher = Aes128Ecb::new(&key());
        let mut data: Vec<u8> = (0..64).map(|i| (i * 3 + 1) as u8).collect();
        psp_decrypt(&cipher, &iv(), 0, &mut data).expect("descifrado");
        assert_eq!(data, hex(GOLDEN_INDEX_0));
    }

    #[test]
    fn golden_index_five() {
        let cipher = Aes128Ecb::new(&key());
        let mut data: Vec<u8> = (0..32).map(|i| (0xA0 + i) as u8).collect();
        psp_decrypt(&cipher, &iv(), 5, &mut data).expect("descifrado");
        assert_eq!(data, hex(GOLDEN_INDEX_5));
    }

    #[test]
    fn rejects_unaligned_data() {
        let cipher = Aes128Ecb::new(&key());
        let mut data = [0u8; 17];
        let err = psp_decrypt(&cipher, &iv(), 0, &mut data).expect_err("debe fallar");
        assert!(matches!(err, Error::Corrupt(_)));
    }
}
