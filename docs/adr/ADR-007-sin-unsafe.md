# ADR-007 — `unsafe` prohibido en el dominio

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0 en adelante

## Contexto

El C tiene puntos frágiles: `name[name_size] = 0` sin comprobar el tamaño, copias de título a
buffers de 256 bytes, `fread` sin comprobar `fopen`, aritmética de offsets en `uint64_t` sin validar
contra el tamaño real del archivo. Un PKG malformado puede provocar desbordamientos.

## Decisión

- `#![deny(unsafe_code)]` en `pkg2zip-core` y `pkg2zip-cli` (el workspace lo declara como lint).
- Toda lectura valida longitudes antes de indexar; los parsers devuelven `Error`, nunca `panic`.
- Única excepción auditada: `core/plat/console.rs` en Windows, donde `SetConsoleOutputCP` y
  `GetConsoleMode` son `unsafe` (FFI de `windows-sys`). Se aísla en un módulo `#[cfg(windows)]`
  con `#[allow(unsafe_code)]` local y comentario de justificación.

## Consecuencias

- Fuzzing con `cargo-fuzz` productivo: cualquier panic es un bug accionable.
- Ligeramente más código de comprobación (coste despreciable frente a IO).
- El objetivo O5 ("cero panic en entradas malformadas") se puede verificar de forma automática.

## Alternativas descartadas

- **`forbid(unsafe_code)` global:** imposible con la consola de Windows.
- **Usar un crate de consola (p. ej. `crossterm`):** añade dependencia para dos llamadas y no mejora
  la paridad de salida.
