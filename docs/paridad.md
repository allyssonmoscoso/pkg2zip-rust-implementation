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

## Baseline F1 (2026-11-20)

La fase F1 añade la cripto, checksums, zRIF y parsers de formato, con 69 tests en total:

| Área | Prueba |
|---|---|
| AES-128 ECB | FIPS-197 Apéndice C (cifrar y descifrar) |
| AES-128 CTR | NIST SP 800-38A F.5.1 + coherencia de offsets |
| CMAC | RFC 4493 (4 vectores) |
| PSP-AES | 2 vectores golden generados con `legacy/pkg2zip_aes.c` compilado con MSVC |
| CRC-32 | `"123456789"`, incremental y `combine(a, b, len_b)` contra concatenación |
| Adler-32 | `"Wikipedia"` e incremental |
| puff (port) | Streams deflate de `miniz_oxide` (niveles 0/1/6/10), bloques stored, capacidad insuficiente, entrada truncada |
| puff + diccionario | Bloque fijo hecho a mano que copia 3 bytes del diccionario preestablecido y rechaza distancias fuera de rango |
| zRIF | Cabecera zlib (FCHECK/FDICT/DICTID), checksum, método, diccionario desconocido y round-trip base64 |
| Formato | Cabecera PKG, bloques de metadatos, SFO (incluida sanitización de título), `Kind` y región |
| Integración | El fixture `vita_app` se parsea de punta a punta y los items descifrados coinciden con el generador |

**Limitación conocida:** no hay zRIF reales en el repositorio. Los vectores de zRIF son sintéticos
(generados con `miniz_oxide` y cabecera con diccionario). Cuando haya muestras reales se añadirán a
`testdata/zrif/` (gitignored) y un test las consumirá si existen.
## Baseline F2 (2026-11-20)

F2 añade el escritor ZIP, la CLI compatible y los cuatro tipos de Vita. El arnés pasa a
**15 casos** por corrida:

| Caso | Compara |
|---|---|
| `list` | stdout de `-l` byte a byte |
| `extract` | árbol de `-x` (rutas + SHA-256) y stdout estricto salvo el banner |
| `zip` | entradas del zip (nombre, CRC, tamaño y SHA-256) y stdout |
| `zrif` | igual que `extract` añadiendo el zRIF (`work.bin`) |

Corpus actual: `vita_app`, `vita_patch`, `vita_dlc` y `vita_theme`. Los zRIF se generan como
flujos zlib con diccionario y bloque `stored` (no hace falta un compresor).

Resultado: `15 passed, 0 pending, 0 failed`. El ZIP coincide con el del C en entradas y contenido;
los timestamps DOS pueden diferir porque el C usa hora local y Rust UTC (no se comparan).

Notas:

- `APP_VER` se conserva tal cual (`2.10`) y `PSP2_DISP_VER` recorta un cero final (`3.60` → `3.6`),
  replicando la lógica del C.
- PSM/Android sigue pendiente (F4); el arnés marcará esos casos como `pending` cuando exista el
  fixture.

## Baseline F3 (2026-11-20)

F3 añade PSP/PSX/PS3. El corpus crece a **7 fixtures** y la corrida a **29 casos**:

| Fixture | Casos | Cubre |
|---|---|---|
| `vita_*` (4) | `list`, `extract`, `zip`, `zrif`, `psp-pbp` | Vita completo |
| `psp_pbp` | `list`, `psp-pbp` | modo `-p`: EBOOT.PBP, DOCUMENT.DAT y contenido extra |
| `psx_docs` | `list`, `extract`, `zip`, `zrif`, `psp-pbp` | PSX: DOCUMENT.DAT, texture.enc y licencia `.rif` desde zRIF |
| `ps3_doc` | `list`, `extract`, `zip` | PS3: `pspemu/PSP/GAME/<game>/DOCUMENT.DAT` |

Resultado: `29 passed, 0 pending, 0 failed`.

**Cripto validada contra el binario C** (vectores golden generados con MSVC y las funciones
originales):

- `init_psp_decrypt` (IV derivado y descifrado de la cabecera PSAR).
- `init_psx_keys` (KEYS.BIN de PSX/PS3).
- `psp_decrypt` (dos vectores de 64 y 32 bytes).
- `CsoEncoder`: round-trip ISO → CSO → ISO con cabecera, índice y sectores comprimidos.
- LZRC: modo texto plano y validación de entradas truncadas.

**Limitación conocida:** los caminos que dependen de un PSAR/PGD real (ISO/CSO desde EBOOT y
EDAT/KEYS.BIN) no tienen fixture end-to-end. Construir un PSAR sintético exige resolver el punto
fijo `C = P ^ K(CMAC(C))` que impone el MAC del contenedor; eso solo es viable con una muestra real
o con las claves de firma. La lógica está portada y la cripto validada con vectores del C; cuando
haya una muestra se añadirá a `testdata/psp/` (gitignored) y se activará la paridad de ISO/CSO/EDAT.

## Baseline F4 (2026-11-20)

F4 completa PSM y el barrido de bordes. Corpus de **12 fixtures** y **92 casos**:

| Bloque | Casos | Fixtures |
|---|---|---|
| `list`, `extract`, `zip` | todos | todos |
| `zrif`, `zrif-bin`, `zrif-mismatch` | con work.bin | APP/DLC/THEME/PSM/PSX (no PATCH/PSP/PS3) |
| `psp-pbp` (`-p`) | todos | solo cambia en PSP |
| `no-bgdl` (`-b -x`) | todos | solo cambia en temas |
| `quiet` (`-q -x`) | todos | ninguno |
| `android` (`-a -x`) | todos | solo cambia en PSM |

Resultado: `92 passed, 0 pending, 0 failed`.

Cubierto: PSM normal y Android, `content_id`, `pm.dat`, `FAKE.rif`, PDB bgdl, títulos con `:`,
salto de línea y UTF-8, nombre de 120 bytes, archivo vacío, `install_directory`, zRIF `.bin`,
`CONTENT_ID` distinto (mismo error en ambos binarios), firma inválida, `total_size` inflado y tipo
no soportado.

Detalles de paridad descubiertos:

- El C recorta los nombres de PSM con `name + 9` (no con `"content/"`, que son 8); se replica el
  offset exacto para que el árbol coincida.
- El decodificador base64 del C tiene un caso límite con longitudes no múltiplo de 4; los zRIF de
  prueba se comprimen de verdad (con `miniz_oxide`) para que quepan en `raw[1024]` del C.
- Nombres de 1023 bytes no se prueban end-to-end: el binario C falla en Windows por `MAX_PATH`;
  el límite se valida con tests unitarios.
- Los caminos con PSAR/PGD reales (ISO/CSO/EDAT) siguen esperando una muestra del mantenedor
  (ver F3).
## Congelación de `upstream`

- Repositorio original: <https://github.com/lusid1/pkg2zip>
- Commit base de la migración: `6ee3df5` ("Check fread result in rif_load function", `master`).
- Durante F0–F5 no se sincronizan cambios de `upstream`; se revisarán después de 3.0.0 (ADR-009).

## Endurecimiento por fase

| Fase | Se añade a la comparación |
|---|---|
| F2 | ZIP (entradas + CRC), stdout estricto en `-x`, fixtures APP/PATCH/DLC/THEME y zRIF |
| F3 | PSP `-p`, PSX (DOCUMENT.DAT/texture.enc/`.rif`) y PS3; cripto PSP validada con vectores golden |
| F4 | PSM/Android, `-b`, `-q`, zRIF `.bin`/mismatch, errores y matriz de 92 casos |
| F5 | Matriz final como *gate* de release |
