//! CMAC-AES-128 (RFC 4493), usado en los contenedores PSAR/EDAT de PSP.

use aes::Aes128;
use cmac::{Cmac, Mac, digest::KeyInit};

/// Calcula el CMAC-AES-128 de `data` con `key`.
///
/// Equivalente a `aes128_cmac` de `legacy/pkg2zip_aes.c`.
#[must_use]
pub fn cmac_aes128(key: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let mut mac =
        Cmac::<Aes128>::new_from_slice(key).expect("AES-128 siempre acepta claves de 16 bytes");
    mac.update(data);
    mac.finalize().into_bytes().into()
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

    // Vectores del RFC 4493, apéndice A.
    const KEY: &str = "2b7e151628aed2a6abf7158809cf4f3c";

    #[test]
    fn rfc4493_empty_message() {
        let key: [u8; 16] = hex(KEY);
        assert_eq!(
            cmac_aes128(&key, &[]),
            hex::<16>("bb1d6929e95937287fa37d129b756746")
        );
    }

    #[test]
    fn rfc4493_16_bytes() {
        let key: [u8; 16] = hex(KEY);
        let msg: [u8; 16] = hex("6bc1bee22e409f96e93d7e117393172a");
        assert_eq!(
            cmac_aes128(&key, &msg),
            hex::<16>("070a16b46b4d4144f79bdd9dd04a287c")
        );
    }

    #[test]
    fn rfc4493_40_bytes() {
        let key: [u8; 16] = hex(KEY);
        let msg: [u8; 40] =
            hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e5130c81c46a35ce411");
        assert_eq!(
            cmac_aes128(&key, &msg),
            hex::<16>("dfa66747de9ae63030ca32611497c827")
        );
    }

    #[test]
    fn rfc4493_64_bytes() {
        let key: [u8; 16] = hex(KEY);
        let msg: [u8; 64] = hex(
            "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51\
             30c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710",
        );
        assert_eq!(
            cmac_aes128(&key, &msg),
            hex::<16>("51f0bebf7e3b9d92fc49741779363cfe")
        );
    }
}
