//! Prueba de integración: el fixture sintético se puede interpretar con los parsers de
//! `pkg2zip-core` (cabecera, metadatos, SFO, items descifrados).

use pkg2zip_core::bytes::{get32be, get64be};
use pkg2zip_core::crypto::{ctr_xor, derive_keys};
use pkg2zip_core::pkg::{Kind, MetaBlock, MetaInfo, PkgHeader, region, sfo};
use pkg2zip_testkit::pkg::{DEFAULT_CONTENT_ID, build_vita_app};

#[test]
fn parses_synthetic_vita_app_end_to_end() {
    let pkg = build_vita_app("Synthetic Vita App", DEFAULT_CONTENT_ID);
    let bytes = &pkg.bytes;

    let header = PkgHeader::parse(bytes).expect("cabecera");
    header
        .validate(u64::try_from(bytes.len()).expect("tamaño"))
        .expect("tamaño válido");
    assert_eq!(header.key_type, 3);
    assert_eq!(header.hdr_type, 0);

    let mut info = MetaInfo::default();
    let mut offset = header.meta_offset as usize;
    for _ in 0..header.meta_count {
        let block = MetaBlock::parse(&bytes[offset..offset + 16]).expect("bloque");
        info.apply(&block);
        offset += block.file_len() as usize;
    }
    assert_eq!(info.content_type, Some(0x15));
    assert_eq!(info.items_offset, 0);
    assert!(info.items_size > 0);

    let sfo_bytes = &bytes[info.sfo_offset as usize..(info.sfo_offset + info.sfo_size) as usize];
    let sfo = sfo::parse(sfo_bytes).expect("SFO");
    assert_eq!(sfo.title(), "Synthetic Vita App");
    assert_eq!(sfo.category(), "gd");
    assert_eq!(sfo.app_ver.as_deref(), Some("1.00"));
    assert_eq!(sfo.min_version.as_deref(), Some("3.61"));

    let kind = Kind::from_content_type(info.content_type.expect("content type"))
        .expect("tipo")
        .with_category(sfo.category());
    assert_eq!(kind, Kind::VitaApp);

    let id = &sfo.content_id()[7..16];
    assert_eq!(id, "PCSE00000");
    assert_eq!(region(id), "USA");

    // Items: descifrar la tabla y luego nombres/datos con AES-CTR.
    let keys = derive_keys(header.key_type, header.hdr_type, &header.iv).expect("claves");
    let enc = header.enc_offset as usize;
    let count = header.item_count as usize;
    let table_start = enc + info.items_offset as usize;
    let mut table = bytes[table_start..table_start + count * 32].to_vec();
    ctr_xor(
        &keys.main,
        &header.iv,
        u64::from(info.items_offset / 16),
        &mut table,
    )
    .expect("CTR tabla");

    let mut names = Vec::new();
    let mut files = Vec::new();
    for index in 0..count {
        let entry = index * 32;
        let name_offset = get32be(&table[entry..]) as usize;
        let name_size = get32be(&table[entry + 4..]) as usize;
        let data_offset = get64be(&table[entry + 8..]) as usize;
        let data_size = get64be(&table[entry + 16..]) as usize;
        let flags = table[entry + 27];

        let mut name = bytes[enc + name_offset..enc + name_offset + name_size].to_vec();
        ctr_xor(&keys.main, &header.iv, name_offset as u64 / 16, &mut name).expect("CTR nombre");
        let name = String::from_utf8(name).expect("nombre UTF-8");
        names.push(name.clone());

        if flags != 4 {
            let mut data = bytes[enc + data_offset..enc + data_offset + data_size].to_vec();
            ctr_xor(&keys.main, &header.iv, data_offset as u64 / 16, &mut data).expect("CTR datos");
            files.push((name, data));
        }
    }

    assert_eq!(names, ["sce_sys", "eboot.bin", "sce_sys/package/digs.bin"]);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].1, pkg.files[0].1, "eboot.bin descifrado");
    assert_eq!(files[1].1, pkg.files[1].1, "digs.bin descifrado");
}
