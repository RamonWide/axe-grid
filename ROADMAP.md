# Axe-Grid roadmap

**Stage: early alpha** · next: 0.2 "safe daily editing" (confirming the new scene, block and
modifier tools on a unit, resetting a block, first checks on macOS, Linux and the other models)

> **Last reviewed:** 2026-10-12 · **Last updated:** 2026-10-12 · **Change:** scene names, scene and
> block copy and paste, modifiers, the looper's position, the Tone Match steps; all built against
> the simulator, most not yet confirmed on a unit ·
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
| **Feature breadth** | **~55%** (50–60%) | estimated: areas below, weighted by how often a player uses them |
| **Ready for real work** | **~30%** (25–35%) | estimated: breadth, discounted for one tested unit and one tested OS; unchanged by the 2026-10-12 additions until they are confirmed on a unit |
| Commands | 83 | measured: `axegrid-cli commands` |
| Block types with full pages | 38 | measured: `layouts.json` |
| Device message types implemented | 41 of roughly 56 the original uses | measured / estimated |
| Automated tests | 187, all against the simulator | measured |
| Units tested | 1 (Axe-Fx II XL, firmware 32.00) | measured |
| Operating systems tested with a unit | 1 of 3 (Windows 11) | measured |

**Why early alpha.** The core loop works on real hardware: connect, read a preset, edit any
block's parameters, re-route the grid, switch scenes and save. But whole areas of the original are
missing (global settings, meters), the newest tools have only run against the simulator, and
everything has been checked on a single unit. Twice a test made that unit stop responding until
it was power-cycled: a grid built with one shunt ID in several cells, and a read-only modifier
query one past the last field. Both are guarded against now and written up in
[gaps](docs/gaps.md#hardware-incident-2026-10-11-resolved).

**Alpha gate (passed):** the six core workflows (connect, browse presets and scenes, edit a
block, re-route the grid, tune and set tempo, save) complete end to end on real hardware.

**Beta gate (not yet):** ~70% ready; undo; modifiers confirmed on a unit; preset backup and restore; at least one
tested unit of each model family (II, XL, XL+); all three operating systems tested with a unit;
no known way to hang the unit.

## By dimension

| Dimension | % ready | Size to finish | Detail |
|---|---:|:-:|---|
| Features (see below) | ~35% | L | [gaps](docs/gaps.md#missing-compared-with-the-original-editor) |
| Hardware coverage (models, firmware versions) | ~30% | M | One XL tested. II Mark I/II and XL+ are recognised by model byte but untested |
| Protocol verification | ~60% of what is used | S | [gaps](docs/gaps.md#hardware-verification): store, tuner calibration, setting modifier fields, the looper's position and the Tone Match steps remain |
| Platforms | ~55% | M | Windows tested, with an installer that adds desktop and Start-menu shortcuts. macOS and Linux build in CI, untested with a unit. No signed builds |
| Connection | ~70% | M | The unit is found by itself at start (USB port first, then other MIDI ports). Any MIDI port the system offers. USB needs a system driver: the vendor's, or [AxeTwoDriver](https://github.com/RamonWide/AxeTwoDriver) on current Windows |
| Stability and safety | ~60% | M | No panics by lint; the device session runs on its own thread, so a slow or silent unit does not freeze the window; undo and redo for edits · commands still wait their turn behind a silent unit, and none can be cancelled |
| Automation (CLI, control channel, MCP) | ~80% | S | Works end to end; no per-client capabilities yet ([SECURITY](SECURITY.md)) |
| Offline use | 0% | M | Nothing can be viewed or edited without a unit or the simulator |
| Localization | 0% | M | English only |

## Features

| Area | % ready | Size | What is there · what is missing |
|---|---:|:-:|---|
| Routing grid | ~75% | S | Place, move, remove, cable, runs to input and output · row and column tools, clear grid, cables across a column move |
| Block parameters | ~75% | M | Pages for 38 block types, all control kinds, X/Y, bypass, type lists read from the unit · a few special controls, lists kept between sessions |
| Presets | ~70% | S | Select, rename, save, browser with all names, copy, save to another slot, export, import, backup of all · swap, bank and bundle files, exporting unsaved edits |
| Scenes | ~70% | S | Select, per-scene output levels, names kept on the computer, copy and paste · swap, set all to X or Y; copy and paste not yet confirmed on a unit |
| Modifiers and controllers | ~40% | M | The Controllers block's pages; a modifier window on every knob and fader: attach, edit, remove, a sketched curve · confirmation on a unit (only reading is), a list of sources, marks on modified controls, the unit's own curve |
| Graphs and meters | ~45% | M | Computed curves for 13 block types: EQs, filter, delay and reverb EQ, amp speaker, cab, crossover, compressor, gates, ADSR · all meters, Tone Match, LFO and sequencer displays, the unit's own graph data |
| Utilities (tuner, tempo, looper, Tone Match) | ~75% | S | Tuner, tap and BPM, looper buttons with state and a position bar, the three Tone Match steps · the Tone Match display; position and matching not yet confirmed on a unit |
| Block tools | ~50% | S | Copy and paste a block, one X/Y set or both, copy X to Y and Y to X · reset to defaults (how the unit does it is not known), block library, global blocks |
| Cabinet management | ~45% | M | User cab list, export, import (not yet confirmed on a unit), use in the cab block; XL and XL+ only · `.wav`/`.ir` conversion, scratchpads, Mark I/II |
| Global, I/O and MIDI settings | 0% | M | Everything |
| Undo and redo | ~75% | S | Parameters, bypass, X/Y, the grid with its cables, the preset name (checked on a unit); modifier fields, a pasted scene, a pasted block · scenes, changes made on the front panel, a history that survives loading a preset |

## Milestones

### 0.1: first public alpha (this release)

Everything in the [changelog](CHANGELOG.md#010---2026-10-11).

### 0.2: safe daily editing

The aim is that a player can use Axe-Grid for an evening without reaching for the original.

- ~~Undo and redo for parameter and grid edits.~~ Done, with bypass, X/Y and the preset name.
- ~~Move the device session off the UI thread, so a slow or silent unit cannot freeze the
  window.~~ Done.
- ~~Read type lists (amp, cab, effect types) from the unit, removing the dependency on the
  original editor's definition file.~~ Done; checked over USB, not yet over a 5-pin MIDI cable.
- ~~Preset browser with the names of all slots.~~ Done.
- ~~Scene names, copy and paste; block copy and paste.~~ Done against the simulator; to be
  confirmed on a unit. Scene names live on the computer, because the unit has none.
- Block reset. Open: no known message resets a block, and taking it out of the grid and putting
  it back keeps its values.
- First hardware checks on macOS and Linux, and on an Axe-Fx II (Mark I or II) and an XL+.
  **This needs owners of those units: see [Hardware reports](CONTRIBUTING.md#hardware-reports).**
- Signed Windows builds and notarized macOS builds, if the project can fund the certificates.

### 0.3: modifiers and feedback

- ~~Modifiers: attach, edit and remove, with the curve display.~~ Done against the simulator.
  Reading a modifier is confirmed on a unit; setting one is not yet. The curve is a sketch.
- Level and gain-reduction meters.
- ~~Graphs for delay, reverb, filter, graphic EQ and the amp's speaker page.~~ Done, computed
  from the settings.
- ~~Looper position and the Tone Match workflow.~~ Done against the simulator; to be confirmed on
  a unit. The Tone Match spectrum is still missing.

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
