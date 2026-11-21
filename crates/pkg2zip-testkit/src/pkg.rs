//! Generador de PKG sintéticos.
//!
//! Construye archivos con la misma estructura que un PKG real de PS Vita (cabecera de 192 bytes +
//! extensión, bloques de metadatos, SFO y tabla de items cifrada con AES-128-CTR), para poder
//! ejercitar el parser sin distribuir PKG reales.
//!
//! El binario C de `legacy/` es el oráculo: si acepta estos fixtures, el generador es correcto.

use pkg2zip_core::bytes::{align_up, get32be, set16be, set32be, set64be};
use pkg2zip_core::crypto::{ctr_xor, derive_keys, keys::PkgKeys};

use crate::sfo;

/// Tamaño de la cabecera PKG.
pub const HEADER_SIZE: usize = 192;
/// Tamaño de la extensión de cabecera (se lee junto con la cabecera).
pub const HEADER_EXT_SIZE: usize = 64;
/// Offset donde empiezan los bloques de metadatos.
pub const META_OFFSET: usize = 0x100;

/// `content_type` de una aplicación de Vita.
pub const CONTENT_TYPE_VITA_APP: u32 = 0x15;
/// `key_type` de Vita (clave derivada con `AES-ECB(pkg_vita_3, iv)`).
pub const KEY_TYPE_VITA: u8 = 3;

/// `CONTENT_ID` por defecto: prefijo de 7 caracteres + ID de 9 + sufijo.
pub const DEFAULT_CONTENT_ID: &str = "UP0000-PCSE00000_00-0000000000000000";

/// IV fijo de los fixtures (determinista a propósito).
pub const DEFAULT_IV: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];

/// Estructura de un item del PKG.
struct Item {
    name: &'static str,
    data: Vec<u8>,
    flags: u8,
}

const FLAG_FILE: u8 = 0;
const FLAG_DIRECTORY: u8 = 4;

/// PKG sintético ya serializado.
#[derive(Debug, Clone)]
pub struct SyntheticPkg {
    /// Bytes completos del archivo.
    pub bytes: Vec<u8>,
    /// Título declarado en el SFO.
    pub title: String,
    /// `CONTENT_ID` declarado en el SFO.
    pub content_id: String,
    /// Archivos incluidos en el fixture: `(nombre, contenido en claro)`.
    pub files: Vec<(String, Vec<u8>)>,
}

impl SyntheticPkg {
    /// ID de 9 caracteres que el C extrae como `content + 7`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.content_id[7..16]
    }

    /// Región detectada por el C a partir del prefijo del ID.
    #[must_use]
    pub fn region(&self) -> &'static str {
        match &self.content_id[7..11] {
            "PCSE" | "PCSA" => "USA",
            "PCSB" | "PCSF" => "EUR",
            "PCSG" | "PCSC" => "JPN",
            "PCSH" | "PCSD" => "ASA",
            _ => "UNK",
        }
    }

    /// Nombre esperado del zip (contrato de `-l`).
    #[must_use]
    pub fn expected_zip_name(&self) -> String {
        format!("{} [{}] [{}].zip", self.title, self.id(), self.region())
    }
}

/// Genera un patrón de bytes determinista (xorshift) para el contenido de los archivos.
fn pattern(seed: u32, len: usize) -> Vec<u8> {
    let mut x = seed | 1;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        out.push((x & 0xff) as u8);
    }
    out
}

fn write_block(out: &mut [u8], block_type: u32, size: u32, fields: impl FnOnce(&mut [u8])) {
    set32be(&mut out[0..4], block_type);
    set32be(&mut out[4..8], size);
    fields(out);
}

/// Construye un PKG sintético de tipo "Vita APP" en formato zip (`category = "gd"`).
///
/// Incluye:
/// * un directorio (`sce_sys`),
/// * un archivo normal (`eboot.bin`),
/// * `sce_sys/package/digs.bin`, que el C renombra a `body.bin` sin descifrar.
#[must_use]
pub fn build_vita_app(title: &str, content_id: &str) -> SyntheticPkg {
    let sfo = sfo::build(&[
        ("TITLE", title),
        ("STITLE", title),
        ("CONTENT_ID", content_id),
        ("CATEGORY", "gd"),
        ("APP_VER", "01.00"),
        ("PSP2_DISP_VER", "03.61"),
    ]);

    let items = vec![
        Item {
            name: "sce_sys",
            data: Vec::new(),
            flags: FLAG_DIRECTORY,
        },
        Item {
            name: "eboot.bin",
            data: pattern(0x1234_5678, 4096),
            flags: FLAG_FILE,
        },
        Item {
            name: "sce_sys/package/digs.bin",
            data: pattern(0x0bad_f00d, 256),
            flags: FLAG_FILE,
        },
    ];

    let item_count = items.len();
    let table_size = item_count * 32;

    // Nombres alineados a 16, luego datos alineados a 16.
    let mut name_offsets = Vec::with_capacity(item_count);
    let mut cursor = align_up(table_size, 16);
    for item in &items {
        name_offsets.push(cursor);
        cursor = align_up(cursor + item.name.len(), 16);
    }

    let mut data_offsets = Vec::with_capacity(item_count);
    for item in &items {
        data_offsets.push(cursor);
        cursor = align_up(cursor + item.data.len(), 16);
    }
    let items_size = cursor;

    let sfo_offset = META_OFFSET + 3 * 16;
    let enc_offset = align_up(sfo_offset + sfo.len(), 16);
    let pkg_size = enc_offset + items_size;

    let mut out = vec![0u8; pkg_size];

    // --- Cabecera ---
    set32be(&mut out[0..4], 0x7f50_4b47); // "\x7fPKG"
    set16be(&mut out[6..8], 0); // hdr_type
    set32be(
        &mut out[8..12],
        u32::try_from(META_OFFSET).expect("meta offset"),
    );
    set32be(&mut out[12..16], 3); // meta_count
    set32be(
        &mut out[20..24],
        u32::try_from(item_count).expect("item count"),
    );
    set64be(&mut out[24..32], u64::try_from(pkg_size).expect("pkg size"));
    set64be(
        &mut out[32..40],
        u64::try_from(enc_offset).expect("enc offset"),
    );
    set64be(
        &mut out[40..48],
        u64::try_from(items_size).expect("enc size"),
    );
    out[0x70..0x80].copy_from_slice(&DEFAULT_IV);
    out[0xe7] = KEY_TYPE_VITA;

    // --- Metadatos: content_type, SFO, items ---
    let mut meta = META_OFFSET;
    write_block(&mut out[meta..meta + 16], 2, 8, |b| {
        set32be(&mut b[8..12], CONTENT_TYPE_VITA_APP);
    });
    meta += 16;
    write_block(&mut out[meta..meta + 16], 14, 8, |b| {
        set32be(
            &mut b[8..12],
            u32::try_from(sfo_offset).expect("sfo offset"),
        );
        set32be(&mut b[12..16], u32::try_from(sfo.len()).expect("sfo size"));
    });
    meta += 16;
    write_block(&mut out[meta..meta + 16], 13, 8, |b| {
        set32be(&mut b[8..12], 0); // items_offset (relativo a enc_offset)
        set32be(
            &mut b[12..16],
            u32::try_from(items_size).expect("items size"),
        );
    });

    // --- SFO (sin cifrar) ---
    out[sfo_offset..sfo_offset + sfo.len()].copy_from_slice(&sfo);

    // --- Región cifrada: tabla + nombres + datos ---
    let keys: PkgKeys = derive_keys(KEY_TYPE_VITA, 0, &DEFAULT_IV).expect("clave Vita 3");
    let enc = &mut out[enc_offset..];

    let mut table = vec![0u8; table_size];
    for (i, item) in items.iter().enumerate() {
        let entry = i * 32;
        set32be(
            &mut table[entry..entry + 4],
            u32::try_from(name_offsets[i]).expect("name off"),
        );
        set32be(
            &mut table[entry + 4..entry + 8],
            u32::try_from(item.name.len()).expect("name len"),
        );
        set64be(
            &mut table[entry + 8..entry + 16],
            u64::try_from(data_offsets[i]).expect("data off"),
        );
        set64be(
            &mut table[entry + 16..entry + 24],
            u64::try_from(item.data.len()).expect("data len"),
        );
        table[entry + 24] = 0; // psp_type
        table[entry + 27] = item.flags;
    }
    ctr_xor(&keys.main, &DEFAULT_IV, 0, &mut table).expect("CTR tabla");
    enc[..table_size].copy_from_slice(&table);

    for (i, item) in items.iter().enumerate() {
        let offset = name_offsets[i];
        let name = item.name.as_bytes();
        let padded = align_up(name.len(), 16);
        let mut buf = vec![0u8; padded];
        buf[..name.len()].copy_from_slice(name);
        ctr_xor(
            &keys.main,
            &DEFAULT_IV,
            u64::try_from(offset / 16).expect("bloque nombre"),
            &mut buf[..name.len()],
        )
        .expect("CTR nombre");
        enc[offset..offset + padded].copy_from_slice(&buf);
    }

    for (i, item) in items.iter().enumerate() {
        if item.data.is_empty() {
            continue;
        }
        let offset = data_offsets[i];
        let mut buf = item.data.clone();
        ctr_xor(
            &keys.main,
            &DEFAULT_IV,
            u64::try_from(offset / 16).expect("bloque datos"),
            &mut buf,
        )
        .expect("CTR datos");
        enc[offset..offset + buf.len()].copy_from_slice(&buf);
    }

    let files = items
        .iter()
        .filter(|item| item.flags == FLAG_FILE)
        .map(|item| (item.name.to_owned(), item.data.clone()))
        .collect();

    SyntheticPkg {
        bytes: out,
        title: title.to_owned(),
        content_id: content_id.to_owned(),
        files,
    }
}

/// Comprueba que un buffer parece un PKG sintético de este generador (para tests).
#[must_use]
pub fn is_synthetic_pkg(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER_SIZE + HEADER_EXT_SIZE && get32be(bytes) == 0x7f50_4b47
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_valid_looking_pkg() {
        let pkg = build_vita_app("Synthetic Vita App", DEFAULT_CONTENT_ID);
        assert!(is_synthetic_pkg(&pkg.bytes));

        let bytes = &pkg.bytes;
        assert_eq!(get32be(&bytes[8..]), META_OFFSET as u32);
        assert_eq!(get32be(&bytes[12..]), 3);
        assert_eq!(get32be(&bytes[20..]), 3);
        assert_eq!(bytes[0xe7], KEY_TYPE_VITA);

        let total = u64::from_be_bytes(bytes[24..32].try_into().expect("total"));
        assert_eq!(total as usize, bytes.len());

        let enc_offset = u64::from_be_bytes(bytes[32..40].try_into().expect("enc"));
        assert!(enc_offset as usize > HEADER_SIZE + HEADER_EXT_SIZE);
        assert_eq!(enc_offset % 16, 0, "enc_offset alineado a AES");
    }

    #[test]
    fn names_and_data_are_encrypted() {
        let pkg = build_vita_app("Synthetic Vita App", DEFAULT_CONTENT_ID);
        assert!(
            !pkg.bytes.windows(9).any(|w| w == b"eboot.bin"),
            "el nombre no debe aparecer en claro"
        );
        assert_eq!(pkg.id(), "PCSE00000");
        assert_eq!(pkg.region(), "USA");
        assert_eq!(
            pkg.expected_zip_name(),
            "Synthetic Vita App [PCSE00000] [USA].zip"
        );
    }

    #[test]
    fn deterministic_output() {
        let a = build_vita_app("T", DEFAULT_CONTENT_ID);
        let b = build_vita_app("T", DEFAULT_CONTENT_ID);
        assert_eq!(a.bytes, b.bytes);
    }
}
