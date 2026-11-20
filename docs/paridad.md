# Protocolo de paridad C ↔ Rust

Este documento describe cómo se verifica que la reescritura en Rust se comporta **igual** que la
implementación C de `legacy/`, y registra los resultados de cada fase.

## Principio

El binario C congelado es el **oráculo**. El arnés ejecuta ambos binarios sobre el mismo PKG, en
directorios temporales separados, y compara lo observable:

| Dimensión | Comparación |
|---|---|
| Código de salida | Estricta (`0`/`1`); el código `3` del binario Rust significa "pendiente" |
| `-l` | stdout byte a byte (normalizando finales de línea) |
| `-x` | Árbol completo: rutas de archivos y directorios + SHA-256 por archivo |
| stdout en `-x` | Se registra como *warning* hasta F2; luego será estricta |
| stderr en errores | Actualmente *warning*; se endurece en F4 |
| ZIP | Pendiente: se compara descomprimiendo con `zipfile`/`unzip` en F2 |
| CSO | Pendiente en F3; se compara el ISO resultante y el índice (ver ADR-008) |

## Corpus sintético

No se distribuyen PKG reales. `pkg2zip-testkit` genera fixtures deterministas con la misma
estructura que un PKG de Vita:

| Fixture | Estado | Cubre |
|---|---|---|
| `vita_app.pkg` | F0 | cabecera 192+64, 3 bloques de metadatos, SFO, items cifrados AES-CTR, `digs.bin` → `body.bin`, `head/tail/stat.bin` |
| resto de tipos | F2–F4 | DLC, PATCH, PSM, tema, PSP/PSX/PS3 |

`vita_app.pkg` incluye un directorio (`sce_sys`), un archivo (`eboot.bin`, 4.096 bytes) y
`sce_sys/package/digs.bin` (256 bytes) para ejercitar el renombrado sin descifrado.

El generador comparte con el parser la derivación de claves (`pkg2zip-core::crypto`), de modo que un
error de cripto rompe ambos lados de la comparación de forma simétrica y detectable.

## Cómo ejecutarlo

```powershell
# 1. Binario C de referencia (Windows; en Linux/macOS usar scripts/build-legacy.sh)
pwsh ./scripts/build-legacy.ps1

# 2. Corpus sintético
pwsh ./scripts/gen-corpus.ps1

# 3. Arnés (compila el testkit y compara)
pwsh ./scripts/parity.ps1
```

```sh
./scripts/build-legacy.sh
./scripts/gen-corpus.sh
./scripts/parity.sh
```

El reporte queda en `target/parity.json`:

```json
{
  "tool_version": "3.0.0",
  "legacy": "target/legacy/pkg2zip.exe",
  "rust": "target/debug/pkg2zip.exe",
  "passed": 0,
  "pending": 2,
  "failed": 0,
  "cases": [
    { "fixture": "vita_app.pkg", "case": "list", "status": "pending", "details": [], "warnings": [] }
  ]
}
```

El proceso sale con `1` si hay algún caso `fail`; los `pending` no rompen CI (pero deben bajar a cero
al final de cada fase).

## Baseline F0 (2026-11-20)

| Comprobación | Resultado |
|---|---|
| `legacy/pkg2zip -l target/corpus/vita_app.pkg` | `Synthetic Vita App [PCSE00000] [USA].zip` (exit 0) |
| `legacy/pkg2zip -x …` | `app/PCSE00000/{eboot.bin, sce_sys/package/{body.bin, head.bin, stat.bin, tail.bin}}` (exit 0) |
| `pkg2zip --version` (Rust) | `pkg2zip 3.0.0` |
| `pkg2zip` sin argumentos (Rust) | mensaje de error + ayuda, exit 1 |
| `pkg2zip <pkg>` (Rust, F0) | exit 3 (pendiente) |
| Arnés de paridad | `0 passed, 2 pending, 0 failed` |

Esto demuestra que el fixture es válido para el parser C y que el arnés distingue correctamente
"pendiente" de "fallo".

## Congelación de `upstream`

- Repositorio original: <https://github.com/lusid1/pkg2zip>
- Commit base de la migración: `6ee3df5` ("Check fread result in rif_load function", `master`).
- Durante F0–F5 no se sincronizan cambios de `upstream`; se revisarán después de 3.0.0 (ADR-009).

## Endurecimiento por fase

| Fase | Se añade a la comparación |
|---|---|
| F2 | ZIP (entradas + CRC), stdout estricto en `-x`, fixtures DLC/PATCH/PSM/tema |
| F3 | PSP/PSX: ISO byte a byte, CSO (contenido + índice), EDAT, KEYS.BIN |
| F4 | Todos los tipos y flags (`-b`, `-p`, `-d`, `-a`, `-cN`), errores y stderr |
| F5 | Matriz final como *gate* de release |
