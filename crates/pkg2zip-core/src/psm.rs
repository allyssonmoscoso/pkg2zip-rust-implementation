//! PSM: layout `psm/<id>/RO` + `RW`, `pm.dat`, `content_id` y `FAKE.rif`.
//!
//! Port de la rama PSM de `legacy/pkg2zip.c` (incluye `-a` para Android).

use std::path::Path;

use crate::crypto::{PkgKeys, ctr_xor};
use crate::error::{Error, Result};
use crate::output::Sink;
use crate::pkg::{MetaInfo, PkgHeader};
use crate::plat::{DataFile, Ui};
use crate::zrif::{rif_load, zrif_decode};

const CHUNK_SIZE: usize = 1 << 16;
const ITEM_SIZE: usize = 32;
const CONTENT_ID_LEN: usize = 0x30;
const RIF_SIZE: usize = 1024;
const RIF_CONTENT_OFFSET: usize = 0x50;
const PM_SIZE: usize = 1 << 16;

/// Contexto de un PKG PSM.
#[derive(Debug)]
pub struct PsmContext<'a> {
    /// Archivo PKG.
    pub pkg: &'a DataFile,
    /// Cabecera.
    pub header: &'a PkgHeader,
    /// Metadatos.
    pub info: &'a MetaInfo,
    /// Claves derivadas.
    pub keys: PkgKeys,
    /// `CONTENT_ID` de la cabecera (`0x30..0x60`), recortado en el primer NUL.
    pub content_id: String,
    /// zRIF o ruta a `.bin`.
    pub zrif: Option<&'a str>,
}

impl PsmContext<'_> {
    /// ID de 9 caracteres (`CONTENT_ID[7..16]`).
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si el `CONTENT_ID` de la cabecera es inválido.
    pub fn id(&self) -> Result<&str> {
        self.content_id
            .get(7..16)
            .ok_or(Error::Corrupt("invalid CONTENT_ID in pkg header"))
    }
}

/// Carga y valida el zRIF de PSM (1024 bytes, `CONTENT_ID` en `0x50`).
///
/// # Errors
///
/// [`Error::Zrif*`] o [`Error::ZrifMismatch`].
pub fn load_rif(context: &PsmContext<'_>) -> Result<Option<Vec<u8>>> {
    let Some(zrif) = context.zrif else {
        return Ok(None);
    };
    let rif = if zrif.ends_with(".bin") {
        rif_load(Path::new(zrif), RIF_SIZE)?
    } else {
        zrif_decode(zrif, RIF_SIZE)?
    };

    let rif_id = &rif[RIF_CONTENT_OFFSET..RIF_CONTENT_OFFSET + CONTENT_ID_LEN];
    let mut pkg_id = [0u8; CONTENT_ID_LEN];
    let bytes = context.content_id.as_bytes();
    let len = bytes.len().min(CONTENT_ID_LEN);
    pkg_id[..len].copy_from_slice(&bytes[..len]);

    if rif_id != pkg_id {
        return Err(Error::ZrifMismatch {
            rif: String::from_utf8_lossy(rif_id)
                .trim_end_matches('\0')
                .to_owned(),
            pkg: context.content_id.clone(),
        });
    }

    Ok(Some(rif))
}

/// Desempaqueta un PKG PSM.
///
/// # Errors
///
/// Cualquier error de formato, cripto o IO.
pub fn unpack(
    context: &PsmContext<'_>,
    android: bool,
    zipped: bool,
    rif: Option<&[u8]>,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    let id = context.id()?;
    sink.add_folder("psm")?;
    let root = format!("psm/{id}");
    sink.add_folder(&root)?;

    let (rw_folder, ro_folder) = if android {
        (root.clone(), root.clone())
    } else {
        (format!("{root}/RW"), format!("{root}/RO"))
    };

    ui.progress_init(context.pkg.len());

    let enc = context.header.enc_offset;
    let items_offset = u64::from(context.info.items_offset);
    let item_count = context.header.item_count as usize;

    for index in 0..item_count {
        let relative = items_offset + (index as u64) * ITEM_SIZE as u64;
        let mut entry: [u8; 32] = context
            .pkg
            .read_vec(enc + relative, ITEM_SIZE)?
            .try_into()
            .expect("32 bytes");
        ctr_xor(
            &context.keys.main,
            &context.header.iv,
            relative / 16,
            &mut entry,
        )?;

        let name_offset = u64::from(u32::from_be_bytes(entry[0..4].try_into().expect("4 bytes")));
        let name_size = u32::from_be_bytes(entry[4..8].try_into().expect("4 bytes"));
        let data_offset = u64::from_be_bytes(entry[8..16].try_into().expect("8 bytes"));
        let data_size = u64::from_be_bytes(entry[16..24].try_into().expect("8 bytes"));
        let flags = entry[27];

        if name_size as usize >= crate::output::ZIP_MAX_FILENAME {
            return Err(Error::Corrupt("pkg file contains file with very long name"));
        }
        check_range(context, name_offset, u64::from(name_size))?;
        check_range(context, data_offset, data_size)?;

        let mut name = context
            .pkg
            .read_vec(enc + name_offset, name_size as usize)?;
        ctr_xor(
            &context.keys.main,
            &context.header.iv,
            name_offset / 16,
            &mut name,
        )?;
        let name = String::from_utf8_lossy(&name).into_owned();

        // El C usa `name + 9` (no `strip_prefix("content/")`, que son 8): se replica tal cual.
        let stripped = String::from_utf8_lossy(name.as_bytes().get(9..).unwrap_or_default());
        if stripped.is_empty() {
            continue;
        }
        let base = if name.contains("runtime") {
            &root
        } else {
            &ro_folder
        };
        let path = format!("{base}/{stripped}");

        if flags == 4 || flags == 18 {
            if name.contains('/') {
                sink.add_folder(&path)?;
            }
            continue;
        }

        sink.add_parent(&path)?;
        sink.begin_file(&path, false)?;
        let mut offset = data_offset;
        let mut remaining = data_size;
        while remaining != 0 {
            let size = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("cabe en usize");
            ui.progress(context.header.enc_offset + offset);
            let mut buffer = context.pkg.read_vec(enc + offset, size)?;
            ctr_xor(
                &context.keys.main,
                &context.header.iv,
                offset / 16,
                &mut buffer,
            )?;
            sink.write(&buffer)?;
            offset += size as u64;
            remaining -= size as u64;
        }
        sink.end_file()?;
    }

    ui.info(if zipped {
        "[*] creating completed"
    } else {
        "[*] unpacking completed"
    });

    if let Some(rif) = rif {
        ui.info(&format!("[*] creating {ro_folder}/License"));
        sink.add_folder(&format!("{ro_folder}/License"))?;
        ui.info(&format!("[*] creating {ro_folder}/License/FAKE.rif"));
        let path = format!("{ro_folder}/License/FAKE.rif");
        sink.begin_file(&path, false)?;
        sink.write(rif)?;
        sink.end_file()?;
    }

    ui.info(&format!("[*] creating {rw_folder}"));
    sink.add_folder(&rw_folder)?;
    for sub in ["Documents", "Temp", "System"] {
        ui.info(&format!("[*] creating {rw_folder}/{sub}"));
        sink.add_folder(&format!("{rw_folder}/{sub}"))?;
    }

    if !android {
        ui.info(&format!("[*] creating {rw_folder}/System/content_id"));
        let path = format!("{rw_folder}/System/content_id");
        sink.begin_file(&path, false)?;
        sink.write(&context.header.content_id)?;
        sink.end_file()?;
    }

    ui.info(&format!("[*] creating {rw_folder}/System/pm.dat"));
    let path = format!("{rw_folder}/System/pm.dat");
    sink.begin_file(&path, false)?;
    sink.write(&[0u8; PM_SIZE])?;
    sink.end_file()?;

    Ok(())
}

fn check_range(context: &PsmContext<'_>, offset: u64, size: u64) -> Result<()> {
    let enc = context.header.enc_offset;
    let end = enc
        .checked_add(offset)
        .and_then(|value| value.checked_add(size))
        .ok_or(Error::Corrupt("pkg file is too short, possibly corrupted"))?;
    if context.pkg.len() < end {
        return Err(Error::Corrupt("pkg file is too short, possibly corrupted"));
    }
    Ok(())
}
