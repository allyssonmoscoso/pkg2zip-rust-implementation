//! Salida: escritor ZIP y fachada "zip o carpeta".
//!
//! Equivalente a `pkg2zip_zip.c` + `pkg2zip_out.c`.

pub mod sink;
pub mod zip;

pub use sink::Sink;
pub use zip::{ZIP_MAX_FILENAME, ZipWriter};
