#!/usr/bin/env sh
# Genera el corpus sintético en target/corpus (o el directorio indicado).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

cargo run -q -p pkg2zip-testkit -- gen "${1:-target/corpus}"
