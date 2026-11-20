# ADR-006 — Parser de CLI propio

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0/F2

## Contexto

La CLI de 2.6 tiene rarezas que los usuarios y scripts ya asumen:

- `-cN` con el número **pegado** (`-c9`), y `-c` sin número equivale a `.iso`;
- `-l` implica silencio (`verbose = 0`);
- `-l` junto con `-x` es un error específico;
- un argumento no reconocido se interpreta como ruta de PKG (el primero) y zRIF (el segundo),
  incluso si empieza por `-`;
- el zRIF posicional también acepta una ruta `.bin`;
- `-h` sale con código 0; sin argumentos, código 1.

## Decisión

Parser manual de ~120 líneas con tabla de flags y tests de tabla, en `pkg2zip-cli/src/args.rs`.
No se usa `clap` en F0–F5 para no alterar mensajes ni el orden de parseo.

## Consecuencias

- Paridad exacta con el binario C, incluidos los casos raros.
- `--version` se añade como opción nueva (aditiva).
- Si en el futuro se quiere ayuda autogenerada, se puede introducir `clap` con
  `disable_help_flag`/`allow_hyphen_values` sin cambiar el contrato, pero no es necesario.

## Alternativas descartadas

- **`clap` derive:** cambiaría mensajes, el tratamiento de argumentos desconocidos y el orden de
  error; el coste de mantener la compatibilidad sería mayor que el parser propio.
