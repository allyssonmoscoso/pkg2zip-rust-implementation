//! Cabecera de un PKG (192 bytes + 64 de extensión).

use crate::bytes::{get16be, get32be, get64be};
use crate::error::{Error, Result};

/// Tamaño de la cabecera.
pub const HEADER_SIZE: usize = 192;
/// Tamaño de la extensión de cabecera (se lee junto con la cabecera).
pub const HEADER_EXT_SIZE: usize = 64;
/// Bytes que hay que leer del inicio del archivo.
pub const HEADER_TOTAL: usize = HEADER_SIZE + HEADER_EXT_SIZE;

/// Firma `\x7fPKG`.
pub const PKG_MAGIC: u32 = 0x7f50_4b47;

/// Cabecera de un PKG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkgHeader {
    /// `hdr_type` (1 para PKG de PS3).
    pub hdr_type: u16,
    /// Offset de la tabla de metadatos.
    pub meta_offset: u32,
    /// Número de bloques de metadatos.
    pub meta_count: u32,
    /// Número de items.
    pub item_count: u32,
    /// Tamaño total declarado.
    pub total_size: u64,
    /// Offset del área cifrada.
    pub enc_offset: u64,
    /// Tamaño del área cifrada.
    pub enc_size: u64,
    /// IV de AES-CTR.
    pub iv: [u8; 16],
    /// `key_type` (1 PSP/PS3, 2 Vita2, 3 Vita3, 4 Vita4/PS3).
    pub key_type: u8,
    /// Serial de 9 caracteres de la cabecera (`0x37..0x40`), usado por PSP/PSX/PS3.
    pub serial: [u8; 9],
    /// Título de tema PSP en la cabecera (`0x44..0x54`).
    pub theme_title: [u8; 16],
    /// `CONTENT_ID` de PSM en la cabecera (`0x30..0x60`).
    pub content_id: [u8; 0x30],
}

impl PkgHeader {
    /// Parsea los primeros [`HEADER_TOTAL`] bytes del archivo.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] si el slice es más corto que la cabecera, [`Error::NotAPkg`] si la firma
    /// no coincide.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_TOTAL {
            return Err(Error::TooShort("pkg header"));
        }
        if get32be(bytes) != PKG_MAGIC {
            return Err(Error::NotAPkg);
        }

        let mut iv = [0u8; 16];
        iv.copy_from_slice(&bytes[0x70..0x80]);
        let mut serial = [0u8; 9];
        serial.copy_from_slice(&bytes[0x37..0x40]);
        let mut theme_title = [0u8; 16];
        theme_title.copy_from_slice(&bytes[0x44..0x54]);
        let mut content_id = [0u8; 0x30];
        content_id.copy_from_slice(&bytes[0x30..0x60]);

        Ok(Self {
            hdr_type: get16be(&bytes[6..]),
            meta_offset: get32be(&bytes[8..]),
            meta_count: get32be(&bytes[12..]),
            item_count: get32be(&bytes[20..]),
            total_size: get64be(&bytes[24..]),
            enc_offset: get64be(&bytes[32..]),
            enc_size: get64be(&bytes[40..]),
            iv,
            key_type: bytes[0xe7] & 7,
            serial,
            theme_title,
            content_id,
        })
    }

    /// Comprueba que el archivo tenga el tamaño que la cabecera declara.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] si el PKG es más pequeño que `total_size` o que la tabla de items.
    pub fn validate(&self, pkg_size: u64) -> Result<()> {
        if pkg_size < self.total_size {
            return Err(Error::Corrupt("pkg file is too small"));
        }
        let items_end = self.enc_offset + u64::from(self.item_count) * 32;
        if pkg_size < items_end {
            return Err(Error::Corrupt("pkg file is too small"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::{set16be, set32be, set64be};

    fn header_bytes() -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_TOTAL];
        set32be(&mut bytes[0..4], PKG_MAGIC);
        set16be(&mut bytes[6..8], 0);
        set32be(&mut bytes[8..12], 0x100);
        set32be(&mut bytes[12..16], 3);
        set32be(&mut bytes[20..24], 4);
        set64be(&mut bytes[24..32], 0x2000);
        set64be(&mut bytes[32..40], 0x400);
        set64be(&mut bytes[40..48], 0x1c00);
        bytes[0x70..0x80].copy_from_slice(&[0x5A; 16]);
        bytes[0xe7] = 3;
        bytes
    }

    #[test]
    fn parses_fields() {
        let header = PkgHeader::parse(&header_bytes()).expect("cabecera");
        assert_eq!(header.meta_offset, 0x100);
        assert_eq!(header.meta_count, 3);
        assert_eq!(header.item_count, 4);
        assert_eq!(header.total_size, 0x2000);
        assert_eq!(header.enc_offset, 0x400);
        assert_eq!(header.enc_size, 0x1c00);
        assert_eq!(header.iv, [0x5A; 16]);
        assert_eq!(header.key_type, 3);
    }

    #[test]
    fn rejects_short_or_bad_magic() {
        assert!(matches!(
            PkgHeader::parse(&[0u8; 10]),
            Err(Error::TooShort(_))
        ));
        let mut bytes = header_bytes();
        bytes[0] = 0;
        assert!(matches!(PkgHeader::parse(&bytes), Err(Error::NotAPkg)));
    }

    #[test]
    fn validate_checks_declared_sizes() {
        let header = PkgHeader::parse(&header_bytes()).expect("cabecera");
        header.validate(0x2000).expect("tamaño exacto");
        assert!(matches!(header.validate(0x1fff), Err(Error::Corrupt(_))));
        assert!(matches!(header.validate(0x4ff), Err(Error::Corrupt(_))));
    }
}
