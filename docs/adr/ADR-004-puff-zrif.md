# ADR-004 — Portar `puff` para el inflate de zRIF

- **Fecha:** 2026-11-20
- **Estado:** aceptada (sujeta a spike en F1)
- **Fase:** F1

## Contexto

Un zRIF es base64 de un flujo **zlib deflate con diccionario preestablecido** de 1.024 bytes
(ID `0x627d1d5d`). El C usa una copia modificada de `puff` (433 líneas, dominio público) que
inicializa el estado con el diccionario ya copiado en el buffer de salida.

Opciones evaluadas:

| Opción | Problema |
|---|---|
| `flate2` + `miniz_oxide` | `Decompress` no soporta diccionario preestablecido |
| `flate2` + `set_dictionary` | Solo existe con backend zlib en **C** (`any_c_zlib`) → rompe el objetivo "sin C" |
| `zlib-rs` 0.6 (alto nivel) | `InflateConfig` solo expone `window_bits`, sin diccionario |
| `libz-rs-sys` | API zlib en Rust puro, pero añade capa FFI/ABI y `unsafe` |
| Portar `puff` | ~400 líneas propias, sin dependencias, paridad exacta |

## Decisión

Portar `puff.c` a Rust seguro con la firma:

```rust
pub fn inflate_with_dict(dict: &[u8], input: &[u8], capacity: usize) -> Result<Vec<u8>>;
```

Se conserva la validación de cabecera zlib (FCHECK, FDICT, DICTID), el chequeo Adler-32 y el
`memmove` final que descarta el diccionario del resultado.

## Consecuencias

- Cero dependencias nuevas y cero `unsafe`.
- Código auditable y testeable con zRIF reales y con fuzzing diferencial contra `zlib` de Python
  (solo en tests).
- Riesgo: bug sutil de porting; mitigado con vectores reales, fuzzing y revisión línea a línea.
- Si el port se desviara del cronograma, el plan B es `libz-rs-sys` (sigue siendo Rust puro, pero
  con `unsafe` y ABI).

## Alternativas descartadas

- **Compilar zlib en C:** viola el objetivo O1 (cero C).
- **Recomprimir zRIF a un flujo sin diccionario:** no es posible sin la clave privada del formato.
