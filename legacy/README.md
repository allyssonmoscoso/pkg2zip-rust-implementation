# legacy/ — implementación C de pkg2zip (congelada)

Este directorio contiene la implementación original en C de `pkg2zip` 2.6, movida aquí en la
Fase 0 del plan de migración a Rust (`docs/PLAN-MIGRACION-RUST.md`).

**No forma parte del producto final.** Se conserva únicamente como:

1. **Oráculo de paridad**: el arnés `pkg2zip-testkit parity` ejecuta este binario y compara su salida
   con la implementación Rust.
2. **Referencia de comportamiento** para los módulos que se van portando.

Se eliminará en la Fase 5 (release 3.0.0). La última versión estable en C quedará archivada en el
tag `v2.6-c-final`.

## Compilar

En Windows, desde la raíz del repositorio (localiza Visual Studio con `vswhere`):

```powershell
pwsh ./scripts/build-legacy.ps1        # -> target/legacy/pkg2zip.exe
```

En Linux/macOS (requiere un compilador de C):

```sh
./scripts/build-legacy.sh              # -> target/legacy/pkg2zip
```

También se puede compilar a mano:

```sh
make -C legacy                         # gcc/clang
# o, en Windows con MinGW:
mingw32-make -C legacy
```

`build.cmd` / `build32.cmd` son los scripts históricos de MSVC y asumen Visual Studio 2017 en una
ruta fija; por eso están en desuso frente a `scripts/build-legacy.ps1`.

## Contenido

| Archivo | Rol |
|---|---|
| `pkg2zip.c` | CLI, cabecera PKG, SFO, detección de tipo, layouts, extras |
| `pkg2zip_psp.c` | PBP/PSAR → ISO/CSO, LZRC, EDAT, KEYS.BIN, temas PSP |
| `pkg2zip_zip.c` | Escritor ZIP/ZIP64 |
| `pkg2zip_aes*.c` | AES-128 ECB/CTR/CMAC y variante PSP |
| `pkg2zip_crc32*.c` | CRC-32 y `crc32_combine` |
| `pkg2zip_zrif.c` | zRIF (base64 + inflate con diccionario) |
| `pkg2zip_sys.c` | IO posicional, consola UTF-8, progreso |
| `pkg2zip_out.c` | Fachada zip/carpeta |
| `miniz_tdef.*`, `puff.*` | Deflate (CSO) e inflate con diccionario (zRIF), vendoreados |
| `makefile`, `build*.cmd`, `.travis.yml`, `.appveyor.yml`, `flake.*` | Build/CI histórico de la etapa C |

## Licencia

El código C original es *Unlicense* (dominio público). `miniz_tdef` y `puff` conservan sus licencias
originales (dominio público / zlib) en sus cabeceras.
