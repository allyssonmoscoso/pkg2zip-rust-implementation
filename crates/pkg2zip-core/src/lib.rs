//! Núcleo de `pkg2zip`: formatos PKG/SFO, criptografía, checksums y escritura de salida.
//!
//! Este crate no conoce la CLI ni la consola: recibe rutas y devuelve [`Result`]. La capa
//! `pkg2zip-cli` traduce errores a mensajes y códigos de salida.
//!
//! Estado de la migración: Fase 1 (cripto, checksums, zRIF y parsers de formato).

#![deny(unsafe_code)]

pub mod bytes;
pub mod checksum;
pub mod crypto;
pub mod error;
pub mod output;
pub mod pkg;
pub mod plat;
pub mod vita;
pub mod zrif;

pub use error::{Error, Result};

/// Versión del crate (la que imprime `pkg2zip --version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
