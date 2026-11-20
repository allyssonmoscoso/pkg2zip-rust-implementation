//! Criptografía de `pkg2zip`.
//!
//! * [`aes`] — AES-128 ECB/CTR.
//! * [`keys`] — claves fijas del formato y derivación de la clave principal.
//!
//! En fases posteriores se añadirán CMAC (RFC 4493) y la variante de cifrado de PSP
//! (`legacy/pkg2zip_aes.c`, `aes128_psp_decrypt`).

pub mod aes;
pub mod keys;

pub use aes::{Aes128Ecb, ctr_xor};
pub use keys::{
    PKG_PS3_KEY, PKG_PSP_KEY, PKG_VITA_2, PKG_VITA_3, PKG_VITA_4, PkgKeys, derive_keys,
};
