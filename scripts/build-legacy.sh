#!/usr/bin/env sh
# Compila la implementación C congelada de legacy/ con el compilador de C del sistema.
# Uso: ./scripts/build-legacy.sh [directorio_de_salida]
# Salida por defecto: target/legacy/pkg2zip
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
legacy="$root/legacy"
out="${1:-$root/target/legacy}"

if [ ! -d "$legacy" ]; then
    echo "error: no existe $legacy" >&2
    exit 1
fi

mkdir -p "$out"

CC=${CC:-cc}
CFLAGS=${CFLAGS:--std=c99 -O2 -DNDEBUG -D_GNU_SOURCE -fvisibility=hidden -Wall -Wextra}

# Flags SIMD solo en x86 (las rutas software funcionan en cualquier arquitectura).
SIMD=""
case "$(uname -m)" in
    x86_64 | amd64 | i?86) SIMD="-maes -mssse3 -mpclmul -msse4" ;;
esac

echo "[legacy] compilando con $CC -> $out/pkg2zip"

# shellcheck disable=SC2086
$CC $CFLAGS $SIMD -o "$out/pkg2zip" \
    "$legacy/pkg2zip.c" \
    "$legacy/pkg2zip_aes.c" \
    "$legacy/pkg2zip_aes_x86.c" \
    "$legacy/pkg2zip_crc32.c" \
    "$legacy/pkg2zip_crc32_x86.c" \
    "$legacy/pkg2zip_out.c" \
    "$legacy/pkg2zip_psp.c" \
    "$legacy/pkg2zip_sys.c" \
    "$legacy/pkg2zip_zip.c" \
    "$legacy/pkg2zip_zrif.c" \
    "$legacy/miniz_tdef.c" \
    "$legacy/puff.c"

echo "[legacy] OK: $out/pkg2zip"
