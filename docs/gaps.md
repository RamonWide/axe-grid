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

Still unverified on hardware:

| Item | Risk if wrong | Where |
|---|---|---|
| `1D` store preset | A slot is **overwritten** | `Request::StorePreset` |
| Tuner note numbering (0 = A) and centre (63) | Wrong note name or needle | `Engine::tuner_json` |
| Tuner and CC 15 when the owner remapped the tuner controller | Another function is triggered | `live_cmds.rs` |
| `07` modifiers | Not implemented | `message.rs` |

`hwtest` never builds a store request. Keep it that way: test `preset.store`
only on a slot the owner of the unit has named as disposable.

USB: on current Windows 11 the vendor's 2014 kernel driver is refused
(Device Manager code 52), so the unit exposes no USB MIDI port to any
application. Axe-Grid works with any MIDI port Windows offers; it does not
yet talk to the device over USB by itself. Parked for now; a
user-space connection is a possible later project.

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
2. **Preset management**: the preset browser with names of all slots,
   import and export of preset, bank and bundle files, copy/swap between
   slots, the block library and global blocks, snapshots.
3. **Cabinet management**: user cab list, import/export and audition of
   impulse responses, the scratchpad slots.
4. **Scene tools**: copy and paste a scene, swap scenes, scene names,
   scene revert, per-scene X/Y copy, "set all blocks to X/Y".
5. **Block tools**: copy and paste a block or its X/Y state, reset a block
   to defaults, "edit block" recall, block undo.
6. **Grid tools**: move or shift a whole row or column, insert/delete a
   column, clear the grid, feedback loops shown on the grid.
7. **Curves**: only the Parametric EQ draws one, computed locally. The
   delay, reverb, filter, amp speaker and tone-match graphs, and the
   controllers' LFO/ADSR/sequencer displays, are absent; the device's own
   graph data is not read.
8. **Meters**: no level or gain-reduction meters anywhere.
9. **Global and I/O settings, MIDI mapping, firmware tools, preferences,
   display offset, "pause communication", keyboard-shortcut editor.**
10. **Undo/redo.**

Partly there:

- **Tone Match**: its two buttons set the capture and match parameters;
  the capture workflow and the spectrum display are missing.
- **Looper**: transport buttons and their state work; no position bar.
- **Graphic EQ and other fader pages**: faders work; no curve.
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
- The `52` currently registered commands cover a fraction of the original's
  surface; the original has roughly 56 distinct device messages.
