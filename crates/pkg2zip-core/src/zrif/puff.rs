//! Port a Rust de `puff.c` (Mark Adler, dominio público), con el parche de diccionario que usa
//! `pkg2zip` para los zRIF.
//!
//! `puff` es un inflate mínimo que decodifica un flujo deflate crudo y escribe en un buffer que
//! empieza con el diccionario preestablecido. El resultado es `buffer[dict.len()..]`.
//!
//! Decisiones del port:
//!
//! * Nada de `unsafe`: los buffers son `Vec<u8>` y todos los índices se comprueban.
//! * El buffer de salida **es** la ventana: las distancias se resuelven contra `out`, que ya
//!   contiene el diccionario. Por eso una distancia puede apuntar dentro del diccionario.
//! * Los errores reproducen las condiciones del C (`puff` devuelve códigos; aquí son variantes).

use std::fmt;

const MAX_BITS: usize = 15;
const MAX_LCODES: usize = 286;
const MAX_DCODES: usize = 30;
const MAX_CODES: usize = MAX_LCODES + MAX_DCODES;
const FIX_LCODES: usize = 288;

/// Error de descompresión deflate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuffError {
    /// La entrada se agotó antes de terminar el flujo.
    TruncatedInput,
    /// La salida no cabe en la capacidad indicada.
    OutputFull,
    /// El flujo deflate es inválido.
    Invalid(&'static str),
}

impl fmt::Display for PuffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedInput => f.write_str("deflate stream truncated"),
            Self::OutputFull => f.write_str("deflate output does not fit"),
            Self::Invalid(msg) => write!(f, "invalid deflate stream: {msg}"),
        }
    }
}

impl std::error::Error for PuffError {}

/// Descomprime un flujo deflate crudo usando `dict` como ventana inicial.
///
/// Devuelve **solo** los bytes nuevos (sin el diccionario).
///
/// # Errores
///
/// Devuelve [`PuffError`] si el flujo está truncado, es inválido o no cabe en `capacity`.
pub fn inflate_with_dict(dict: &[u8], input: &[u8], capacity: usize) -> Result<Vec<u8>, PuffError> {
    if dict.len() > capacity {
        return Err(PuffError::OutputFull);
    }

    let mut state = Inflate {
        input,
        pos: 0,
        bit_buf: 0,
        bit_cnt: 0,
        out: dict.to_vec(),
        capacity,
    };

    loop {
        let last = state.bits(1)? == 1;
        let block_type = state.bits(2)?;
        match block_type {
            0 => state.stored()?,
            1 => state.fixed()?,
            2 => state.dynamic()?,
            _ => return Err(PuffError::Invalid("block type 3")),
        }
        if last {
            break;
        }
    }

    let dict_len = dict.len();
    Ok(state.out.split_off(dict_len))
}

struct Inflate<'a> {
    input: &'a [u8],
    pos: usize,
    bit_buf: u32,
    bit_cnt: u32,
    out: Vec<u8>,
    capacity: usize,
}

impl Inflate<'_> {
    fn bits(&mut self, need: u32) -> Result<u32, PuffError> {
        while self.bit_cnt < need {
            if self.pos == self.input.len() {
                return Err(PuffError::TruncatedInput);
            }
            self.bit_buf |= u32::from(self.input[self.pos]) << self.bit_cnt;
            self.pos += 1;
            self.bit_cnt += 8;
        }

        let mask = if need == 0 { 0 } else { (1u32 << need) - 1 };
        let value = self.bit_buf & mask;
        self.bit_buf >>= need;
        self.bit_cnt -= need;
        Ok(value)
    }

    fn ensure(&mut self, extra: usize) -> Result<(), PuffError> {
        if self.out.len() + extra > self.capacity {
            return Err(PuffError::OutputFull);
        }
        Ok(())
    }

    fn push(&mut self, byte: u8) -> Result<(), PuffError> {
        self.ensure(1)?;
        self.out.push(byte);
        Ok(())
    }

    fn stored(&mut self) -> Result<(), PuffError> {
        self.bit_buf = 0;
        self.bit_cnt = 0;

        if self.pos + 4 > self.input.len() {
            return Err(PuffError::TruncatedInput);
        }
        let raw = self.input[self.pos..self.pos + 4]
            .try_into()
            .expect("4 bytes");
        self.pos += 4;
        let [lo, hi, nlo, nhi] = raw;
        let len = usize::from(u16::from_le_bytes([lo, hi]));
        let len_u16 = u16::try_from(len).expect("len es un u16");
        let nlen = u16::from_le_bytes([nlo, nhi]);
        if nlen != !len_u16 {
            return Err(PuffError::Invalid("stored length complement"));
        }

        if self.pos + len > self.input.len() {
            return Err(PuffError::TruncatedInput);
        }
        self.ensure(len)?;
        self.out
            .extend_from_slice(&self.input[self.pos..self.pos + len]);
        self.pos += len;
        Ok(())
    }

    fn decode(&mut self, h: &Huffman) -> Result<u16, PuffError> {
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: usize = 0;

        for len in 1..=MAX_BITS {
            code = (code << 1) | self.bits(1)?;
            let count = u32::from(h.counts[len]);
            if code < first + count {
                let position = index + (code - first) as usize;
                return h
                    .symbols
                    .get(position)
                    .copied()
                    .ok_or(PuffError::Invalid("invalid Huffman symbol"));
            }
            index += count as usize;
            first = (first + count) << 1;
        }

        Err(PuffError::Invalid("ran out of codes"))
    }

    fn codes(&mut self, lencode: &Huffman, distcode: &Huffman) -> Result<(), PuffError> {
        loop {
            let symbol = self.decode(lencode)?;
            if symbol < 256 {
                self.push(symbol as u8)?;
            } else if symbol > 256 {
                let index = usize::from(symbol - 257);
                if index >= LENS.len() {
                    return Err(PuffError::Invalid("invalid length code"));
                }
                let length = usize::from(LENS[index]) + self.bits(u32::from(LEXT[index]))? as usize;

                let dist_symbol = usize::from(self.decode(distcode)?);
                if dist_symbol >= DISTS.len() {
                    return Err(PuffError::Invalid("invalid distance code"));
                }
                let distance = usize::from(DISTS[dist_symbol])
                    + self.bits(u32::from(DEXT[dist_symbol]))? as usize;

                if distance > self.out.len() {
                    return Err(PuffError::Invalid("distance too far back"));
                }

                self.ensure(length)?;
                let start = self.out.len() - distance;
                for offset in 0..length {
                    let byte = self.out[start + offset];
                    self.out.push(byte);
                }
            } else {
                return Ok(());
            }
        }
    }

    fn fixed(&mut self) -> Result<(), PuffError> {
        let (lencode, distcode) = fixed_tables();
        self.codes(lencode, distcode)
    }

    fn dynamic(&mut self) -> Result<(), PuffError> {
        let nlen = self.bits(5)? as usize + 257;
        let ndist = self.bits(5)? as usize + 1;
        let ncode = self.bits(4)? as usize + 4;
        if nlen > MAX_LCODES || ndist > MAX_DCODES {
            return Err(PuffError::Invalid("bad code counts"));
        }

        let mut lengths = vec![0u16; MAX_CODES];
        for &slot in ORDER.iter().take(ncode) {
            lengths[usize::from(slot)] = self.bits(3)? as u16;
        }

        let (code_lengths, left) = Huffman::construct(&lengths[..19], 19)?;
        if left != 0 {
            return Err(PuffError::Invalid("incomplete code-length code"));
        }

        let mut index = 0;
        while index < nlen + ndist {
            let symbol = self.decode(&code_lengths)?;
            if symbol < 16 {
                lengths[index] = symbol;
                index += 1;
            } else {
                let (length, repeat) = match symbol {
                    16 => {
                        if index == 0 {
                            return Err(PuffError::Invalid("repeat with no last length"));
                        }
                        (lengths[index - 1], 3 + self.bits(2)? as usize)
                    }
                    17 => (0, 3 + self.bits(3)? as usize),
                    _ => (0, 11 + self.bits(7)? as usize),
                };
                if index + repeat > nlen + ndist {
                    return Err(PuffError::Invalid("too many code lengths"));
                }
                for _ in 0..repeat {
                    lengths[index] = length;
                    index += 1;
                }
            }
        }

        if lengths[256] == 0 {
            return Err(PuffError::Invalid("missing end-of-block code"));
        }

        let (lencode, left) = Huffman::construct(&lengths[..nlen], nlen)?;
        if left != 0 && nlen != usize::from(lencode.counts[0] + lencode.counts[1]) {
            return Err(PuffError::Invalid("incomplete literal/length code"));
        }

        let (distcode, left) = Huffman::construct(&lengths[nlen..nlen + ndist], ndist)?;
        if left != 0 && ndist != usize::from(distcode.counts[0] + distcode.counts[1]) {
            return Err(PuffError::Invalid("incomplete distance code"));
        }

        self.codes(&lencode, &distcode)
    }
}

#[derive(Debug)]
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    /// Construye la tabla canónica. Devuelve `left`: 0 completa, >0 incompleta.
    fn construct(lengths: &[u16], n: usize) -> Result<(Self, i32), PuffError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in &lengths[..n] {
            let length = usize::from(length);
            if length > MAX_BITS {
                return Err(PuffError::Invalid("code length above 15"));
            }
            counts[length] += 1;
        }

        if counts[0] as usize == n {
            return Ok((
                Self {
                    counts,
                    symbols: Vec::new(),
                },
                0,
            ));
        }

        let mut left: i32 = 1;
        for &count in counts.iter().skip(1) {
            left <<= 1;
            left -= i32::from(count);
            if left < 0 {
                return Err(PuffError::Invalid("over-subscribed Huffman code"));
            }
        }

        let mut offsets = [0usize; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offsets[len + 1] = offsets[len] + usize::from(counts[len]);
        }

        let mut symbols = vec![0u16; n];
        for (symbol, &length) in lengths[..n].iter().enumerate() {
            let length = usize::from(length);
            if length == 0 {
                continue;
            }
            let slot = offsets[length];
            if slot >= n {
                return Err(PuffError::Invalid("symbol table overflow"));
            }
            symbols[slot] = u16::try_from(symbol).expect("símbolo cabe en u16");
            offsets[length] += 1;
        }

        Ok((Self { counts, symbols }, left))
    }
}

fn fixed_tables() -> &'static (Huffman, Huffman) {
    use std::sync::OnceLock;

    static TABLES: OnceLock<(Huffman, Huffman)> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut lengths = [0u16; FIX_LCODES];
        for (symbol, slot) in lengths.iter_mut().enumerate() {
            *slot = match symbol {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        let (lencode, _) = Huffman::construct(&lengths, FIX_LCODES).expect("tabla fija válida");

        let dist_lengths = [5u16; MAX_DCODES];
        let (distcode, _) =
            Huffman::construct(&dist_lengths, MAX_DCODES).expect("tabla de distancias válida");

        (lencode, distcode)
    })
}

/// Bases de longitud para los símbolos 257..285.
const LENS: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Bits extra de longitud para los símbolos 257..285.
const LEXT: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Bases de distancia para los símbolos 0..29.
const DISTS: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Bits extra de distancia para los símbolos 0..29.
const DEXT: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Permutación de longitudes de código del bloque dinámico.
const ORDER: [u8; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn miniz(data: &[u8], level: u8) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec(data, level)
    }

    #[test]
    fn inflates_dynamic_and_fixed_blocks() {
        // Datos variados: fuerzan bloques dinámicos con matches y literales.
        let mut data = Vec::new();
        for i in 0..30_000u32 {
            data.push((i.wrapping_mul(31) % 251) as u8);
            if i % 97 == 0 {
                data.extend_from_slice(b"pkg2zip-puff-port-");
            }
        }

        for level in [0u8, 1, 6, 10] {
            let compressed = miniz(&data, level);
            let out = inflate_with_dict(&[], &compressed, data.len()).expect("inflate");
            assert_eq!(out, data, "nivel {level}");
        }
    }

    #[test]
    fn respects_output_capacity() {
        let data = vec![0x55u8; 4096];
        let compressed = miniz(&data, 6);
        let err = inflate_with_dict(&[], &compressed, 100).expect_err("no cabe");
        assert_eq!(err, PuffError::OutputFull);
    }

    #[test]
    fn truncated_input_is_reported() {
        let data = vec![0x42u8; 1024];
        let compressed = miniz(&data, 6);
        let err = inflate_with_dict(&[], &compressed[..compressed.len() - 4], 4096)
            .expect_err("truncado");
        assert_eq!(err, PuffError::TruncatedInput);
    }

    /// Escribe bits en el orden del flujo deflate (LSB primero para valores, MSB primero para
    /// códigos Huffman).
    struct BitWriter {
        bytes: Vec<u8>,
        bit: u32,
        bits: u32,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit: 0,
                bits: 0,
            }
        }
        fn push_bit(&mut self, value: u32) {
            self.bit |= (value & 1) << self.bits;
            self.bits += 1;
            if self.bits == 8 {
                self.bytes.push(self.bit as u8);
                self.bit = 0;
                self.bits = 0;
            }
        }
        fn push_lsb(&mut self, value: u32, count: u32) {
            for i in 0..count {
                self.push_bit((value >> i) & 1);
            }
        }
        fn push_code(&mut self, code: u32, count: u32) {
            for i in (0..count).rev() {
                self.push_bit((code >> i) & 1);
            }
        }
        fn finish(mut self) -> Vec<u8> {
            if self.bits > 0 {
                self.bytes.push(self.bit as u8);
            }
            self.bytes
        }
    }

    #[test]
    fn match_into_preset_dictionary() {
        // Bloque fijo: match de longitud 3 a distancia 3 (los 3 últimos bytes del diccionario),
        // luego el literal 'A', luego fin de bloque.
        let mut w = BitWriter::new();
        w.push_lsb(1, 1); // BFINAL
        w.push_lsb(1, 2); // BTYPE = 01 (fijo)
        w.push_code(1, 7); // símbolo 257 (longitud 3)
        w.push_code(2, 5); // símbolo de distancia 2 (distancia 3)
        w.push_code(0x30 + 65, 8); // literal 'A'
        w.push_code(0, 7); // fin de bloque
        let stream = w.finish();

        let dict = vec![0xAAu8; 1024];
        let out = inflate_with_dict(&dict, &stream, dict.len() + 4).expect("inflate");
        assert_eq!(out, vec![0xAA, 0xAA, 0xAA, b'A']);
    }

    #[test]
    fn distance_beyond_dictionary_is_rejected() {
        // Match de longitud 3 a distancia 1025 sobre un diccionario de 1024 bytes.
        let mut w = BitWriter::new();
        w.push_lsb(1, 1);
        w.push_lsb(1, 2);
        w.push_code(1, 7); // longitud 3
        w.push_code(20, 5); // distancia base 1025, 9 bits extra
        w.push_lsb(0, 9); // 1025
        w.push_code(0, 7); // fin de bloque
        let stream = w.finish();

        let dict = vec![0u8; 1024];
        let err = inflate_with_dict(&dict, &stream, 2048).expect_err("distancia inválida");
        assert_eq!(err, PuffError::Invalid("distance too far back"));
    }

    #[test]
    fn stored_block_roundtrip() {
        // Nivel 0 de miniz produce bloques stored.
        let data = b"bloque stored sin compresion".repeat(10);
        let compressed = miniz(&data, 0);
        let out = inflate_with_dict(&[], &compressed, data.len()).expect("inflate");
        assert_eq!(out, data);
    }
}
