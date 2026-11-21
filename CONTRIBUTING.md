# Contributing

Thanks for helping improve `pkg2zip`. This repository is a Rust rewrite of the original C tool; the
goal is to keep the CLI and output contract identical to 2.6 while making the code safer and easier
to extend.

## Layout

```text
crates/pkg2zip-core/     domain: crypto, checksums, zRIF, pkg parsers, ZIP, PSP/PSM/Vita flows
crates/pkg2zip-cli/      the `pkg2zip` binary (argument parsing, messages, exit codes)
crates/pkg2zip-testkit/  dev-only: synthetic pkg generator and differential harness
docs/                    plan, ADRs and parity protocol
```

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

MSRV is declared in `Cargo.toml` (`rust-version = "1.89"`); CI checks it.

## Differential testing

The frozen C implementation is archived in the tag `v2.6-c-final`. To compare:

```sh
git worktree add ../pkg2zip-c v2.6-c-final
make -C ../pkg2zip-c                    # gcc/clang; on Windows use MSVC or MinGW
cargo run -p pkg2zip-testkit -- gen target/corpus
cargo run -p pkg2zip-testkit -- parity \
  --corpus target/corpus \
  --legacy ../pkg2zip-c/pkg2zip \
  --rust target/debug/pkg2zip \
  --report target/parity.json
```

The corpus is synthetic (generated and gitignored) and the harness compares exit codes, stdout,
output trees, ZIP entries/CRC/SHA-256 and, for `-c`, the CSO content. Real samples can be dropped
into `testdata/` (gitignored).

## Pull requests

1. Create a feature branch from `develop` (`feat/...`, `fix/...`, `docs/...`).
2. Keep commits focused; Conventional Commits are preferred (`feat:`, `fix:`, `test:`, `docs:`).
3. Make sure the local checks above pass, and run the parity harness when the change touches the
   domain.
4. Open the PR against `develop` with `gh pr create --base develop --fill`. CI must be green.
5. Squash merge (`gh pr merge --squash --delete-branch`).

Releases are cut from `develop` into `master` and tagged `vX.Y.Z`; the release workflow publishes
the binaries and `SHA256SUMS`.

## Code notes

* No C dependencies: `cargo tree` must stay free of crates that build C (CI enforces it).
* `pkg2zip-core` forbids `unsafe` except for the Windows console FFI.
* Errors use `pkg2zip_core::Error`; the CLI adds the `ERROR: ` prefix and maps to exit codes.
* Keep stdout messages and output paths identical to 2.6; the parity harness is the contract.
* Document non-obvious format decisions in `docs/adr/` (ADRs) and update `docs/paridad.md` when the
  test matrix changes.
