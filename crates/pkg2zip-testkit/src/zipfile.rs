//! Lector mínimo de ZIP para comparar salidas en el arnés de paridad.
//!
//! Soporta entradas `STORE` y `DEFLATE`, valida el CRC y devuelve nombre + CRC + tamaño +
//! SHA-256 del contenido. No soporta ZIP64 (los fixtures de F2 son pequeños); si aparece un
//! tamaño `0xFFFFFFFF` se devuelve un error explícito.

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fs;
use std::io::Read;
use std::path::Path;

use pkg2zip_core::checksum::crc32;
use sha2::{Digest, Sha256};

/// Error del lector.
pub type Error = Box<dyn StdError + Send + Sync>;
/// Resultado del lector.
pub type Result<T> = std::result::Result<T, Error>;

/// Información comparable de una entrada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// CRC-32 declarado (y validado contra el contenido).
    pub crc: u32,
    /// Tamaño descomprimido.
    pub size: u64,
    /// SHA-256 hex del contenido.
    pub sha256: String,
}

/// Foto del zip: nombre de entrada -> información.
pub type Snapshot = BTreeMap<String, EntryInfo>;

/// Lee un zip y devuelve su contenido comparable.
///
/// # Errors
///
/// Si el archivo no es un zip válido, usa ZIP64 o un método de compresión desconocido.
pub fn snapshot(path: &Path) -> Result<Snapshot> {
    let bytes = fs::read(path)?;
    let eocd = find_eocd(&bytes)?;
    let count = u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into()?) as usize;
    let central_offset = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into()?) as usize;

    let mut result = Snapshot::new();
    let mut position = central_offset;

    for _ in 0..count {
        if bytes.get(position..position + 4) != Some(b"PK\x01\x02") {
            return Err("central directory corrupta".into());
        }
        let method = u16::from_le_bytes(bytes[position + 10..position + 12].try_into()?);
        let declared_crc = u32::from_le_bytes(bytes[position + 16..position + 20].try_into()?);
        let compressed = u32::from_le_bytes(bytes[position + 20..position + 24].try_into()?);
        let size = u32::from_le_bytes(bytes[position + 24..position + 28].try_into()?);
        let name_len = u16::from_le_bytes(bytes[position + 28..position + 30].try_into()?) as usize;
        let extra_len =
            u16::from_le_bytes(bytes[position + 30..position + 32].try_into()?) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[position + 32..position + 34].try_into()?) as usize;
        let local_offset_raw = u32::from_le_bytes(bytes[position + 42..position + 46].try_into()?);

        let name =
            String::from_utf8_lossy(&bytes[position + 46..position + 46 + name_len]).into_owned();
        position += 46 + name_len + extra_len + comment_len;

        if compressed == u32::MAX || size == u32::MAX || local_offset_raw == u32::MAX {
            return Err("zip64 no soportado por el testkit".into());
        }

        let data = read_entry_data(
            &bytes,
            local_offset_raw as usize,
            compressed as usize,
            method,
        )?;
        if crc32(&data) != declared_crc {
            return Err(format!("CRC inválido en '{name}'").into());
        }
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let digest = hasher.finalize();
        let mut sha = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            write!(sha, "{byte:02x}").expect("String no falla");
        }

        result.insert(
            name,
            EntryInfo {
                crc: declared_crc,
                size: u64::from(size),
                sha256: sha,
            },
        );
    }

    Ok(result)
}

fn read_entry_data(
    bytes: &[u8],
    local_offset: usize,
    compressed_size: usize,
    method: u16,
) -> Result<Vec<u8>> {
    if bytes.get(local_offset..local_offset + 4) != Some(b"PK\x03\x04") {
        return Err("cabecera local corrupta".into());
    }
    let name_len =
        u16::from_le_bytes(bytes[local_offset + 26..local_offset + 28].try_into()?) as usize;
    let extra_len =
        u16::from_le_bytes(bytes[local_offset + 28..local_offset + 30].try_into()?) as usize;
    let start = local_offset + 30 + name_len + extra_len;
    let end = start
        .checked_add(compressed_size)
        .ok_or("tamaño de entrada desbordado")?;
    let raw = bytes.get(start..end).ok_or("entrada truncada")?;

    match method {
        0 => Ok(raw.to_vec()),
        8 => {
            let mut decoder = flate2::read::DeflateDecoder::new(raw);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        other => Err(format!("método de compresión no soportado: {other}").into()),
    }
}

fn find_eocd(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 22 {
        return Err("archivo demasiado corto para ser zip".into());
    }
    let start = bytes.len().saturating_sub(22 + 65_535);
    for position in (start..=bytes.len() - 22).rev() {
        if &bytes[position..position + 4] == b"PK\x05\x06" {
            return Ok(position);
        }
    }
    Err("no se encontró el EOCD del zip".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkg2zip_core::output::ZipWriter;

    #[test]
    fn reads_stored_and_deflated_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.zip");
        let mut zip = ZipWriter::create(&path).expect("crear");
        zip.add_folder("app/ID").expect("carpeta");
        zip.begin_file("app/ID/a.bin", false).expect("a");
        zip.write_file(b"hola").expect("write");
        zip.end_file().expect("end");
        zip.begin_file("app/ID/b.bin", true).expect("b");
        zip.write_file(&b"pkg2zip ".repeat(100)).expect("write");
        zip.end_file().expect("end");
        zip.close().expect("close");

        let snapshot = snapshot(&path).expect("leer zip");
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot["app/ID/a.bin"].size, 4);
        assert_eq!(snapshot["app/ID/b.bin"].size, 800);
        assert!(snapshot.contains_key("app/ID/"));
    }
}
