//! Generación de zRIF válidos para los fixtures.
//!
//! No hace falta un compresor con diccionario: se usa un bloque deflate `stored` (válido aunque el
//! diccionario no se use), con la cabecera zlib marcando `FDICT` y el ID del diccionario de zRIF.
//! Así se ejercita el decodificador completo (cabecera, diccionario, inflate, Adler-32).

use pkg2zip_core::checksum::adler32;
use pkg2zip_core::zrif::ZRIF_DICT_ID;

/// Tamaño del RIF de Vita (PSM usa 1024; F2 solo Vita normal).
pub const RIF_SIZE: usize = 512;

/// Construye un RIF de 512 bytes con el `CONTENT_ID` en el offset 0x10.
#[must_use]
pub fn rif(content_id: &str) -> Vec<u8> {
    let mut rif = vec![0u8; RIF_SIZE];
    let bytes = content_id.as_bytes();
    let len = bytes.len().min(0x30);
    rif[0x10..0x10 + len].copy_from_slice(&bytes[..len]);
    rif
}

/// Construye un zRIF (base64) para el `CONTENT_ID` indicado.
#[must_use]
pub fn zrif_string(content_id: &str) -> String {
    base64(&zlib_stored(&rif(content_id)))
}

fn zlib_stored(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let cmf: u8 = 0x28; // CINFO = 2 (ventana 1024), CM = 8 (deflate)
    let base_flg: u8 = 0xE0; // FLEVEL = 3, FDICT = 1
    let base = (u16::from(cmf) << 8) | u16::from(base_flg);
    let check = (31 - base % 31) % 31;
    out.push(cmf);
    out.push(base_flg | u8::try_from(check).expect("check < 31"));
    out.extend_from_slice(&ZRIF_DICT_ID.to_be_bytes());

    // Bloque stored: BFINAL=1, BTYPE=00, padding a byte.
    out.push(0x01);
    let len = u16::try_from(payload.len()).expect("payload cabe en u16");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(payload);
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

    #[test]
    fn generated_zrif_decodes_to_the_rif() {
        let content_id = "UP0000-PCSE00000_00-0000000000000000";
        let encoded = zrif_string(content_id);
        let decoded = zrif_decode(&encoded, RIF_SIZE).expect("zRIF válido");
        assert_eq!(decoded, rif(content_id));
    }

    #[test]
    fn rif_contains_content_id_at_0x10() {
        let rif = rif(DEFAULT);
        assert_eq!(&rif[0x10..0x10 + 7], b"UP0000-");
        assert_eq!(&rif[0x17..0x20], b"PCSE00000");
    }

    const DEFAULT: &str = "UP0000-PCSE00000_00-0000000000000000";
}
