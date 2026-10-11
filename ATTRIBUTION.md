# Attribution

Every file in this repository that was not written for it is listed here.
`cargo test` fails if a binary asset appears in the tree without a row.

## Assets in this repository

| Path | What | Origin | License |
|---|---|---|---|
| `crates/axegrid-model/data/blocks.json` | Block IDs with symbolic names | Numeric IDs and identifiers of the Axe-Fx II MIDI protocol, recovered by analysis of the original editor; see `docs/clean-room.md`, "Data that originates in the original" | Facts and identifiers; no license claimed |
| `crates/axegrid-model/data/params.json` | Parameter IDs with symbolic names per effect type | Same | Same |
| `crates/axegrid-model/data/layouts.json` | For each effect: which parameter each editor control edits, the control's label and kind, and the page and row it sits on | Converted from the control-layout description inside the original editor, so that people who know the original find every control where they expect it. Positions in pixels, graphics, skin and help text were not taken. See `docs/clean-room.md`, "Data that originates in the original" | Short functional labels and an arrangement of controls; no license claimed. Replaceable with an independently written layout |

| `assets/icon/axe-grid.svg` | The app icon: a routing grid with one cable | Drawn for this project | MIT, with the rest of the repository |
| `assets/icon/axe-grid.png`, `assets/icon/axe-grid.ico` | The same icon as bitmaps, for the window, the Windows executables and shortcuts | Rendered from `assets/icon/axe-grid.svg` by `scripts/render-icon.py` | MIT, with the rest of the repository |
| `docs/images/axe-grid-amp.png`, `docs/images/axe-grid-parametric-eq.png`, `docs/images/axe-grid-routing.png`, `docs/images/axe-grid-output-mixer.png`, `docs/images/axe-grid-tuner.png` | Screenshots of Axe-Grid for the README | Captured from this project's own app, connected to a maintainer's Axe-Fx II XL showing a preset built for the purpose. The amp type name visible in one of them is text the device reported at run time | MIT, with the rest of the repository |

There are no fonts, sounds, presets or impulse responses in this repository,
and no images other than the icon (in three formats) and the screenshots listed above. The
interface is painted at run time by `crates/axegrid-ui-egui` (`theme.rs`,
`knob.rs`), an original design of this project.

## Compiled into the binaries

The desktop app embeds the default fonts that ship with the `egui` crate
(Ubuntu Light, Hack, Noto Emoji, emoji-icon-font), each under its own open
license as distributed by `epaint_default_fonts`. Rust crate dependencies are
listed in `Cargo.lock`; run `cargo tree` for the full set and
`cargo license` (third-party tool) for their licenses.

## Not included, by design

Nothing from Fractal Audio's software: no skin images, fonts, layout files,
interface text, factory preset or model names, firmware, or documentation.
