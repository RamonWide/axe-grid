# AGENTS.md: guide for AI agents and contributors

Axe-Grid is an open-source editor for Fractal Audio's Axe-Fx II family of
guitar processors (II, II XL, II XL+), written in **Rust only**. It is an
independent, clean-room project: not affiliated with or endorsed by Fractal
Audio Systems. The desktop app is native egui/eframe. No webview shells, no
JavaScript.

The product name is written **Axe-Grid** in user-facing text. Machine names
are lowercase without the hyphen: crates `axegrid-*`, binaries `axegrid` and
`axegrid-cli`, URIs `axegrid://`.

Read this file first, then `docs/`.

## 1. Orientation

| Read | Why |
|---|---|
| `docs/architecture.md` | Crate map, dependency layers, how a click becomes a MIDI message |
| `CONTRIBUTING.md` | Set-up per operating system, the pull-request checklist, the "add a command" steps, hardware reports |
| `docs/clean-room.md` | What you may and may not look at or copy. **Read before writing code.** |
| `spec/SPEC.md` | The functional specification the code is written from, with confidence tags |
| `docs/commands.md` | Generated reference of every command |
| `docs/control-protocol.md`, `docs/mcp.md` | How scripts and agents drive the app |
| `docs/gaps.md` | Known shortfalls, ranked: the work list to pick from |
| `ROADMAP.md` | Where the project stands against the original editor, and the milestones |
| `docs/install.md`, `docs/releasing.md` | The guide for people who only use the app; how a release is made |

## 2. Workspace map

```text
crates/
  axegrid-sysex       L0  SysEx framing, 7-bit codecs, typed requests and replies (no workspace deps)
  axegrid-model       L1  block and parameter IDs, routing grid, definition cache format
  axegrid-device      L2  request/reply session over a transport; MIDI ports; simulated device
  axegrid-engine      L3  editor state + the command registry (every action is a command)
  axegrid-automation  L4  control methods, loopback control channel, bridge client, MCP server
  axegrid-ui-egui     L4  egui shell (thin: reads state, runs commands)
apps/
  axegrid             desktop app; optional `--control` server
  axegrid-cli         headless: commands | run | batch | serve | mcp
  axegrid-setup       Windows installer: copies the programs, adds desktop and Start-menu shortcuts
spec/                 functional specification (prose and tables only)
scripts/              doc generation; render-icon.py turns the icon drawing into .png and .ico
assets/icon/          the app icon (original artwork) and its rendered bitmaps
packaging/            macOS bundle and Linux desktop-entry files used by the release workflow
.github/              CI, the release workflow, issue and pull-request templates
```

A crate may depend only on lower layers. Nothing below `axegrid-ui-egui` may
use egui, eframe or winit. `axegrid-sysex` depends on nothing in the
workspace.

## 3. Golden rules

1. **Everything is a command.** New user-visible behaviour is a
   `CommandSpec` in `crates/axegrid-engine/src/*_cmds.rs` with an id, label,
   menu path, shortcut, parameter docs, an `enabled` predicate and a `run`
   function, plus tests. The UI, CLI, control channel and MCP all dispatch by
   id through `Engine::run`. A command with a menu path appears in the menu
   bar automatically; a shortcut string is bound automatically. The UI never
   talks to the device or mutates state itself. If you find yourself adding a
   button that does something no command does, add the command first.
2. **Never crash.** A malformed file, a bad command parameter, a device that
   answers garbage or not at all must produce an error the user or agent can
   act on. Non-test code has no `unwrap`, `expect`, `panic!`, `todo!` or
   unchecked indexing on input-derived values (clippy enforces the first
   four). Parameters arrive as untrusted JSON: read them through `Args`,
   which range-checks. `unsafe` is forbidden workspace-wide.
3. **Clean-room.** Implement from `spec/SPEC.md` and public documentation
   only. Do not read decompiler output while writing code, and never copy
   code, images, fonts, layout files, skins or text from Fractal Audio's
   software. See `docs/clean-room.md`; `tests/clean_room.rs` enforces the
   asset part.
4. **Evidence over guesses.** Wire-format facts carry a confidence tag in the
   spec: `[B]` binary-verified, `[F]` observed in files, `[P]` public docs
   unverified, `[?]` unknown. If you implement something tagged `[P]` or
   `[?]`, say so in the command's doc string and list it in `docs/gaps.md`.
   Upgrading a tag needs evidence, not confidence.
5. **Hardware is someone's instrument.** A command that writes to the device
   (store preset, anything that overwrites) says so in its doc string. Tests
   run against `SimDevice`, never against real ports.

## 4. Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p axegrid-cli -- commands | python scripts/gen-command-docs.py > docs/commands.md
```

## 5. Driving the app as an agent

```sh
# In process, no hardware:
axegrid-cli --simulator mcp
# A running window:
axegrid --control 0 --control-token-file <private path>
axegrid-cli mcp --bridge 127.0.0.1:<port> --control-token-file <private path>
```

Call `command_list` first; it returns every command with parameter docs and
whether it is enabled right now. `state_inspect` shows what the UI shows.
