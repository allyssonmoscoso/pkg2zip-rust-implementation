//! CRC-32 (IEEE) incremental y combinación de CRC.
//!
//! Equivalente a `crc32_init/update/done/combine` de `legacy/pkg2zip_crc32.c`, pero usando
//! `crc32fast` (PCLMULQDQ en x86_64, software en el resto).
//!
//! La combinación se usa en el CSO: el CRC de `cabecera || índice` se combina con el CRC de los
//! datos comprimidos, que se escribieron antes de conocer la cabecera.

use crc32fast::Hasher;

/// CRC-32 incremental.
#[derive(Debug, Clone)]
pub struct Crc32 {
    hasher: Hasher,
}

impl Crc32 {
    /// Inicia el cálculo.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }

    /// Añade datos.
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    /// Finaliza y devuelve el CRC.
    #[must_use]
    pub fn finish(self) -> u32 {
        self.hasher.finalize()
    }

    /// Reinicia el estado (equivalente a crear uno nuevo).
    pub fn reset(&mut self) {
        self.hasher.reset();
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

/// CRC-32 de un buffer completo.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Calcula `crc32(x || y)` a partir de `crc32(x)`, `crc32(y)` y la longitud de `y`.
///
/// Equivalente a `crc32_combine(a, b, blen)` del C.
#[must_use]
pub fn combine(a: u32, b: u32, len_b: u64) -> u32 {
    let mut first = Hasher::new_with_initial_len(a, 0);
    let second = Hasher::new_with_initial_len(b, len_b);
    first.combine(&second);
    first.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn incremental_matches_one_shot() {
        let data = b"pkg2zip: CRC incremental sobre varios trozos";
        let mut ctx = Crc32::new();
        for chunk in data.chunks(7) {
            ctx.update(chunk);
        }
        assert_eq!(ctx.finish(), crc32(data));
    }

    #[test]
    fn combine_matches_concatenation() {
        let x = b"hello";
        let y = b" world";
        let mut joined = x.to_vec();
        joined.extend_from_slice(y);

        let combined = combine(crc32(x), crc32(y), y.len() as u64);
        assert_eq!(combined, crc32(&joined));
    }

    #[test]
    fn combine_with_empty_suffix() {
        let x = b"abc";
        assert_eq!(combine(crc32(x), 0, 0), crc32(x));
    }

    #[test]
    fn reset_starts_over() {
        let mut ctx = Crc32::new();
        ctx.update(b"basura");
        ctx.reset();
        ctx.update(b"123456789");
        assert_eq!(ctx.finish(), 0xCBF4_3926);
    }
}
