# pkg2zip

[![CI](https://github.com/allyssonmoscoso/pkg2zip-rust-implementation/actions/workflows/ci.yml/badge.svg?branch=develop)](https://github.com/allyssonmoscoso/pkg2zip-rust-implementation/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/allyssonmoscoso/pkg2zip-rust-implementation)](https://github.com/allyssonmoscoso/pkg2zip-rust-implementation/releases/latest)
[![License](https://img.shields.io/badge/license-Unlicense-blue.svg)](LICENSE)

Utility that decrypts PlayStation Vita/PSP/PSX pkg files and creates a zip package (or loose files)
ready to copy to the console. **Version 3.0 is a complete Rust rewrite**: no C compiler, no external
libraries, and the same command line and output layout as 2.6.

* **Vita**: main application, DLC, patch, theme and PSM pkgs.
* **PSP**: games, updates, DLC, themes, PSP-Go/Mini/NeoGeo, ISO/CSO output.
* **PSX/PS3**: PS1 classics for Adrenaline, `KEYS.BIN` and `.rif` generation.

## Features

* **Portable**: Windows, Linux and macOS (x86_64 and aarch64). Pure Rust, no C toolchain needed.
* **Small and fast**: streaming IO, AES-NI/PCLMULQDQ via `aes`/`crc32fast`, no temporary files.
* **Same contract as 2.6**: identical output names, folder layout, CLI flags, messages and exit
  codes. `-l` output is byte-for-byte identical.
* **Safer**: malformed pkgs return an error instead of overflowing; `..`/absolute paths from a pkg
  are rejected in `-x` mode.
* **Optional fake license**: writes `work.bin` (NoNpDrm) or `FAKE.rif` (NoPsmDrm) from a zRIF, or
  validates an existing `.bin`.

## Install

### Binaries

Download the archive for your platform from the
[latest release](https://github.com/allyssonmoscoso/pkg2zip-rust-implementation/releases/latest):

| Platform | File |
|---|---|
| Windows x86_64 | `pkg2zip-v3.0.1-windows-x86_64.zip` |
| Linux x86_64 | `pkg2zip-v3.0.1-linux-x86_64.tar.gz` |
| Linux x86_64 (static musl) | `pkg2zip-v3.0.1-linux-x86_64-musl.tar.gz` |
| Linux aarch64 | `pkg2zip-v3.0.1-linux-aarch64.tar.gz` |
| macOS (universal) | `pkg2zip-v3.0.1-macos-universal.tar.gz` |

Every release ships `SHA256SUMS`.

### From source

```sh
cargo install --git https://github.com/allyssonmoscoso/pkg2zip-rust-implementation --tag v3.0.1
```

or, from a checkout:

```sh
cargo build --release --locked
./target/release/pkg2zip --version
```

Requirements: Rust stable (MSRV 1.89). No C compiler, no `pkg-config`, no system libraries.

## Usage

```text
pkg2zip [-x] [-c[N]] [-b] [-p] [-d] [-a] [-q] <file.pkg> [zRIF]
```

Extract to a zip (default):

```sh
pkg2zip package.pkg
# -> "Title [PCSE00000] [USA].zip"
```

With a fake license (writes `work.bin`/`FAKE.rif` and validates `CONTENT_ID`):

```sh
pkg2zip package.pkg "zRIF_string"
pkg2zip package.pkg path/to/work.bin
```

Print only the output name (script-friendly):

```sh
pkg2zip -l package.pkg
```

Loose files instead of a zip:

```sh
pkg2zip -x package.pkg
```

PSP/PSX options:

| Flag | Effect |
|---|---|
| `-p` | Keep `EBOOT.PBP` instead of building an ISO/CSO |
| `-cN` | Produce a CSO with compression level `N` (0–9); `-c` alone = ISO |
| `-d` | Always decrypt PSP DLC/EDAT files |

PSM only:

| Flag | Effect |
|---|---|
| `-a` | Android layout (no `RO`/`RW` split) |

Other flags: `-b` disables the `bgdl` layout for Vita themes; `-q` silences stdout;
`-h/--help`; `--version`.

## zRIF helpers

The repository keeps the original Python helpers:

```sh
python rif2zrif.py path/to/work.bin     # print zRIF
python zrif2rif.py zRIF work.bin        # write work.bin
```

## Building and testing

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The test suite covers crypto vectors (FIPS-197, NIST SP 800-38A, RFC 4493), the zRIF/puff port,
ZIP/ZIP64, ISO/CSO and the PKG parsers. `pkg2zip-testkit` generates synthetic pkgs and can compare
outputs against the archived C implementation (see [docs/paridad.md](docs/paridad.md)).

## Differences from 2.6

The observable contract is intentionally unchanged. The only differences:

* ZIP timestamps use UTC instead of local time (the C used `localtime`). Entry names, CRCs and
  contents are identical.
* `-x` rejects absolute paths and `..` components coming from the pkg.
* Errors are more descriptive (for example a missing file no longer crashes) and all of them still
  exit with code 1. Unsupported/unimplemented paths used to abort; 3.0 always returns a message.
* `--version` was added.

## Verification

* **zRIF**: validated against 3.703 real licenses from the NoPayStation Vita database (padding after
  the Adler-32 included) and end-to-end against the C binary.
* **92 differential cases** against the frozen C binary: `list`, `extract`, `zip`, `zrif`,
  `zrif-bin`, `zrif-mismatch`, `psp-pbp`, `no-bgdl`, `quiet` and `android`, over 12 synthetic
  fixtures. Output trees, ZIP entries/CRC/SHA-256 and stdout are compared.
* **102 unit/integration tests**, including AES/CTR/CMAC/PSP-AES golden vectors generated from the
  C implementation.

## Known limitations

* ISO/CSO from a real PSP `EBOOT.PBP` and EDAT/KEYS.BIN decryption are implemented and their crypto
  is validated against C vectors, but there is no synthetic end-to-end fixture yet: a valid PSAR/PGD
  requires inverting the container MAC. Add a real sample under `testdata/psp/` (gitignored) to run
  those comparisons.
* PSM pkgs do not contain the real title, so the output name uses the content id (`[PSM]`), same as
  2.6.
* The original C sources are archived in the tag [`v2.6-c-final`](https://github.com/allyssonmoscoso/pkg2zip-rust-implementation/tree/v2.6-c-final)
  for reference and differential testing.

## License

This is free and unencumbered software released into the public domain. See [LICENSE](LICENSE).
