//! Núcleo de `pkg2zip`: formatos PKG/SFO, criptografía, checksums y escritura de salida.
//!
//! Este crate no conoce la CLI ni la consola: recibe rutas y devuelve [`Result`]. La capa
//! `pkg2zip-cli` traduce errores a mensajes y códigos de salida.
//!
//! Estado de la migración: Fase 0 (esqueleto + AES/CTR). Ver `docs/PLAN-MIGRACION-RUST.md`.

#![deny(unsafe_code)]

pub mod bytes;
pub mod crypto;
pub mod error;

pub use error::{Error, Result};

/// Versión del crate (la que imprime `pkg2zip --version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
