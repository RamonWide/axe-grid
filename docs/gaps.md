# Known gaps

Ranked by how much they limit real use. Pick from the top.

## Hardware verification

Verified on 2026-10-11 against an Axe-Fx II XL (model byte `06`, firmware
reported as 32.00) over a 5-pin MIDI interface.

Read side, with `cargo run -p axegrid-device --example probe`: probe and
model detection, firmware query, preset number byte order, preset name, grid
cell order (column-major), block-state word, unsolicited tempo and scene
messages, parameter query with device text, the linear 0 to 65534 raw range,
the parameter dump (`01`, a pure read), and the choice index matching the
installed definition cache.

Write side, with `cargo run -p axegrid-device --example hwtest` and then the
real commands through `axegrid-cli batch`, all inside the edit buffer of an
empty preset that was reloaded afterwards: `preset.select`, `preset.rename`,
`scene.select`, `grid.place_block`, `param.set` / `nudge` /
`set_normalized`, `block.bypass` / `engage` / `toggle_bypass`,
`block.set_xy` / `toggle_xy`, `device.disconnect`, `grid.place_shunt`,
`grid.clear_cell`, `grid.connect` / `disconnect`, `tempo.set`, `tuner.on` /
`off` (readings arrive; the unit stops sending when switched off), and
`block.select` on the Input Gate and Output blocks.

Library, checked on the same unit: the names of all 768 preset slots
(`preset.names`) and of the 1024 user cabs (`cab.names`); `preset.export`
(the file is byte for byte what the unit sent); `cab.export` (the samples
match the vendor file the cab was loaded from); `preset.import` into the edit
buffer of an empty preset, which was reloaded afterwards; and
`device.connect_auto` over the unit's USB port; and `param.set_displayed` on the
input gate (a level, a ratio and two times, and a value past the end of the range). The level, envelope and
frequency curves were checked for sensible shapes against the texts the unit
reports for each block type.

Still unverified on hardware:

| Item | Risk if wrong | Where |
|---|---|---|
| `1D` store preset | A slot is **overwritten** | `Request::StorePreset` |
| `preset.copy` (select, then `1D` to another slot) | The target slot is **overwritten** | `library_cmds.rs` |
| `cab.import` (`7A` with a slot number) | A user cab is **overwritten**; the slot addressing is from the message layout, never sent | `Dump::frames` |
| Library on a Mark I/II or an XL+ | Wrong slot counts; cab numbers laid out differently | `library.rs`, `transfer.rs` |
| Tuner note numbering (0 = A) and centre (63) | Wrong note name or needle | `Engine::tuner_json` |
| Tuner and CC 15 when the owner remapped the tuner controller | Another function is triggered | `live_cmds.rs` |
| `07` modifiers | Not implemented | `message.rs` |

`hwtest` never builds a store request. Keep it that way: test `preset.store`,
`preset.copy` and `cab.import` only on a slot the owner of the unit has named
as disposable.

USB: on current Windows 11 the vendor's 2014 kernel driver is refused
(Device Manager code 52), so the unit exposes no USB MIDI port to any
application. Axe-Grid works with any MIDI port Windows offers; it does not
talk to the device over USB by itself. The sister project
[AxeTwoDriver](https://github.com/RamonWide/AxeTwoDriver) closes this gap
from the system side: once installed, the unit appears as the MIDI port
"AXE-FX II Audio Intfc", and Axe-Grid has been checked over it (connect,
read the preset and grid, change scene) on one Axe-Fx II XL.

## Hardware incident (2026-10-11, resolved)

While the grid commands were being exercised on a maintainer's Axe-Fx II XL,
the unit stopped answering MIDI, including the probe, a few seconds after
this was built in the edit buffer of an empty preset: a Parametric EQ with
shunts in the other eleven cells of its row, every shunt placed as block
ID 200, cabled from input to output. Nothing had been stored; a power cycle
brought the unit back with the test preset still empty.

The cause was not isolated, but the evidence points at the repeated ID: the
unit's own stored presets give every shunt a different ID, and the same row
built with IDs 200 to 210 (`free_shunt` in `edit_cmds.rs`) ran for minutes
and survived preset changes. **Never place one shunt ID in two cells.**
Before sending a new kind of write, read a stored preset to see how the
device itself represents it.

Checked on the unit after the fix, through the desktop app:
`grid.connect_to_input` / `connect_to_output` (also by clicking a handle and
then the INPUT or OUTPUT bar), `grid.move_block` by mouse drag, the mouse
wheel on knobs and lists, the looper's Record and Play buttons with their
state reported back, Tone Match "Start Both" (both captures read back as
on), and the Parametric EQ curve following its knobs. Tone Match "Match"
and the looper's other five buttons were not pressed.

## Missing compared with the original editor

Not implemented at all:

1. **Modifiers**: attaching a controller (LFO, envelope, pedal, scene
   controller) to a parameter, the modifier dialog and its curve.
2. **Bank and bundle files, swapping two presets, the block library and
   global blocks, snapshots.** (The preset browser, copy, import, export and
   backup are there; see "Partly there".)
3. **Cabinet tools beyond the list**: converting `.wav` and `.ir` files,
   the scratchpad slots, renaming or clearing a slot, mixing IRs.
4. **Scene tools**: copy and paste a scene, swap scenes, scene names,
   scene revert, per-scene X/Y copy, "set all blocks to X/Y".
5. **Block tools**: copy and paste a block or its X/Y state, reset a block
   to defaults, "edit block" recall, block undo.
6. **Grid tools**: move or shift a whole row or column, insert/delete a
   column, clear the grid, feedback loops shown on the grid.
7. **Measured graphs**: the Tone Match spectrum, and the controllers' LFO
   and sequencer displays. Every curve that is drawn is computed from the
   settings; the device's own graph data is not read.
8. **Meters**: no level or gain-reduction meters anywhere.
9. **Global and I/O settings, MIDI mapping, firmware tools, preferences,
   display offset, "pause communication", keyboard-shortcut editor.**
10. **Undo/redo.**

Partly there:

- **Preset management**: all names, load, copy to another slot, save to
  another slot, export, import into the edit buffer, and a backup of every
  preset as one file each. A preset cannot be exported with unsaved edits
  (save first), and restoring a backup is one import and save per preset.
- **Cabinet management**: the user-cab list, export, import and "use in the
  cab block" for an XL or XL+. Import is unverified (above). On a Mark I/II
  the window says that its cabinet memory is not known.
- **Curves**: frequency curves for the Parametric and Graphic EQ, Filter,
  the delay's and reverb's EQ pages, the amp's Speaker page, the cab's cuts
  and the Crossover; level curves for the Compressor (Studio type), the
  Gate/Expander, the input gate and the Multiband Compressor (its middle
  band); the two ADSR envelopes. They are textbook shapes, so the amp's
  speaker curve in particular is a sketch: its two resonance amounts are
  drawn as decibels, which the unit does not promise.

- **Tone Match**: its two buttons set the capture and match parameters;
  the capture workflow and the spectrum display are missing.
- **Looper**: transport buttons and their state work; no position bar.
- **Graphic EQ and other fader pages**: the faders shown are the ones of
  the chosen type (before, a type whose name said more than "10 Band" showed
  none).
- **Tuner**: needle and note come from public documentation of the message
  and have not been checked against a played string.
- **Dragging blocks**: within a column cables follow the block; across
  columns the device drops them.
- **Choice lists** need the original editor's cache file
  (`definitions.load_installed`); reading them from the device (`16`, `28`)
  is not wired up.
- **Offline use**: nothing can be viewed or edited without a device.

## Engineering

- The session runs on the UI thread; a silent device freezes the window for
  the 1.5 s timeout per request, and the window asks for the load every two
  seconds. Move it to a worker with a command queue.
- No undo. The device holds the truth; an undo stack would replay inverse
  `param.set` commands.
- The control channel has no per-command capabilities or filesystem roots.
- `axegrid-cli` reports a failed bridge connection with the usage exit code.
- Reading the names of all presets and cabs, and a backup, run a few
  requests per frame from the window; from the CLI one call does all of it
  and blocks until it is done.
- The `62` currently registered commands cover a fraction of the original's
  surface; the original has roughly 56 distinct device messages.
