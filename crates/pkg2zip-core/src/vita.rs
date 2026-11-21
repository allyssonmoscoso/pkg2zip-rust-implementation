//! Desempaquetado de PKG de PS Vita: APP, PATCH, DLC y THEME.
//!
//! Port de la rama "Vita" de `legacy/pkg2zip.c`: layout de carpetas, items cifrados con AES-CTR,
//! extras (`work.bin`, `head/tail/stat.bin`), renombrado de `digs.bin`/`cert.bin` y PDB de temas
//! bgdl. PSM se completa en F4.

use std::path::Path;

use crate::crypto::{PkgKeys, ctr_xor};
use crate::error::{Error, Result};
use crate::output::Sink;
use crate::pkg::naming::{self, Naming};
use crate::pkg::{Kind, MetaInfo, PkgHeader, Sfo, region};
use crate::plat::{DataFile, Ui, is_dir};
use crate::zrif::{rif_load, zrif_decode};

const CHUNK_SIZE: usize = 1 << 16;
const ITEM_SIZE: usize = 32;
const RIF_SIZE_VITA: usize = 512;
const RIF_SIZE_PSM: usize = 1024;
const CONTENT_ID_LEN: usize = 0x30;
const PDB_SIZE: usize = 0x200;

/// Datos de un PKG de Vita listo para desempaquetar.
#[derive(Debug)]
pub struct VitaContext<'a> {
    /// Archivo PKG.
    pub pkg: &'a DataFile,
    /// Cabecera.
    pub header: &'a PkgHeader,
    /// Metadatos.
    pub info: &'a MetaInfo,
    /// SFO.
    pub sfo: &'a Sfo,
    /// Tipo de PKG (ya refinado con `CATEGORY`).
    pub kind: Kind,
    /// Claves derivadas.
    pub keys: PkgKeys,
    /// zRIF o ruta a `.bin`, si se pasó.
    pub zrif: Option<&'a str>,
}

/// ID de 9 caracteres (`CONTENT_ID[7..16]`).
///
/// # Errors
///
/// [`Error::Corrupt`] si el `CONTENT_ID` es demasiado corto.
pub fn id9(sfo: &Sfo) -> Result<&str> {
    sfo.content_id()
        .get(7..16)
        .ok_or(Error::Corrupt("invalid CONTENT_ID in sfo"))
}

/// Sufijo de DLC (`CONTENT_ID[20..]`, `id2` en el C).
#[must_use]
pub fn dlc_id(sfo: &Sfo) -> &str {
    sfo.content_id().get(20..).unwrap_or("")
}

/// Construye el nombre raíz (sin `.zip`) con la tabla de regiones.
///
/// # Errors
///
/// [`Error::Corrupt`] si el `CONTENT_ID` es inválido.
pub fn root_name(context: &VitaContext<'_>) -> Result<String> {
    let id = id9(context.sfo)?;
    let naming = Naming {
        kind: context.kind,
        title: context.sfo.title(),
        id,
        region: region(id),
        app_ver: context.sfo.app_ver.as_deref().unwrap_or(""),
        content_type: context.info.content_type.unwrap_or(0),
        install_directory: context.info.install_directory.as_deref(),
        dlc_id: dlc_id(context.sfo),
        category: context.sfo.category(),
    };
    Ok(naming::root_name(&naming))
}

/// Carga y valida el zRIF, si se proporcionó.
///
/// # Errors
///
/// [`Error::Zrif*`] o [`Error::ZrifMismatch`] si el zRIF es inválido o no corresponde al PKG.
pub fn load_rif(context: &VitaContext<'_>) -> Result<Option<Vec<u8>>> {
    let Some(zrif) = context.zrif else {
        return Ok(None);
    };
    if context.kind == Kind::VitaPatch {
        return Ok(None);
    }

    let rif_size = if context.kind == Kind::VitaPsm {
        RIF_SIZE_PSM
    } else {
        RIF_SIZE_VITA
    };
    let rif = if zrif.ends_with(".bin") {
        rif_load(Path::new(zrif), rif_size)?
    } else {
        zrif_decode(zrif, rif_size)?
    };

    let content_offset = if context.kind == Kind::VitaPsm {
        0x50
    } else {
        0x10
    };
    let rif_id = &rif[content_offset..content_offset + CONTENT_ID_LEN];

    let mut pkg_id = [0u8; CONTENT_ID_LEN];
    let content = context.sfo.content_id().as_bytes();
    let len = content.len().min(CONTENT_ID_LEN);
    pkg_id[..len].copy_from_slice(&content[..len]);

    if rif_id != pkg_id {
        return Err(Error::ZrifMismatch {
            rif: String::from_utf8_lossy(rif_id)
                .trim_end_matches('\0')
                .to_owned(),
            pkg: context.sfo.content_id().to_owned(),
        });
    }

    Ok(Some(rif))
}

/// Desempaqueta el PKG en `sink`.
///
/// El llamador debe invocar [`Sink::finish`] al terminar (el C lo hace en `out_end`).
///
/// # Errors
///
/// Cualquier error de lectura/escritura, PKG corrupto o funcionalidad no migrada.
pub fn unpack(
    context: &VitaContext<'_>,
    zipped: bool,
    bgdl: bool,
    rif: Option<&[u8]>,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    if context.kind == Kind::VitaPsm {
        return Err(Error::NotImplemented("PSM unpacking arrives in phase F4"));
    }

    let id = id9(context.sfo)?;
    let root = prepare_root(context.kind, id, dlc_id(context.sfo), zipped, bgdl, sink)?;

    ui.progress_init(context.pkg.len());

    let enc_offset = context.header.enc_offset;
    let items_offset = u64::from(context.info.items_offset);
    let item_count = context.header.item_count as usize;
    let items_table_offset = enc_offset
        .checked_add(items_offset)
        .ok_or(Error::TooShort("pkg file"))?;

    for index in 0..item_count {
        let relative = items_offset + (index as u64) * ITEM_SIZE as u64;
        let mut entry = context.pkg.read_vec(
            items_table_offset + (index as u64) * ITEM_SIZE as u64,
            ITEM_SIZE,
        )?;
        ctr_xor(
            &context.keys.main,
            &context.header.iv,
            relative / 16,
            &mut entry,
        )?;
        unpack_item(context, &root, &entry, sink, ui)?;
    }

    ui.info(if zipped {
        "[*] creating completed"
    } else {
        "[*] unpacking completed"
    });

    if matches!(
        context.kind,
        Kind::VitaApp | Kind::VitaDlc | Kind::VitaPatch | Kind::VitaTheme
    ) {
        write_head_tail_stat(context, &root, sink, ui)?;
    }

    if matches!(
        context.kind,
        Kind::VitaApp | Kind::VitaDlc | Kind::VitaTheme
    ) && let Some(rif) = rif
    {
        ui.info("[*] creating sce_sys/package/work.bin");
        let path = format!("{root}/sce_sys/package/work.bin");
        sink.add_parent(&path)?;
        sink.begin_file(&path, false)?;
        sink.write(rif)?;
        sink.end_file()?;
    }

    if context.kind == Kind::VitaTheme && bgdl {
        write_theme_pdb(context, &root, sink, ui)?;
    }

    Ok(())
}

fn prepare_root(
    kind: Kind,
    id: &str,
    dlc: &str,
    zipped: bool,
    bgdl: bool,
    sink: &mut Sink,
) -> Result<String> {
    match kind {
        Kind::VitaApp => {
            sink.add_folder("app")?;
            let root = format!("app/{id}");
            sink.add_folder(&root)?;
            Ok(root)
        }
        Kind::VitaPatch => {
            sink.add_folder("patch")?;
            let root = format!("patch/{id}");
            sink.add_folder(&root)?;
            Ok(root)
        }
        Kind::VitaDlc => {
            sink.add_folder("addcont")?;
            let base = format!("addcont/{id}");
            sink.add_folder(&base)?;
            let root = format!("{base}/{dlc}");
            sink.add_folder(&root)?;
            Ok(root)
        }
        Kind::VitaTheme => {
            if bgdl {
                sink.add_folder("bgdl/t")?;
                let task = if zipped {
                    1
                } else {
                    let mut task = 1u32;
                    while is_dir(&format!("bgdl/t/{task:08x}")) {
                        task += 1;
                    }
                    task
                };
                let dir = format!("bgdl/t/{task:08x}");
                sink.add_folder(&dir)?;
                let root = format!("{dir}/{id}");
                sink.add_folder(&root)?;
                Ok(root)
            } else {
                sink.add_folder("app")?;
                let root = format!("app/{id}");
                sink.add_folder(&root)?;
                Ok(root)
            }
        }
        _ => Err(Error::NotImplemented("only Vita APP/PATCH/DLC/THEME in F2")),
    }
}

fn unpack_item(
    context: &VitaContext<'_>,
    root: &str,
    entry: &[u8],
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    let name_offset = get32be(&entry[0..]) as usize;
    let name_size = get32be(&entry[4..]) as usize;
    let data_offset = get64be(&entry[8..]);
    let data_size = get64be(&entry[16..]);
    let flags = entry[27];

    if name_size >= crate::output::ZIP_MAX_FILENAME {
        return Err(Error::Corrupt("pkg file contains file with very long name"));
    }
    check_range(context, name_offset as u64, name_size as u64)?;
    check_range(context, data_offset, data_size)?;

    let mut name = context
        .pkg
        .read_vec(context.header.enc_offset + name_offset as u64, name_size)?;
    ctr_xor(
        &context.keys.main,
        &context.header.iv,
        name_offset as u64 / 16,
        &mut name,
    )?;
    let name = String::from_utf8_lossy(&name).into_owned();

    if flags == 4 || flags == 18 {
        let path = format!("{root}/{name}");
        sink.add_folder(&path)?;
        return Ok(());
    }

    let mut encrypted = true;
    let mut name = name;
    if matches!(
        context.kind,
        Kind::VitaApp | Kind::VitaDlc | Kind::VitaPatch | Kind::VitaTheme
    ) && (name == "sce_sys/package/digs.bin" || name == "sce_sys/package/cert.bin")
    {
        sink.add_folder(&format!("{root}/sce_sys/package"))?;
        ui.info(&format!("[*] renaming {name} to body.bin"));
        name = "sce_sys/package/body.bin".to_owned();
        encrypted = false;
    }

    let path = format!("{root}/{name}");
    sink.add_parent(&path)?;
    sink.begin_file(&path, false)?;

    let mut offset = data_offset;
    let mut remaining = data_size;
    while remaining != 0 {
        let size = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("cabe en usize");
        ui.progress(context.header.enc_offset + offset);
        let mut buffer = context
            .pkg
            .read_vec(context.header.enc_offset + offset, size)?;
        if encrypted {
            ctr_xor(
                &context.keys.main,
                &context.header.iv,
                offset / 16,
                &mut buffer,
            )?;
        }
        sink.write(&buffer)?;
        offset += size as u64;
        remaining -= size as u64;
    }
    sink.end_file()?;
    Ok(())
}

fn check_range(context: &VitaContext<'_>, offset: u64, size: u64) -> Result<()> {
    let enc = context.header.enc_offset;
    let end = enc
        .checked_add(offset)
        .and_then(|value| value.checked_add(size))
        .ok_or(Error::TooShort("pkg file is too short, possibly corrupted"))?;
    if context.pkg.len() < end {
        return Err(Error::TooShort("pkg file is too short, possibly corrupted"));
    }
    Ok(())
}

fn write_head_tail_stat(
    context: &VitaContext<'_>,
    root: &str,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    ui.info("[*] creating sce_sys/package/head.bin");
    let head_path = format!("{root}/sce_sys/package/head.bin");
    sink.add_parent(&head_path)?;
    sink.begin_file(&head_path, false)?;
    copy_range(
        context.pkg,
        0,
        context.header.enc_offset + u64::from(context.info.items_size),
        sink,
    )?;
    sink.end_file()?;

    ui.info("[*] creating sce_sys/package/tail.bin");
    let tail_path = format!("{root}/sce_sys/package/tail.bin");
    sink.begin_file(&tail_path, false)?;
    let tail_start = context.header.enc_offset + context.header.enc_size;
    copy_range(context.pkg, tail_start, context.pkg.len(), sink)?;
    sink.end_file()?;

    ui.info("[*] creating sce_sys/package/stat.bin");
    let stat_path = format!("{root}/sce_sys/package/stat.bin");
    sink.begin_file(&stat_path, false)?;
    sink.write(&[0u8; 768])?;
    sink.end_file()?;
    Ok(())
}

fn copy_range(pkg: &DataFile, start: u64, end: u64, sink: &mut Sink) -> Result<()> {
    let mut offset = start;
    while offset < end {
        let size = usize::try_from((end - offset).min(CHUNK_SIZE as u64)).expect("cabe en usize");
        let buffer = pkg.read_vec(offset, size)?;
        sink.write(&buffer)?;
        offset += size as u64;
    }
    Ok(())
}

fn write_theme_pdb(
    context: &VitaContext<'_>,
    root: &str,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    let Some(parent) = root.rsplit_once('/').map(|(parent, _)| parent) else {
        return Err(Error::Corrupt("theme root without parent"));
    };

    let title = context.sfo.title();
    let content = context.sfo.content_id();
    let id = id9(context.sfo)?;
    let pdb = build_pdb(title, content, id);

    let mut with_flag = pdb;
    with_flag[0x20] = 0x02;
    ui.info("[*] creating d0.pdb");
    let d0 = format!("{parent}/d0.pdb");
    sink.begin_file(&d0, false)?;
    sink.write(&with_flag)?;
    sink.end_file()?;

    with_flag[0x20] = 0x00;
    ui.info("[*] creating d1.pdb");
    let d1 = format!("{parent}/d1.pdb");
    sink.begin_file(&d1, false)?;
    sink.write(&with_flag)?;
    sink.end_file()?;

    ui.info("[*] creating f0.pdb");
    let f0 = format!("{parent}/f0.pdb");
    sink.begin_file(&f0, false)?;
    sink.end_file()?;
    Ok(())
}

/// Construye el PDB de 0x200 bytes del tema (offsets fijos del formato, como el C).
fn build_pdb(title: &str, content_id: &str, id: &str) -> [u8; PDB_SIZE] {
    let mut pdb = [0u8; PDB_SIZE];

    pdb[0x04..0x14].copy_from_slice(&[
        0x64, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x14..0x24].copy_from_slice(&[
        0x65, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x24..0x31].copy_from_slice(&[
        0x66, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]);
    pdb[0x31..0x41].copy_from_slice(&[
        0x68, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x41..0x51].copy_from_slice(&[
        0x6B, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x51..0x61].copy_from_slice(&[
        0x6C, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x61..0x71].copy_from_slice(&[
        0x6D, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x71..0x7E].copy_from_slice(&[
        0x6E, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]);
    pdb[0x7E..0x8E].copy_from_slice(&[
        0x6F, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);
    pdb[0x8E..0x9B].copy_from_slice(&[
        0x70, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    ]);
    pdb[0x9B..0xA8].copy_from_slice(&[
        0x71, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    ]);
    pdb[0xA8..0xB8].copy_from_slice(&[
        0x72, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ]);
    pdb[0xB8..0xC5].copy_from_slice(&[
        0x73, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]);
    pdb[0xC5..0xD2].copy_from_slice(&[
        0x74, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]);
    pdb[0xD2..0xDE].copy_from_slice(&[
        0x69, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x00,
    ]);

    let title_bytes = title.as_bytes();
    let title_len = title_bytes.len().min(0x40);
    pdb[0xDE..0xDE + title_len].copy_from_slice(&title_bytes[..title_len]);

    pdb[0x122..0x12E].copy_from_slice(&[
        0xD9, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00,
    ]);
    let content_bytes = content_id.as_bytes();
    let content_start = 7.min(content_bytes.len());
    let content_len = (content_bytes.len() - content_start).min(0x25);
    pdb[0x12E..0x12E + content_len]
        .copy_from_slice(&content_bytes[content_start..content_start + content_len]);

    pdb[0x153..0x160].copy_from_slice(&[
        0xDA, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    ]);
    pdb[0x160..0x16C].copy_from_slice(&[
        0xDC, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00,
    ]);

    let id_bytes = id.as_bytes();
    let id_len = id_bytes.len().min(9);
    pdb[0x16C..0x16C + id_len].copy_from_slice(&id_bytes[..id_len]);
    pdb
}

fn get32be(bytes: &[u8]) -> u32 {
    crate::bytes::get32be(bytes)
}

fn get64be(bytes: &[u8]) -> u64 {
    crate::bytes::get64be(bytes)
}
