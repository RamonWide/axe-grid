## What and why

<!-- What does this change, and what problem does it solve? Link the issue if there is one. -->

## How it was tested

<!-- Tests added or changed. If you tried it on a real unit: model, firmware, operating system. -->

## Checklist

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] New behaviour is an engine command with tests, and the UI only calls it
- [ ] `docs/commands.md` is regenerated if a command was added or changed
- [ ] `docs/gaps.md`, `ROADMAP.md` and `CHANGELOG.md` are updated if what works has changed
- [ ] Nothing here is copied from Fractal Audio's software or from decompiler output (`docs/clean-room.md`)
- [ ] Screenshots attached, if the window changed
