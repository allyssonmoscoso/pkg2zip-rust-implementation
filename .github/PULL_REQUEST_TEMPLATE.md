## Summary

<!-- What does this PR change and why? -->

## Phase / issue

<!-- e.g. F2, or the issue number -->

## Checklist

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Parity harness run when the domain changed (see `docs/paridad.md`)
- [ ] Docs updated (`README`, `CHANGELOG`, ADRs) if behaviour changed
- [ ] No new C dependencies (`cargo tree`)

## Compatibility

<!-- Any change in CLI, output names, messages or exit codes? If yes, explain why. -->
