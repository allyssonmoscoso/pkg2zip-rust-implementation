//! Generación de zRIF válidos para los fixtures.
//!
//! No hace falta un compresor con diccionario: se usa un bloque deflate `stored` (válido aunque el
//! diccionario no se use), con la cabecera zlib marcando `FDICT` y el ID del diccionario de zRIF.
//! Así se ejercita el decodificador completo (cabecera, diccionario, inflate, Adler-32).

use pkg2zip_core::checksum::adler32;
use pkg2zip_core::zrif::ZRIF_DICT_ID;

/// Tamaño del RIF de Vita.
pub const RIF_SIZE: usize = 512;
/// Offset del `CONTENT_ID` en un RIF de Vita.
pub const RIF_CONTENT_OFFSET: usize = 0x10;

/// Construye un RIF con el `CONTENT_ID` en el offset indicado.
#[must_use]
pub fn rif_at(content_id: &str, size: usize, offset: usize) -> Vec<u8> {
    let mut rif = vec![0u8; size];
    let bytes = content_id.as_bytes();
    let len = bytes.len().min(0x30);
    rif[offset..offset + len].copy_from_slice(&bytes[..len]);
    rif
}

/// RIF de Vita (512 bytes, `CONTENT_ID` en 0x10).
#[must_use]
pub fn rif(content_id: &str) -> Vec<u8> {
    rif_at(content_id, RIF_SIZE, RIF_CONTENT_OFFSET)
}

/// zRIF de Vita.
#[must_use]
pub fn zrif_string(content_id: &str) -> String {
    zrif_string_at(content_id, RIF_SIZE, RIF_CONTENT_OFFSET)
}

/// zRIF para un RIF de `size` bytes con el `CONTENT_ID` en `offset` (PSM usa 1024 / 0x50).
#[must_use]
pub fn zrif_string_at(content_id: &str, size: usize, offset: usize) -> String {
    base64(&zlib_compressed(&rif_at(content_id, size, offset)))
}

fn zlib_compressed(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let cmf: u8 = 0x28; // CINFO = 2 (ventana 1024), CM = 8 (deflate)
    let base_flg: u8 = 0xE0; // FLEVEL = 3, FDICT = 1
    let base = (u16::from(cmf) << 8) | u16::from(base_flg);
    let check = (31 - base % 31) % 31;
    out.push(cmf);
    out.push(base_flg | u8::try_from(check).expect("check < 31"));
    out.extend_from_slice(&ZRIF_DICT_ID.to_be_bytes());
    // Deflate real: el buffer `raw[1024]` del C no admite un bloque stored de 1024+ bytes.
    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(payload, 9));
    out.extend_from_slice(&adler32(payload).to_be_bytes());
    out
}
/// Base64 estándar sin padding (como los zRIF habituales).
#[must_use]
pub fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |&byte| u32::from(byte));
        let b2 = chunk.get(2).map_or(0, |&byte| u32::from(byte));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple as usize & 0x3f] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkg2zip_core::zrif::zrif_decode;

    const CONTENT_ID: &str = "UP0000-PCSE00000_00-0000000000000000";

    #[test]
    fn generated_zrif_decodes_to_the_rif() {
        let decoded = zrif_decode(&zrif_string(CONTENT_ID), RIF_SIZE).expect("zRIF válido");
        assert_eq!(decoded, rif(CONTENT_ID));
    }

    #[test]
    fn psm_zrif_decodes_with_1024_bytes() {
        let encoded = zrif_string_at(CONTENT_ID, 1024, 0x50);
        let decoded = zrif_decode(&encoded, 1024).expect("zRIF PSM");
        assert_eq!(decoded, rif_at(CONTENT_ID, 1024, 0x50));
        assert_eq!(&decoded[0x50..0x50 + 7], b"UP0000-");
    }
}
