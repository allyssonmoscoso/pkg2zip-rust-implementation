//! Acceso a archivos con IO posicional (`pread`/`pwrite`), como `legacy/pkg2zip_sys.c`.

use std::fs::{File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Archivo de entrada abierto con su tamaño.
#[derive(Debug)]
pub struct DataFile {
    file: File,
    len: u64,
    path: PathBuf,
}

impl DataFile {
    /// Abre un archivo y obtiene su tamaño.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] con el mensaje `cannot open '<ruta>'`.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .map_err(|e| Error::io(format!("cannot open '{}'", path.display()), e))?;
        let len = file
            .metadata()
            .map_err(|e| Error::io(format!("cannot get size of '{}'", path.display()), e))?
            .len();
        Ok(Self {
            file,
            len,
            path: path.to_owned(),
        })
    }

    /// Tamaño del archivo en bytes.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.len
    }

    /// ¿Está vacío?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Lee exactamente `buf.len()` bytes en `offset`.
    ///
    /// # Errors
    ///
    /// [`Error::TooShort`] si el rango se sale del archivo, o [`Error::Io`] si falla la lectura.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(Error::TooShort("file"))?;
        if end > self.len {
            return Err(Error::TooShort("file"));
        }
        read_exact_at(&self.file, buf, offset)
            .map_err(|e| Error::io(format!("cannot read '{}'", self.path.display()), e))
    }

    /// Lee un bloque nuevo de `size` bytes.
    ///
    /// # Errors
    ///
    /// Igual que [`Self::read_at`], más [`Error::TooShort`] si `size` no cabe en `usize`.
    pub fn read_vec(&self, offset: u64, size: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; size];
        self.read_at(offset, &mut buf)?;
        Ok(buf)
    }
}

/// Archivo de salida con escritura secuencial y posicional.
#[derive(Debug)]
pub struct OutputFile {
    file: File,
    position: u64,
}

impl OutputFile {
    /// Crea (o trunca) un archivo.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] con el mensaje `cannot create '<ruta>'`.
    pub fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| Error::io(format!("cannot create '{}'", path.display()), e))?;
        Ok(Self { file, position: 0 })
    }

    /// Escribe de forma secuencial, avanzando la posición.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si falla la escritura.
    pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
        write_all_at(&self.file, data, self.position)
            .map_err(|e| Error::io("cannot write output file", e))?;
        self.position += data.len() as u64;
        Ok(())
    }

    /// Escribe en un offset absoluto sin mover la posición secuencial.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si falla la escritura.
    pub fn write_all_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        write_all_at(&self.file, data, offset)
            .map_err(|e| Error::io("cannot write output file", e))?;
        Ok(())
    }

    /// Posición secuencial actual.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Fija la posición secuencial (equivalente a `out_set_offset`).
    pub fn set_position(&mut self, position: u64) {
        self.position = position;
    }

    /// Sincroniza y cierra el archivo.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] si falla el `flush`.
    pub fn close(mut self) -> Result<()> {
        self.file
            .flush()
            .map_err(|e| Error::io("cannot close output file", e))
    }
}

/// Crea un directorio y sus padres (equivalente a `sys_mkdir`).
///
/// # Errors
///
/// [`Error::Io`] con el mensaje `cannot create '<ruta>' folder`.
pub fn create_dir_all(path: &str) -> Result<()> {
    std::fs::create_dir_all(path)
        .map_err(|e| Error::io(format!("cannot create '{path}' folder"), e))
}

/// Indica si `path` es un directorio (equivalente a `sys_test_dir`).
#[must_use]
pub fn is_dir(path: &str) -> bool {
    std::path::Path::new(path).is_dir()
}

#[cfg(unix)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !buf.is_empty() {
        match file.read_at(buf, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(read) => {
                buf = &mut buf[read..];
                offset += read as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(read) => {
                buf = &mut buf[read..];
                offset += read as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn write_all_at(file: &File, mut data: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !data.is_empty() {
        match file.write_at(data, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(written) => {
                data = &data[written..];
                offset += written as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn write_all_at(file: &File, mut data: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !data.is_empty() {
        match file.seek_write(data, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(written) => {
                data = &data[written..];
                offset += written as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Interfaz mínima de salida para el flujo de desempaquetado.
///
/// La implementa la CLI (que decide si hay TTY, `-q`, etc.). El núcleo solo emite mensajes; nunca
/// escribe directamente en stdout.
pub trait Ui {
    /// Mensaje informativo (`[*] ...`).
    fn info(&mut self, message: &str);
    /// Progreso absoluto en bytes (se ignora si no hay TTY).
    fn progress(&mut self, position: u64);
    /// Inicializa el total para calcular el porcentaje.
    fn progress_init(&mut self, total: u64) {
        let _ = total;
    }
}

/// `Ui` que descarta todo (tests).
#[derive(Debug, Default)]
pub struct SilentUi;

impl Ui for SilentUi {
    fn info(&mut self, _message: &str) {}
    fn progress(&mut self, _position: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_write_at_offsets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data.bin");
        let mut out = OutputFile::create(&path).expect("crear");
        out.write_all(b"hola").expect("write");
        out.write_all_at(10, b"mundo").expect("write_at");
        out.close().expect("close");

        let data = DataFile::open(&path).expect("abrir");
        assert_eq!(data.len(), 15);
        let mut buf = [0u8; 4];
        data.read_at(0, &mut buf).expect("leer");
        assert_eq!(&buf, b"hola");
        data.read_at(10, &mut buf).expect("leer");
        assert_eq!(&buf, b"mund");

        let mut too_far = [0u8; 4];
        assert!(data.read_at(13, &mut too_far).is_err());
    }

    #[test]
    fn creates_directories_and_detects_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a/b/c");
        let nested_str = nested.to_string_lossy().into_owned();
        assert!(!is_dir(&nested_str));
        create_dir_all(&nested_str).expect("mkdir");
        assert!(is_dir(&nested_str));
    }
}
