//! zRIF: licencia falsa de NoNpDrm/NoPsmDrm en formato texto.
//!
//! Un zRIF es base64 de un flujo zlib **deflate con diccionario preestablecido** de 1024 bytes
//! (ID `0x627d1d5d`), seguido de un Adler-32. El resultado es el `work.bin` (512 o 1024 bytes).
//!
//! Equivalente a `zrif_decode` y `rif_load` de `legacy/pkg2zip_zrif.c`.

pub mod dict;
pub mod puff;

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::checksum::adler32;
use crate::error::{Error, Result};

pub use dict::{ZRIF_DICT, ZRIF_DICT_ID};

/// Mayor capacidad que puede producir un zRIF (PSM usa 1024 bytes de RIF).
const MAX_RIF_SIZE: usize = 1024;

/// Decodifica un zRIF a los bytes del RIF.
///
/// `rif_size` debe ser 512 (Vita/PSX) o 1024 (PSM), igual que en el C.
///
/// # Errors
///
/// Devuelve `Error::Zrif*` si el base64, la cabecera zlib, el diccionario, la descompresión, el
/// checksum o el tamaño no son válidos.
pub fn zrif_decode(encoded: &str, rif_size: usize) -> Result<Vec<u8>> {
    let raw = base64_decode(encoded)?;
    let out = zlib_inflate_dict(&raw, ZRIF_DICT.len() + rif_size.max(MAX_RIF_SIZE))?;
    if out.len() != rif_size {
        return Err(Error::ZrifSize);
    }
    Ok(out)
}

/// Carga un `work.bin`/`.rif` ya decodificado desde disco.
///
/// # Errors
///
/// `Error::Io` si no se puede leer, o `Error::ZrifSize` si el archivo no tiene `rif_size` bytes.
pub fn rif_load(path: &Path, rif_size: usize) -> Result<Vec<u8>> {
    let mut file =
        File::open(path).map_err(|e| Error::io(format!("cannot open '{}'", path.display()), e))?;
    let mut rif = vec![0u8; rif_size];
    file.read_exact(&mut rif)
        .map_err(|e| Error::io(format!("cannot read '{}'", path.display()), e))?;
    Ok(rif)
}

/// Decodifica base64 (tolerante a padding ausente).
///
/// # Errors
///
/// `Error::ZrifBase64` ante caracteres inválidos o longitudes imposibles.
pub fn base64_decode(input: &str) -> Result<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut len = bytes.len();
    while len > 0 && bytes[len - 1] == b'=' {
        len -= 1;
    }

    let mut out = Vec::with_capacity(len / 4 * 3 + 2);
    let mut index = 0;

    while index + 4 <= len {
        let [a, b, c, d] = [
            value(bytes[index])?,
            value(bytes[index + 1])?,
            value(bytes[index + 2])?,
            value(bytes[index + 3])?,
        ];
        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
        out.push((c << 6) | d);
        index += 4;
    }

    match len - index {
        0 => {}
        2 => {
            let a = value(bytes[index])?;
            let b = value(bytes[index + 1])?;
            out.push((a << 2) | (b >> 4));
        }
        3 => {
            let a = value(bytes[index])?;
            let b = value(bytes[index + 1])?;
            let c = value(bytes[index + 2])?;
            out.push((a << 2) | (b >> 4));
            out.push((b << 4) | (c >> 2));
        }
        _ => return Err(Error::ZrifBase64),
    }

    Ok(out)
}

fn value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::ZrifBase64),
    }
}

/// Descomprime un flujo zlib con el diccionario preestablecido del zRIF.
///
/// `capacity` es el tamaño máximo del buffer de salida, **incluyendo** el diccionario (igual que
/// `destlen` en `puff`). Devuelve solo los bytes nuevos.
///
/// # Errors
///
/// `Error::Zrif*` según el problema detectado.
pub fn zlib_inflate_dict(input: &[u8], capacity: usize) -> Result<Vec<u8>> {
    if input.len() < 6 {
        return Err(Error::ZrifTooShort);
    }

    let cmf = input[0];
    let flg = input[1];
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err(Error::ZrifHeader);
    }
    if cmf & 0x0f != 8 {
        return Err(Error::ZrifMethod);
    }

    let (dict, offset): (&[u8], usize) = if flg & 0x20 != 0 {
        let id = u32::from_be_bytes(input[2..6].try_into().expect("4 bytes"));
        if id != ZRIF_DICT_ID {
            return Err(Error::ZrifDictionary);
        }
        (&ZRIF_DICT, 6)
    } else {
        (&[], 2)
    };

    if input.len() < offset + 4 {
        return Err(Error::ZrifTooShort);
    }
    let adler_offset = input.len() - 4;
    if adler_offset <= offset {
        return Err(Error::ZrifTooShort);
    }
    let payload = &input[offset..adler_offset];
    let expected_adler = u32::from_be_bytes(
        input[adler_offset..]
            .try_into()
            .expect("los últimos 4 bytes son el Adler-32"),
    );

    let out = puff::inflate_with_dict(dict, payload, capacity).map_err(|_| Error::ZrifInflate)?;
    if adler32(&out) != expected_adler {
        return Err(Error::ZrifChecksum);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::adler32;

    fn payload() -> Vec<u8> {
        (0..512u32)
            .map(|i| (i.wrapping_mul(37).wrapping_add(11) % 256) as u8)
            .collect()
    }

    fn zlib_stream(payload: &[u8], level: u8, with_dict: bool) -> Vec<u8> {
        let mut raw = miniz_oxide::deflate::compress_to_vec(payload, level);
        let mut out = Vec::new();
        let cmf: u8 = 0x28; // CINFO = 2 (ventana 1024), CM = 8 (deflate)
        let base_flg: u8 = 0xC0 | if with_dict { 0x20 } else { 0 };
        let base = (u16::from(cmf) << 8) | u16::from(base_flg);
        let check = (31 - base % 31) % 31;
        let flg = base_flg | u8::try_from(check).expect("check < 31");

        out.push(cmf);
        out.push(flg);
        if with_dict {
            out.extend_from_slice(&ZRIF_DICT_ID.to_be_bytes());
        }
        out.append(&mut raw);
        out.extend_from_slice(&adler32(payload).to_be_bytes());
        out
    }

    fn base64_encode(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b0 = u32::from(chunk[0]);
            let b1 = chunk.get(1).map_or(0, |&b| u32::from(b));
            let b2 = chunk.get(2).map_or(0, |&b| u32::from(b));
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

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_decode("aGVsbG8=").expect("base64"), b"hello");
        assert_eq!(base64_decode("aGVsbG8").expect("sin padding"), b"hello");
        assert_eq!(base64_decode("Zm9v").expect("base64"), b"foo");
        assert_eq!(base64_decode("").expect("vacío"), b"");
        assert!(matches!(base64_decode("!!!!"), Err(Error::ZrifBase64)));
    }

    #[test]
    fn inflates_zlib_without_dictionary() {
        let data = payload();
        let raw = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
        let out = zlib_inflate_dict(&raw, data.len()).expect("inflate");
        assert_eq!(out, data);
    }

    #[test]
    fn inflates_stream_with_dictionary_header() {
        let data = payload();
        let raw = zlib_stream(&data, 6, true);
        let out = zlib_inflate_dict(&raw, ZRIF_DICT.len() + data.len()).expect("inflate");
        assert_eq!(out, data);
    }

    #[test]
    fn inflates_stored_blocks_with_dictionary_header() {
        let data = payload();
        let raw = zlib_stream(&data, 0, true);
        let out = zlib_inflate_dict(&raw, ZRIF_DICT.len() + data.len()).expect("inflate");
        assert_eq!(out, data);
    }

    #[test]
    fn rejects_bad_checksum() {
        let data = payload();
        let mut raw = zlib_stream(&data, 6, true);
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        assert!(matches!(
            zlib_inflate_dict(&raw, ZRIF_DICT.len() + data.len()),
            Err(Error::ZrifChecksum)
        ));
    }

    #[test]
    fn rejects_unknown_dictionary() {
        let data = payload();
        let mut raw = zlib_stream(&data, 6, true);
        raw[2] ^= 0xff;
        assert!(matches!(
            zlib_inflate_dict(&raw, ZRIF_DICT.len() + data.len()),
            Err(Error::ZrifDictionary)
        ));
    }

    #[test]
    fn rejects_bad_header_check() {
        let data = payload();
        let mut raw = zlib_stream(&data, 6, true);
        raw[1] ^= 0x01;
        assert!(matches!(
            zlib_inflate_dict(&raw, ZRIF_DICT.len() + data.len()),
            Err(Error::ZrifHeader)
        ));
    }

    #[test]
    fn rejects_non_deflate_method() {
        let mut raw = zlib_stream(&payload(), 6, false);
        // CM = 9 (método inexistente); recalcular FCHECK para aislar el error de método.
        let cmf: u8 = 0x29;
        let base = (u16::from(cmf) << 8) | u16::from(raw[1] & 0xE0);
        let check = (31 - base % 31) % 31;
        raw[0] = cmf;
        raw[1] = (raw[1] & 0xE0) | u8::try_from(check).expect("check < 31");
        assert!(matches!(
            zlib_inflate_dict(&raw, 2048),
            Err(Error::ZrifMethod)
        ));
    }

    #[test]
    fn zrif_decode_roundtrip() {
        let data = payload();
        let raw = zlib_stream(&data, 6, true);
        let encoded = base64_encode(&raw);
        let out = zrif_decode(&encoded, data.len()).expect("zRIF");
        assert_eq!(out, data);
    }

    #[test]
    fn zrif_decode_rejects_wrong_size() {
        let data = payload();
        let encoded = base64_encode(&zlib_stream(&data, 6, true));
        assert!(matches!(zrif_decode(&encoded, 1024), Err(Error::ZrifSize)));
    }

    #[test]
    fn rif_load_reads_exact_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("work.bin");
        std::fs::write(&path, [0xABu8; 512]).expect("write");
        let rif = rif_load(&path, 512).expect("rif");
        assert_eq!(rif, vec![0xAB; 512]);

        std::fs::write(&path, [0u8; 10]).expect("write");
        assert!(rif_load(&path, 512).is_err());
    }
}
