//! PSP / PSX / PS3: PBP → PSAR (ISO/CSO), EDAT, KEYS.BIN y temas PSP.
//!
//! Port de `legacy/pkg2zip_psp.c` y de la rama PSP/PSX/PS3 de `legacy/pkg2zip.c`.
//!
//! Advertencia de paridad: construir fixtures sintéticos de PSAR/PGD requiere invertir el MAC del
//! contenedor, así que de momento están cubiertos por tests unitarios y por los vectores del C para
//! la cripto; la paridad end-to-end de ISO/CSO/EDAT necesita una muestra real (ver
//! `docs/paridad.md`).

pub mod lzrc;

use crate::bytes::{get32be, get32le, get64be};
use crate::checksum::{Crc32, combine};
use crate::crypto::{Aes128Ecb, PkgKeys, cmac_aes128, ctr_xor, psp_decrypt};
use crate::error::{Error, Result};
use crate::output::Sink;
use crate::pkg::{Kind, MetaInfo, PkgHeader, Sfo, sfo};
use crate::plat::{DataFile, Ui};

/// Clave kirk 38 (AMCTRL).
const KIRK7_38: [u8; 16] = [
    0x12, 0x46, 0x8d, 0x7e, 0x1c, 0x42, 0x20, 0x9b, 0xba, 0x54, 0x26, 0x83, 0x5e, 0xb0, 0x33, 0x03,
];
/// Clave kirk 39 (AMCTRL).
const KIRK7_39: [u8; 16] = [
    0xc4, 0x3b, 0xb6, 0xd6, 0x53, 0xee, 0x67, 0x49, 0x3e, 0xa9, 0x5f, 0xbc, 0x0c, 0xed, 0x6f, 0x8a,
];
/// Clave kirk 63 (AMCTRL).
const KIRK7_63: [u8; 16] = [
    0x9c, 0x9b, 0x13, 0x72, 0xf8, 0xc6, 0x40, 0xcf, 0x1c, 0x62, 0xf5, 0xd5, 0x92, 0xdd, 0xb5, 0x82,
];
const AMCTL3: [u8; 16] = [
    0xe3, 0x50, 0xed, 0x1d, 0x91, 0x0a, 0x1f, 0xd0, 0x29, 0xbb, 0x1c, 0x3e, 0xf3, 0x40, 0x77, 0xfb,
];
const AMCTL4: [u8; 16] = [
    0x13, 0x5f, 0xa4, 0x7c, 0xab, 0x39, 0x5b, 0xa4, 0x76, 0xb8, 0xcc, 0xa9, 0x8f, 0x3a, 0x04, 0x45,
];
const AMCTL5: [u8; 16] = [
    0x67, 0x8d, 0x7f, 0xa3, 0x2a, 0x9c, 0xa0, 0xd1, 0x50, 0x8a, 0xd8, 0x38, 0x5e, 0x4b, 0x01, 0x7e,
];

const ISO_SECTOR: usize = 2048;
const CSO_HEADER_SIZE: usize = 24;
const CHUNK_SIZE: usize = 1 << 16;

/// Contexto de un PKG PSP/PSX/PS3.
#[derive(Debug)]
pub struct PspContext<'a> {
    /// Archivo PKG.
    pub pkg: &'a DataFile,
    /// Cabecera.
    pub header: &'a PkgHeader,
    /// Metadatos.
    pub info: &'a MetaInfo,
    /// Claves derivadas.
    pub keys: PkgKeys,
    /// Tipo de PKG.
    pub kind: Kind,
    /// Título (SFO de items, PBP o PSAR).
    pub title: String,
    /// ID de 9 caracteres.
    pub id: String,
    /// `CATEGORY` del SFO (vacío si no aplica).
    pub category: String,
    /// RIF decodificado (solo PSX con zRIF).
    pub rif: Option<Vec<u8>>,
}

/// Opciones de desempaquetado PSP.
#[derive(Debug, Clone, Copy)]
pub struct PspOptions {
    /// `-p`: mantener EBOOT.PBP en vez de ISO.
    pub pbp: bool,
    /// `-d`: descifrar siempre EDAT.
    pub ddlc: bool,
    /// `-cN`: 0 = ISO, 1..=9 = CSO con ese nivel.
    pub cso: u8,
    /// Salida en zip (afecta al mensaje de fin).
    pub zipped: bool,
}

/// Busca y parsea el `PARAM.SFO` de los items (PSP/PSX).
///
/// # Errors
///
/// [`Error::Corrupt`] si no aparece el SFO o el item es inválido.
pub fn find_psp_sfo(
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    keys: &PkgKeys,
) -> Result<Sfo> {
    for index in 0..header.item_count as usize {
        let entry = read_item_entry(pkg, header, info, keys, index)?;
        let item = parse_item(&entry);
        let key = item_key(keys, item.psp_type)?;
        let name = read_item_name(pkg, header, &item, key)?;
        if name == "PARAM.SFO" {
            let data = read_encrypted(pkg, header, item.data_offset, item.data_size, key)?;
            return sfo::parse(&data);
        }
    }
    Err(Error::Corrupt("cannot find PARAM.SFO in pkg"))
}

/// Busca el `DISC_ID` en el `PARAM.SFO` del `EBOOT.PBP` (PSP/PSX).
///
/// Devuelve cadena vacía si no hay EBOOT o no declara `DISC_ID`.
///
/// # Errors
///
/// [`Error::Corrupt`] si el PBP o su SFO son inválidos.
pub fn find_pbp_disc_id(
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    keys: &PkgKeys,
) -> Result<String> {
    for index in 0..header.item_count as usize {
        let entry = read_item_entry(pkg, header, info, keys, index)?;
        let item = parse_item(&entry);
        let key = item_key(keys, item.psp_type)?;
        let name = read_item_name(pkg, header, &item, key)?;
        if name != "USRDIR/CONTENT/EBOOT.PBP"
            && name != "USRDIR/CONTENT/PBOOT.PBP"
            && name != "USRDIR/CONTENT/PARAM.PBP"
        {
            continue;
        }
        if item.data_size < 0x28 {
            return Err(Error::Corrupt("eboot.pbp file is too small"));
        }
        let eboot = read_encrypted(pkg, header, item.data_offset, 0x28, key)?;
        if !eboot.starts_with(b"\0PBP") {
            return Err(Error::Corrupt("wrong eboot.pbp header signature!"));
        }
        let sfo_offset = get32le(&eboot[0x08..]) as u64;
        let icon0_offset = get32le(&eboot[0x0C..]) as u64;
        if icon0_offset < sfo_offset {
            return Err(Error::Corrupt("invalid PBP SFO offsets"));
        }
        let sfo_size = icon0_offset - sfo_offset;
        let aligned = sfo_offset - sfo_offset % 16;
        let misalignment = (sfo_offset % 16) as usize;
        if sfo_size < 16 {
            return Err(Error::Corrupt("sfo information is too small"));
        }
        if sfo_size > 16 * 1024 {
            return Err(Error::Corrupt(
                "sfo information is too big, pkg file is probably corrupted",
            ));
        }
        let total =
            usize::try_from(sfo_size).map_err(|_| Error::TooShort("pkg file"))? + misalignment;
        let data = read_encrypted(pkg, header, item.data_offset + aligned, total as u64, key)?;
        let parsed = sfo::parse(&data[misalignment..misalignment + sfo_size as usize])?;
        return Ok(parsed.disc_id.unwrap_or_default());
    }
    Ok(String::new())
}

/// Desempaqueta un PKG PSP/PSX/PS3.
///
/// # Errors
///
/// Cualquier error de formato, cripto o IO.
pub fn unpack(
    context: &PspContext<'_>,
    options: PspOptions,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    ui.progress_init(context.pkg.len());

    let enc = context.header.enc_offset;

    for index in 0..context.header.item_count as usize {
        let entry = read_item_entry(
            context.pkg,
            context.header,
            context.info,
            &context.keys,
            index,
        )?;
        let item = parse_item(&entry);
        if item.flags == 4 || item.flags == 18 {
            continue;
        }
        let key = if matches!(context.kind, Kind::Psp | Kind::Psx | Kind::PspTheme) {
            item_key(&context.keys, item.psp_type)?
        } else {
            &context.keys.main
        };
        let name = read_item_name(context.pkg, context.header, &item, key)?;

        check_item_range(context, &item)?;

        match context.kind {
            Kind::Psp => handle_psp_item(context, options, sink, ui, &item, &name, key, enc)?,
            Kind::Psx => handle_psx_item(context, sink, ui, &item, &name, key, enc)?,
            Kind::Ps3 => handle_ps3_item(context, sink, ui, &item, &name, key, enc)?,
            Kind::PspTheme => {
                let path = format!("pspemu/PSP/THEME/{name}");
                sink.add_parent(&path)?;
                unpack_edat(context, &path, &item, sink, enc)?;
            }
            _ => return Err(Error::NotImplemented("kind not supported in psp module")),
        }
    }

    ui.info(if options.zipped {
        "[*] creating completed"
    } else {
        "[*] unpacking completed"
    });

    if context.kind == Kind::Psx
        && let Some(rif) = &context.rif
    {
        let content = &rif[0x10..];
        let end = content
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(content.len());
        let content_id = String::from_utf8_lossy(&content[..end]);
        ui.info_raw("[*] creating rif");
        sink.add_folder("pspemu/PSP/LICENSE")?;
        let path = format!("pspemu/PSP/LICENSE/{content_id}.rif");
        sink.begin_file(&path, false)?;
        sink.write(rif)?;
        sink.end_file()?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments, unused_assignments)]
fn handle_psp_item(
    context: &PspContext<'_>,
    options: PspOptions,
    sink: &mut Sink,
    ui: &mut dyn Ui,
    item: &Item,
    name: &str,
    key: &[u8; 16],
    enc: u64,
) -> Result<()> {
    let id = &context.id;
    let title = &context.title;
    let mut path: Option<String> = None;

    if name == "USRDIR/CONTENT/EBOOT.PBP" {
        if options.pbp {
            path = Some(format!("pspemu/PSP/GAME/{id}/EBOOT.PBP"));
        } else {
            let extension = if options.cso > 0 { "cso" } else { "iso" };
            let iso_path = format!("pspemu/ISO/{title} [{id}].{extension}");
            sink.add_parent(&iso_path)?;
            let file_offset = sink.begin_file(&iso_path, options.cso == 0)?;
            unpack_eboot(context, file_offset, item, options.cso, sink, ui)?;
            sink.end_file()?;
            return Ok(());
        }
    } else if name == "USRDIR/CONTENT/DOCUMENT.DAT" {
        if !options.pbp {
            return Ok(());
        }
        path = Some(format!("pspemu/PSP/GAME/{id}/DOCUMENT.DAT"));
    } else if name == "USRDIR/CONTENT/DOCINFO.EDAT" {
        if !options.pbp {
            return Ok(());
        }
        path = Some(format!("pspemu/PSP/GAME/{id}/DOCINFO.EDAT"));
    } else if name == "USRDIR/CONTENT/PSP-KEY.EDAT" {
        if !options.pbp || options.ddlc {
            let key_path = format!("pspemu/PSP/GAME/{id}/PSP-KEY.EDAT");
            sink.add_parent(&key_path)?;
            unpack_psp_key(context, &key_path, item, sink, enc)?;
            return Ok(());
        }
        path = Some(format!("pspemu/PSP/GAME/{id}/PSP-KEY.EDAT"));
    } else if name.starts_with("USRDIR/CONTENT/") {
        let rest = name.strip_prefix("USRDIR/CONTENT/").unwrap_or_default();
        if rest.is_empty() {
            return Ok(());
        }
        let lower = rest.to_ascii_lowercase();
        let target = format!("pspemu/PSP/GAME/{id}/{rest}");
        if lower.ends_with(".edat") && (!options.pbp || options.ddlc) {
            sink.add_parent(&target)?;
            unpack_edat(context, &target, item, sink, enc)?;
            return Ok(());
        }
        path = Some(target);
    } else {
        return Ok(());
    }

    if let Some(path) = path {
        write_item(context, &path, item, key, sink, ui, enc)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, unused_assignments)]
fn handle_psx_item(
    context: &PspContext<'_>,
    sink: &mut Sink,
    ui: &mut dyn Ui,
    item: &Item,
    name: &str,
    key: &[u8; 16],
    enc: u64,
) -> Result<()> {
    let id = &context.id;
    let mut path: Option<String> = None;

    match name {
        "USRDIR/CONTENT/DOCUMENT.DAT" => {
            path = Some(format!("pspemu/PSP/GAME/{id}/DOCUMENT.DAT"));
        }
        "USRDIR/CONTENT/EBOOT.PBP" => {
            sink.add_folder(&format!("pspemu/PSP/GAME/{id}"))?;
            let keys_path = format!("pspemu/PSP/GAME/{id}/KEYS.BIN");
            unpack_keys_bin(context, &keys_path, item, sink, enc)?;
            path = Some(format!("pspemu/PSP/GAME/{id}/EBOOT.PBP"));
        }
        "USRDIR/CONTENT/texture.enc" => {
            path = Some(format!("ps1emu/{id}/texture.enc"));
        }
        _ => {}
    }

    if let Some(path) = path {
        write_item(context, &path, item, key, sink, ui, enc)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, unused_assignments)]
fn handle_ps3_item(
    context: &PspContext<'_>,
    sink: &mut Sink,
    ui: &mut dyn Ui,
    item: &Item,
    name: &str,
    key: &[u8; 16],
    enc: u64,
) -> Result<()> {
    let game: String = name.chars().take(9).collect();
    let mut path: Option<String> = None;

    if name.contains("DOCUMENT.DAT") {
        path = Some(format!("pspemu/PSP/GAME/{game}/DOCUMENT.DAT"));
    } else if name.contains("EBOOT.PBP") {
        sink.add_folder(&format!("pspemu/PSP/GAME/{game}"))?;
        let keys_path = format!("pspemu/PSP/GAME/{game}/KEYS.BIN");
        unpack_keys_bin(context, &keys_path, item, sink, enc)?;
        path = Some(format!("pspemu/PSP/GAME/{game}/EBOOT.PBP"));
    }

    if let Some(path) = path {
        write_item(context, &path, item, key, sink, ui, enc)?;
    }
    Ok(())
}

fn write_item(
    context: &PspContext<'_>,
    path: &str,
    item: &Item,
    key: &[u8; 16],
    sink: &mut Sink,
    ui: &mut dyn Ui,
    enc: u64,
) -> Result<()> {
    sink.add_parent(path)?;
    sink.begin_file(path, false)?;
    copy_item_data(context, item, key, sink, ui, enc)?;
    sink.end_file()
}

fn copy_item_data(
    context: &PspContext<'_>,
    item: &Item,
    key: &[u8; 16],
    sink: &mut Sink,
    ui: &mut dyn Ui,
    enc: u64,
) -> Result<()> {
    let mut offset = item.data_offset;
    let mut remaining = item.data_size;
    while remaining != 0 {
        let size = usize::try_from(remaining.min(CHUNK_SIZE as u64)).expect("cabe en usize");
        ui.progress(enc + offset);
        let mut buffer = context.pkg.read_vec(enc + offset, size)?;
        ctr_xor(key, &context.header.iv, offset / 16, &mut buffer)?;
        sink.write(&buffer)?;
        offset += size as u64;
        remaining -= size as u64;
    }
    Ok(())
}

fn unpack_eboot(
    context: &PspContext<'_>,
    file_offset: u64,
    item: &Item,
    cso_level: u8,
    sink: &mut Sink,
    ui: &mut dyn Ui,
) -> Result<()> {
    if item.data_size < 0x28 {
        return Err(Error::Corrupt("eboot.pbp file is to short!"));
    }
    let main_key = &context.keys.main;
    let eboot = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset,
        0x28,
        main_key,
    )?;
    if !eboot.starts_with(b"\0PBP") {
        return Err(Error::Corrupt("wrong eboot.pbp header signature!"));
    }
    let psar_offset = u64::from(get32le(&eboot[0x24..]));
    if psar_offset % 16 != 0 {
        return Err(Error::Corrupt("data.psar offset is not aligned"));
    }
    if psar_offset + 256 > item.data_size {
        return Err(Error::Corrupt("eboot.pbp file is to short!"));
    }

    let psar = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + psar_offset,
        256,
        main_key,
    )?;
    if !psar.starts_with(b"NPUMDIMG") {
        return Err(Error::Corrupt("wrong data.psar header signature!"));
    }
    let iso_block = get32le(&psar[0x0C..]);
    if iso_block > 16 {
        return Err(Error::Corrupt(
            "unsupported data.psar block size, max 16 supported!",
        ));
    }

    let mac = cmac_aes128(&KIRK7_38, &psar[..0xc0]);
    let (psp_key, psp_iv) = init_psp_decrypt(true, &mac, &psar, 0xc0, 0xa0)?;
    let mut header = psar.clone();
    psp_decrypt(&psp_key, &psp_iv, 0, &mut header[0x40..0xa0])?;

    let iso_start = get32le(&header[0x54..]);
    let iso_end = get32le(&header[0x64..]);
    let iso_total = iso_end.wrapping_sub(iso_start).wrapping_sub(1);
    let block_size = iso_block * ISO_SECTOR as u32;
    let block_count = iso_total.div_ceil(iso_block);
    let iso_table = u64::from(get32le(&header[0x6C..]));

    if iso_table + u64::from(block_count) * 32 > item.data_size {
        return Err(Error::Corrupt(
            "offset table in data.psar file is too large!",
        ));
    }

    let iso_size = u64::from(block_count) * u64::from(block_size);
    let mut cso = if cso_level > 0 {
        let encoder = CsoEncoder::new(iso_size, cso_level);
        sink.set_offset(file_offset + u64::from(encoder.initial_size()))?;
        Some(encoder)
    } else {
        None
    };

    let base = item.data_offset + psar_offset;
    for index in 0..block_count {
        let table_offset = base + iso_table + u64::from(index) * 32;
        let table = read_encrypted(context.pkg, context.header, table_offset, 32, main_key)?;
        let mut values = [0u32; 8];
        for (slot, value) in values.iter_mut().enumerate() {
            *value = get32le(&table[slot * 4..]);
        }
        let block_offset = values[4] ^ values[2] ^ values[3];
        let current_size = values[5] ^ values[1] ^ values[2];
        let block_flags = values[6] ^ values[0] ^ values[3];

        if psar_offset + u64::from(current_size) > item.data_size {
            return Err(Error::Corrupt("iso block size/offset is to large!"));
        }

        let absolute = item.data_offset + psar_offset + u64::from(block_offset);
        ui.progress(context.header.enc_offset + absolute);
        let mut data = read_encrypted(
            context.pkg,
            context.header,
            absolute,
            u64::from(current_size),
            main_key,
        )?;

        if block_flags & 4 == 0 {
            psp_decrypt(&psp_key, &psp_iv, block_offset / 16, &mut data)?;
        }

        if current_size == block_size {
            write_iso_block(&mut cso, sink, &data)?;
        } else {
            let uncompressed = lzrc::decompress(&data, block_size as usize)?;
            if uncompressed.len() != block_size as usize {
                return Err(Error::Corrupt(
                    "internal error - lzrc decompression failed! pkg may be corrupted?",
                ));
            }
            write_iso_block(&mut cso, sink, &uncompressed)?;
        }
    }

    if let Some(encoder) = cso {
        encoder.finish(sink, file_offset)?;
    }

    Ok(())
}

fn write_iso_block(cso: &mut Option<CsoEncoder>, sink: &mut Sink, data: &[u8]) -> Result<()> {
    match cso {
        None => sink.write(data),
        Some(encoder) => {
            let (sectors, _rest) = data.as_chunks::<ISO_SECTOR>();
            for sector in sectors {
                encoder.write_sector(sink, sector)?;
            }
            Ok(())
        }
    }
}

/// Escritor CSO incremental (cabecera + índice + sectores comprimidos).
#[derive(Debug)]
struct CsoEncoder {
    total_iso: u64,
    level: u8,
    index: Vec<u32>,
    offset: u32,
    initial: u32,
}

impl CsoEncoder {
    fn new(total_iso: u64, level: u8) -> Self {
        let sectors = total_iso.div_ceil(ISO_SECTOR as u64);
        let count = sectors + 1;
        let initial = u32::try_from(CSO_HEADER_SIZE as u64 + count * 4).expect("índice CSO cabe");
        Self {
            total_iso,
            level,
            index: Vec::new(),
            offset: initial,
            initial,
        }
    }

    fn initial_size(&self) -> u32 {
        self.initial
    }

    fn write_sector(&mut self, sink: &mut Sink, sector: &[u8]) -> Result<()> {
        let compressed = miniz_oxide::deflate::compress_to_vec(sector, self.level);
        if compressed.len() < ISO_SECTOR {
            self.index.push(self.offset);
            sink.write(&compressed)?;
            self.offset += u32::try_from(compressed.len()).expect("sector comprimido cabe");
        } else {
            self.index.push(self.offset | 0x8000_0000);
            sink.write(sector)?;
            self.offset += ISO_SECTOR as u32;
        }
        Ok(())
    }

    fn finish(mut self, sink: &mut Sink, file_offset: u64) -> Result<()> {
        self.index.push(self.offset);
        let data_len = self.offset - self.initial;

        let mut header = [0u8; CSO_HEADER_SIZE];
        header[..4].copy_from_slice(b"CISO");
        header[4..8].copy_from_slice(&(CSO_HEADER_SIZE as u32).to_le_bytes());
        header[8..16].copy_from_slice(&self.total_iso.to_le_bytes());
        header[16..20].copy_from_slice(&(ISO_SECTOR as u32).to_le_bytes());
        header[20] = 1;

        let mut index_bytes = Vec::with_capacity(self.index.len() * 4);
        for value in &self.index {
            index_bytes.extend_from_slice(&value.to_le_bytes());
        }

        let mut hasher = Crc32::new();
        hasher.update(&header);
        hasher.update(&index_bytes);
        let header_crc = hasher.finish();
        let data_crc = sink.zip_crc32();
        let combined = combine(header_crc, data_crc, u64::from(data_len));

        sink.set_zip_crc32(combined);
        sink.write_at(file_offset, &header)?;
        sink.write_at(file_offset + CSO_HEADER_SIZE as u64, &index_bytes)?;
        Ok(())
    }
}

/// `(data_offset, data_size)` del primer item (el C lo usa para el título de temas PSP).
///
/// # Errors
///
/// Cualquier error de lectura o de tabla de items.
pub fn first_item_data(
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    keys: &PkgKeys,
) -> Result<(u64, u64)> {
    let entry = read_item_entry(pkg, header, info, keys, 0)?;
    let item = parse_item(&entry);
    Ok((item.data_offset, item.data_size))
}

fn read_item_entry(
    pkg: &DataFile,
    header: &PkgHeader,
    info: &MetaInfo,
    keys: &PkgKeys,
    index: usize,
) -> Result<[u8; 32]> {
    let relative = u64::from(info.items_offset) + (index as u64) * 32;
    let mut entry: [u8; 32] = pkg
        .read_vec(header.enc_offset + relative, 32)?
        .try_into()
        .expect("32 bytes");
    ctr_xor(&keys.main, &header.iv, relative / 16, &mut entry)?;
    Ok(entry)
}

fn read_item_name(
    pkg: &DataFile,
    header: &PkgHeader,
    item: &Item,
    key: &[u8; 16],
) -> Result<String> {
    let mut name = pkg.read_vec(
        header.enc_offset + item.name_offset,
        item.name_size as usize,
    )?;
    ctr_xor(key, &header.iv, item.name_offset / 16, &mut name)?;
    Ok(String::from_utf8_lossy(&name).into_owned())
}

fn read_encrypted(
    pkg: &DataFile,
    header: &PkgHeader,
    offset: u64,
    size: u64,
    key: &[u8; 16],
) -> Result<Vec<u8>> {
    let size = usize::try_from(size).map_err(|_| Error::TooShort("pkg file"))?;
    let mut buffer = pkg.read_vec(header.enc_offset + offset, size)?;
    ctr_xor(key, &header.iv, offset / 16, &mut buffer)?;
    Ok(buffer)
}

struct Item {
    name_offset: u64,
    name_size: u32,
    data_offset: u64,
    data_size: u64,
    psp_type: u8,
    flags: u8,
}

fn parse_item(entry: &[u8; 32]) -> Item {
    Item {
        name_offset: u64::from(get32be(&entry[0..])),
        name_size: get32be(&entry[4..]),
        data_offset: get64be(&entry[8..]),
        data_size: get64be(&entry[16..]),
        psp_type: entry[24],
        flags: entry[27],
    }
}

fn item_key(keys: &PkgKeys, psp_type: u8) -> Result<&[u8; 16]> {
    if psp_type == 0x90 {
        Ok(&keys.main)
    } else {
        keys.ps3
            .as_ref()
            .ok_or(Error::Corrupt("pkg is missing the PS3 key"))
    }
}

fn check_item_range(context: &PspContext<'_>, item: &Item) -> Result<()> {
    let enc = context.header.enc_offset;
    let name_end = enc
        .checked_add(item.name_offset)
        .and_then(|value| value.checked_add(u64::from(item.name_size)))
        .ok_or(Error::TooShort("pkg file"))?;
    let data_end = enc
        .checked_add(item.data_offset)
        .and_then(|value| value.checked_add(item.data_size))
        .ok_or(Error::TooShort("pkg file"))?;
    if context.pkg.len() < name_end || context.pkg.len() < data_end {
        return Err(Error::Corrupt("pkg file is too short, possibly corrupted"));
    }
    if item.name_size >= 1024 {
        return Err(Error::Corrupt("pkg file contains file with very long name"));
    }
    Ok(())
}

fn init_psp_decrypt(
    eboot: bool,
    mac: &[u8; 16],
    header: &[u8],
    offset1: usize,
    offset2: usize,
) -> Result<(Aes128Ecb, [u8; 16])> {
    if header.len() < offset2 + 16 || header.len() < offset1 + 16 {
        return Err(Error::Corrupt("PSP header is too short"));
    }
    let key = Aes128Ecb::new(&KIRK7_63);
    let mut tmp = [0u8; 16];
    tmp.copy_from_slice(&header[offset1..offset1 + 16]);
    if eboot {
        key.decrypt_block(&mut tmp);
    }
    let aes38 = Aes128Ecb::new(&KIRK7_38);
    aes38.decrypt_block(&mut tmp);

    let mut iv = [0u8; 16];
    for index in 0..16 {
        iv[index] =
            mac[index] ^ tmp[index] ^ header[offset2 + index] ^ AMCTL3[index] ^ AMCTL5[index];
    }
    let aes39 = Aes128Ecb::new(&KIRK7_39);
    aes39.decrypt_block(&mut iv);
    for index in 0..16 {
        iv[index] ^= AMCTL4[index];
    }
    Ok((key, iv))
}

fn init_psx_keys(mac: &[u8; 16], header: &[u8], offset: usize) -> Result<[u8; 16]> {
    if header.len() < offset + 16 {
        return Err(Error::Corrupt("PSP header is too short"));
    }
    let mut tmp = [0u8; 16];
    tmp.copy_from_slice(&header[offset..offset + 16]);
    Aes128Ecb::new(&KIRK7_38).decrypt_block(&mut tmp);

    let mut keys = [0u8; 16];
    for index in 0..16 {
        keys[index] = mac[index] ^ tmp[index] ^ AMCTL3[index];
    }
    Ok(keys)
}

fn unpack_psp_key(
    context: &PspContext<'_>,
    path: &str,
    item: &Item,
    sink: &mut Sink,
    enc: u64,
) -> Result<()> {
    if item.data_size < 0x90 + 0xa0 {
        return Err(Error::Corrupt("PSP-KEY.EDAT file is to short!"));
    }
    let header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + 0x90,
        0xa0,
        &context.keys.main,
    )?;
    if !header.starts_with(b"\0PGD") {
        return Err(Error::Corrupt("wrong PSP-KEY.EDAT header signature!"));
    }
    let key_index = get32le(&header[4..]);
    let drm_type = get32le(&header[8..]);
    if key_index != 1 || drm_type != 1 {
        return Err(Error::Corrupt(
            "unsupported PSP-KEY.EDAT file, key/drm type is wrong!",
        ));
    }
    let mac = cmac_aes128(&KIRK7_38, &header[..0x70]);
    let (key, iv) = init_psp_decrypt(false, &mac, &header, 0x70, 0x10)?;
    let mut header = header;
    psp_decrypt(&key, &iv, 0, &mut header[0x30..0x60])?;

    let data_size = get32le(&header[0x44..]);
    let data_offset = get32le(&header[0x4c..]);
    if data_size != 0x10 || data_offset != 0x90 {
        return Err(Error::Corrupt(
            "unsupported PSP-KEY.EDAT file, data/offset is wrong!",
        ));
    }

    let (key, iv) = init_psp_decrypt(false, &mac, &header, 0x70, 0x30)?;
    psp_decrypt(&key, &iv, 0, &mut header[0x90..0xa0])?;

    let _ = enc;
    sink.begin_file(path, false)?;
    sink.write(&header[0x90..0xa0])?;
    sink.end_file()?;
    Ok(())
}

fn unpack_edat(
    context: &PspContext<'_>,
    path: &str,
    item: &Item,
    sink: &mut Sink,
    enc: u64,
) -> Result<()> {
    if item.data_size < 0x90 + 0xa0 {
        return Err(Error::Corrupt("EDAT file is to short!"));
    }
    let item_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset,
        90,
        &context.keys.main,
    )?;
    let key_header_offset = u64::from(item_header[0xC]);

    let key_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + key_header_offset,
        0xa0,
        &context.keys.main,
    )?;
    if !key_header.starts_with(b"\0PGD") {
        return Err(Error::Corrupt("wrong EDAT header signature!"));
    }
    let key_index = get32le(&key_header[4..]);
    let drm_type = get32le(&key_header[8..]);
    if key_index != 1 || drm_type != 1 {
        return Err(Error::Corrupt(
            "unsupported EDAT file, key/drm type is wrong!",
        ));
    }

    let mac = cmac_aes128(&KIRK7_38, &key_header[..0x70]);
    let (key, iv) = init_psp_decrypt(false, &mac, &key_header, 0x70, 0x10)?;
    let mut key_header = key_header;
    psp_decrypt(&key, &iv, 0, &mut key_header[0x30..0x60])?;

    let data_size = get32le(&key_header[0x44..]);
    let data_offset = get32le(&key_header[0x4c..]);
    if data_offset != 0x90 {
        return Err(Error::Corrupt(
            "unsupported EDAT file, data offset is wrong!",
        ));
    }

    let (key, iv) = init_psp_decrypt(false, &mac, &key_header, 0x70, 0x30)?;
    let block_size = 0x10u32;
    let block_count = data_size.div_ceil(block_size);

    sink.begin_file(path, false)?;
    for index in 0..block_count {
        let offset = item.data_offset
            + key_header_offset
            + u64::from(data_offset)
            + u64::from(index * block_size);
        let mut block = read_encrypted(
            context.pkg,
            context.header,
            offset,
            block_size.into(),
            &context.keys.main,
        )?;
        psp_decrypt(&key, &iv, index * block_size / 16, &mut block)?;
        let out_size = if (index + 1) * block_size > data_size {
            usize::try_from(data_size - index * block_size).expect("cabe en usize")
        } else {
            block_size as usize
        };
        sink.write(&block[..out_size])?;
    }
    sink.end_file()?;
    let _ = enc;
    Ok(())
}

fn unpack_keys_bin(
    context: &PspContext<'_>,
    path: &str,
    item: &Item,
    sink: &mut Sink,
    enc: u64,
) -> Result<()> {
    if item.data_size < 0x28 + 0x10 + 0x1f0 {
        return Err(Error::Corrupt("EBOOT file is to short!"));
    }
    let eboot = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset,
        0x28,
        &context.keys.main,
    )?;
    if !eboot.starts_with(b"\0PBP") {
        return Err(Error::Corrupt("wrong eboot header signature!"));
    }
    let psar_offset = u64::from(get32le(&eboot[0x24..]));
    if psar_offset % 16 != 0 {
        return Err(Error::Corrupt("data.psar offset is not aligned"));
    }
    if psar_offset + 16 > item.data_size {
        return Err(Error::Corrupt("eboot file is to short!"));
    }

    let psar_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + psar_offset,
        16,
        &context.keys.main,
    )?;
    let key_header_offset = if psar_header.starts_with(b"PSTITLE") {
        0x1f0u64
    } else if psar_header.starts_with(b"PSISO") {
        0x3f0u64
    } else {
        return Err(Error::Corrupt("wrong psar header signature!"));
    };

    let key_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + psar_offset + 16 + key_header_offset,
        0x90,
        &context.keys.main,
    )?;
    if !key_header.starts_with(b"\0PGD") {
        return Err(Error::Corrupt("wrong key header signature!"));
    }
    let key_index = get32le(&key_header[4..]);
    let drm_type = get32le(&key_header[8..]);
    if key_index != 1 || drm_type != 1 {
        return Err(Error::Corrupt(
            "unsupported EBOOT file, key/drm type is unsupported!",
        ));
    }

    let mac = cmac_aes128(&KIRK7_38, &key_header[..0x70]);
    let keys_bin = init_psx_keys(&mac, &key_header, 0x70)?;

    let _ = enc;
    sink.begin_file(path, false)?;
    sink.write(&keys_bin)?;
    sink.end_file()
}

/// Devuelve el título de un tema PSP leyendo el PTF descifrado.
///
/// `data_offset`/`data_size` son los del primer item del PKG (igual que el C).
///
/// # Errors
///
/// [`Error::Corrupt`] si el EDAT del tema es inválido.
pub fn get_psp_theme_title(
    context: &PspContext<'_>,
    data_offset: u64,
    data_size: u64,
) -> Result<String> {
    let item = Item {
        name_offset: 0,
        name_size: 0,
        data_offset,
        data_size,
        psp_type: 0,
        flags: 0,
    };
    if item.data_size < 0x90 + 0xa0 {
        return Err(Error::Corrupt("theme EDAT file is to short!"));
    }
    let item_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset,
        90,
        &context.keys.main,
    )?;
    let key_header_offset = u64::from(item_header[0xC]);
    let key_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + key_header_offset,
        0xa0,
        &context.keys.main,
    )?;
    if !key_header.starts_with(b"\0PGD") {
        return Err(Error::Corrupt("wrong EDAT header signature!"));
    }
    let mac = cmac_aes128(&KIRK7_38, &key_header[..0x70]);
    let (key, iv) = init_psp_decrypt(false, &mac, &key_header, 0x70, 0x10)?;
    let mut key_header = key_header;
    psp_decrypt(&key, &iv, 0, &mut key_header[0x30..0x60])?;
    let data_offset = get32le(&key_header[0x4c..]);

    let mut theme_header = read_encrypted(
        context.pkg,
        context.header,
        item.data_offset + key_header_offset + u64::from(data_offset),
        256,
        &context.keys.main,
    )?;
    let (key, iv) = init_psp_decrypt(false, &mac, &key_header, 0x70, 0x30)?;
    psp_decrypt(&key, &iv, 0, &mut theme_header)?;
    if !theme_header.starts_with(b"\0PTF") {
        return Err(Error::Corrupt("wrong PTF header signature!"));
    }
    let title = &theme_header[8..8 + 128];
    let end = title.iter().position(|&b| b == 0).unwrap_or(title.len());
    Ok(String::from_utf8_lossy(&title[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectores golden generados con `legacy/pkg2zip_aes.c` + las funciones `init_psp_decrypt` /
    // `init_psx_keys` copiadas literalmente de `legacy/pkg2zip_psp.c` (MSVC x64, ruta AES-NI).
    const GOLDEN_IV: &str = "f4067154469d4e0f40cbc6071f5ffe9a";
    const GOLDEN_BLOCK: &str = "66aa072a7ae9a10564a1d0fec1ef4c1f47e31d8200e62270ec1a49cd7be9561a\
                                60d16aa2eda4d6742d5a271a915a4e3774c15dfc4bdc88799ff1dde641d1d50d\
                                ccd280d6064d9f277c41b77ef975d96c0908fccce61e6f1fd498b8948a3a32eb";
    const GOLDEN_PSX_KEYS: &str = "6568f27b57755447a4f014a47456756c";

    fn hex_to_vec(text: &str) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        (0..clean.len() / 2)
            .map(|i| u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).expect("hex"))
            .collect()
    }

    fn sample_header() -> Vec<u8> {
        (0..256).map(|i| (i * 3 + 7) as u8).collect()
    }

    fn sample_mac() -> [u8; 16] {
        let mut mac = [0u8; 16];
        for (i, byte) in mac.iter_mut().enumerate() {
            *byte = (0xA0 + i) as u8;
        }
        mac
    }

    #[test]
    fn golden_init_psp_decrypt() {
        let header = sample_header();
        let mac = sample_mac();
        let (key, iv) = init_psp_decrypt(true, &mac, &header, 0xc0, 0xa0).expect("init");
        assert_eq!(iv, hex_to_vec(GOLDEN_IV).as_slice());

        let mut block = header[0x40..0xa0].to_vec();
        psp_decrypt(&key, &iv, 0, &mut block).expect("psp decrypt");
        assert_eq!(block, hex_to_vec(GOLDEN_BLOCK));
    }

    #[test]
    fn golden_init_psx_keys() {
        let header = sample_header();
        let mac = sample_mac();
        let keys = init_psx_keys(&mac, &header, 0x70).expect("keys");
        assert_eq!(keys, hex_to_vec(GOLDEN_PSX_KEYS).as_slice());
    }

    #[test]
    fn cso_roundtrip_preserves_iso() {
        // El sink de archivos exige rutas relativas (protección contra traversal), así que el test
        // usa un directorio relativo al crate y lo limpia al terminar.
        let dir = std::path::PathBuf::from(format!("cso-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("out.cso");
        let relative = path.to_string_lossy().replace('\\', "/");
        let iso: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(7) % 251) as u8)
            .collect();

        let mut sink = crate::output::Sink::create(&relative, false).expect("sink");
        let mut encoder = CsoEncoder::new(iso.len() as u64, 9);
        sink.begin_file(&relative, false).expect("begin");
        sink.set_offset(u64::from(encoder.initial_size()))
            .expect("offset");
        for sector in iso.as_chunks::<ISO_SECTOR>().0 {
            encoder.write_sector(&mut sink, sector).expect("sector");
        }
        encoder.finish(&mut sink, 0).expect("finish");
        sink.end_file().expect("end");
        sink.finish().expect("close");

        let bytes = std::fs::read(&path).expect("leer cso");
        let decoded = decode_cso(&bytes);
        assert_eq!(decoded, iso);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    fn decode_cso(bytes: &[u8]) -> Vec<u8> {
        assert_eq!(&bytes[..4], b"CISO");
        let header_size = u32::from_le_bytes(bytes[4..8].try_into().expect("header")) as usize;
        let total = u64::from_le_bytes(bytes[8..16].try_into().expect("total")) as usize;
        let block = u32::from_le_bytes(bytes[16..20].try_into().expect("block")) as usize;
        let count = total.div_ceil(block) + 1;
        let mut index = Vec::with_capacity(count);
        for i in 0..count {
            let start = header_size + i * 4;
            index.push(u32::from_le_bytes(
                bytes[start..start + 4].try_into().expect("idx"),
            ));
        }

        let mut out = Vec::with_capacity(total);
        for i in 0..count - 1 {
            let start = (index[i] & 0x7fff_ffff) as usize;
            let end = (index[i + 1] & 0x7fff_ffff) as usize;
            let raw = &bytes[start..end];
            if index[i] & 0x8000_0000 != 0 {
                out.extend_from_slice(raw);
            } else {
                let mut decoder = flate2::read::DeflateDecoder::new(raw);
                let mut sector = Vec::new();
                std::io::Read::read_to_end(&mut decoder, &mut sector).expect("inflate");
                out.extend_from_slice(&sector);
            }
        }
        out.truncate(total);
        out
    }
}
