//! Adler-32, usado para validar el contenido de un zRIF (RFC 1950).

/// Módulo primo de Adler-32.
pub const ADLER32_MOD: u32 = 65_521;

/// Mayor bloque que evita desbordar `u32` en el acumulador, como en zlib.
const NMAX: usize = 5552;

/// Calcula el Adler-32 de un buffer (estado inicial `1`).
#[must_use]
pub fn adler32(data: &[u8]) -> u32 {
    adler32_update(1, data)
}

/// Actualiza un Adler-32 con más datos.
#[must_use]
pub fn adler32_update(state: u32, data: &[u8]) -> u32 {
    let mut a = state & 0xffff;
    let mut b = (state >> 16) & 0xffff;

    for chunk in data.chunks(NMAX) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= ADLER32_MOD;
        b %= ADLER32_MOD;
    }

    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
    }

    #[test]
    fn incremental_matches_one_shot() {
        let data = b"zRIF: adler32 sobre datos troceados";
        let whole = adler32(data);
        let mut state = 1;
        for chunk in data.chunks(5) {
            state = adler32_update(state, chunk);
        }
        assert_eq!(state, whole);
    }
}
