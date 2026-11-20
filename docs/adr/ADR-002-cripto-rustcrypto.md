# ADR-002 — Criptografía con RustCrypto, sin SIMD propio

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0/F1

## Contexto

El C implementa AES-128 a mano con tablas T y dos rutas SIMD (`pkg2zip_aes_x86.c`, AES-NI/SSSE3),
más CMAC (RFC 4493), CTR con contador big-endian de 128 bits y una variante propia de PSP
(`aes128_psp_decrypt`). Mantener eso en Rust sería más código y más superficie de error.

## Decisión

- AES-128 ECB/CTR: crates `aes` 0.9 + `ctr` 0.10 (RustCrypto).
- CMAC: crate `cmac` 0.8.
- Variante PSP: implementación propia de ~60 líneas sobre `aes::Aes128::decrypt_block`, replicando
  el encadenamiento de `legacy/pkg2zip_aes.c` (contador little-endian en los últimos 4 bytes).
- La selección de AES-NI/PCLMUL es interna a los crates vía `cpufeatures`: no hay `cfg(target_feature)`
  ni `unsafe` en nuestro código.

## Consecuencias

- Menos código propio y revisado por la comunidad.
- Rendimiento AES-NI equivalente al C (objetivo: ≥ 95 %; se mide en F1).
- API fijada con versiones exactas (`=0.9.4`, `=0.10.1`) porque son releases recientes de RustCrypto
  con cambios de traits (`cipher` 0.5).
- Verificación: FIPS-197, NIST SP 800-38A F.5.1, RFC 4493 y comparación con el binario C.

## Alternativas descartadas

- **Portar el AES del C tal cual:** más código, `unsafe` para SIMD y sin ventaja de rendimiento.
- **`ring`:** compila C/asm y no expone AES-ECB de bloque suelto, que el formato PKG necesita para
  derivar la clave principal.
