//! Escritor ZIP/ZIP64 compatible con `legacy/pkg2zip_zip.c`.
//!
//! Soporta lo que el formato necesita:
//! * entradas almacenadas (`STORE`) o deflate,
//! * fijar el CRC después de escribir (el CSO lo necesita),
//! * escribir en offsets absolutos y recolocar la posición secuencial.
//!
//! El layout (orden de entradas, atributos DOS, campos ZIP64) se conserva para que el zip sea
//! equivalente al del binario C. El C vuelve a leer el nombre desde el archivo para construir el
//! directorio central; aquí se guarda en memoria (mismo resultado, sin IO extra).

use std::time::{SystemTime, UNIX_EPOCH};

use flate2::{Compress, Compression, FlushCompress, Status};

use crate::checksum::Crc32;
use crate::error::{Error, Result};
use crate::plat::file::OutputFile;

const ZIP_VERSION: u16 = 45;
const ZIP_METHOD_STORE: u16 = 0;
const ZIP_METHOD_DEFLATE: u16 = 8;
const ZIP_UTF8_FLAG: u16 = 1 << 11;

const ZIP_DOS_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const ZIP_DOS_ATTRIBUTE_ARCHIVE: u32 = 0x20;

const ZIP_LOCAL_HEADER_SIZE: usize = 30;
const ZIP_GLOBAL_HEADER_SIZE: usize = 46;
const ZIP64_EOC_DIR_SIZE: usize = 56;
const ZIP64_EOC_DIR_LOCATOR_SIZE: usize = 20;
const ZIP_EOC_DIR_SIZE: usize = 22;

const ZIP_LOCAL_HEADER_CRC32_OFFSET: usize = 14;

/// Longitud máxima de nombre aceptada (igual que `ZIP_MAX_FILENAME` en el C).
pub const ZIP_MAX_FILENAME: usize = 1024;

#[derive(Debug, Clone)]
struct ZipEntry {
    name: Vec<u8>,
    offset: u64,
    size: u64,
    compressed: u64,
    crc32: u32,
    compress: bool,
    is_folder: bool,
}

/// Escritor ZIP/ZIP64.
#[derive(Debug)]
pub struct ZipWriter {
    file: OutputFile,
    total: u64,
    entries: Vec<ZipEntry>,
    current: Option<usize>,
    compressor: Option<Compress>,
    crc: Crc32,
    crc_override: Option<u32>,
    time: u16,
    date: u16,
}

impl ZipWriter {
    /// Crea el archivo zip y fija la fecha/hora DOS (UTC).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si no se puede crear el archivo.
    pub fn create(path: &std::path::Path) -> Result<Self> {
        let (date, time) = dos_datetime(SystemTime::now());
        Ok(Self {
            file: OutputFile::create(path)?,
            total: 0,
            entries: Vec::new(),
            current: None,
            compressor: None,
            crc: Crc32::new(),
            crc_override: None,
            time,
            date,
        })
    }

    /// Añade una entrada de directorio (`nombre/`).
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si el nombre excede [`ZIP_MAX_FILENAME`], o [`Error::Io`].
    pub fn add_folder(&mut self, name: &str) -> Result<()> {
        if name.len() + 1 > ZIP_MAX_FILENAME {
            return Err(Error::Corrupt("dirname too long"));
        }
        let mut entry_name = name.as_bytes().to_vec();
        entry_name.push(b'/');

        self.entries.push(ZipEntry {
            name: entry_name.clone(),
            offset: self.total,
            size: 0,
            compressed: 0,
            crc32: 0,
            compress: false,
            is_folder: true,
        });

        let mut header = [0u8; ZIP_LOCAL_HEADER_SIZE];
        header[..4].copy_from_slice(b"PK\x03\x04");
        set16le(&mut header[4..], ZIP_VERSION);
        set16le(&mut header[6..], ZIP_UTF8_FLAG);
        set16le(&mut header[8..], ZIP_METHOD_STORE);
        set16le(&mut header[10..], self.time);
        set16le(&mut header[12..], self.date);
        set16le(
            &mut header[26..],
            u16::try_from(entry_name.len()).expect("nombre acotado"),
        );

        self.file.write_all(&header)?;
        self.total += ZIP_LOCAL_HEADER_SIZE as u64;
        self.file.write_all(&entry_name)?;
        self.total += entry_name.len() as u64;
        Ok(())
    }

    /// Empieza un archivo y devuelve el tamaño de la cabecera local escrita.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si el nombre es demasiado largo.
    pub fn begin_file(&mut self, name: &str, compress: bool) -> Result<u64> {
        if name.len() > ZIP_MAX_FILENAME {
            return Err(Error::Corrupt("filename too long"));
        }
        let offset = self.total;
        self.entries.push(ZipEntry {
            name: name.as_bytes().to_vec(),
            offset,
            size: 0,
            compressed: 0,
            crc32: 0,
            compress,
            is_folder: false,
        });
        self.current = Some(self.entries.len() - 1);
        self.crc = Crc32::new();
        self.crc_override = None;
        self.compressor = compress.then(|| Compress::new(Compression::fast(), false));

        let mut header = [0u8; ZIP_LOCAL_HEADER_SIZE];
        header[..4].copy_from_slice(b"PK\x03\x04");
        set16le(&mut header[4..], ZIP_VERSION);
        set16le(&mut header[6..], ZIP_UTF8_FLAG);
        set16le(
            &mut header[8..],
            if compress {
                ZIP_METHOD_DEFLATE
            } else {
                ZIP_METHOD_STORE
            },
        );
        set16le(&mut header[10..], self.time);
        set16le(&mut header[12..], self.date);
        set16le(
            &mut header[26..],
            u16::try_from(name.len()).expect("nombre acotado"),
        );

        self.file.write_all(&header)?;
        self.total += ZIP_LOCAL_HEADER_SIZE as u64;
        self.file.write_all(name.as_bytes())?;
        self.total += name.len() as u64;

        Ok(self.total - offset)
    }

    /// Escribe datos de la entrada actual.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si no hay entrada activa o el deflate falla; [`Error::Io`].
    pub fn write_file(&mut self, data: &[u8]) -> Result<()> {
        let index = self.current.ok_or(Error::Corrupt("no current zip entry"))?;
        self.entries[index].size += data.len() as u64;
        self.crc.update(data);

        if self.entries[index].compress {
            self.deflate(data, FlushCompress::None)?;
        } else {
            self.file.write_all(data)?;
            self.entries[index].compressed += data.len() as u64;
            self.total += data.len() as u64;
        }
        Ok(())
    }

    /// Termina la entrada actual y parchea su cabecera local con CRC y tamaños.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si no hay entrada activa; [`Error::Io`].
    pub fn end_file(&mut self) -> Result<()> {
        let index = self.current.ok_or(Error::Corrupt("no current zip entry"))?;

        if self.entries[index].compress {
            self.deflate(&[], FlushCompress::Finish)?;
            self.compressor = None;
        }

        let crc = self
            .crc_override
            .take()
            .unwrap_or_else(|| self.crc.current());
        self.entries[index].crc32 = crc;

        if self.entries[index].size != 0 {
            let mut update = [0u8; 12];
            set32le(&mut update[0..], crc);
            set32le(
                &mut update[4..],
                u32::try_from(self.entries[index].compressed.min(u64::from(u32::MAX)))
                    .expect("saturado"),
            );
            set32le(
                &mut update[8..],
                u32::try_from(self.entries[index].size.min(u64::from(u32::MAX))).expect("saturado"),
            );
            self.file.write_all_at(
                self.entries[index].offset + ZIP_LOCAL_HEADER_CRC32_OFFSET as u64,
                &update,
            )?;
        }

        self.current = None;
        Ok(())
    }

    /// Cierra el zip escribiendo el directorio central y los registros finales.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si falla alguna escritura.
    pub fn close(mut self) -> Result<()> {
        let central_offset = self.total;

        let names: Vec<Vec<u8>> = self.entries.iter().map(|e| e.name.clone()).collect();
        for (index, entry) in self.entries.iter().enumerate() {
            let name = &names[index];
            let filename_length = u16::try_from(name.len()).expect("nombre acotado");

            let mut extra = [0u8; 28];
            let mut extra_size: u16 = 0;
            let attributes = if entry.is_folder {
                let attrs = ZIP_DOS_ATTRIBUTE_ARCHIVE | ZIP_DOS_ATTRIBUTE_DIRECTORY;
                if entry.offset > 0xffff_ffff {
                    extra_size += 8;
                }
                attrs
            } else {
                if entry.size > 0xffff_ffff {
                    extra_size += 8;
                }
                if entry.compressed > 0xffff_ffff {
                    extra_size += 8;
                }
                if entry.offset > 0xffff_ffff {
                    extra_size += 8;
                }
                ZIP_DOS_ATTRIBUTE_ARCHIVE
            };
            if extra_size != 0 {
                extra_size += 4;
            }

            let mut global = vec![0u8; ZIP_GLOBAL_HEADER_SIZE + name.len()];
            global[..4].copy_from_slice(b"PK\x01\x02");
            set16le(&mut global[4..], ZIP_VERSION);
            set16le(&mut global[6..], ZIP_VERSION);
            set16le(&mut global[8..], ZIP_UTF8_FLAG);
            set16le(
                &mut global[10..],
                if entry.compress {
                    ZIP_METHOD_DEFLATE
                } else {
                    ZIP_METHOD_STORE
                },
            );
            set16le(&mut global[12..], self.time);
            set16le(&mut global[14..], self.date);
            set32le(&mut global[16..], entry.crc32);
            set32le(
                &mut global[20..],
                u32::try_from(entry.compressed.min(u64::from(u32::MAX))).expect("saturado"),
            );
            set32le(
                &mut global[24..],
                u32::try_from(entry.size.min(u64::from(u32::MAX))).expect("saturado"),
            );
            set16le(&mut global[28..], filename_length);
            set16le(&mut global[30..], extra_size);
            set32le(&mut global[38..], attributes);
            set32le(
                &mut global[42..],
                u32::try_from(entry.offset.min(u64::from(u32::MAX))).expect("saturado"),
            );
            global[ZIP_GLOBAL_HEADER_SIZE..].copy_from_slice(name);
            self.file.write_all(&global)?;
            self.total += global.len() as u64;

            set16le(&mut extra[0..], 1);
            set16le(&mut extra[2..], extra_size.saturating_sub(4));
            let mut extra_offset = 4;
            if entry.compressed > 0xffff_ffff {
                set64le(&mut extra[extra_offset..], entry.compressed);
                extra_offset += 8;
            }
            if entry.size > 0xffff_ffff {
                set64le(&mut extra[extra_offset..], entry.size);
                extra_offset += 8;
            }
            if entry.offset > 0xffff_ffff {
                set64le(&mut extra[extra_offset..], entry.offset);
            }
            if extra_size > 4 {
                self.file.write_all(&extra[..usize::from(extra_size)])?;
                self.total += u64::from(extra_size);
            }
        }

        let end_of_central_offset = self.total;
        let central_size = end_of_central_offset - central_offset;

        let mut zip64_eoc = [0u8; ZIP64_EOC_DIR_SIZE];
        zip64_eoc[..4].copy_from_slice(b"PK\x06\x06");
        set64le(
            &mut zip64_eoc[4..],
            (ZIP64_EOC_DIR_SIZE - std::mem::size_of::<u32>() - std::mem::size_of::<u64>()) as u64,
        );
        set16le(&mut zip64_eoc[12..], ZIP_VERSION);
        set16le(&mut zip64_eoc[14..], ZIP_VERSION);
        set64le(&mut zip64_eoc[24..], self.entries.len() as u64);
        set64le(&mut zip64_eoc[32..], self.entries.len() as u64);
        set64le(&mut zip64_eoc[40..], central_size);
        set64le(&mut zip64_eoc[48..], central_offset);
        self.file.write_all(&zip64_eoc)?;
        self.total += ZIP64_EOC_DIR_SIZE as u64;

        let mut locator = [0u8; ZIP64_EOC_DIR_LOCATOR_SIZE];
        locator[..4].copy_from_slice(b"PK\x06\x07");
        set64le(&mut locator[8..], end_of_central_offset);
        set32le(&mut locator[16..], 1);
        self.file.write_all(&locator)?;
        self.total += ZIP64_EOC_DIR_LOCATOR_SIZE as u64;

        let mut eoc = [0u8; ZIP_EOC_DIR_SIZE];
        eoc[..4].copy_from_slice(b"PK\x05\x06");
        set16le(
            &mut eoc[8..],
            u16::try_from(self.entries.len().min(usize::from(u16::MAX))).expect("saturado"),
        );
        set16le(
            &mut eoc[10..],
            u16::try_from(self.entries.len().min(usize::from(u16::MAX))).expect("saturado"),
        );
        set32le(
            &mut eoc[12..],
            u32::try_from(central_size.min(u64::from(u32::MAX))).expect("saturado"),
        );
        set32le(
            &mut eoc[16..],
            u32::try_from(central_offset.min(u64::from(u32::MAX))).expect("saturado"),
        );
        self.file.write_all(&eoc)?;

        self.file.close()
    }

    /// Escribe en un offset absoluto de la entrada actual (solo entradas sin comprimir).
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si la entrada está comprimida.
    pub fn write_file_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let index = self.current.ok_or(Error::Corrupt("no current zip entry"))?;
        if self.entries[index].compress {
            return Err(Error::Corrupt(
                "cannot write at specific offset for compressed files",
            ));
        }
        self.file
            .write_all_at(self.entries[index].offset + offset, data)?;
        self.entries[index].size += data.len() as u64;
        self.entries[index].compressed += data.len() as u64;
        Ok(())
    }

    /// Recoloca la posición secuencial relativa al inicio de la entrada actual.
    pub fn set_offset(&mut self, offset: u64) {
        if let Some(index) = self.current {
            let position = self.entries[index].offset + offset;
            self.file.set_position(position);
            self.total = position;
        }
    }

    /// CRC-32 actual de la entrada en curso.
    #[must_use]
    pub fn crc32(&self) -> u32 {
        self.crc.current()
    }

    /// Fija el CRC-32 de la entrada actual (usado por el CSO).
    pub fn set_crc32(&mut self, crc: u32) {
        self.crc_override = Some(crc);
    }

    /// Número de entradas escritas.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn deflate(&mut self, input: &[u8], flush: FlushCompress) -> Result<()> {
        let index = self.current.ok_or(Error::Corrupt("no current zip entry"))?;
        let mut buffer = [0u8; 16 * 1024];
        let mut consumed = 0usize;

        loop {
            let Some(compressor) = self.compressor.as_mut() else {
                return Err(Error::Corrupt("missing deflate state"));
            };
            let before_in = compressor.total_in();
            let before_out = compressor.total_out();
            let status = compressor
                .compress(&input[consumed..], &mut buffer, flush)
                .map_err(|_| Error::Corrupt("failed to compress zip entry"))?;
            let used = usize::try_from(compressor.total_in() - before_in).expect("cabe en usize");
            let produced =
                usize::try_from(compressor.total_out() - before_out).expect("cabe en usize");
            consumed += used;

            if produced != 0 {
                self.file.write_all(&buffer[..produced])?;
                self.entries[index].compressed += produced as u64;
                self.total += produced as u64;
            }

            if status == Status::StreamEnd {
                return Ok(());
            }
            if consumed == input.len() && produced == 0 {
                if flush == FlushCompress::Finish {
                    return Err(Error::Corrupt("deflate stream did not finish"));
                }
                return Ok(());
            }
        }
    }
}

fn set16le(bytes: &mut [u8], value: u16) {
    bytes[..2].copy_from_slice(&value.to_le_bytes());
}

fn set32le(bytes: &mut [u8], value: u32) {
    bytes[..4].copy_from_slice(&value.to_le_bytes());
}

fn set64le(bytes: &mut [u8], value: u64) {
    bytes[..8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
fn get16le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes[..2].try_into().expect("2 bytes"))
}

/// Fecha/hora DOS en UTC (el C usa hora local; solo cambia el timestamp del zip).
fn dos_datetime(now: SystemTime) -> (u16, u16) {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = i64::try_from(seconds / 86_400).expect("días caben en i64");
    let seconds_of_day = seconds % 86_400;

    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    let year = year.clamp(1980, 2107);
    let date = u16::try_from(((year - 1980) << 9) | (i64::from(month) << 5) | i64::from(day))
        .expect("fecha DOS");
    let time = u16::try_from((hour << 11) | (minute << 5) | (second / 2)).expect("hora DOS");
    (date, time)
}

/// Convierte días desde 1970-01-01 a `(año, mes, día)` en UTC.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * mp + 2) / 5 + 1).expect("día válido");
    let month = u32::try_from(mp + if mp < 10 { 3 } else { -9 }).expect("mes válido");
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn read_local_crc(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(data[offset + 14..offset + 18].try_into().expect("crc"))
    }

    #[test]
    fn writes_folder_and_file_and_central_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.zip");

        let mut zip = ZipWriter::create(&path).expect("crear");
        zip.add_folder("app/PCSE00000").expect("carpeta");
        zip.begin_file("app/PCSE00000/hello.bin", false)
            .expect("archivo");
        let content = b"contenido de prueba";
        zip.write_file(content).expect("write");
        zip.end_file().expect("end");
        zip.close().expect("close");

        let data = std::fs::read(&path).expect("leer zip");
        assert_eq!(&data[..4], b"PK\x03\x04");
        assert_eq!(read_local_crc(&data, 0), 0, "la carpeta no tiene CRC");
        let file_offset = ZIP_LOCAL_HEADER_SIZE + "app/PCSE00000/".len();
        assert_eq!(
            read_local_crc(&data, file_offset),
            crate::checksum::crc32(content)
        );

        // EOCD al final: 22 bytes.
        let eocd = data.len() - ZIP_EOC_DIR_SIZE;
        assert_eq!(&data[eocd..eocd + 4], b"PK\x05\x06");
        let entries = get16le(&data[eocd + 10..]);
        assert_eq!(entries, 2);

        let central_offset = u32::from_le_bytes(
            data[eocd + 16..eocd + 20]
                .try_into()
                .expect("offset central"),
        ) as usize;
        assert_eq!(&data[central_offset..central_offset + 4], b"PK\x01\x02");
        let name_len = usize::from(get16le(&data[central_offset + 28..]));
        assert_eq!(
            &data[central_offset + 46..central_offset + 46 + name_len],
            b"app/PCSE00000/"
        );
    }

    #[test]
    fn compresses_entries_when_requested() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("deflate.zip");

        let content = b"pkg2zip ".repeat(200);
        let mut zip = ZipWriter::create(&path).expect("crear");
        zip.begin_file("data.bin", true).expect("archivo");
        zip.write_file(&content).expect("write");
        zip.end_file().expect("end");
        zip.close().expect("close");

        let data = std::fs::read(&path).expect("leer");
        let method = u16::from_le_bytes(data[8..10].try_into().expect("método"));
        assert_eq!(method, ZIP_METHOD_DEFLATE);
        let compressed_size = u32::from_le_bytes(data[18..22].try_into().expect("tamaño")) as usize;
        let name_len = usize::from(get16le(&data[26..]));
        let start = ZIP_LOCAL_HEADER_SIZE + name_len;
        let mut decoder = flate2::read::DeflateDecoder::new(&data[start..start + compressed_size]);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).expect("inflate");
        assert_eq!(out, content);
    }

    #[test]
    fn write_at_requires_stored_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("x.zip");
        let mut zip = ZipWriter::create(&path).expect("crear");
        zip.begin_file("a.bin", true).expect("archivo");
        assert!(zip.write_file_at(0, b"x").is_err());
    }

    #[test]
    fn dos_datetime_is_sane() {
        let (date, time) = dos_datetime(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        let year = ((date >> 9) & 0x7f) + 1980;
        let month = (date >> 5) & 0x0f;
        let day = date & 0x1f;
        assert!((1980..=2107).contains(&year));
        assert!((1..=12).contains(&month));
        assert!((1..=31).contains(&day));
        assert_eq!(time >> 11, 22, "2023-11-14T22:13:20Z");
        assert!(crate::checksum::crc32(b"abc") != 0);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
