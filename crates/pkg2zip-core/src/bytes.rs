//! Lectura y escritura de enteros en little/big endian.
//!
//! Espejo de los helpers de `legacy/pkg2zip_utils.h`. Todas las funciones asumen que el slice tiene
//! la longitud suficiente; los parsers validan longitudes antes de llamarlas.

/// Lee un `u16` little endian.
#[inline]
#[must_use]
pub fn get16le(b: &[u8]) -> u16 {
    u16::from_le_bytes(b[..2].try_into().expect("se requieren 2 bytes"))
}

/// Lee un `u32` little endian.
#[inline]
#[must_use]
pub fn get32le(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().expect("se requieren 4 bytes"))
}

/// Lee un `u64` little endian.
#[inline]
#[must_use]
pub fn get64le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().expect("se requieren 8 bytes"))
}

/// Lee un `u16` big endian.
#[inline]
#[must_use]
pub fn get16be(b: &[u8]) -> u16 {
    u16::from_be_bytes(b[..2].try_into().expect("se requieren 2 bytes"))
}

/// Lee un `u32` big endian.
#[inline]
#[must_use]
pub fn get32be(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().expect("se requieren 4 bytes"))
}

/// Lee un `u64` big endian.
#[inline]
#[must_use]
pub fn get64be(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().expect("se requieren 8 bytes"))
}

/// Escribe un `u16` little endian.
#[inline]
pub fn set16le(b: &mut [u8], v: u16) {
    b[..2].copy_from_slice(&v.to_le_bytes());
}

/// Escribe un `u32` little endian.
#[inline]
pub fn set32le(b: &mut [u8], v: u32) {
    b[..4].copy_from_slice(&v.to_le_bytes());
}

/// Escribe un `u64` little endian.
#[inline]
pub fn set64le(b: &mut [u8], v: u64) {
    b[..8].copy_from_slice(&v.to_le_bytes());
}

/// Escribe un `u16` big endian.
#[inline]
pub fn set16be(b: &mut [u8], v: u16) {
    b[..2].copy_from_slice(&v.to_be_bytes());
}

/// Escribe un `u32` big endian.
#[inline]
pub fn set32be(b: &mut [u8], v: u32) {
    b[..4].copy_from_slice(&v.to_be_bytes());
}

/// Escribe un `u64` big endian.
#[inline]
pub fn set64be(b: &mut [u8], v: u64) {
    b[..8].copy_from_slice(&v.to_be_bytes());
}

/// Alinea `n` al siguiente múltiplo de `align` (potencia de dos).
#[inline]
#[must_use]
pub const fn align_up(n: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (n + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_little_endian() {
        let mut b = [0u8; 8];
        set16le(&mut b, 0x1234);
        assert_eq!(&b[..2], &[0x34, 0x12]);
        assert_eq!(get16le(&b), 0x1234);

        set32le(&mut b, 0x89ab_cdef);
        assert_eq!(&b[..4], &[0xef, 0xcd, 0xab, 0x89]);
        assert_eq!(get32le(&b), 0x89ab_cdef);

        set64le(&mut b, 0x0123_4567_89ab_cdef);
        assert_eq!(get64le(&b), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn roundtrip_big_endian() {
        let mut b = [0u8; 8];
        set16be(&mut b, 0x7f50);
        assert_eq!(&b[..2], &[0x7f, 0x50]);
        assert_eq!(get16be(&b), 0x7f50);

        set32be(&mut b, 0x7f50_4b47);
        assert_eq!(&b[..4], &[0x7f, 0x50, 0x4b, 0x47]);
        assert_eq!(get32be(&b), 0x7f50_4b47);
        assert_eq!(get32be(b"PKG\x7f"), 0x504b_477f);

        set64be(&mut b, 0x0123_4567_89ab_cdef);
        assert_eq!(get64be(&b), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn align_up_works() {
        assert_eq!(align_up(0, 16), 0);
        assert_eq!(align_up(1, 16), 16);
        assert_eq!(align_up(16, 16), 16);
        assert_eq!(align_up(17, 16), 32);
        assert_eq!(align_up(0x130, 16), 0x130);
    }
}
