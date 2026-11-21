//! Construcción de `PARAM.SFO` (formato PSF) para los fixtures sintéticos.
//!
//! Referencia: <http://vitadevwiki.com/vita/System_File_Object_(SFO)_(PSF)>.
//! El parser del binario C lee: magic `0x46535000` (LE), `keys_offset`, `values_offset`,
//! `count` y entradas de 16 bytes (`key_offset: u16`, `fmt: u16`, `len: u32`, `max_len: u32`,
//! `data_offset: u32`), con offsets relativos a las secciones correspondientes.

use pkg2zip_core::bytes::{align_up, set16le, set32le};

/// Magic de SFO tal como lo lee `get32le` en el C (`"\0PSF"` en little endian).
pub const SFO_MAGIC: u32 = 0x4653_5000;

/// Versión de SFO que escribe el builder (`0x00000101`).
pub const SFO_VERSION: u32 = 0x0000_0101;

/// Formato de parámetro "string UTF-8" (`0x0404`).
pub const SFO_FMT_UTF8: u16 = 0x0404;

/// Construye un SFO con parámetros de texto (siempre terminados en NUL).
///
/// # Panics
///
/// Si `entries` está vacío o hay más de `u16::MAX` bytes de claves (irrelevante para fixtures).
#[must_use]
pub fn build(entries: &[(&str, &str)]) -> Vec<u8> {
    assert!(!entries.is_empty(), "el SFO necesita al menos un parámetro");

    let count = entries.len();
    let keys_offset = 20 + 16 * count;

    let mut keys = Vec::new();
    let mut key_offsets = Vec::with_capacity(count);
    for (key, _) in entries {
        key_offsets.push(u16::try_from(keys.len()).expect("sección de claves demasiado grande"));
        keys.extend_from_slice(key.as_bytes());
        keys.push(0);
    }

    let values_offset = align_up(keys_offset + keys.len(), 4);

    let mut values = Vec::new();
    let mut data_offsets = Vec::with_capacity(count);
    let mut lengths = Vec::with_capacity(count);
    let mut max_lengths = Vec::with_capacity(count);
    for (_, value) in entries {
        data_offsets
            .push(u32::try_from(values.len()).expect("sección de valores demasiado grande"));
        values.extend_from_slice(value.as_bytes());
        values.push(0);
        let len = value.len() + 1;
        let max_len = align_up(len, 4);
        values.resize(values.len() + (max_len - len), 0);
        lengths.push(u32::try_from(len).expect("valor demasiado grande"));
        max_lengths.push(u32::try_from(max_len).expect("valor demasiado grande"));
    }

    let mut out = vec![0u8; values_offset + values.len()];
    set32le(&mut out[0..4], SFO_MAGIC);
    set32le(&mut out[4..8], SFO_VERSION);
    set32le(
        &mut out[8..12],
        u32::try_from(keys_offset).expect("offsets caben en u32"),
    );
    set32le(
        &mut out[12..16],
        u32::try_from(values_offset).expect("offsets caben en u32"),
    );
    set32le(
        &mut out[16..20],
        u32::try_from(count).expect("count cabe en u32"),
    );

    for (i, _) in entries.iter().enumerate() {
        let entry = 20 + i * 16;
        set16le(&mut out[entry..entry + 2], key_offsets[i]);
        set16le(&mut out[entry + 2..entry + 4], SFO_FMT_UTF8);
        set32le(&mut out[entry + 4..entry + 8], lengths[i]);
        set32le(&mut out[entry + 8..entry + 12], max_lengths[i]);
        set32le(&mut out[entry + 12..entry + 16], data_offsets[i]);
    }

    out[keys_offset..keys_offset + keys.len()].copy_from_slice(&keys);
    out[values_offset..values_offset + values.len()].copy_from_slice(&values);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkg2zip_core::bytes::{get16le, get32le};

    #[test]
    fn header_and_entries_are_consistent() {
        let sfo = build(&[("TITLE", "Hola"), ("CATEGORY", "gd")]);
        assert_eq!(get32le(&sfo), SFO_MAGIC);
        assert_eq!(get32le(&sfo[4..]), SFO_VERSION);

        let keys_offset = get32le(&sfo[8..]) as usize;
        let values_offset = get32le(&sfo[12..]) as usize;
        let count = get32le(&sfo[16..]) as usize;
        assert_eq!(count, 2);

        // Primera entrada: TITLE -> "Hola"
        let entry = 20;
        let key_off = get16le(&sfo[entry..]) as usize;
        let data_off = get32le(&sfo[entry + 12..]) as usize;
        let key = &sfo[keys_offset + key_off..];
        assert_eq!(&key[..5], b"TITLE");
        let value = &sfo[values_offset + data_off..];
        assert_eq!(&value[..5], b"Hola\0");

        // Las claves y valores no se solapan.
        assert!(keys_offset >= 20 + 16 * count);
        assert!(values_offset >= keys_offset);
    }

    #[test]
    fn values_are_padded_to_four_bytes() {
        let sfo = build(&[("A", "b")]);
        assert_eq!(sfo.len() % 4, 0);
        let values_offset = get32le(&sfo[12..]) as usize;
        let data_off = get32le(&sfo[32..]) as usize;
        assert_eq!(&sfo[values_offset + data_off..], b"b\0\0\0");
    }
}
