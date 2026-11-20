#!/usr/bin/env sh
# Ejecuta el arnés de paridad C vs Rust sobre el corpus sintético.
# Uso: ./scripts/parity.sh [corpus] [legacy] [rust] [report]
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

cargo run -q -p pkg2zip-testkit -- parity \
    --corpus "${1:-target/corpus}" \
    --legacy "${2:-target/legacy/pkg2zip}" \
    --rust "${3:-target/debug/pkg2zip}" \
    --report "${4:-target/parity.json}"
