# Changelog

All notable changes to this project are documented here.

## [3.0.0] - 2026-11-20

Complete rewrite in Rust. The C implementation lives on in the tag `v2.6-c-final`.

### Added

* Pure Rust implementation of the PKG decryption and packaging pipeline: Vita (APP, PATCH, DLC,
  THEME, PSM), PSP (ISO/CSO, `EBOOT.PBP`, EDAT, KEYS.BIN, themes), PSX and PS3.
* ZIP/ZIP64 writer with deferred CRC and in-place patching (needed for CSO).
* zRIF decoder with a Rust port of `puff` (preset dictionary) and `.bin` support.
* AES-128 ECB/CTR, CMAC, the PSP AES variant, CRC-32 (`combine`) and Adler-32.
* LZRC decoder for PSP `data.psar` blocks; ISO/CSO writer with per-sector deflate.
* `pkg2zip-testkit`: synthetic pkg generator and a differential harness (92 cases).
* `--version`.
* Release automation for Windows, Linux (gnu/musl, x86_64/aarch64) and universal macOS, with
  `SHA256SUMS`.
* CI with fmt, clippy, tests on three OSes, MSRV check, `no-c` guard, `cargo-deny` and
  `cargo-machete`.

### Changed

* ZIP timestamps use UTC instead of local time. Names, CRCs and contents are unchanged.
* Error messages are more descriptive; missing files, truncated headers and invalid offsets return
  an error instead of crashing.
* `-x` rejects absolute paths and `..` components from the pkg (path traversal hardening).

### Removed

* The C sources, `makefile`, `build*.cmd`, Travis/AppVeyor configs and `flake.nix` (archived in
  `v2.6-c-final`).
* The broken CMake GitHub workflow.

### Compatibility

* CLI flags, output names, folder layouts, `-l` output, messages and exit codes are the same as
  2.6 (except the version banner).
* Verified with 92 differential cases against the C binary and 102 unit/integration tests.

### Known limitations

* Real PSP `EBOOT.PBP` (PSAR) and EDAT/KEYS.BIN paths are implemented but lack a synthetic
  end-to-end fixture; crypto is validated against golden vectors from the C implementation.
* PSM output names use the content id (`[PSM]`), as in 2.6.

## [2.6] - previous C release

See the `v2.6-c-final` tag for the previous implementation.
