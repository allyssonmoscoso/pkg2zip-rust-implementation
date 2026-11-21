//! Parser de `PARAM.SFO` (PSF) de PS Vita/PSP/PS3.
//!
//! Port de `parse_sfo_content` (`legacy/pkg2zip.c`), incluyendo la sanitización del título para
//! que sea un nombre de archivo válido en Windows.

use crate::bytes::{get16le, get32le};
use crate::error::{Error, Result};

/// Magic de SFO leído como `u32` little endian.
pub const SFO_MAGIC: u32 = 0x4653_5000;
/// Tamaño máximo aceptado (el C usa un buffer de 16 KiB).
pub const MAX_SFO_SIZE: usize = 16 * 1024;
/// Longitud máxima del título en bytes (el C usa un buffer de 256).
const MAX_TITLE_LEN: usize = 255;
/// Longitud máxima de los demás strings (content id, categoría, ...).
const MAX_STRING_LEN: usize = 255;

/// Campos de un SFO que necesita `pkg2zip`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sfo {
    /// `STITLE` o `TITLE` (sanitizado).
    pub title: Option<String>,
    /// `CONTENT_ID`.
    pub content_id: Option<String>,
    /// `CATEGORY`.
    pub category: Option<String>,
    /// `APP_VER` (sin ceros decorativos, como el C).
    pub app_ver: Option<String>,
    /// `PSP2_DISP_VER` (sin ceros decorativos, como el C).
    pub min_version: Option<String>,
    /// `DISC_ID`.
    pub disc_id: Option<String>,
}

impl Sfo {
    /// Título para nombres de archivo, o cadena vacía.
    #[must_use]
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or("")
    }

    /// `CONTENT_ID` o cadena vacía.
    #[must_use]
    pub fn content_id(&self) -> &str {
        self.content_id.as_deref().unwrap_or("")
    }

    /// `CATEGORY` o cadena vacía.
    #[must_use]
    pub fn category(&self) -> &str {
        self.category.as_deref().unwrap_or("")
    }
}

/// Parsea un SFO completo.
///
/// # Errors
///
/// [`Error::Corrupt`] con los mismos mensajes que el C si la firma, el tamaño o los offsets no son
/// válidos, o si falta el título.
pub fn parse(bytes: &[u8]) -> Result<Sfo> {
    if bytes.len() < 16 {
        return Err(Error::Corrupt("sfo information is too small"));
    }
    if bytes.len() > MAX_SFO_SIZE {
        return Err(Error::Corrupt(
            "sfo information is too big, pkg file is probably corrupted",
        ));
    }
    if get32le(bytes) != SFO_MAGIC {
        return Err(Error::Corrupt("incorrect sfo signature"));
    }

    let keys_offset = get32le(&bytes[8..]) as usize;
    let values_offset = get32le(&bytes[12..]) as usize;
    let count = get32le(&bytes[16..]) as usize;

    if 20 + count.saturating_mul(16) > bytes.len() {
        return Err(Error::Corrupt("sfo information is too small"));
    }

    let mut title_index: Option<usize> = None;
    let mut content_index = None;
    let mut category_index = None;
    let mut minver_index = None;
    let mut pkgver_index = None;
    let mut discid_index = None;

    for index in 0..count {
        let entry = 20 + index * 16;
        let key_offset = usize::from(get16le(&bytes[entry..]));
        let key = c_string(bytes, keys_offset + key_offset)?;

        match key {
            b"TITLE" => {
                if title_index.is_none() {
                    title_index = Some(index);
                }
            }
            b"STITLE" => title_index = Some(index),
            b"CONTENT_ID" => content_index = Some(index),
            b"CATEGORY" => category_index = Some(index),
            b"PSP2_DISP_VER" => minver_index = Some(index),
            b"APP_VER" => pkgver_index = Some(index),
            b"DISC_ID" => discid_index = Some(index),
            _ => {}
        }
    }

    let title_index = title_index.ok_or(Error::Corrupt(
        "cannot find title from sfo file, pkg is probably corrupted",
    ))?;

    let mut sfo = Sfo::default();

    let title = value(bytes, values_offset, title_index)?;
    sfo.title = Some(sanitize_title(title));

    if let Some(index) = content_index {
        sfo.content_id = Some(string_value(bytes, values_offset, index)?);
    }
    if let Some(index) = category_index {
        sfo.category = Some(string_value(bytes, values_offset, index)?);
    }
    if let Some(index) = discid_index {
        sfo.disc_id = Some(string_value(bytes, values_offset, index)?);
    }
    if let Some(index) = minver_index {
        sfo.min_version = Some(min_version_value(bytes, values_offset, index)?);
    }
    if let Some(index) = pkgver_index {
        sfo.app_ver = Some(app_version_value(bytes, values_offset, index)?);
    }

    Ok(sfo)
}

fn value(bytes: &[u8], values_offset: usize, index: usize) -> Result<&[u8]> {
    let entry = 20 + index * 16;
    let data_offset = get32le(&bytes[entry + 12..]) as usize;
    c_string(bytes, values_offset + data_offset)
}

fn string_value(bytes: &[u8], values_offset: usize, index: usize) -> Result<String> {
    let raw = value(bytes, values_offset, index)?;
    Ok(String::from_utf8_lossy(&raw[..raw.len().min(MAX_STRING_LEN)]).into_owned())
}

/// `APP_VER`: se salta el cero inicial, sin recortar el final (como el C).
fn app_version_value(bytes: &[u8], values_offset: usize, index: usize) -> Result<String> {
    let raw = value(bytes, values_offset, index)?;
    let raw = raw.strip_prefix(b"0").unwrap_or(raw);
    Ok(String::from_utf8_lossy(raw).into_owned())
}

/// `PSP2_DISP_VER`: se salta el cero inicial y se recorta un cero final (como el C).
fn min_version_value(bytes: &[u8], values_offset: usize, index: usize) -> Result<String> {
    let raw = value(bytes, values_offset, index)?;
    let raw = raw.strip_prefix(b"0").unwrap_or(raw);
    let mut out = raw.to_vec();
    if out.last() == Some(&b'0') {
        out.pop();
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn c_string(bytes: &[u8], offset: usize) -> Result<&[u8]> {
    if offset >= bytes.len() {
        return Err(Error::Corrupt("sfo offset out of range"));
    }
    let rest = &bytes[offset..];
    let end = rest
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(Error::Corrupt("sfo string is not terminated"))?;
    Ok(&rest[..end])
}

/// Sanitiza el título como el C: conserva `32..=126` salvo `<>"/\|?*`, convierte `:` en `" -"`,
/// `\n` en espacio y conserva los bytes >= 128 (UTF-8), con tope de 255 bytes.
fn sanitize_title(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut max = MAX_TITLE_LEN;
    let mut index = 0;

    while index < max && index < raw.len() {
        let byte = raw[index];
        if byte == 0 {
            break;
        }
        if (32..127).contains(&byte) && !b"<>\"\\/|?*".contains(&byte) {
            if byte == b':' {
                out.extend_from_slice(b" -");
                max -= 1;
            } else {
                out.push(byte);
            }
        } else if byte == b'\n' {
            out.push(b' ');
        } else if byte >= 128 {
            out.push(byte);
        }
        index += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::{align_up, set16le, set32le};

    fn build(entries: &[(&str, &str)]) -> Vec<u8> {
        let count = entries.len();
        let keys_offset = 20 + 16 * count;
        let mut keys = Vec::new();
        let mut key_offsets = Vec::new();
        for (key, _) in entries {
            key_offsets.push(keys.len() as u16);
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);
        }
        let values_offset = align_up(keys_offset + keys.len(), 4);

        let mut values = Vec::new();
        let mut data_offsets = Vec::new();
        let mut lengths = Vec::new();
        for (_, value) in entries {
            data_offsets.push(values.len() as u32);
            values.extend_from_slice(value.as_bytes());
            values.push(0);
            let len = value.len() + 1;
            lengths.push(len as u32);
            let padded = align_up(len, 4);
            values.resize(values.len() + (padded - len), 0);
        }

        let mut out = vec![0u8; values_offset + values.len()];
        set32le(&mut out[0..4], SFO_MAGIC);
        set32le(&mut out[4..8], 0x0000_0101);
        set32le(&mut out[8..12], keys_offset as u32);
        set32le(&mut out[12..16], values_offset as u32);
        set32le(&mut out[16..20], count as u32);
        for index in 0..count {
            let entry = 20 + index * 16;
            set16le(&mut out[entry..], key_offsets[index]);
            set16le(&mut out[entry + 2..], 0x0404);
            set32le(&mut out[entry + 4..], lengths[index]);
            set32le(
                &mut out[entry + 8..],
                align_up(lengths[index] as usize, 4) as u32,
            );
            set32le(&mut out[entry + 12..], data_offsets[index]);
        }
        out[keys_offset..keys_offset + keys.len()].copy_from_slice(&keys);
        out[values_offset..values_offset + values.len()].copy_from_slice(&values);
        out
    }

    #[test]
    fn parses_main_fields() {
        let sfo = build(&[
            ("TITLE", "Mi Juego"),
            ("CONTENT_ID", "UP0000-PCSE00000_00-0000000000000000"),
            ("CATEGORY", "gd"),
            ("APP_VER", "01.00"),
            ("PSP2_DISP_VER", "03.61"),
            ("DISC_ID", "PCSE00000"),
        ]);
        let parsed = parse(&sfo).expect("SFO");
        assert_eq!(parsed.title(), "Mi Juego");
        assert_eq!(parsed.content_id(), "UP0000-PCSE00000_00-0000000000000000");
        assert_eq!(parsed.category(), "gd");
        assert_eq!(parsed.app_ver.as_deref(), Some("1.00"));
        assert_eq!(parsed.min_version.as_deref(), Some("3.61"));
        assert_eq!(parsed.disc_id.as_deref(), Some("PCSE00000"));
    }

    #[test]
    fn stitle_overrides_title() {
        let sfo = build(&[("TITLE", "Largo"), ("STITLE", "Corto")]);
        assert_eq!(parse(&sfo).expect("SFO").title(), "Corto");
    }

    #[test]
    fn sanitizes_title_for_windows() {
        let sfo = build(&[("TITLE", "A:B/C\\D*E?F\"G<H>I|J\nK")]);
        assert_eq!(parse(&sfo).expect("SFO").title(), "A -BCDEFGHIJ K");
    }

    #[test]
    fn keeps_utf8_bytes() {
        let sfo = build(&[("TITLE", "Ñandú ☃")]);
        assert_eq!(parse(&sfo).expect("SFO").title(), "Ñandú ☃");
    }

    #[test]
    fn rejects_missing_title() {
        let sfo = build(&[("CATEGORY", "gd")]);
        assert!(matches!(
            parse(&sfo),
            Err(Error::Corrupt(
                "cannot find title from sfo file, pkg is probably corrupted"
            ))
        ));
    }

    #[test]
    fn rejects_bad_header() {
        assert!(matches!(
            parse(&[0u8; 8]),
            Err(Error::Corrupt("sfo information is too small"))
        ));
        let mut sfo = build(&[("TITLE", "X")]);
        sfo[1] = 0;
        assert!(matches!(
            parse(&sfo),
            Err(Error::Corrupt("incorrect sfo signature"))
        ));
    }

    #[test]
    fn trims_versions_like_c() {
        let sfo = build(&[("TITLE", "X"), ("APP_VER", "02.10")]);
        assert_eq!(parse(&sfo).expect("SFO").app_ver.as_deref(), Some("2.10"));

        let sfo = build(&[("TITLE", "X"), ("PSP2_DISP_VER", "03.60")]);
        assert_eq!(
            parse(&sfo).expect("SFO").min_version.as_deref(),
            Some("3.6")
        );
    }
}
