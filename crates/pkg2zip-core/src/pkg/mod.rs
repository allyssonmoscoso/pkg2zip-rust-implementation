//! Formato PKG: cabecera, metadatos, SFO, tipo y región.

pub mod header;
pub mod kind;
pub mod meta;
pub mod region;
pub mod sfo;

pub use header::{HEADER_EXT_SIZE, HEADER_SIZE, HEADER_TOTAL, PKG_MAGIC, PkgHeader};
pub use kind::Kind;
pub use meta::{INSTALL_DIRECTORY_LEN, MetaBlock, MetaInfo};
pub use region::region;
pub use sfo::Sfo;
