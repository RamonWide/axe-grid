# Contributing to Axe-Grid

Thank you for helping. This page applies to people and AI agents alike. Read
[`AGENTS.md`](AGENTS.md) first for the map of the code and the five golden rules; this page is the
practical part.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## The rules in one screen

- **Language:** Rust only. No JavaScript, no web view.
- **Licence:** contributions are accepted under the [MIT License](LICENSE). By opening a pull
  request you confirm that you wrote the change, or have the right to submit it, under that
  licence ([Developer Certificate of Origin](https://developercertificate.org/)). Sign your commits
  off with `git commit -s`.
- **Clean-room:** do not copy code, artwork, fonts, skins, text or files from Fractal Audio's
  software, and do not paste decompiler output anywhere in this repository, including issues and
  pull requests. Work from [`spec/SPEC.md`](spec/SPEC.md) and public documentation. The full rules
  are in [`docs/clean-room.md`](docs/clean-room.md) and they are not optional.
- **Never crash:** non-test code has no `unwrap`, `expect`, `panic!`, `todo!` and no `unsafe`.
  Errors go through `Result`; parameters arrive as untrusted JSON and are read through `Args`.
- **Commands, not handlers:** a new feature is an engine command with tests, and the UI calls it.
- **Hardware is someone's instrument:** tests run against the simulator, never against real ports.
  A command that overwrites anything on the device says so in its doc string.
- **Evidence over guesses:** a wire-format fact needs a confidence tag in the spec.
- **Tests:** required for every change.
- **Style:** `cargo fmt`, and `cargo clippy` with no warnings. Match the surrounding code.
  Comments explain why.

## Set up

You need a stable [Rust](https://rustup.rs) toolchain, 1.95 or newer. The repository's
`rust-toolchain.toml` selects stable with `rustfmt` and `clippy`.

| System | Extra packages |
|---|---|
| Windows | None. The MSVC build tools that `rustup` asks for are enough. |
| macOS | Xcode command-line tools: `xcode-select --install` |
| Debian, Ubuntu | `sudo apt install build-essential pkg-config libasound2-dev` |
| Fedora | `sudo dnf install gcc pkg-config alsa-lib-devel` |
| Arch | `sudo pacman -S base-devel alsa-lib` |

On Linux the window also needs the usual desktop libraries at run time (libxkbcommon, OpenGL, and
X11 or Wayland), which a desktop installation already has.

```sh
git clone https://github.com/RamonWide/axe-grid
cd axe-grid
cargo build
cargo test --workspace               # everything runs against the simulated device
cargo run -p axegrid                 # desktop app; press "Simulator" to try it
cargo run -p axegrid-cli -- --simulator run app.inspect
```

No hardware is needed for any of this.

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p axegrid-cli -- commands | python scripts/gen-command-docs.py > docs/commands.md
```

CI runs the same four steps on Windows, macOS and Linux, and checks that the code still builds
with the oldest supported Rust.

Then:

- Keep the change small and focused: one logical change per pull request.
- Say what changed and why in the description, and how you tested it.
- If you changed the window, attach before and after screenshots.
- If you changed what works or what is missing, update [`docs/gaps.md`](docs/gaps.md), and
  [`ROADMAP.md`](ROADMAP.md) if a number there moved.
- Add a line to [`CHANGELOG.md`](CHANGELOG.md) under "Unreleased" for anything a user would notice.

## Add a command

1. **Check the spec.** If the command needs a device message, find it in `spec/SPEC.md`. If it is
   not there, it needs analysis first: open an issue rather than guessing a wire format. Note the
   confidence tag.
2. **Protocol** (only if a new message is needed): add a `Request` variant and its payload in
   `crates/axegrid-sysex/src/message.rs`, the `Reply` variant and parser if it has its own reply,
   and a test with literal bytes.
3. **Simulator:** teach `SimDevice::handle` to answer it, so the command can be tested end to end.
4. **Command:** add a `CommandSpec` to the fitting `crates/axegrid-engine/src/*_cmds.rs`:
   - `id`: `namespace.verb_object`, lowercase, stable forever once released.
   - `menu`: `Menu/Item` to appear in the menu bar, or empty. Menu commands must work with no
     required parameters.
   - `shortcut`: for example `Ctrl+S`, `F5`, `B`; empty for none.
   - `doc`: one or two sentences an agent can act on. Say if it overwrites anything on the device,
     and say if it relies on a `[P]` or `[?]` fact.
   - `params`: one `req(..)`/`opt(..)` per parameter.
   - `enabled`: `always`, `connected`, or your own predicate returning the reason it is unavailable.
   - `run`: read parameters through `Args` only; return `CommandError`, never panic; update `State`
     so the UI reflects the result. If the command edits the preset, record the edit so that it can
     be undone: `record_param`, `record_xy`, `record_rename` or `recording_grid` in
     `history_cmds.rs`. A command that loads another preset clears the history.
5. **Tests** in `crates/axegrid-engine/tests/commands.rs`: the happy path, and its bad inputs added
   to `hostile_parameters_are_errors_not_panics`.
6. **UI** (optional): if it deserves a dedicated widget, push `("your.command", json!({..}))` onto
   the frame's action list in `axegrid-ui-egui`. Do not put logic in the widget.
7. **Docs:** regenerate `docs/commands.md` and update `docs/gaps.md` if you closed or opened a gap.

## Hardware reports

Owners of a unit can help most by confirming how their model behaves, especially the Axe-Fx II
(Mark I and II) and the XL+, which nobody has tested yet, and anything running on macOS or Linux.
Use the **Hardware report** issue form. Include:

- the model and firmware version, the operating system, and USB or MIDI interface;
- what you did, what you expected, and what the unit did;
- for protocol findings, the exact bytes sent and received, in hex.

Items tagged `[P]` (from public documentation, unverified) and `[?]` (unknown) in
[`spec/SPEC.md`](spec/SPEC.md) and the "Still unverified" table in
[`docs/gaps.md`](docs/gaps.md) are the ones worth checking.

Two rules when testing on a real unit:

1. **Back up first, and test in the edit buffer of a preset slot you do not need.** Never test
   `preset.store` on a slot that holds something.
2. **Never send a message whose format you are guessing.** One unit stopped responding until it was
   power-cycled after a malformed grid was sent to it; the story is in `docs/gaps.md`.

Never attach presets, cabinet files or firmware that you did not create yourself.

## Reporting bugs and asking for features

Use the [issue forms](https://github.com/RamonWide/axe-grid/issues/new/choose). Search first; a
thumbs-up on an existing issue helps more than a duplicate. Security problems go through
[`SECURITY.md`](SECURITY.md), not public issues.

## Style

- Match the surrounding code. Comments explain why, not what.
- Errors say what was wrong with which input.
- Public items in `axegrid-sysex` and `axegrid-model` cite the spec section they implement.
- Commit messages say what and why in the imperative: "Read choice lists from the device".

## Releases

Maintainers: see [`docs/releasing.md`](docs/releasing.md).
