//! Fixtures de PSP, PSX y PS3 que no requieren PSAR/PGD válidos.
//!
//! El parser C solo entra en PSAR/PGD cuando el item es un EBOOT con PSAR real o un `.edat`; para
//! el resto de rutas (modo `-p`, PSX sin EBOOT, PS3 con DOCUMENT.DAT) basta con la cabecera, el
//! `PARAM.SFO` y los items cifrados con AES-CTR. Estos fixtures cubren esas rutas end-to-end.

use pkg2zip_core::bytes::{align_up, set16be, set32be, set64be};
use pkg2zip_core::crypto::{ctr_xor, derive_keys};

use crate::pkg::{META_OFFSET, SyntheticPkg, is_synthetic_pkg};
use crate::sfo;

const IV: [u8; 16] = [
    0x0f, 0x1e, 0x2d, 0x3c, 0x4b, 0x5a, 0x69, 0x78, 0x87, 0x96, 0xa5, 0xb4, 0xc3, 0xd2, 0xe1, 0xf0,
];

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

struct Spec {
    title: String,
    content_id: Option<&'static str>,
    dirs: &'static [&'static str],
    content_type: u32,
    key_type: u8,
    hdr_type: u16,
    serial: &'static str,
    items: Vec<(String, Vec<u8>)>,
}

fn build(spec: &Spec) -> SyntheticPkg {
    let item_count = spec.items.len();
    let table_size = item_count * 32;

    let mut name_offsets = Vec::with_capacity(item_count);
    let mut cursor = align_up(table_size, 16);
    for (name, _) in &spec.items {
        name_offsets.push(cursor);
        cursor = align_up(cursor + name.len(), 16);
    }

    let mut data_offsets = Vec::with_capacity(item_count);
    for (_, data) in &spec.items {
        data_offsets.push(cursor);
        cursor = align_up(cursor + data.len(), 16);
    }
    let items_size = cursor;

    let meta_len = 2 * 16;
    let enc_offset = align_up(META_OFFSET + meta_len, 16);
    let pkg_size = enc_offset + items_size;

    let mut out = vec![0u8; pkg_size];
    set32be(&mut out[0..4], 0x7f50_4b47);
    set16be(&mut out[6..8], spec.hdr_type);
    set32be(&mut out[8..12], META_OFFSET as u32);
    set32be(&mut out[12..16], 2);
    set32be(&mut out[20..24], item_count as u32);
    set64be(&mut out[24..32], pkg_size as u64);
    set64be(&mut out[32..40], enc_offset as u64);
    set64be(&mut out[40..48], items_size as u64);
    out[0x70..0x80].copy_from_slice(&IV);
    out[0xe7] = spec.key_type;
    if let Some(content_id) = spec.content_id {
        let bytes = content_id.as_bytes();
        let len = bytes.len().min(0x30);
        out[0x30..0x30 + len].copy_from_slice(&bytes[..len]);
    } else {
        let serial = spec.serial.as_bytes();
        let serial_len = serial.len().min(9);
        out[0x37..0x37 + serial_len].copy_from_slice(&serial[..serial_len]);
    }

    set32be(&mut out[META_OFFSET..META_OFFSET + 4], 2);
    set32be(&mut out[META_OFFSET + 4..META_OFFSET + 8], 8);
    set32be(
        &mut out[META_OFFSET + 8..META_OFFSET + 12],
        spec.content_type,
    );
    let second = META_OFFSET + 16;
    set32be(&mut out[second..second + 4], 13);
    set32be(&mut out[second + 4..second + 8], 8);
    set32be(&mut out[second + 8..second + 12], 0);
    set32be(&mut out[second + 12..second + 16], items_size as u32);

    let keys = derive_keys(spec.key_type, spec.hdr_type, &IV).expect("claves del fixture");
    let enc = &mut out[enc_offset..];

    let mut table = vec![0u8; table_size];
    for (index, (name, data)) in spec.items.iter().enumerate() {
        let entry = index * 32;
        set32be(&mut table[entry..entry + 4], name_offsets[index] as u32);
        set32be(&mut table[entry + 4..entry + 8], name.len() as u32);
        set64be(
            &mut table[entry + 8..entry + 16],
            data_offsets[index] as u64,
        );
        set64be(&mut table[entry + 16..entry + 24], data.len() as u64);
        table[entry + 24] = 0x90;
        table[entry + 27] = if spec.dirs.contains(&name.as_str()) {
            4
        } else {
            0
        };
    }
    ctr_xor(&keys.main, &IV, 0, &mut table).expect("CTR tabla");
    enc[..table_size].copy_from_slice(&table);

    for (index, (name, _)) in spec.items.iter().enumerate() {
        let offset = name_offsets[index];
        let padded = align_up(name.len(), 16);
        let mut buffer = vec![0u8; padded];
        buffer[..name.len()].copy_from_slice(name.as_bytes());
        ctr_xor(
            &keys.main,
            &IV,
            (offset / 16) as u64,
            &mut buffer[..name.len()],
        )
        .expect("CTR nombre");
        enc[offset..offset + padded].copy_from_slice(&buffer);
    }

    for (index, (_, data)) in spec.items.iter().enumerate() {
        if data.is_empty() {
            continue;
        }
        let offset = data_offsets[index];
        let mut buffer = data.clone();
        ctr_xor(&keys.main, &IV, (offset / 16) as u64, &mut buffer).expect("CTR datos");
        enc[offset..offset + buffer.len()].copy_from_slice(&buffer);
    }

    let content_id = format!("UP0000-{}_00-0000000000000000", spec.serial);
    let pkg = SyntheticPkg {
        bytes: out,
        title: spec.title.clone(),
        content_id,
        files: spec.items.clone(),
    };
    assert!(is_synthetic_pkg(&pkg.bytes));
    pkg
}

/// `EBOOT.PBP` mínimo con cabecera y `PARAM.SFO` (suficiente para `-p` y para leer `DISC_ID`).
fn pbp(title: &str, disc_id: &str) -> Vec<u8> {
    let sfo = sfo::build(&[("TITLE", title), ("DISC_ID", disc_id)]);
    let sfo_offset = 0x28usize;
    let icon0_offset = sfo_offset + sfo.len();
    let psar_offset = align_up(icon0_offset, 16);
    let mut out = vec![0u8; psar_offset + 0x100];
    out[..4].copy_from_slice(b"\0PBP");
    set32be(&mut out[0x08..0x0C], sfo_offset as u32);
    set32be(&mut out[0x0C..0x10], icon0_offset as u32);
    set32be(&mut out[0x24..0x28], psar_offset as u32);
    out[sfo_offset..sfo_offset + sfo.len()].copy_from_slice(&sfo);
    // PSAR de relleno: el modo -p no lo parsea.
    out[psar_offset..psar_offset + 8].copy_from_slice(b"DUMMYPSR");
    out
}

fn param_sfo(title: &str, category: &str) -> Vec<u8> {
    sfo::build(&[("TITLE", title), ("CATEGORY", category)])
}

/// PSP con `PARAM.SFO`, EBOOT.PBP y archivos de contenido (modo `-p`).
#[must_use]
pub fn build_psp() -> SyntheticPkg {
    let title = "Synthetic PSP";
    let disc_id = "UCUS98616";
    build(&Spec {
        title: title.to_owned(),
        content_id: None,
        dirs: &[],
        content_type: 0x07,
        key_type: 1,
        hdr_type: 0,
        serial: disc_id,
        items: vec![
            ("PARAM.SFO".to_owned(), param_sfo(title, "UG")),
            ("USRDIR/CONTENT/EBOOT.PBP".to_owned(), pbp(title, disc_id)),
            (
                "USRDIR/CONTENT/DOCUMENT.DAT".to_owned(),
                pattern(0x1111_2222, 512),
            ),
            (
                "USRDIR/CONTENT/DATA.BIN".to_owned(),
                pattern(0x3333_4444, 256),
            ),
        ],
    })
}

/// PSX sin EBOOT (DOCUMENT.DAT + texture.enc), con licencia `.rif` si se pasa zRIF.
#[must_use]
pub fn build_psx() -> SyntheticPkg {
    build(&Spec {
        title: "Synthetic PSX".to_owned(),
        content_id: None,
        dirs: &[],
        content_type: 0x06,
        key_type: 1,
        hdr_type: 0,
        serial: "UCUS98616",
        items: vec![
            ("PARAM.SFO".to_owned(), param_sfo("Synthetic PSX", "ME")),
            (
                "USRDIR/CONTENT/DOCUMENT.DAT".to_owned(),
                pattern(0x5555_6666, 512),
            ),
            (
                "USRDIR/CONTENT/texture.enc".to_owned(),
                pattern(0x7777_8888, 256),
            ),
        ],
    })
}

/// PS3 (PSX inside) con un DOCUMENT.DAT.
#[must_use]
pub fn build_ps3() -> SyntheticPkg {
    build(&Spec {
        title: "UCUS98616".to_owned(),
        content_id: None,
        dirs: &[],
        content_type: 0x01,
        key_type: 4,
        hdr_type: 1,
        serial: "UCUS98616",
        items: vec![(
            "UCUS98616/DOCUMENT.DAT".to_owned(),
            pattern(0x9999_aaaa, 512),
        )],
    })
}

/// PSM con `CONTENT_ID` en la cabecera y items bajo `content/`.
#[must_use]
pub fn build_psm() -> SyntheticPkg {
    let content_id = "UP0000-PCSE00000_00-0000000000000000";
    build(&Spec {
        title: String::new(),
        content_id: Some(content_id),
        dirs: &["content/runtime"],
        content_type: 0x18,
        key_type: 3,
        hdr_type: 0,
        serial: "PCSE00000",
        items: vec![
            ("content/runtime".to_owned(), Vec::new()),
            (
                "content/runtime/lib.elf".to_owned(),
                pattern(0x2468_ace0, 512),
            ),
            ("content/data.bin".to_owned(), pattern(0x1357_9bdf, 256)),
            ("content/asset.bin".to_owned(), pattern(0x0f1e_2d3c, 128)),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkg2zip_core::bytes::get32be;

    #[test]
    fn psp_fixture_has_expected_header() {
        let pkg = build_psp();
        assert_eq!(get32be(&pkg.bytes[12..]), 2);
        assert_eq!(get32be(&pkg.bytes[20..]), 4);
        assert_eq!(pkg.bytes[0xe7], 1);
        assert_eq!(&pkg.bytes[0x37..0x40], b"UCUS98616");
    }

    #[test]
    fn ps3_fixture_uses_ps3_key_and_item() {
        let pkg = build_ps3();
        assert_eq!(pkg.bytes[0xe7], 4);
        assert_eq!(&pkg.bytes[0x37..0x40], b"UCUS98616");
        assert_eq!(get32be(&pkg.bytes[20..]), 1);
    }

    #[test]
    fn psx_fixture_has_document_and_texture() {
        let pkg = build_psx();
        assert_eq!(pkg.bytes[0xe7], 1);
        assert!(
            pkg.files
                .iter()
                .any(|(name, _)| name.ends_with("texture.enc"))
        );
    }
}
