//! Bloques de metadatos de la cabecera PKG.
//!
//! Cada bloque son 16 bytes: `type: u32 BE`, `size: u32 BE` y 8 bytes de datos. El bucle del C
//! avanza `8 + size` bytes, que para los bloques que usamos (size = 8) es 16.
//!
//! Tipos relevantes:
//!
//! * `2`  — `content_type`
//! * `10` — directorio de instalación de DLC (string de 0x28 bytes en `data[8..]`)
//! * `13` — tabla de items (`items_offset`, `items_size`)
//! * `14` — SFO (`sfo_offset`, `sfo_size`)

use crate::bytes::get32be;
use crate::error::{Error, Result};

/// Tipo de bloque `content_type`.
pub const META_CONTENT_TYPE: u32 = 2;
/// Tipo de bloque de directorio de instalación (DLC).
pub const META_INSTALL_DIRECTORY: u32 = 10;
/// Tipo de bloque de la tabla de items.
pub const META_ITEMS: u32 = 13;
/// Tipo de bloque del SFO.
pub const META_SFO: u32 = 14;

/// Longitud del directorio de instalación (`install_directory[0x28]`).
pub const INSTALL_DIRECTORY_LEN: usize = 0x28;

/// Un bloque de metadatos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaBlock {
    /// Tipo de bloque.
    pub kind: u32,
    /// Tamaño que declara el bloque.
    pub size: u32,
    /// Siguientes 8 bytes del bloque.
    pub data: [u8; 8],
}

impl MetaBlock {
    /// Parsea los 16 bytes de un bloque.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] si el slice tiene menos de 16 bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 16 {
            return Err(Error::TooShort("metadata block"));
        }
        let mut data = [0u8; 8];
        data.copy_from_slice(&bytes[8..16]);
        Ok(Self {
            kind: get32be(bytes),
            size: get32be(&bytes[4..]),
            data,
        })
    }

    /// `content_type` declarado por un bloque tipo 2.
    #[must_use]
    pub fn content_type(&self) -> Option<u32> {
        (self.kind == META_CONTENT_TYPE).then(|| get32be(&self.data))
    }

    /// `(offset, size)` declarados por un bloque tipo 13 o 14.
    #[must_use]
    pub fn offset_size(&self) -> Option<(u32, u32)> {
        if self.kind == META_ITEMS || self.kind == META_SFO {
            Some((get32be(&self.data), get32be(&self.data[4..])))
        } else {
            None
        }
    }

    /// Bytes que ocupa el bloque en el archivo (cabecera de 8 + `size`).
    #[must_use]
    pub fn file_len(&self) -> u32 {
        8 + self.size
    }
}

/// Metadatos agregados de un PKG.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MetaInfo {
    /// `content_type` (bloque tipo 2).
    pub content_type: Option<u32>,
    /// Offset de la tabla de items, relativo a `enc_offset`.
    pub items_offset: u32,
    /// Tamaño de la región de items (usado para `head.bin`).
    pub items_size: u32,
    /// Offset del SFO (absoluto).
    pub sfo_offset: u32,
    /// Tamaño del SFO.
    pub sfo_size: u32,
    /// Directorio de instalación de un DLC, si existe.
    pub install_directory: Option<String>,
}

impl MetaInfo {
    /// Incorpora un bloque ya parseado.
    pub fn apply(&mut self, block: &MetaBlock) {
        match block.kind {
            META_CONTENT_TYPE => self.content_type = block.content_type(),
            META_ITEMS => {
                if let Some((offset, size)) = block.offset_size() {
                    self.items_offset = offset;
                    self.items_size = size;
                }
            }
            META_SFO => {
                if let Some((offset, size)) = block.offset_size() {
                    self.sfo_offset = offset;
                    self.sfo_size = size;
                }
            }
            _ => {}
        }
    }

    /// Incorpora el directorio de instalación de un bloque tipo 10.
    ///
    /// `data` son los `size` bytes de datos del bloque (los que siguen a los 8 bytes de cabecera).
    /// El C lee el string en `data[8..8 + 0x28]`.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] si el bloque no contiene los 0x28 bytes esperados.
    pub fn apply_install_directory(&mut self, data: &[u8]) -> Result<()> {
        let start = 8;
        let end = start + INSTALL_DIRECTORY_LEN;
        if data.len() < end {
            return Err(Error::TooShort("install directory block"));
        }
        let raw = &data[start..end];
        let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        self.install_directory = Some(String::from_utf8_lossy(&raw[..nul]).into_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::set32be;

    fn block(kind: u32, size: u32, first: u32, second: u32) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        set32be(&mut bytes[0..4], kind);
        set32be(&mut bytes[4..8], size);
        set32be(&mut bytes[8..12], first);
        set32be(&mut bytes[12..16], second);
        bytes
    }

    #[test]
    fn parses_content_type_block() {
        let block = MetaBlock::parse(&block(2, 8, 0x15, 0)).expect("bloque");
        assert_eq!(block.kind, 2);
        assert_eq!(block.file_len(), 16);
        assert_eq!(block.content_type(), Some(0x15));
        assert_eq!(block.offset_size(), None);
    }

    #[test]
    fn parses_offset_size_blocks() {
        let items = MetaBlock::parse(&block(13, 8, 0x40, 0x1000)).expect("items");
        assert_eq!(items.offset_size(), Some((0x40, 0x1000)));

        let sfo = MetaBlock::parse(&block(14, 8, 0x200, 0x180)).expect("sfo");
        assert_eq!(sfo.offset_size(), Some((0x200, 0x180)));
    }

    #[test]
    fn aggregates_meta_info() {
        let mut info = MetaInfo::default();
        info.apply(&MetaBlock::parse(&block(2, 8, 0x16, 0)).expect("tipo"));
        info.apply(&MetaBlock::parse(&block(13, 8, 0, 0x2000)).expect("items"));
        info.apply(&MetaBlock::parse(&block(14, 8, 0x300, 0x200)).expect("sfo"));

        assert_eq!(info.content_type, Some(0x16));
        assert_eq!((info.items_offset, info.items_size), (0, 0x2000));
        assert_eq!((info.sfo_offset, info.sfo_size), (0x300, 0x200));

        let mut data = vec![0u8; 0x30];
        data[8..12].copy_from_slice(b"DLC\0");
        info.apply_install_directory(&data).expect("install dir");
        assert_eq!(info.install_directory.as_deref(), Some("DLC"));

        assert!(matches!(
            info.apply_install_directory(&data[..0x10]),
            Err(Error::TooShort(_))
        ));
    }

    #[test]
    fn file_len_uses_declared_size() {
        let bytes = block(10, 0x30, 0, 0);
        let block = MetaBlock::parse(&bytes).expect("bloque");
        assert_eq!(block.file_len(), 0x38);
    }
}
