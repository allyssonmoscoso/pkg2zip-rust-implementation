//! Fachada de salida: escribe un ZIP o archivos sueltos según `-x`.
//!
//! Réplica de `legacy/pkg2zip_out.c`. Las rutas usan `/` como separador (igual que el C) y son
//! relativas al directorio de trabajo.

use std::path::Path;

use crate::error::{Error, Result};
use crate::plat::file::{self, OutputFile};

use super::zip::ZipWriter;

/// Destino de la salida.
#[derive(Debug)]
pub enum Sink {
    /// Todo dentro de un ZIP.
    Zip(ZipWriter),
    /// Archivos sueltos en el sistema de archivos.
    Fs(FsSink),
}

/// Estado de la salida a archivos sueltos.
#[derive(Debug, Default)]
pub struct FsSink {
    file: Option<OutputFile>,
    position: u64,
}

impl Sink {
    /// Prepara la salida: crea el zip o deja el modo carpeta listo.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si no se puede crear el zip.
    pub fn create(name: &str, zipped: bool) -> Result<Self> {
        if zipped {
            Ok(Self::Zip(ZipWriter::create(Path::new(name))?))
        } else {
            Ok(Self::Fs(FsSink::default()))
        }
    }

    /// Añade una carpeta (entrada de zip o `mkdir` recursivo).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] o [`Error::Corrupt`] si la ruta no es relativa y segura.
    pub fn add_folder(&mut self, path: &str) -> Result<()> {
        match self {
            Self::Zip(zip) => zip.add_folder(path),
            Self::Fs(_) => {
                safe_relative(path)?;
                file::create_dir_all(path)
            }
        }
    }

    /// Añade la carpeta padre de un archivo.
    ///
    /// # Errors
    ///
    /// Igual que [`Self::add_folder`].
    pub fn add_parent(&mut self, path: &str) -> Result<()> {
        match path.rfind('/') {
            Some(index) => self.add_folder(&path[..index]),
            None => Ok(()),
        }
    }

    /// Empieza un archivo; devuelve el tamaño de la cabecera (relevante para el CSO).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] o [`Error::Corrupt`].
    pub fn begin_file(&mut self, path: &str, compress: bool) -> Result<u64> {
        match self {
            Self::Zip(zip) => zip.begin_file(path, compress),
            Self::Fs(sink) => {
                safe_relative(path)?;
                sink.file = Some(OutputFile::create(Path::new(path))?);
                sink.position = 0;
                Ok(0)
            }
        }
    }

    /// Escribe datos del archivo actual.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] o [`Error::Corrupt`].
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        match self {
            Self::Zip(zip) => zip.write_file(data),
            Self::Fs(sink) => {
                let file = sink
                    .file
                    .as_mut()
                    .ok_or(Error::Corrupt("no current output file"))?;
                file.write_all(data)?;
                sink.position = file.position();
                Ok(())
            }
        }
    }

    /// Escribe en un offset absoluto del archivo actual.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] o [`Error::Corrupt`].
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        match self {
            Self::Zip(zip) => zip.write_file_at(offset, data),
            Self::Fs(sink) => {
                let file = sink
                    .file
                    .as_mut()
                    .ok_or(Error::Corrupt("no current output file"))?;
                file.write_all_at(offset, data)
            }
        }
    }

    /// Fija la posición secuencial del archivo actual.
    ///
    /// # Errors
    ///
    /// [`Error::Corrupt`] si no hay archivo actual.
    pub fn set_offset(&mut self, offset: u64) -> Result<()> {
        match self {
            Self::Zip(zip) => {
                zip.set_offset(offset);
                Ok(())
            }
            Self::Fs(sink) => {
                let file = sink
                    .file
                    .as_mut()
                    .ok_or(Error::Corrupt("no current output file"))?;
                file.set_position(offset);
                sink.position = offset;
                Ok(())
            }
        }
    }

    /// Termina el archivo actual.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] o [`Error::Corrupt`].
    pub fn end_file(&mut self) -> Result<()> {
        match self {
            Self::Zip(zip) => zip.end_file(),
            Self::Fs(sink) => {
                let file = sink
                    .file
                    .take()
                    .ok_or(Error::Corrupt("no current output file"))?;
                file.close()
            }
        }
    }

    /// Cierra la salida (directorio central del zip).
    ///
    /// # Errors
    ///
    /// [`Error::Io`].
    pub fn finish(self) -> Result<()> {
        match self {
            Self::Zip(zip) => zip.close(),
            Self::Fs(_) => Ok(()),
        }
    }

    /// CRC-32 actual de la entrada zip en curso (0 si no es zip).
    #[must_use]
    pub fn zip_crc32(&self) -> u32 {
        match self {
            Self::Zip(zip) => zip.crc32(),
            Self::Fs(_) => 0,
        }
    }

    /// Fija el CRC-32 de la entrada zip en curso.
    pub fn set_zip_crc32(&mut self, crc: u32) {
        if let Self::Zip(zip) = self {
            zip.set_crc32(crc);
        }
    }
}

/// Rechaza rutas absolutas o con `..` (mejora de seguridad sobre el C).
fn safe_relative(path: &str) -> Result<()> {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return Err(Error::Corrupt("unsafe path in pkg"));
    }
    if path.split(['/', '\\']).any(|part| part == "..") {
        return Err(Error::Corrupt("unsafe path in pkg"));
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(Error::Corrupt("unsafe path in pkg"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_paths() {
        assert!(safe_relative("../evil").is_err());
        assert!(safe_relative("/abs").is_err());
        assert!(safe_relative("C:/x").is_err());
        assert!(safe_relative("app/PCSE00000/eboot.bin").is_ok());
    }

    #[test]
    fn fs_sink_writes_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");

        let mut sink = Sink::create("out", false).expect("sink");
        sink.add_folder("app/ID").expect("carpeta");
        sink.begin_file("app/ID/a.bin", false).expect("archivo");
        sink.write(b"hola").expect("write");
        sink.end_file().expect("end");
        sink.finish().expect("finish");

        let written = std::fs::read(dir.path().join("app/ID/a.bin")).expect("leer");
        assert_eq!(written, b"hola");

        std::env::set_current_dir(cwd).expect("restaurar cwd");
    }
}
