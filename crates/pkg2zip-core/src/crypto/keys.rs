//! Claves de PKG y derivación de la clave principal.
//!
//! Espejo de la selección de claves de `legacy/pkg2zip.c` (`key_type` de `pkg_header[0xe7]`).
//! Las claves son parte del formato público del proyecto original (ya estaban en el C).

use crate::crypto::aes::Aes128Ecb;
use crate::error::{Error, Result};

/// Clave de PKG de PSP (`key_type == 1`).
pub const PKG_PSP_KEY: [u8; 16] = [
    0x07, 0xf2, 0xc6, 0x82, 0x90, 0xb5, 0x0d, 0x2c, 0x33, 0x81, 0x8d, 0x70, 0x9b, 0x60, 0xe6, 0x2b,
];

/// Clave de PKG de PS3 (`key_type == 1`, o `key_type == 4` con `hdr_type == 1`).
pub const PKG_PS3_KEY: [u8; 16] = [
    0x2e, 0x7b, 0x71, 0xd7, 0xc9, 0xc9, 0xa1, 0x4e, 0xa3, 0x22, 0x1f, 0x18, 0x88, 0x28, 0xb8, 0xf8,
];

/// Clave Vita 2 (`key_type == 2`).
pub const PKG_VITA_2: [u8; 16] = [
    0xe3, 0x1a, 0x70, 0xc9, 0xce, 0x1d, 0xd7, 0x2b, 0xf3, 0xc0, 0x62, 0x29, 0x63, 0xf2, 0xec, 0xcb,
];

/// Clave Vita 3 (`key_type == 3`).
pub const PKG_VITA_3: [u8; 16] = [
    0x42, 0x3a, 0xca, 0x3a, 0x2b, 0xd5, 0x64, 0x9f, 0x96, 0x86, 0xab, 0xad, 0x6f, 0xd8, 0x80, 0x1f,
];

/// Clave Vita 4 (`key_type == 4`).
pub const PKG_VITA_4: [u8; 16] = [
    0xaf, 0x07, 0xfd, 0x59, 0x65, 0x25, 0x27, 0xba, 0xf1, 0x33, 0x89, 0x66, 0x8b, 0x17, 0xd9, 0xea,
];

/// Claves derivadas de una cabecera PKG.
#[derive(Debug, Clone, Copy)]
pub struct PkgKeys {
    /// Clave principal (AES-CTR de items y nombres).
    pub main: [u8; 16],
    /// Clave PS3 para items marcados con `psp_type != 0x90` (solo PSP/PSX/PS3 la usan).
    pub ps3: Option<[u8; 16]>,
}

/// Deriva las claves a partir de `key_type`, `hdr_type` e `iv` de la cabecera.
///
/// Para `key_type` 2, 3 y 4 la clave principal es `AES-ECB(clave_fija, iv)`; para 1 y 4+PS3 es una
/// clave fija.
///
/// # Errores
///
/// [`Error::UnsupportedKeyType`] si `key_type` no está en `1..=4`.
pub fn derive_keys(key_type: u8, hdr_type: u16, iv: &[u8; 16]) -> Result<PkgKeys> {
    let ecb = |key: &[u8; 16]| {
        let ctx = Aes128Ecb::new(key);
        let mut block = *iv;
        ctx.encrypt_block(&mut block);
        block
    };

    match key_type {
        1 => Ok(PkgKeys {
            main: PKG_PSP_KEY,
            ps3: Some(PKG_PS3_KEY),
        }),
        2 => Ok(PkgKeys {
            main: ecb(&PKG_VITA_2),
            ps3: None,
        }),
        3 => Ok(PkgKeys {
            main: ecb(&PKG_VITA_3),
            ps3: None,
        }),
        4 if hdr_type == 1 => Ok(PkgKeys {
            main: PKG_PS3_KEY,
            ps3: Some(PKG_PS3_KEY),
        }),
        4 => Ok(PkgKeys {
            main: ecb(&PKG_VITA_4),
            ps3: None,
        }),
        other => Err(Error::UnsupportedKeyType(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psp_and_ps3_have_both_keys() {
        let keys = derive_keys(1, 0, &[0u8; 16]).expect("key_type 1");
        assert_eq!(keys.main, PKG_PSP_KEY);
        assert_eq!(keys.ps3, Some(PKG_PS3_KEY));
    }

    #[test]
    fn vita_key_is_ecb_of_iv() {
        let iv = [0x11u8; 16];
        let keys = derive_keys(3, 0, &iv).expect("key_type 3");
        assert_eq!(keys.ps3, None);

        let mut expected = iv;
        Aes128Ecb::new(&PKG_VITA_3).encrypt_block(&mut expected);
        assert_eq!(keys.main, expected);
        assert_ne!(keys.main, iv);
    }

    #[test]
    fn ps3_uses_fixed_key_for_hdr_type_1() {
        let keys = derive_keys(4, 1, &[0x22u8; 16]).expect("key_type 4 PS3");
        assert_eq!(keys.main, PKG_PS3_KEY);
    }

    #[test]
    fn unknown_key_type_is_rejected() {
        let err = derive_keys(7, 0, &[0u8; 16]).expect_err("key_type 7 inválido");
        assert!(matches!(err, Error::UnsupportedKeyType(7)));
    }
}
