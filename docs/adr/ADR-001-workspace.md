# ADR-001 — Workspace de tres crates

- **Fecha:** 2026-11-20
- **Estado:** aceptada
- **Fase:** F0

## Contexto

El código C mezcla dominio (formatos, cripto, ZIP), IO de plataforma y CLI en un único binario.
Los tests de paridad necesitan además **generar** PKG (es decir, cifrar con las mismas claves) y
comparar árboles de salida, cosas que no deben formar parte del binario de producción.

## Decisión

Workspace con tres crates:

| Crate | Rol |
|---|---|
| `pkg2zip-core` | Librería con todo el dominio; sin CLI ni variables globales |
| `pkg2zip-cli` | Binario `pkg2zip`: parseo de argumentos, mensajes, códigos de salida |
| `pkg2zip-testkit` | Herramienta de desarrollo: PKG sintéticos y arnés de paridad (`publish = false`) |

`pkg2zip-testkit` reutiliza la cripto de `core`, de modo que el generador y el parser comparten la
misma derivación de claves y no pueden divergir.

## Consecuencias

- `core` se puede testear con sinks en memoria, sin tocar el sistema de archivos.
- El binario de producción no arrastra `serde`, `sha2`, `tempfile` ni `walkdir`.
- Compilación incremental más rápida por crate.
- Coste: dependencias internas por `path` y versionado conjunto (mismo workspace).

## Alternativas descartadas

- **Un solo crate:** mezclaría dependencias de test con el binario y dificultaría tests aislados.
- **Dos crates (core + cli) con el testkit como módulo de tests:** el generador necesita un
  ejecutable propio para el arnés de paridad y para generar fixtures desde scripts.
