//! Decodificador LZRC (range coder) usado por los bloques `data.psar` de PSP.
//!
//! Port de `lzrc_decompress` de `legacy/pkg2zip_psp.c` (código original de libkirk).
//! El formato no documenta un encoder; aquí se decodifica y se valida con el modo "texto plano"
//! (primer byte con el bit 0x80) y con tests de límites.

use crate::error::{Error, Result};

const MAX_OUT: usize = 16 * 2048;

/// Descomprime un bloque LZRC.
///
/// `out_len` es el tamaño máximo esperado (16 * 2048 en el formato).
///
/// # Errors
///
/// [`Error::Corrupt`] si la entrada está truncada, el flujo es inválido o se sale del buffer.
pub fn decompress(input: &[u8], out_len: usize) -> Result<Vec<u8>> {
    if input.len() < 5 {
        return Err(Error::Corrupt(
            "internal error - lzrc input underflow! pkg may be corrupted?",
        ));
    }

    let lc = input[0];
    let first_code = u32::from_be_bytes(input[1..5].try_into().expect("4 bytes"));

    if lc & 0x80 != 0 {
        let length = first_code as usize;
        if input.len() < 5 + length {
            return Err(Error::Corrupt(
                "internal error - lzrc input underflow! pkg may be corrupted?",
            ));
        }
        return Ok(input[5..5 + length].to_vec());
    }

    let mut probabilities = Probabilities::default();
    let mut decoder = RangeDecoder {
        input,
        in_ptr: 5,
        range: 0xffff_ffff,
        code: first_code,
        out: vec![0u8; MAX_OUT],
        out_ptr: 0,
        out_len: out_len.min(MAX_OUT),
        lc,
    };

    decoder.run(&mut probabilities)
}

/// Probabilidades adaptativas del codificador (todas empiezan en 0x80).
struct Probabilities {
    literal: [[u8; 256]; 8],
    dist_bits: [[u8; 64]; 8],
    dist: [[u8; 8]; 64],
    match_flag: [[u8; 8]; 8],
    length: [[u8; 31]; 8],
}

impl Default for Probabilities {
    fn default() -> Self {
        Self {
            literal: [[0x80; 256]; 8],
            dist_bits: [[0x80; 64]; 8],
            dist: [[0x80; 8]; 64],
            match_flag: [[0x80; 8]; 8],
            length: [[0x80; 31]; 8],
        }
    }
}

struct RangeDecoder<'a> {
    input: &'a [u8],
    in_ptr: usize,
    range: u32,
    code: u32,
    out: Vec<u8>,
    out_ptr: usize,
    out_len: usize,
    lc: u8,
}

impl RangeDecoder<'_> {
    fn normalize(&mut self) -> Result<()> {
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            let byte = *self.input.get(self.in_ptr).ok_or(Error::Corrupt(
                "internal error - lzrc input underflow! pkg may be corrupted?",
            ))?;
            self.in_ptr += 1;
            self.code = (self.code << 8) + u32::from(byte);
        }
        Ok(())
    }

    fn bit(&mut self, probability: &mut u8) -> Result<u32> {
        self.normalize()?;
        let bound = (self.range >> 8) * u32::from(*probability);
        *probability = probability.wrapping_sub(*probability >> 3);

        if self.code < bound {
            self.range = bound;
            *probability = probability.wrapping_add(31);
            Ok(1)
        } else {
            self.code -= bound;
            self.range -= bound;
            Ok(0)
        }
    }

    fn bittree(&mut self, probabilities: &mut [u8], limit: usize) -> Result<usize> {
        let mut number = 1usize;
        loop {
            number = (number << 1) + self.bit(&mut probabilities[number])? as usize;
            if number >= limit {
                break;
            }
        }
        Ok(number)
    }

    fn number(&mut self, probabilities: &mut [u8], n: u32) -> Result<usize> {
        let mut number = 1usize;

        if n > 3 {
            number = (number << 1) + self.bit(&mut probabilities[3])? as usize;
            if n > 4 {
                number = (number << 1) + self.bit(&mut probabilities[3])? as usize;
                if n > 5 {
                    self.normalize()?;
                    for _ in 0..n - 5 {
                        self.range >>= 1;
                        number <<= 1;
                        if self.code < self.range {
                            number += 1;
                        } else {
                            self.code -= self.range;
                        }
                    }
                }
            }
        }

        if n > 0 {
            number = (number << 1) + self.bit(&mut probabilities[0])? as usize;
            if n > 1 {
                number = (number << 1) + self.bit(&mut probabilities[1])? as usize;
                if n > 2 {
                    number = (number << 1) + self.bit(&mut probabilities[2])? as usize;
                }
            }
        }

        Ok(number)
    }

    fn run(&mut self, probabilities: &mut Probabilities) -> Result<Vec<u8>> {
        let mut state = 0usize;
        let mut last_byte = 0u8;

        loop {
            let mut match_step = 0usize;
            let bit = self.bit(&mut probabilities.match_flag[state][match_step])?;

            if bit == 0 {
                state = state.saturating_sub(1);
                let index = (usize::from(last_byte) >> (self.lc & 7)) & 0x07;
                let byte = self.bittree(&mut probabilities.literal[index], 0x100)?;
                let byte = byte.wrapping_sub(0x100) as u8;

                if self.out_ptr == self.out_len {
                    return Err(Error::Corrupt(
                        "internal error - lzrc output overflow! pkg may be corrupted?",
                    ));
                }
                self.out[self.out_ptr] = byte;
                self.out_ptr += 1;
                last_byte = byte;
            } else {
                let mut len_bits = 0u32;
                for _ in 0..7 {
                    match_step += 1;
                    let bit = self.bit(&mut probabilities.match_flag[state][match_step])?;
                    if bit == 0 {
                        break;
                    }
                    len_bits += 1;
                }

                let match_len = if len_bits == 0 {
                    1
                } else {
                    let len_state = (((len_bits - 1) << 2)
                        + ((self.out_ptr as u32) << (len_bits - 1) & 0x03))
                        as usize;
                    let length =
                        self.number(&mut probabilities.length[state][len_state..], len_bits)?;
                    if length == 0xFF {
                        return Ok(self.out[..self.out_ptr].to_vec());
                    }
                    length
                };

                let mut dist_state = 0usize;
                let mut limit = 8usize;
                if match_len > 2 {
                    dist_state += 7;
                    limit = 44;
                }
                let dist_bits = self.bittree(
                    &mut probabilities.dist_bits[usize::try_from(len_bits).expect("len_bits <= 7")],
                    dist_state,
                )?;
                let Some(dist_bits) = dist_bits.checked_sub(limit) else {
                    return Err(Error::Corrupt(
                        "internal error - lzrc match_dist out of range! pkg may be corrupted?",
                    ));
                };

                let match_dist = if dist_bits > 0 {
                    let Some(probabilities_dist) = probabilities.dist.get_mut(dist_bits) else {
                        return Err(Error::Corrupt(
                            "internal error - lzrc match_dist out of range! pkg may be corrupted?",
                        ));
                    };
                    self.number(probabilities_dist, dist_bits as u32)?
                } else {
                    1
                };

                if match_dist > self.out_ptr {
                    return Err(Error::Corrupt(
                        "internal error - lzrc match_dist out of range! pkg may be corrupted?",
                    ));
                }
                if self.out_ptr + match_len + 1 > self.out_len {
                    return Err(Error::Corrupt(
                        "internal error - lzrc output overflow! pkg may be corrupted?",
                    ));
                }

                let source = self.out_ptr - match_dist;
                for offset in 0..=match_len {
                    let byte = self.out[source + offset];
                    self.out[self.out_ptr] = byte;
                    self.out_ptr += 1;
                }
                last_byte = self.out[self.out_ptr - 1];

                state = 6 + ((self.out_ptr + 1) & 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(data: &[u8]) -> Vec<u8> {
        let mut input = vec![0x80];
        input.extend_from_slice(&(data.len() as u32).to_be_bytes());
        input.extend_from_slice(data);
        input
    }

    #[test]
    fn decodes_plain_text_mode() {
        let data = b"lzrc texto plano";
        let out = decompress(&plain(data), 2048).expect("texto plano");
        assert_eq!(out, data);
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(decompress(&[0x80, 0, 0], 2048).is_err());
        assert!(decompress(&plain(b"abc")[..6], 2048).is_err());
    }

    #[test]
    fn rejects_impossible_plain_length() {
        let mut input = vec![0x80];
        input.extend_from_slice(&u32::MAX.to_be_bytes());
        input.extend_from_slice(b"abc");
        assert!(decompress(&input, 2048).is_err());
    }

    #[test]
    fn zero_length_plain_text() {
        assert_eq!(
            decompress(&plain(b""), 2048).expect("vacío"),
            Vec::<u8>::new()
        );
    }
}
