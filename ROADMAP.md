# Axe-Grid roadmap

**Stage: early alpha** · next: 0.2 "safe daily editing" (undo, a session that cannot freeze the
window, type lists read from the unit, first checks on macOS, Linux and the other models)

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-11 · **Change:** first version ·
> **Target:** the original editor for the Axe-Fx II family

A one-page summary of how close Axe-Grid 0.1.0 is to the original editor for the Axe-Fx II.
Detail: [gaps](docs/gaps.md) (what is verified on hardware and the ranked work list),
[command reference](docs/commands.md) (generated), [specification](spec/SPEC.md) (the protocol,
with confidence tags), [architecture](docs/architecture.md).

**How to read the numbers.** "Measured" values are counted from the code or the test logs.
Percentages are the maintainers' estimates of how much of an area a player can rely on today,
not measurements; they are deliberately conservative and are given as ranges where the
uncertainty is large. Sizes are rough: **S** up to a few days of focused work, **M** a week or
two, **L** a month or more. There is not yet enough project history to give calibrated hours.

## Headline numbers

| | Value | Kind |
|---|---|---|
| **Feature breadth** | **~45%** (40–50%) | estimated: areas below, weighted by how often a player uses them |
| **Ready for real work** | **~30%** (25–35%) | estimated: breadth, discounted for no undo, one tested unit and one tested OS |
| Commands | 62 | measured: `axegrid-cli commands` |
| Block types with full pages | 38 | measured: `layouts.json` |
| Device message types implemented | 40 of roughly 56 the original uses | measured / estimated |
| Automated tests | 146, all against the simulator | measured |
| Units tested | 1 (Axe-Fx II XL, firmware 32.00) | measured |
| Operating systems tested with a unit | 1 of 3 (Windows 11) | measured |

**Why early alpha.** The core loop works on real hardware: connect, read a preset, edit any
block's parameters, re-route the grid, switch scenes and save. But whole areas of the original are
missing (modifiers, global settings), there is no undo, and everything has been checked
on a single unit. One test sequence made that unit stop responding until it was power-cycled; the
cause was fixed and is written up in [gaps](docs/gaps.md#hardware-incident-2026-10-11-resolved).

**Alpha gate (passed):** the six core workflows (connect, browse presets and scenes, edit a
block, re-route the grid, tune and set tempo, save) complete end to end on real hardware.

**Beta gate (not yet):** ~70% ready; undo; modifiers; preset backup and restore; at least one
tested unit of each model family (II, XL, XL+); all three operating systems tested with a unit;
no known way to hang the unit.

## By dimension

| Dimension | % ready | Size to finish | Detail |
|---|---:|:-:|---|
| Features (see below) | ~35% | L | [gaps](docs/gaps.md#missing-compared-with-the-original-editor) |
| Hardware coverage (models, firmware versions) | ~30% | M | One XL tested. II Mark I/II and XL+ are recognised by model byte but untested |
| Protocol verification | ~70% of what is used | S | [gaps](docs/gaps.md#hardware-verification): store, tuner calibration and modifiers remain |
| Platforms | ~55% | M | Windows tested, with an installer that adds desktop and Start-menu shortcuts. macOS and Linux build in CI, untested with a unit. No signed builds |
| Connection | ~70% | M | The unit is found by itself at start (USB port first, then other MIDI ports). Any MIDI port the system offers. USB needs a system driver: the vendor's, or [AxeTwoDriver](https://github.com/RamonWide/AxeTwoDriver) on current Windows |
| Stability and safety | ~50% | M | No panics by lint; the session still runs on the UI thread; no undo |
| Automation (CLI, control channel, MCP) | ~80% | S | Works end to end; no per-client capabilities yet ([SECURITY](SECURITY.md)) |
| Offline use | 0% | M | Nothing can be viewed or edited without a unit or the simulator |
| Localization | 0% | M | English only |

## Features

| Area | % ready | Size | What is there · what is missing |
|---|---:|:-:|---|
| Routing grid | ~75% | S | Place, move, remove, cable, runs to input and output · row and column tools, clear grid, cables across a column move |
| Block parameters | ~70% | M | Pages for 38 block types, all control kinds, X/Y, bypass · type lists without the original's definition file, a few special controls |
| Presets | ~70% | S | Select, rename, save, browser with all names, copy, save to another slot, export, import, backup of all · swap, bank and bundle files, exporting unsaved edits |
| Scenes | ~40% | S | Select, per-scene output levels · names, copy and paste, swap, set all to X or Y |
| Modifiers and controllers | ~5% | L | The Controllers block's pages · attaching a controller to a parameter, the modifier dialog and curve |
| Graphs and meters | ~45% | M | Computed curves for 13 block types: EQs, filter, delay and reverb EQ, amp speaker, cab, crossover, compressor, gates, ADSR · all meters, Tone Match, LFO and sequencer displays, the unit's own graph data |
| Utilities (tuner, tempo, looper, Tone Match) | ~65% | S | Tuner, tap and BPM, looper buttons with state, capture buttons · looper position, Tone Match workflow and display |
| Block tools | 0% | S | Copy and paste a block or its X/Y state, reset to defaults, block library, global blocks |
| Cabinet management | ~45% | M | User cab list, export, import (not yet confirmed on a unit), use in the cab block; XL and XL+ only · `.wav`/`.ir` conversion, scratchpads, Mark I/II |
| Global, I/O and MIDI settings | 0% | M | Everything |
| Undo and redo | 0% | M | Everything |

## Milestones

### 0.1: first public alpha (this release)

Everything in the [changelog](CHANGELOG.md#010---2026-10-11).

### 0.2: safe daily editing

The aim is that a player can use Axe-Grid for an evening without reaching for the original.

- Undo and redo for parameter and grid edits.
- Move the device session off the UI thread, so a slow or silent unit cannot freeze the window.
- Read type lists (amp, cab, effect types) from the unit, removing the dependency on the original
  editor's definition file.
- ~~Preset browser with the names of all slots.~~ Done.
- Scene names, copy and paste; block copy, paste and reset.
- First hardware checks on macOS and Linux, and on an Axe-Fx II (Mark I or II) and an XL+.
  **This needs owners of those units: see [Hardware reports](CONTRIBUTING.md#hardware-reports).**
- Signed Windows builds and notarized macOS builds, if the project can fund the certificates.

### 0.3: modifiers and feedback

- Modifiers: attach, edit and remove, with the curve display.
- Level and gain-reduction meters.
- ~~Graphs for delay, reverb, filter, graphic EQ and the amp's speaker page.~~ Done, computed
  from the settings.
- Looper position and the Tone Match workflow.

### 0.4: library

- ~~Preset import and export~~ (done); backup is one file per preset, bank files are still to do.
- Cabinet management: the list, export and import are there; confirm import on a unit, add
  `.wav` conversion and the Mark I/II.
- Offline viewing of preset files.

### Later, and open questions

- **USB without a vendor driver.** On current Windows the vendor's driver may not load. This is
  now handled by a sister project, [AxeTwoDriver](https://github.com/RamonWide/AxeTwoDriver): it
  hands the unit's USB MIDI to Windows' own driver, after which Axe-Grid connects over the USB
  cable alone. Verified on one Axe-Fx II XL on Windows 11. Nothing is planned inside Axe-Grid.
- **Global, I/O and MIDI settings.**
- **More installers** (DMG, AppImage, Flatpak, `.deb`) and packages for package managers. Windows
  has `axegrid-setup.exe`.
- **Localization.**

### Not planned

- **Firmware updating.** A failed update can leave a unit unusable; the vendor's tool exists for
  this and Axe-Grid will not attempt it.
- **Shipping factory content:** presets, cabinet files, model names. Axe-Grid shows what your own
  unit reports and never bundles the vendor's content ([clean-room rules](docs/clean-room.md)).
- **Other product families.** They use different protocols. A sister project could reuse the
  architecture, but it is out of scope here.

## How to help move a number

| If you… | The most useful thing is |
|---|---|
| own an Axe-Fx II Mark I/II or an XL+ | a [hardware report](CONTRIBUTING.md#hardware-reports): does it connect, does the grid read correctly |
| use macOS or Linux | the same, on your system |
| write Rust | an item from [gaps](docs/gaps.md), starting with the engineering section |
| write neither | bug reports with exact steps, and telling us which missing feature stops you switching |
