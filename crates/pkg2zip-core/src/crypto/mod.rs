//! Criptografía de `pkg2zip`.
//!
//! * [`aes`] — AES-128 ECB/CTR.
//! * [`cmac`] — CMAC-AES-128 (RFC 4493).
//! * [`keys`] — claves fijas del formato y derivación de la clave principal.
//! * [`psp`] — variante de cifrado AES de PSP (`aes128_psp_decrypt`).

pub mod aes;
pub mod cmac;
pub mod keys;
pub mod psp;

pub use aes::{Aes128Ecb, ctr_xor};
pub use cmac::cmac_aes128;
pub use keys::{
    PKG_PS3_KEY, PKG_PSP_KEY, PKG_VITA_2, PKG_VITA_3, PKG_VITA_4, PkgKeys, derive_keys,
};
pub use psp::psp_decrypt;
