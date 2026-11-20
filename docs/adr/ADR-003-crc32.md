# ADR-003 — CRC-32 con `crc32fast` y `Hasher::combine`

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0/F2

## Contexto

El C calcula CRC-32 con tablas de 4×256 y una ruta PCLMULQDQ. Además necesita `crc32_combine`
(aritmética en GF(2)) para el CSO: el CRC de la cabecera+índice se combina con el CRC de los datos
comprimidos, que se escribieron **antes** de conocer la cabecera.

## Decisión

- CRC incremental: `crc32fast` (usa PCLMULQDQ en x86_64, software en otras arquitecturas).
- Combinación: `crc32fast::Hasher::combine`, con `new_with_initial_len` cuando haga falta fijar la
  longitud del bloque inicial.
- Se conservan como tests los cuatro vectores de `crc32_combine` extraídos del C, además del
  conocido `"123456789" -> 0xCBF43926`.

## Consecuencias

- Se eliminan ~650 líneas de C (tablas + ruta SIMD) sin perder rendimiento.
- La semántica de `combine` debe validarse contra el C en F2 (el CSO depende de ella); si hubiera
  diferencias, se implementa el `crc32_combine` del C como función local de ~40 líneas.

## Alternativas descartadas

- **Portar `pkg2zip_crc32.c` completo:** mantiene tablas y `unsafe` SIMD sin beneficio.
- **`crc` crate:** no aporta `combine` y su rendimiento es menor en x86_64.
