# Plan de migración de `pkg2zip` de C a Rust

| Campo | Valor |
|---|---|
| Documento | Plan de migración completa a Rust |
| Rama de trabajo | `develop` (creada desde `master` en `6ee3df5`) |
| Versión objetivo | `pkg2zip 3.0.0` (reescritura; reemplaza a 2.6) |
| Estado | Propuesta para aprobación — **no se ha escrito código Rust todavía** |
| Fecha | 2026-11-20 |
| Alcance | Reemplazar el 100 % del C por Rust, sin dependencia de compilador de C, con binarios para Windows/Linux/macOS y paridad funcional con 2.6 |

---

## 0. Resumen ejecutivo

El proyecto es una utilidad de línea de comandos de ~5.000 líneas de C propio (más ~2.000 líneas
vendoreadas de `miniz_tdef` y `puff`) que descifra y desempaqueta PKG de PS Vita, PSP, PSX, PS3 y
PSM, escribiendo un ZIP (o archivos sueltos), ISO/CSO, EBOOT.PBP, licencias `work.bin`/`FAKE.rif`,
EDAT descifrados y estructuras auxiliares (`head.bin`, `tail.bin`, `stat.bin`, PDB de temas).

La migración es viable **sin dependencias de C** porque todo el trabajo pesado tiene equivalente en
Rust puro:

| Necesidad | Implementación C actual | Sustituto Rust puro |
|---|---|---|
| AES-128 ECB/CTR, AES-NI | `pkg2zip_aes*.c` | crates `aes` + `ctr` (detección de AES-NI en runtime) |
| CMAC (RFC 4493) | `pkg2zip_aes.c` | crate `cmac` |
| AES variante PSP (kirk) | `pkg2zip_aes.c` | implementación propia sobre `aes` (~60 líneas) |
| CRC-32 + `crc32_combine` | `pkg2zip_crc32*.c` | `crc32fast` (PCLMULQDQ) + `Hasher::combine` |
| Deflate para CSO | `miniz_tdef.c` | `miniz_oxide` (`compress_to_vec`, deflate crudo) |
| Inflate de zRIF con diccionario | `puff.c` modificado | **port de `puff`** (~400 líneas, dominio público) |
| ZIP/ZIP64 | `pkg2zip_zip.c` | escritor propio (~450 líneas) |
| CLI | `main()` | parser propio (~120 líneas) para conservar rarezas |

**Estrategia:** migración incremental por módulos en `develop`, con una **red de seguridad de
paridad** basada en un corpus sintético generado por Rust y comparación diferencial contra el binario
C congelado. El C no se borra hasta que la paridad esté demostrada; al final se elimina por completo
y el build pasa a ser `cargo build --release` a secas.

**Duración estimada:** 23–26 días de trabajo efectivo de una persona (≈ 5–6 semanas de calendario),
en 7 fases con compuertas de aceptación explícitas.

| Fase | Objetivo | Días |
|---|---|---|
| F0 | Preparación: rama, workspace, CI base, arnés de paridad y binario C de referencia | 2 |
| F1 | Núcleo: bytes, AES/CTR/CMAC/PSP-AES, CRC-32, SFO, zRIF | 4 |
| F2 | Salida: ZIP/ZIP64, sinks, IO multiplataforma, CLI, ruta Vita APP/PATCH/DLC/THEME | 5 |
| F3 | PSP/PSX/PS3: EBOOT/PSAR, LZRC, ISO/CSO, EDAT, KEYS.BIN, tema PSP | 6 |
| F4 | PSM (normal y Android), bgdl, bordes, barrido de paridad | 3 |
| F5 | CI/release multiplataforma, documentación, borrado del C, tag 3.0.0 | 3 |
| F6 (opcional) | Optimizaciones (paralelismo de sectores CSO, CRC paralelo) y extras | 3 |

---

## 1. Situación de partida

### 1.1 Inventario de código

Estado: rama `master`, commit `6ee3df5` ("Check fread result in rif_load function"), árbol limpio.
Remotos: `origin` = `allyssonmoscoso/pkg2zip-rust-implementation`, `upstream` = `lusid1/pkg2zip`.

| Archivo | Líneas | Responsabilidad |
|---|---:|---|
| `pkg2zip.c` | 1.449 | `main`, parseo de CLI, cabecera PKG, metadatos, detección de tipo, SFO, detección de región, nombres de salida, orquestación del desempaquetado, PDB de temas bgdl, layouts PSM, `head/tail/stat.bin`, `work.bin` |
| `pkg2zip_psp.c` | 794 | `unpack_psp_eboot` (PSAR → ISO/CSO), decodificador LZRC, `unpack_psp_key`, `unpack_psp_edat`, `unpack_keys_bin`, `get_psp_theme_title` |
| `pkg2zip_zip.c` | 437 | Escritor ZIP/ZIP64, entradas almacenadas, directorio central, escritura en offsets y CRC diferido |
| `pkg2zip_sys.c` | 362 | IO posicional (`pread`/`pwrite` vs `ReadFile`+`OVERLAPPED`), `mkdir` recursivo, consola UTF-8 en Windows, progreso |
| `pkg2zip_aes.c` | 559 | AES-128 software (tablas T), ECB, CTR (contador BE de 128 bits), CMAC, cifrado PSP |
| `pkg2zip_out.c` | 137 | Fachada "zip o carpeta" sobre el sink de salida |
| `pkg2zip_zrif.c` | 185 | Base64, inflate zlib con diccionario preestablecido, verificación Adler-32, carga de `.bin` |
| `pkg2zip_crc32.c` | 327 | CRC-32 por tablas (4×256), `crc32_combine` en GF(2) |
| `pkg2zip_aes_x86.c` | 175 | Rutas AES-NI/SSSE3 |
| `pkg2zip_crc32_x86.c` | 336 | Rutas PCLMULQDQ/SSE4.1 |
| `pkg2zip_utils.h` | 129 | Lectura/escritura LE/BE, `min32/64`, `str_endswith` |
| headers menores (`_aes.h`, `_zip.h`, `_out.h`, `_psp.h`, `_sys.h`, `_zrif.h`, `_crc32.h`) | 266 | Contratos entre módulos |
| `miniz_tdef.c/.h` | 1.547 | Compresor deflate (solo se usa en CSO) — vendoreado, dominio público |
| `puff.c/.h` | 470 | Inflate con diccionario (solo zRIF) — vendoreado, dominio público, modificado |

**Totales:** 7.044 líneas C/headers; **5.027 líneas propias** + 2.017 vendoreadas.

Otros artefactos: `makefile` (gcc/clang), `build.cmd`/`build32.cmd` (MSVC con ruta fija a VS2017),
`.travis.yml` (Linux/macOS), `.appveyor.yml` (MinGW/VS2015), `flake.nix` (devshell C, solo
`x86_64-linux`), `rif2zrif.py`/`zrif2rif.py` (scripts Python que se instalan como comandos),
`.github/workflows/cmake-multi-platform.yml`.

> **Hallazgo:** el workflow de GitHub Actions invoca CMake, pero el repositorio **no contiene
> `CMakeLists.txt`**, por lo que ese CI no puede funcionar. Travis/AppVeyor son legados (trusty,
> VS2015). En la práctica hoy no hay CI verde que proteja la migración: la Fase 0 debe crear uno.

### 1.2 Funcionalidad a preservar

**Tipos de PKG** (detectados por `content_type` de la cabecera):

| Tipo | `content_type` | Salida esperada |
|---|---|---|
| Vita APP | `0x15` | `app/<ID>/...` + `sce_sys/package/{work,head,tail,stat}.bin` |
| Vita PATCH | `0x15` con `CATEGORY == "gp"` | `patch/<ID>/...` |
| Vita DLC | `0x16` | `addcont/<ID>/<DLC-ID>/...` |
| Vita PSM | `0x18`, `0x1d` | `psm/<ID>/RO` y `/RW`, `License/FAKE.rif`, `System/{content_id,pm.dat}` |
| Vita THEME | `0x1f` | `bgdl/t/<NNNNNNNN>/<ID>/...` + `d0/d1/f0.pdb` (o `app/...` con `-b`) |
| PSP | `0x07`, `0x0e`, `0x0f`, `0x10` | `pspemu/PSP/GAME/<ID>` o `pspemu/ISO/<Título> [<ID>].iso|cso` |
| PSP THEME | `0x09` | `pspemu/PSP/THEME/<archivo>` |
| PSX | `0x06` | `pspemu/PSP/GAME/<ID>/{EBOOT.PBP,KEYS.BIN,DOCUMENT.DAT}`, `ps1emu/<ID>/texture.enc`, licencia `.rif` |
| PS3 (PSX inside) | `0x01` | `pspemu/PSP/GAME/<GAME>/{EBOOT.PBP,KEYS.BIN,DOCUMENT.DAT}` |

**Opciones de CLI:** `-x/--extract`, `-l/--list`, `-cN`, `-p/--psp`, `-d/--decrypt`,
`-b/--no-bgdl`, `-q/--quiet`, `-a/--android-psm`, `-h/--help`; posicionales `pkg` y `zRIF`
(este último también acepta ruta a `.bin` desde 2.6).

**Comportamientos contractuales** (los consumen scripts y usuarios):

1. Nombre del zip y salida de `-l`: `"<Título> [<ID>] [<Región>] <sufijos>.zip"`, con `%.9s` del ID.
2. Detección de región por prefijo de 4 letras (`EUR/ASA/JPN/USA/KOR/INT/UNK`).
3. Estructura de carpetas dentro del zip (Vita espera esa ruta exacta; Adrenaline también).
4. ZIP con entradas **stored** (sin comprimir) y ZIP64 cuando hace falta.
5. Mensajes `[*] ...`/`ERROR: ...` en stdout/stderr y códigos de salida `0`/`1`.
6. Progreso `[*] unpacking... N%\r` solo si stdout es TTY.
7. `-l` implica `-q`; `-l` con `-x` es error.
8. `-c` sin número equivale a `.iso`; `-cN` se recorta a 0–9.
9. Cualquier argumento no reconocido se interpreta como `pkg` (el primero) y `zRIF` (el segundo), incluso si empieza por `-`.
10. `zRIF` se valida contra `CONTENT_ID` del SFO antes de escribir la licencia.

### 1.3 Dependencias y build actual

- **Terceros vendoreados:** `miniz_tdef` (deflate, dominio público) y `puff` (inflate, dominio
  público, con parche de diccionario). El resto es código propio sin dependencias externas.
- **Build:** `make` con `-maes -mssse3` y `-mpclmul -msse4` para las rutas SIMD; MSVC con
  `build.cmd`. No hay `CMakeLists.txt`, `Cargo.toml` ni contenedor.
- **Instalación:** `make install` copia el binario y los dos scripts Python a `$(PREFIX)/bin`.

### 1.4 Entorno verificado (esta máquina, 2026-11-20)

| Hecho | Valor | Implicación |
|---|---|---|
| `rustc` / `cargo` | 1.100.0 (`C:\Users\allys\.cargo\bin`, no está en `PATH` de la sesión) | Añadir al `PATH` o usar ruta absoluta en scripts |
| Toolchain activo | `stable-x86_64-pc-windows-msvc` | Único target instalado; los demás se añaden con `rustup target add` |
| `cargo build` de prueba | OK (0,8 s) | Enlaza correctamente con MSVC; no hace falta `cc` en `PATH` |
| crates.io | Accesible (HTTP 200 en `index.crates.io`) | Se pueden usar crates; fijar versiones con `Cargo.lock` |
| gcc/clang/cmake/make/nmake | **Ausentes** del `PATH` | El build C de referencia debe hacerse con MSVC vía `VsDevCmd` |
| Visual Studio | 2026 Community (`C:\Program Files\Microsoft Visual Studio\18\Community`) + Windows SDK | `build.cmd` actual (ruta fija a VS2017) está roto; hay que localizar VS con `vswhere` |
| `python` | Disponible (Windows Store shim) | Sirve para validar ZIPs/CSOs en pruebas, no como dependencia del producto |

**Conclusión:** el entorno permite construir el binario C de referencia (MSVC) y el proyecto Rust.
El plan debe hacer que el build C de referencia sea *opcional y aislado* (`legacy/`), porque el
objetivo final es no necesitar compilador de C.

---

## 2. Objetivos, no-objetivos y criterios de éxito

### 2.1 Objetivos

| ID | Objetivo | Verificación |
|---|---|---|
| O1 | Cero código C en el producto final | `git ls-files '*.c' '*.h'` vacío; `cargo tree` sin crates que requieran compilador C; build limpio en un contenedor solo con Rust |
| O2 | Paridad funcional con 2.6 | Corpus de paridad: mismos árboles de salida, mismos nombres, mismos CRC/SHA-256, mismos códigos de salida |
| O3 | Binarios para Windows, Linux y macOS (x86_64 y aarch64) | CI de release produce artefactos para ≥ 6 targets y se ejecutan en sus SO |
| O4 | Rendimiento igual o mejor | Benchmarks: sin regresión > 5 % en el camino crítico; ≥ 1,3× en CSO con paralelismo (F6) |
| O5 | Robustez superior | Cero `panic` en entradas malformadas; fuzzing 24 h sin fallos; límites de memoria acotados |
| O6 | Mantenibilidad | Código modular, `cargo fmt`, `clippy -D warnings`, cobertura ≥ 80 % en módulos de formato, documentación de arquitectura |

### 2.2 No-objetivos (explícitos)

- Añadir formatos nuevos (PS4/PS5, PKG de otras plataformas) o cambiar el formato de salida.
- GUI, servidor, integración con VitaShell/Adrenaline más allá del layout de archivos.
- Reimplementar los scripts Python en las fases 1–5 (se evalúa como extra F6; no son dependencia de C).
- Multi-plataforma "exótico" (wasm, Android nativo, BSD) salvo que salga gratis.
- Cambiar la licencia (el proyecto es *Unlicense*; los crates MIT/Apache son compatibles).

### 2.3 Criterios de éxito medibles

1. Para cada caso del corpus: `diff -r` de la salida descomprimida y comparación de SHA-256 por archivo.
2. `pkg2zip -l` byte-idéntico al binario C en el corpus.
3. ZIP válido según `unzip -t`, `7z t` y Python `zipfile`; ISO válido según lectura de sector 16 (PVD).
4. CSO: cabecera e índice válidos y contenido idéntico tras descomprimir con `maxcso`/nuestro inflate.
5. Tiempo total en un PKG sintético de 4 GiB ≤ 105 % del C en el mismo hardware (1 hilo).
6. Pico de RSS < 100 MB procesando el PKG de 4 GiB.
7. `cargo build --release --locked` funciona en Windows, Linux y macOS sin toolchain de C.
8. Los 6+ targets publican artefactos con `SHA256SUMS` en un release de prueba.

---

## 3. Arquitectura objetivo

### 3.1 Layout del repositorio

```text
pkg2zip/
├── Cargo.toml                  # workspace + [workspace.package] + lints
├── Cargo.lock
├── rust-toolchain.toml         # channel = "stable", components fmt/clippy
├── rustfmt.toml
├── clippy.toml
├── README.md                   # reescrito para Rust
├── LICENSE                     # Unlicense (sin cambios)
├── CHANGELOG.md
├── docs/
│   ├── PLAN-MIGRACION-RUST.md  # este documento
│   ├── adr/ADR-001..010.md     # decisiones
│   ├── paridad.md              # matriz y protocolo de paridad
│   └── perf-baseline.md        # números C de referencia
├── crates/
│   ├── pkg2zip-core/           # librería: todo el dominio, sin CLI
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── bytes.rs            # get/set LE/BE, min32/64
│   │   │   ├── error.rs            # Error + ErrorKind + Display
│   │   │   ├── crypto/{mod,aes.rs,psp.rs,cmac.rs}   # aes/ctr reexport, PSP-AES, CMAC
│   │   │   ├── checksum/{mod,crc32.rs,adler32.rs}
│   │   │   ├── zrif/{mod,puff.rs}  # base64 + inflate con diccionario + adler
│   │   │   ├── pkg/{mod,header.rs,meta.rs,sfo.rs,kind.rs,naming.rs,region.rs}
│   │   │   ├── vita/{mod,layout.rs,theme.rs,psm.rs,head_tail.rs}
│   │   │   ├── psp/{mod,eboot.rs,lzrc.rs,edat.rs,keys.rs,theme.rs}
│   │   │   ├── output/{mod,zip.rs,sink.rs}
│   │   │   └── plat/{mod,file.rs,console.rs,progress.rs}
│   │   ├── tests/                  # integración con fixtures generados
│   │   └── benches/                # criterion
│   ├── pkg2zip-cli/            # binario `pkg2zip`
│   │   └── src/{main.rs,args.rs,ui.rs}
│   └── pkg2zip-testkit/        # dev-only: generador de PKG sintéticos y comparador
│       └── src/{lib.rs,pkg_builder.rs,psp_builder.rs,sfo_builder.rs,compare.rs}
├── legacy/                     # C congelado (F0..F5; se borra en F5)
│   ├── *.c, *.h, makefile, build.cmd
│   └── README.md               # "solo referencia; no forma parte del producto"
├── scripts/
│   ├── build-legacy.ps1 / .sh      # localiza MSVC/gcc, compila el C de referencia
│   ├── parity.ps1 / .sh            # ejecuta C y Rust sobre el corpus y compara
│   └── gen-corpus.ps1 / .sh        # invoca pkg2zip-testkit
└── .github/workflows/{ci.yml,release.yml,audit.yml}
```

**Razón de 3 crates:** `core` es testeable sin CLI ni sistema de archivos real (sinks en memoria);
`cli` es una cáscara fina; `testkit` puede generar fixtures desde tests y desde un binario de
desarrollo sin contaminar dependencias del producto.

### 3.2 Crates y responsabilidades

| Crate | Responsabilidad | Dependencias |
|---|---|---|
| `pkg2zip-core` | Formatos, cripto, checksums, zRIF, ZIP, LZRC, layouts, IO y consola | `aes`, `ctr`, `cipher`, `cmac`, `crc32fast`, `miniz_oxide`, `thiserror`; opcional `rayon` (feature `parallel`) |
| `pkg2zip-cli` | Parseo de argumentos, mensajes, progreso, códigos de salida | `pkg2zip-core` |
| `pkg2zip-testkit` | Construcción de PKG sintéticos (Vita/PSP/PSX/PSM/tema/PS3), SFO, comparación de árboles | `pkg2zip-core` (reutiliza cripto para cifrar), `sha2`, `tempfile`, `walkdir` |

### 3.3 Dependencias (versiones verificadas en crates.io el 2026-11-20)

| Crate | Versión | Uso | ¿Rust puro? |
|---|---|---|---|
| `aes` | 0.9.4 | AES-128 ECB/CTR; AES-NI en runtime | Sí |
| `ctr` | 0.10.1 | Modo CTR con contador BE de 128 bits | Sí |
| `cipher` | 0.5.2 | Traits `BlockEncrypt/BlockDecrypt/KeyInit` | Sí |
| `cmac` | 0.8.0 | CMAC RFC 4493 (PSAR/EDAT) | Sí |
| `crc32fast` | 1.5.5 | CRC-32 IEEE con PCLMULQDQ + `Hasher::combine` | Sí |
| `miniz_oxide` | 0.9.1 | Deflate crudo por sector para CSO | Sí |
| `thiserror` | 2.0.21 | Errores tipados | Sí |
| `rayon` (opcional) | 1.12.0 | Compresión CSO en paralelo (F6) | Sí |
| `base64` (opcional) | 0.23.1 | Decodificación zRIF; alternativa: 40 líneas propias | Sí |
| `windows-sys` | fijar en F0 | `SetConsoleOutputCP`, `GetConsoleMode` en Windows | Sí (bindings, sin C) |

**Prohibido:** `libz-sys`, `libz-ng-sys`, `zlib` (C), `cc`/`cmake` en `build.rs`, `ring` con C, y
cualquier `*-sys` que compile C. Regla de CI:
`cargo tree -e normal --prefix none | grep -E '\-sys$' | grep -v '^windows-sys'` debe estar vacío.

Notas de compatibilidad:
- `aes` 0.9 + `ctr` 0.10 + `cmac` 0.8 comparten `cipher` 0.5; verificar en el spike de F1 y fijar
  versiones exactas en `Cargo.toml` (`=0.9.4`, etc.) para builds reproducibles.
- `flate2::Decompress::set_dictionary` **solo existe con backend zlib en C** (`any_c_zlib`), por eso
  zRIF no se resuelve con `flate2`: se porta `puff` (ver ADR-04).

### 3.4 Flujo de datos

```text
args ──► Args::parse ──► Config
                           │
                           ▼
   File::open(pkg) ──► PkgHeader::parse (192 + 64 bytes)
                           │  meta blocks (2,10,13,14) → content_type, sfo, items, install_dir
                           ▼
                 key schedule (psp/ps3/vita2/3/4) ──► PkgKey
                           │
              kind = detect(content_type, sfo.category)
                           │
        ┌──────────────────┴───────────────────┐
        ▼                                      ▼
  Sfo::parse (título, CONTENT_ID, APP_VER)   PspSfo::find / PbpSfo::find
        │                                      │
        └────────────► naming.rs ◄─────────────┘   (nombre de zip, región, layout)
                           │
                           ▼
                 OutputSink::begin(root, zipped)
                           │
        ┌──────────────────┴───────────────────┐
        ▼                                      ▼
  ZipSink (ZIP64, stored)              FsSink (mkdir/write_at)
        │                                      │
        └────────── item loop (AES-CTR) ───────┘
                           │
        ┌──────────────────┴───────────────────┐
        ▼                                      ▼
  extras: work.bin / head/tail/stat     psp: eboot→iso/cso, edat, keys
        │
        ▼
   OutputSink::finish
```

### 3.5 Modelo de errores e IO

- `enum Error` con `thiserror`, variantes por dominio (`NotAPkg`, `Truncated`, `UnsupportedContent`,
  `ZrifMismatch`, `Io { path, source }`, `Corrupt { what }`, ...). `Display` reproduce los mensajes
  actuales (`"ERROR: ..."` se añade en la capa CLI una sola vez, no en cada módulo).
- Sin `panic`/`unwrap` en rutas de datos: solo `expect` en invariantes imposibles, con `#[cold]`.
- `plat::file::File` encapsula `read_at`/`write_at` (`FileExt::seek_read/seek_write` en Windows,
  `read_at/write_at` en Unix), `open`, `create`, `len`, `sync`. Todo acceso valida rangos antes de
  leer (mejora sobre C, que confía en el tamaño del archivo).
- `plat::console` abstrae stdout/stderr: en Windows fija `CP_UTF8` con `windows-sys` y usa
  `WriteConsoleW` cuando es consola (equivalente al C); si está redirigido, escribe bytes UTF-8.
- `plat::progress` mantiene el contrato `[*] unpacking... N%\r` solo en TTY, con `N` calculado igual
  (`progress * 100 / total`).

---

## 4. Decisiones técnicas (ADRs)

> Formato corto: **Contexto → Decisión → Consecuencias**. Cada una se materializa en
> `docs/adr/ADR-00N-*.md` durante F0.

### ADR-01 — Workspace de 3 crates en lugar de un solo crate
**Contexto:** el código C mezcla dominio, IO y CLI; los tests necesitan generar PKG (requiere
cripto de cifrado) y comparar salidas.
**Decisión:** `pkg2zip-core` (lib), `pkg2zip-cli` (bin), `pkg2zip-testkit` (dev).
**Consecuencias:** compila más rápido por crate, tests de core sin FS real, y el testkit no añade
dependencias al binario. Coste: gestión de versiones internas trivial (path deps).

### ADR-02 — Cripto con RustCrypto, sin `unsafe` propio
**Contexto:** el C tiene rutas AES-NI y software; queremos el mismo rendimiento sin mantener SIMD.
**Decisión:** `aes` + `ctr` + `cmac`; la variante PSP se implementa sobre `aes::Aes128` (ECB decrypt)
con el encadenamiento del código C (contador LE en los últimos 4 bytes).
**Consecuencias:** AES-NI/PCLMUL se seleccionan en runtime; `#![deny(unsafe_code)]` en `core`, con una
única excepción auditada en `plat/console` para las llamadas `unsafe` de `windows-sys`.
Verificación: vectores FIPS-197, RFC 4493 y comparación con el binario C.

### ADR-03 — CRC-32 con `crc32fast` y `combine` nativo
**Contexto:** se necesita CRC incremental por entrada ZIP y `crc32_combine` para el CSO (CRC de
cabecera+índice combinado con el CRC de los datos comprimidos).
**Decisión:** `crc32fast::Hasher::combine` (semántica `crc(a||b)`), validado contra vectores del C;
si la firma requiere longitud del primer bloque, usar `new_with_initial_len`.
**Consecuencias:** rendimiento PCLMULQDQ y menos código; se conserva un test que compara los 4
vectores de `crc32_combine` del C.

### ADR-04 — Portar `puff` para zRIF (en vez de `flate2`/`zlib-rs`)
**Contexto:** zRIF es un flujo zlib deflate comprimido con un **diccionario preestablecido** de
1.024 bytes (`0x627d1d5d`). `flate2::Decompress::set_dictionary` solo está disponible con backend
zlib en C (`any_c_zlib`); la API de alto nivel de `zlib-rs` 0.6 no expone diccionario en
`InflateConfig`; `miniz_oxide` no lo soporta.
**Decisión:** portar `puff.c` (433 líneas, dominio público, ya modificado en este repo) a Rust seguro,
con firma `inflate_with_dict(dict: &[u8], input: &[u8], capacity: usize) -> Result<Vec<u8>>`.
**Consecuencias:** cero dependencias nuevas, paridad exacta, código auditable y testeable con zRIF
reales. Riesgo: bugs de porting → mitigado con vectores de zRIF conocidos y fuzzing diferencial
contra `zlib` de Python (solo en tests, no en producto).
**Alternativa descartada:** `libz-rs-sys` (API zlib en Rust puro) — funcionaría, pero añade una capa
FFI/ABI y no aporta nada que el port no dé. Reevaluar solo si el port se desvía del cronograma.

### ADR-05 — Escritor ZIP propio
**Contexto:** el formato exacto (orden de entradas, atributos DOS, ZIP64, CRC diferido, escritura en
offset para el CSO) es parte del contrato; los crates de ZIP no permiten escribir en offsets
arbitrarios ni fijar el CRC después.
**Decisión:** portar `pkg2zip_zip.c` a un `ZipWriter` con `begin_file/write/end_file/close`,
`write_at`, `set_offset`, `set_crc32`.
**Consecuencias:** bytes equivalentes, control total. Se valida con `unzip -t`/`7z t`/`zipfile` y con
un caso > 4 GiB para ZIP64 (sparse file en tests).

### ADR-06 — Parser de CLI propio
**Contexto:** la CLI tiene rarezas (`-cN` pegado, argumento desconocido como ruta, `-l` implica
`-q`, `-c` sin número = ISO, `zRIF` posicional opcional que puede ser `.bin`).
**Decisión:** parser manual de ~120 líneas con tabla de flags y tests de tabla.
**Consecuencias:** paridad exacta y mensajes idénticos; sin dependencia de `clap`. Si en el futuro se
quiere `--help` generado, se evalúa `clap` en un modo compatible (no en F0..F5).

### ADR-07 — Sin `unsafe` en el dominio
**Contexto:** el C tiene overflow potencial (`name[name_size] = 0`, `strcpy` de títulos, `fread`
sin comprobar `fopen`).
**Decisión:** `#![deny(unsafe_code)]` en `pkg2zip-core` y `pkg2zip-cli`; toda lectura valida
longitudes y devuelve `Result`. Única excepción: `plat/console` (Windows), donde se necesita
`unsafe` para `SetConsoleOutputCP`/`GetConsoleMode`; se aísla en un módulo `#[cfg(windows)]`
documentado y con `#[allow(unsafe_code)]` local.
**Consecuencias:** robustez y fuzzing productivo; ligeramente más código de comprobación (coste
despreciable frente a IO).

### ADR-08 — Paridad semántica, no necesariamente byte a byte, en CSO
**Contexto:** el deflate de `miniz_tdef` con `MZ_FIXED` no coincide necesariamente con
`miniz_oxide`; el formato CSO solo exige un flujo deflate válido por sector y un índice correcto.
**Decisión:** comparar el ISO resultante y la validez del índice, no los bytes comprimidos; exigir
tamaño ≤ 102 % del C. Para ZIP (stored) sí se exige equivalencia byte a byte salvo fechas.
**Consecuencias:** evita bloquear la migración por diferencias de compresor; se documenta en
`docs/paridad.md`.

### ADR-09 — `legacy/` congelado durante la migración
**Contexto:** se necesita el binario C como oráculo de paridad, pero el objetivo es eliminarlo.
**Decisión:** mover el C a `legacy/` en F0, sin cambios funcionales (solo rutas de build), y borrarlo
en F5. El CI compila `legacy/` solo en el job de paridad.
**Consecuencias:** `grep` limpio, historia clara; los merges de `upstream` se congelan durante la
migración (se documenta el último commit sincronizado).

### ADR-10 — Versionado 3.0.0 y compatibilidad de CLI
**Contexto:** una reescritura total justifica un major, pero romper la CLI afectaría a scripts.
**Decisión:** `pkg2zip 3.0.0` con CLI y salidas compatibles; los mensajes nuevos (p. ej. contexto de
error) van a stderr solo cuando aportan, sin cambiar el flujo de stdout. `--version` nuevo (aditivo).
**Consecuencias:** los usuarios pueden cambiar el binario sin tocar scripts; el changelog documenta
diferencias (mejoras de robustez y mensajes).

---

## 5. Mapeo C → Rust

| C (archivo/funciones) | Rust (módulo) | Notas de porting |
|---|---|---|
| `pkg2zip.c:main` | `cli/src/main.rs` + `core/pkg/mod.rs` | Separar orquestación de parseo; `run(config) -> Result<Outcome>` |
| `parse_sfo_content`, `parse_sfo` | `core/pkg/sfo.rs` | Parser con límites; devolver `Sfo { title, content_id, category, app_ver, min_ver, disc_id }`; sanitización de título byte a byte (misma tabla `<>\"/\\|?*`, `:` → `" -"`, `\n` → espacio, ≥128 se conserva, tope 255) |
| `find_psp_sfo`, `find_pbp_sfo` | `core/psp/eboot.rs` | Iterar items cifrados con AES-CTR y offsets BE |
| `get_region` | `core/pkg/region.rs` | Tabla de prefijos `&[(&[u8;4], Region)]` con el mismo orden |
| cabecera + meta blocks | `core/pkg/header.rs`, `core/pkg/meta.rs` | `PKG_HEADER_SIZE=192`, ext 64, `meta_offset`/`meta_count`, tipos 2/10/13/14 |
| `pkg_type` + ramas de layout | `core/pkg/kind.rs`, `core/vita/layout.rs` | `enum Kind` con 9 variantes; `naming::root_name(kind, ...)` |
| `out_*` | `core/output/sink.rs` | `trait Sink { add_folder, add_parent, begin_file, write, write_at, set_offset, end_file, finish }`; implementaciones `ZipSink` y `FsSink` |
| `zip_*` | `core/output/zip.rs` | ZIP64, atributos, CRC diferido, `name`/`slash` |
| `sys_*` | `core/plat/file.rs`, `console.rs`, `progress.rs` | `read_at`/`write_at`, `mkdir` recursivo, UTF-8 en consola |
| `aes128_*` | `core/crypto/aes.rs` | Reexportar `aes::Aes128` + `ctr::Ctr128BE`; helper `ctr_xor(key, iv, block_offset, buf)` |
| `aes128_psp_decrypt` | `core/crypto/psp.rs` | Contador LE en bytes 12..16; `prev` inicial = ceros o IV+índice |
| `aes128_cmac` | `core/crypto/cmac.rs` | `cmac::Cmac<Aes128>` |
| `aes128_ecb_*` | `core/crypto/aes.rs` | `encrypt_block`/`decrypt_block` |
| `crc32_*`, `crc32_combine` | `core/checksum/crc32.rs` | `crc32fast` + `combine`; exponer `Crc32` con `update/finish/set` |
| `zrif_decode`, `zlib_inflate`, `base64_decode` | `core/zrif/mod.rs` | Base64 indulgente (sin padding), parseo de cabecera zlib (FCHECK, FDICT, DICTID), `puff` port, Adler-32 |
| `rif_load` | `core/zrif/mod.rs` | Leer exactamente `rif_size` bytes; error si el archivo no existe (mejora sobre C) |
| `unpack_psp_eboot` | `core/psp/eboot.rs` | PBP → PSAR (`NPUMDIMG`), tabla de bloques (8×u32, xor de claves), `iso_block`, ISO/CSO |
| `lzrc_decompress` | `core/psp/lzrc.rs` | Range decoder con probabilidades; mismo estado inicial (`0x80`), `lc & 0x80` = texto plano |
| `unpack_psp_key`, `unpack_psp_edat`, `unpack_keys_bin` | `core/psp/edat.rs`, `core/psp/keys.rs` | PGD (`\x00PGD`), key/drm type 1, MAC, `init_psp_decrypt`, offsets |
| `get_psp_theme_title` | `core/psp/theme.rs` | PTF (`\x00PTF`), título en +8 (128 bytes) |
| PDB bgdl y `head/tail/stat.bin` | `core/vita/theme.rs`, `core/vita/head_tail.rs` | Construcción byte a byte con los mismos offsets mágicos |
| PSM layout + Android | `core/vita/psm.rs` | Prefijo `content/` (9 bytes), `RO`/`RW`, `runtime` → raíz |
| `sys_output_progress*` | `core/plat/progress.rs` | Solo TTY, mismo formato y frecuencia |
| `rif2zrif.py` / `zrif2rif.py` | (F6) `pkg2zip rif` subcomandos | Deflate con diccionario: `libz-rs-sys` o port; **no** bloquea O1..O6 |

**Tamaño esperado:** ~4.500–5.500 líneas Rust (incluye tests unitarios), comparable al C propio.

---

## 6. Estrategia de paridad (la red de seguridad)

### 6.1 Contrato de compatibilidad

| Dimensión | Exigencia |
|---|---|
| Nombres de salida | Idénticos (incluye `%.9s`, región, sufijos, `.zip`) |
| Estructura interna | Idéntica (rutas exactas, carpetas creadas, `head/tail/stat/work.bin`) |
| Contenido | SHA-256 por archivo idéntico |
| ZIP | Entradas y CRC idénticos; bytes idénticos salvo campos de fecha/hora (se fijan en tests) |
| ISO | Byte a byte idéntico |
| CSO | Índice válido + ISO descomprimido idéntico; tamaño ≤ 102 % del C |
| stdout/stderr | Mismos mensajes y orden; `-l` byte a byte; progreso solo TTY |
| Códigos de salida | `0` éxito, `1` error (incluido `-h` → `0`) |

### 6.2 Corpus sintético (`pkg2zip-testkit`)

No hay PKG de muestra en el repositorio, así que el corpus se **genera**. El testkit reutiliza la
cripto de `core` para producir PKG válidos y controlar cada rama:

| Fixture | Cubre |
|---|---|
| `vita_app`, `vita_patch`, `vita_patch_by_category` | SFO, `app/`, `patch/`, versión |
| `vita_dlc` | `addcont/<ID>/<DLC>` y `install_directory` (meta tipo 10) |
| `vita_theme` | `bgdl/t/NNNNNNNN`, `d0/d1/f0.pdb`, `-b` |
| `vita_psm`, `vita_psm_android` | `psm/...`, `RO/RW`, `runtime`, `-a` |
| `psp`, `psp_mini`, `psp_go`, `psp_neo` | `NPUMDIMG`, ISO/CSO, `-p`, `-c0..9` |
| `psx` | `EBOOT.PBP`, `KEYS.BIN`, `DOCUMENT.DAT`, `texture.enc`, `.rif` |
| `psp_theme` | PTF + EDAT |
| `ps3` | `PKG_TYPE_PS3` y layout `pspemu/PSP/GAME` |
| `edge_*` | título con `:`/`\n`/UTF-8, nombre largo (1023), archivo vacío, directorio vacío, `digs.bin`/`cert.bin`, `.edat`, DLC con `PSP-KEY.EDAT`, zRIF `.bin`, `content_id` distinto (error) |

**Truco para LZRC sin compresor:** el decodificador LZRC soporta modo "texto plano" cuando el primer
byte tiene el bit 0x80 (`memcpy(out, input+5, code)`). Un fixture puede marcar el bloque como texto
plano y ejercitar la ruta completa de ISO/CSO sin implementar un compresor. Para la ruta de
"matches" se añade un encoder mínimo de literales (solo `rc_bit` con literales), factible en ~150
líneas, y vectores capturados de un PKG real aportado por el mantenedor (no versionado).

**Fixture > 4 GiB:** archivo disperso (sparse) con un item de 4,1 GiB y contenido mayormente ceros,
para ZIP64 y `head/tail` (se marca `#[ignore]` y se ejecuta en CI nocturno por disco/tiempo).

### 6.3 Pruebas diferenciales C vs Rust

```text
scripts/parity.(ps1|sh)
  ├─ compila legacy/ con MSVC (Windows) o cc (Linux/macOS CI)
  ├─ genera corpus con `cargo run -p pkg2zip-testkit -- gen fixtures/`
  └─ por cada (fixture, flags) de la matriz:
       ├─ ejecuta legacy en out_c/, rust en out_rs/
       ├─ compara: exit code, stdout normalizado, árbol de archivos, SHA-256, ZIP entries+CRC
       └─ escribe reporte JSON; falla si hay diferencias
```

Matriz mínima (flags × tipo): `""`, `-x`, `-l`, `-q`, `-b`, `-p`, `-d`, `-c1`, `-c9`, `-a`,
combinaciones `-x -c9`, `-b -x`, y con/sin `zRIF`. Total ≈ 60–90 ejecuciones por corrida.

### 6.4 Vectores y pruebas unitarias

| Área | Vector/prueba |
|---|---|
| AES-128 | FIPS-197 (Apéndice C), ECB encrypt/decrypt |
| CTR | NIST SP 800-38A F.5.1 (AES-128-CTR) con contador BE |
| CMAC | RFC 4493 (4 casos) |
| PSP-AES | Comparar contra el C sobre 10 bloques con IV/índice fijos (salida del binario legacy capturada como golden) |
| CRC-32 | `"123456789"` → `0xCBF43926`; 4 vectores de `combine` |
| Adler-32 | `"Wikipedia"` → `0x11E60398` |
| zRIF | ≥ 5 cadenas reales conocidas + su `work.bin` esperado (verificar CONTENT_ID) |
| SFO | SFO sintético con/sin `STITLE`, `TITLE`, `DISC_ID`, `APP_VER`, `PSP2_DISP_VER` |
| Region | Tabla completa de prefijos (casos positiva/negativa) |
| Naming | Golden de los 9 tipos × región × sufijos |
| ZIP | Round-trip con `zipfile` de Python; ZIP64 con > 65.535 entradas (test rápido) |
| LZRC | Texto plano + matches con encoder mínimo |
| CSO | Round-trip ISO → CSO → ISO idéntico; índice con bloques no comprimidos |
| PDB/head/tail/stat | Golden de bytes con offsets esperados |

### 6.5 Fuzzing y robustez

- `cargo fuzz` con targets: `pkg_header`, `sfo`, `zrif`, `lzrc`, `edat_header`, `zip_writer` (entrada
  semántica). Objetivo: 24 h sin panic/OOM/timeout.
- Límites: tamaño de SFO ≤ 16 KiB, nombre ≤ 1023, `item_count` acotado por tamaño del archivo,
  reservas con `try_reserve` y error `TooLarge` en vez de abortar.
- Comparación diferencial de zRIF contra `zlib` de Python en tests (no en el binario).

### 6.6 Rendimiento

- `criterion` sobre: AES-CTR (1 GiB), decodificación LZRC (PKG real o sintético grande), escritura
  ZIP (1 GiB), compresión CSO por sector (100 MB), `crc32` (1 GiB).
- End-to-end con `hyperfine` sobre el PKG de 4 GiB: C vs Rust, 5 corridas, mediana.
- Presupuesto: ≤ 105 % del C en `-x` y `-c`; ≤ 100 % en ZIP (stored); F6 busca 1,3–2× en CSO con
  `rayon` (sectores independientes) y CRC paralelo por bloques.

### 6.7 Matriz de validación por tipo

| Tipo | Zip | `-x` | `-l` | `-q` | Extras |
|---|---|---|---|---|---|
| Vita APP | ✔ | ✔ | ✔ | ✔ | work/head/tail/stat |
| Vita PATCH | ✔ | ✔ | ✔ | ✔ | head/tail/stat |
| Vita DLC | ✔ | ✔ | ✔ | ✔ | work.bin, `install_directory` |
| Vita PSM | ✔ | ✔ | ✔ | ✔ | RO/RW, pm.dat, content_id, `-a` |
| Vita THEME | ✔ | ✔ | ✔ | ✔ | PDB, `-b` |
| PSP | ✔ | ✔ | ✔ | ✔ | ISO/CSO, `-p`, `-d` |
| PSP THEME | ✔ | ✔ | ✔ | ✔ | EDAT |
| PSX | ✔ | ✔ | ✔ | ✔ | KEYS.BIN, `.rif` |
| PS3 | ✔ | ✔ | ✔ | ✔ | KEYS.BIN, DOCUMENT.DAT |

---

## 7. Fases de ejecución

> Cada fase termina en un PR a `develop` con CI verde y la compuerta indicada. Ninguna fase borra
> el C ni cambia `master`.

### F0 — Preparación y red de seguridad (2 días)

- [ ] Crear `develop` desde `master` (hecho) y proteger la rama en GitHub (PR obligatorio, CI).
- [ ] Mover C a `legacy/` (sin cambios funcionales); actualizar `makefile`/`build.cmd` a rutas nuevas.
- [ ] `scripts/build-legacy.ps1|sh`: localizar VS con `vswhere` (o `cc`) y compilar
      `legacy/pkg2zip` en `target/legacy/pkg2zip`.
- [ ] Workspace `Cargo.toml` vacío con los 3 crates y `rust-toolchain.toml`; `cargo build` verde.
- [ ] `pkg2zip-testkit` mínimo: construye un `vita_app` sintético; comando `gen`.
- [ ] `scripts/parity.ps1|sh` con la matriz y reporte JSON; primera corrida contra `legacy/`
      (debe fallar por ausencia de Rust: se marca `--expect-missing`).
- [ ] CI `ci.yml`: fmt, clippy, test, build release en ubuntu/windows/macos; job `legacy-parity`
      que compila C y ejecuta el corpus (aún sin Rust, verifica el arnés).
- [ ] Congelar sincronización con `upstream` (registrar commit en `docs/paridad.md`).
- [ ] ADR-001..010 en `docs/adr/`.

**Compuerta F0:** `cargo build` y `cargo test` verdes; `parity` corre el C sobre el corpus y produce
reporte; CI verde en 3 SO.

### F1 — Núcleo criptográfico y de formatos (4 días)

- [ ] `bytes.rs` (get/set LE/BE, `min32/64`), `error.rs`.
- [ ] `crypto`: AES ECB/CTR, `ctr_xor`, CMAC, PSP-AES; tests FIPS/NIST/RFC + golden vs C.
- [ ] `checksum`: CRC-32 + `combine`; Adler-32.
- [ ] `zrif`: base64, cabecera zlib, `puff` port, validación y `rif_load`.
- [ ] `pkg/sfo.rs` + `pkg/header.rs` + `pkg/meta.rs` + `pkg/kind.rs` + `region.rs`; tests de tabla.
- [ ] Benchmarks iniciales de AES/CRC (comparar con C).

**Compuerta F1:** todos los vectores pasan; `cargo test -p pkg2zip-core` ≥ 90 % cobertura en
`crypto`, `checksum`, `zrif`, `pkg/sfo`; un zRIF real decodifica a su `work.bin` esperado.

### F2 — Salida, CLI y ruta Vita (5 días)

- [ ] `output/zip.rs` (ZIP64, stored, CRC diferido, `write_at`, `set_offset`), `sink.rs`.
- [ ] `plat/file.rs`, `plat/console.rs`, `plat/progress.rs`.
- [ ] `cli`: parser compatible + mensajes + códigos de salida; `--version` aditivo.
- [ ] `pkg/naming.rs` (9 tipos, región, sufijos, `-l`).
- [ ] Flujo Vita APP/PATCH/DLC/THEME: items, carpetas, `digs.bin`/`cert.bin` → `body.bin`,
      `work.bin`, `head/tail/stat.bin`, PDB bgdl.
- [ ] Paridad end-to-end de los 4 tipos Vita en zip y `-x`.

**Compuerta F2:** `parity` sin diferencias para los fixtures Vita en `""`, `-x`, `-l`, `-q`, `-b`;
ZIP validado por `unzip -t`; `-l` byte a byte.

### F3 — PSP, PSX y PS3 (6 días)

- [ ] `psp/eboot.rs`: PBP, PSAR `NPUMDIMG`, tabla (xor), `init_psp_decrypt`, ISO.
- [ ] `psp/lzrc.rs` + tests (texto plano, matches con encoder mínimo, vectores reales).
- [ ] CSO: cabecera 24 bytes, índice `u32` con bit `0x80000000`, compresión por sector con
      `miniz_oxide`, CRC combinado (ADR-03), `-c0..9`.
- [ ] `psp/edat.rs`, `psp/keys.rs`, `psp/theme.rs`.
- [ ] PSX (EBOOT, KEYS.BIN, DOCUMENT.DAT, texture.enc, licencia `.rif`) y PS3 (`pspemu/PSP/GAME`).
- [ ] `-p`, `-d`.

**Compuerta F3:** ISO byte a byte idéntico; CSO válido y contenido idéntico; EDAT/KEYS.BIN
idénticos; paridad completa en PSP/PSX/PS3.

### F4 — PSM, bordes y barrido de paridad (3 días)

- [ ] PSM (`0x18`/`0x1d`), `RO/RW`, `runtime`, `content_id`, `pm.dat`, `-a` (Android).
- [ ] Tema Vita bgdl completo (PDB) y `-b`.
- [ ] Casos límite: títulos UTF-8/`:`/`\n`, nombres de 1023, archivos vacíos, `install_directory`,
      `zRIF .bin`, `content_id` distinto, PKG truncado, firma inválida, tipo no soportado.
- [ ] Barrido total de la matriz (≈ 90 corridas) y `docs/paridad.md` con resultados.

**Compuerta F4:** matriz completa sin diferencias; todos los errores devuelven código 1 con mensaje
equivalente; sin `panic`.

### F5 — Multiplataforma, documentación y retirada del C (3 días)

- [ ] `release.yml`: matriz de targets, artefactos, `SHA256SUMS`, smoke tests por SO.
- [ ] `README.md` reescrito (instalación con `cargo install`, binarios, uso, diferencias con 2.6).
- [ ] `CHANGELOG.md` 3.0.0; `CONTRIBUTING.md`; plantilla de PR.
- [ ] Borrar `legacy/`, `makefile`, `build.cmd`, `build32.cmd`, `.travis.yml`, `.appveyor.yml`,
      `cmake-multi-platform.yml`, `flake.nix` (o reescribir el flake para Rust).
- [ ] `cargo deny`, `cargo audit`, `cargo machete` en CI; MSRV declarada.
- [ ] Tag `v3.0.0`; merge `develop → master`; archivar el C en tag `v2.6-c-final`.

**Compuerta F5 (Definition of Done):** ver §12.

### F6 — Mejoras opcionales (3 días)

- [ ] Paralelismo CSO con `rayon` (feature `parallel`, opt-in o por defecto si es estable).
- [ ] CRC por bloques en paralelo para archivos grandes.
- [ ] Reimplementar `rif2zrif`/`zrif2rif` como subcomandos Rust (deflate con diccionario).
- [ ] `--json`/`--dry-run` (aditivos), página `man`, completions.
- [ ] Perfil `release` con LTO/`codegen-units=1` (medir tamaño/velocidad).

---

## 8. Backlog inicial

| ID | Tarea | Fase | Hecho cuando |
|---|---|---|---|
| P0-01 | Rama `develop` protegida y CI mínimo | F0 | PR de prueba bloqueado sin CI |
| P0-02 | Mover C a `legacy/` y arreglar builds | F0 | `legacy/pkg2zip` compila en Windows |
| P0-03 | `scripts/build-legacy` con `vswhere` | F0 | Encuentra VS2026 y compila |
| P0-04 | Workspace + toolchain + fmt/clippy | F0 | `cargo build`/`test` verdes |
| P0-05 | Testkit: `vita_app` sintético | F0 | PKG generado y abierto por el C |
| P0-06 | Arnés `parity` + reporte JSON | F0 | Detecta diferencias simuladas |
| P0-07 | ADR-001..010 | F0 | Revisadas y fusionadas |
| P1-01 | `bytes`/`error` | F1 | Tests de límites |
| P1-02 | AES ECB/CTR + `ctr_xor` | F1 | FIPS/NIST + golden C |
| P1-03 | CMAC y PSP-AES | F1 | RFC 4493 + golden C |
| P1-04 | CRC-32 + combine; Adler-32 | F1 | Vectores + property test |
| P1-05 | `puff` port + zRIF | F1 | 5 zRIF reales |
| P1-06 | SFO/header/meta/kind/region | F1 | Tabla de casos |
| P2-01 | `ZipWriter` + ZIP64 | F2 | `unzip -t`, >4 GiB |
| P2-02 | `Sink` zip/FS | F2 | Paridad `-x` vs zip |
| P2-03 | `plat` IO/consola/progreso | F2 | UTF-8 en consola Windows |
| P2-04 | CLI compatible | F2 | Tabla de flags y `-h` |
| P2-05 | `naming` + layouts Vita | F2 | `-l` byte a byte |
| P2-06 | Extras Vita (work/head/tail/stat) | F2 | SHA-256 idéntico |
| P2-07 | Tema bgdl + PDB | F2/F4 | Golden de bytes |
| P3-01 | EBOOT/PSAR → ISO | F3 | ISO idéntico |
| P3-02 | LZRC | F3 | Texto plano + matches |
| P3-03 | CSO + CRC combinado | F3 | Round-trip idéntico |
| P3-04 | EDAT/KEYS.BIN/PSP-Theme | F3 | SHA-256 idéntico |
| P3-05 | PSX/PS3 | F3 | SHA-256 idéntico |
| P4-01 | PSM + Android | F4 | Layout idéntico |
| P4-02 | Casos límite y errores | F4 | Sin panic, código 1 |
| P4-03 | Barrido de matriz | F4 | 0 diferencias |
| P5-01 | `release.yml` 6+ targets | F5 | Artefactos + SHA256SUMS |
| P5-02 | README/CHANGELOG/docs | F5 | Revisados |
| P5-03 | Borrar C y build solo-Rust | F5 | `cargo build` en limpio |
| P5-04 | `cargo deny/audit/machete` | F5 | CI verde |
| P5-05 | Tag 3.0.0 y merge a master | F5 | Release publicado |
| P6-01 | Paralelismo CSO/CRC | F6 | ≥ 1,3× con mismos bytes |
| P6-02 | `rif2zrif`/`zrif2rif` en Rust | F6 | Round-trip con Python |

---

## 9. CI/CD multiplataforma

### 9.1 CI de PR (`ci.yml`)

| Job | Runner | Contenido |
|---|---|---|
| `fmt` | ubuntu-latest | `cargo fmt --check` |
| `clippy` | ubuntu-latest | `cargo clippy --all-targets -- -D warnings` |
| `test` | ubuntu, windows, macos-14 | `cargo test --locked` (unit + integración + testkit) |
| `msrv` | ubuntu-latest | Compila con la MSRV declarada (`rust-version`) |
| `parity` | ubuntu-latest | Compila `legacy/` y ejecuta `scripts/parity.sh` |
| `no-c` | ubuntu-latest | `cargo tree` sin `*-sys` que compilen C (excepción: `windows-sys`); contenedor `rust:slim` con solo Rust |
| `deny` | ubuntu-latest | `cargo deny check` (licencias, advisories, bans) |
| `fuzz-smoke` | ubuntu-latest | 60 s por target (no bloqueante en PR) |

### 9.2 Release (`release.yml`, al empujar tag `v*`)

| Target | Runner | Artefacto |
|---|---|---|
| `x86_64-pc-windows-msvc` | windows-latest | `pkg2zip-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| `aarch64-pc-windows-msvc` | windows-latest (cross; requiere build tools ARM64) | idem `aarch64` |
| `x86_64-unknown-linux-gnu` | ubuntu-latest | `...-linux-gnu.tar.gz` |
| `aarch64-unknown-linux-gnu` | ubuntu-latest (`cross`) | `...-linux-gnu-aarch64.tar.gz` |
| `x86_64-unknown-linux-musl` | ubuntu-latest (`cross`/zigbuild) | estático `...-linux-musl.tar.gz` |
| `x86_64-apple-darwin` | runner Intel vigente (`macos-13` o su reemplazo) | `...-macos-x86_64.tar.gz` |
| `aarch64-apple-darwin` | `macos-14`+ (arm64) | `...-macos-aarch64.tar.gz` |
| `universal2` (macOS) | `macos-14`+ | `lipo` de los dos anteriores (opcional) |

- `SHA256SUMS` + `SHA256SUMS.sig` (cosign o GPG, opcional).
- Smoke test por artefacto: `pkg2zip --version`, `--help`, y un PKG sintético pequeño → mismo
  SHA-256 que el artefacto Linux de referencia (los PKG de prueba son deterministas).
- `cargo-dist` es una alternativa válida si se quiere automatizar instaladores/`curl | sh`; se evalúa
  en F5 y no cambia el plan de targets.

### 9.3 Versionado

- Semver: 3.0.0 en la primera release con paridad; parches 3.0.x para fixes.
- `pkg2zip --version` imprime `pkg2zip 3.0.0`; el `-h` conserva el banner `pkg2zip v3.0.0`.
- `Cargo.lock` versionado (binario) y `--locked` en CI.
- Canales: `v3.0.0-rc.N` para pruebas con usuarios antes del tag final.

---

## 10. Riesgos y mitigaciones

| ID | Riesgo | Prob. | Impacto | Mitigación |
|---|---|---|---|---|
| R1 | El port de `puff` introduce un bug sutil en zRIF | Media | Alto | Vectores reales, fuzzing diferencial vs `zlib`/Python, revisión línea a línea |
| R2 | LZRC no se puede validar sin PKG real | Alta | Medio | Modo texto plano + encoder mínimo de literales; pedir 1–2 PKG de muestra al mantenedor; tests golden del C sobre vectores capturados |
| R3 | Diferencias de deflate rompen la paridad byte a byte del CSO | Alta | Bajo | ADR-08: comparar ISO y validez, no bytes; tolerancia de tamaño |
| R4 | Nombres/rutas no idénticos (UTF-8, `:`, truncado, `%.9s`) | Media | Alto | Módulo `naming` con golden por tipo/región; comparar `-l` byte a byte |
| R5 | ZIP64 mal implementado (>4 GiB, >65.535 entradas) | Media | Alto | Test con archivo disperso y muchas entradas; validadores externos |
| R6 | Rendimiento inferior al C (AES/CRC/IO) | Media | Medio | Benchmarks desde F1; crates con SIMD; buffering de 64 KiB; medir antes de optimizar |
| R7 | Windows: consola UTF-8, rutas largas, archivos > 2 GiB | Media | Medio | `windows-sys` para CP y `\\?\`; tests en windows-latest; `seek_read/write` |
| R8 | Dependencias con C (`libz-sys`, etc.) entran por transitividad | Baja | Alto | Job `no-c` con `cargo tree`; `cargo deny` bans; build en `rust:slim` |
| R9 | Cambios de API de crates (aes 0.9/ctr 0.10 recientes) | Media | Bajo | Fijar versiones exactas, `Cargo.lock`, spike temprano en F1 |
| R10 | Sin corpus real, la paridad es "sintética" | Alta | Medio | Complementar con PKG aportados por el usuario/mantenedor en `testdata/` (gitignored) y validación manual documentada |
| R11 | El C se borra antes de tiempo y se pierde el oráculo | Baja | Alto | Congelar en `legacy/` + tag `v2.6-c-final`; borrar solo en F5 |
| R12 | Merges de `upstream` durante la migración | Media | Bajo | Congelar sync, registrar commit, resolver después de 3.0.0 |
| R13 | Fuga de licencias (Unlicense vs MIT/Apache) | Baja | Bajo | `cargo deny` allow-list |
| R14 | Coste/tiempo de CI (macOS/Windows) | Media | Bajo | Cache `rust-cache`, matriz mínima en PR, matriz completa solo en release/nightly |

---

## 11. Flujo de trabajo en `develop`

1. **Rama:** `develop` es la rama de integración. `master` queda intacta (C) hasta F5.
2. **Ramas de feature:** `feat/f1-crypto`, `feat/f2-zip`, ... desde `develop`; PR a `develop`.
3. **Commits:** Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`).
   Un commit = un cambio coherente y compilable.
4. **PR:** plantilla con checklist (tests, clippy, docs, paridad si aplica); revisión obligatoria
   (aunque sea self-review documentada) y CI verde. Squash merge.
5. **Definition of done por PR:** `cargo fmt`, `cargo clippy -D warnings`, `cargo test`,
   `scripts/parity` (si toca dominio), docs actualizadas.
6. **Código C:** prohibido añadir features al C durante la migración; solo fixes críticos de
   seguridad, y siempre replicados en Rust.
7. **Sync upstream:** congelado hasta 3.0.0; se documenta el commit base en `docs/paridad.md`.
8. **Release:** en F5, PR `develop → master` con squash, tag `v3.0.0`; el C queda solo en el tag
   `v2.6-c-final` y en la historia.
9. **Protecciones:** `develop` y `master` con PR obligatorio; tags protegidos; Dependabot para crates
   (con `cargo update` revisado).

---

## 12. Criterios de aceptación finales (Definition of Done)

- [ ] **Sin C:** `git ls-files '*.c' '*.h' '*.cpp'` vacío; sin `build.rs` que invoque `cc`/`cmake`;
      `cargo build --release --locked` en un entorno solo-Rust (contenedor `rust:slim`).
- [ ] **Paridad:** 0 diferencias en la matriz completa (≥ 90 ejecuciones) para árboles, hashes,
      nombres, `-l`, stdout/stderr y códigos de salida.
- [ ] **ISO/CSO:** ISO byte a byte; CSO con índice válido y contenido idéntico; tamaño ≤ 102 %.
- [ ] **ZIP:** válido en `unzip -t`, `7z t` y `zipfile`; ZIP64 en >4 GiB y >65.535 entradas.
- [ ] **Robustez:** fuzzing 24 h sin panic/OOM; entradas truncadas → error claro y código 1.
- [ ] **Rendimiento:** ≤ 105 % del C en `-x`/`-c`; sin regresión en ZIP; F6 ≥ 1,3× en CSO.
- [ ] **Multiplataforma:** artefactos Windows/Linux/macOS (x86_64 + aarch64) con `SHA256SUMS`,
      smoke tests ejecutados en cada SO.
- [ ] **Calidad:** `fmt`/`clippy -D warnings` limpios; cobertura ≥ 80 % en módulos de formato;
      `cargo deny`/`audit` verdes; MSRV documentada.
- [ ] **Documentación:** README de Rust, CHANGELOG 3.0.0, ADRs, `docs/paridad.md`,
      `docs/perf-baseline.md`; release notes con diferencias frente a 2.6.
- [ ] **Release:** `v3.0.0` publicado; `master` actualizado; C archivado en `v2.6-c-final`.

---

## Anexo A — Compatibilidad de CLI

| Invocación legacy | Comportamiento esperado (y en Rust) |
|---|---|
| `pkg2zip` (sin args) | stderr: `ERROR: no pkg file specified` + ayuda; exit 1 |
| `pkg2zip -h` / `--help` | stdout: banner `pkg2zip v3.0.0` + ayuda; exit 0 |
| `pkg2zip pkg.pkg` | Crea `<Título> [<ID>] [<Región>].zip` sin `work.bin` |
| `pkg2zip pkg.pkg zRIF` | Igual + `work.bin` validando `CONTENT_ID` |
| `pkg2zip pkg.pkg work.bin` | Carga el `.bin` (tamaño 512 o 1024 según tipo) |
| `pkg2zip -l pkg.pkg` | Imprime solo el nombre del zip; exit 0 |
| `pkg2zip -l -x pkg.pkg` | stderr `ERROR: Listing option without creating zip is useless`; exit 1 |
| `pkg2zip -x pkg.pkg` | Escribe carpeta con la estructura (sin `.zip`) |
| `pkg2zip -c9 pkg.pkg` (PSP) | `pspemu/ISO/<Título> [<ID>].cso` |
| `pkg2zip -c pkg.pkg` (PSP) | Sin número → `.iso` (recorte a 0) |
| `pkg2zip -p pkg.pkg` (PSP) | `pspemu/PSP/GAME/<ID>/EBOOT.PBP` en vez de ISO |
| `pkg2zip -d -p pkg.pkg` | Fuerza descifrado de EDAT/DLC |
| `pkg2zip -b -x pkg.pkg zRIF` (tema) | `app/<ID>` en vez de `bgdl/t/<N>` |
| `pkg2zip -a -x pkg.pkg zRIF` (PSM) | Layout Android (sin `RO/RW`) |
| `pkg2zip -q pkg.pkg` | Silencio total en éxito (errores siguen en stderr) |
| `pkg2zip --desconocido` | Tratado como ruta de PKG (rareza preservada) |

## Anexo B — Mensajes de salida a preservar (extracto)

```text
pkg2zip v3.0.0
[*] loading...
[*] unpacking Vita APP
[*] creating 'Titulo [PCSE00000] [USA].zip' archive
[*] unpacking '...' to directory
[*] unpacking... 42%\r
[*] creating sce_sys/package/work.bin
[*] creating sce_sys/package/head.bin
[*] creating sce_sys/package/tail.bin
[*] creating sce_sys/package/stat.bin
[*] renaming sce_sys/package/digs.bin to body.bin
[*] creating d0.pdb / d1.pdb / f0.pdb
[*] minimum fw version required: 3.61
[*] done!
ERROR: not a pkg file
ERROR: unsupported content type 0x%x
ERROR: pkg file is too small
ERROR: zRIF content id '%s' doesn't match pkg '%s'
ERROR: wrong size of zRIF, is it corrupted?
ERROR: unsupported data.psar block size %u, max 16 supported
```

La fuente de verdad es `pkg2zip.c`; el port debe copiar los literales y el orden. Los tests de
paridad comparan stdout/stderr normalizando `\r` y el progreso.

## Anexo C — Comandos útiles

```bash
# Toolchain (Windows: añadir %USERPROFILE%\.cargo\bin al PATH)
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc \
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl \
  x86_64-apple-darwin aarch64-apple-darwin

# Desarrollo
cargo fmt --all && cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo bench -p pkg2zip-core

# Paridad
pwsh ./scripts/build-legacy.ps1
cargo run -p pkg2zip-testkit -- gen ./target/corpus
pwsh ./scripts/parity.ps1 -Corpus ./target/corpus -Report ./target/parity.json

# Release local
cargo build --release --locked --target x86_64-pc-windows-msvc
```

## Anexo D — Referencias

- Formato PKG PS3/Vita: <https://www.psdevwiki.com/ps3/PKG_files>, <https://wiki.henkaku.xyz/vita/Packages>
- SFO/PSF: <http://vitadevwiki.com/vita/System_File_Object_(SFO)_(PSF)>
- NoNpDrm (estructura de `work.bin`): <https://github.com/TheOfficialFloW/NoNpDrm>
- ZIP APPNOTE (ZIP64): <https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT>
- RFC 1950 (zlib), RFC 1951 (deflate), RFC 4493 (CMAC): <https://www.rfc-editor.org/>
- CSO/ZISO: <https://github.com/unknownbrackets/maxcso>
- `puff`: <https://github.com/madler/zlib/blob/master/contrib/puff/puff.c>

---

### Registro de cambios

| Fecha | Cambio |
|---|---|
| 2026-11-20 | Versión inicial del plan en la rama `develop` |
