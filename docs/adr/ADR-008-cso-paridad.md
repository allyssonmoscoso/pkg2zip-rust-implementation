# ADR-008 — Paridad semántica (no byte a byte) en CSO

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F3

## Contexto

El CSO comprime cada sector de 2.048 bytes con deflate y guarda un índice de offsets; si el sector
comprimido no es más pequeño, se almacena tal cual con el bit `0x80000000`. El C usa `miniz_tdef`
con `MZ_FIXED`; Rust usará `miniz_oxide`, que produce flujos deflate válidos pero **no
necesariamente idénticos**. El ZIP, en cambio, guarda las entradas sin comprimir y sí debe coincidir.

## Decisión

Para CSO se exige:

1. ISO descomprimido **byte a byte** idéntico;
2. cabecera (magic `CISO`, tamaño original, tamaño de bloque, versión) e índice válidos;
3. tamaño total ≤ 102 % del generado por el C.

No se comparan los bytes comprimidos.

## Consecuencias

- La migración no se bloquea por diferencias legítimas de compresor.
- El CRC del CSO se calcula sobre los bytes realmente escritos (cabecera+índice combinado con datos),
  así que se valida de forma independiente con `crc32_combine`.
- Los tests de paridad marcan el caso como `Pass` comparando el árbol extraído/ISO resultante, no el
  archivo `.cso` crudo.

## Alternativas descartadas

- **Exigir bytes idénticos:** obligaría a portar `miniz_tdef` (1.500 líneas) sin beneficio funcional.
- **Usar `libdeflater`:** es C, rompe O1.
