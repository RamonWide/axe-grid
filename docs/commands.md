# Command reference

Generated from the registry by `scripts/gen-command-docs.py`. Do not edit by hand;
regenerate after adding or changing a command (see `CONTRIBUTING.md`).

52 commands.

## device

### `device.list_ports`

Names of the MIDI input and output ports on this computer.

### `device.connect`

Open a pair of MIDI ports, find the Axe-Fx II on them and read its current preset.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `input` | string | yes | MIDI input port name, as listed by device.list_ports |
| `output` | string | yes | MIDI output port name |

### `device.connect_simulator`

Connect to a built-in stand-in device with three presets. No hardware is touched. Menu: Device > Connect to Simulator.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `model` | string | no | "II", "XL" (default) or "XL+" |

### `device.disconnect`

Tell the device the editor is leaving and close the ports. Menu: Device > Disconnect.

### `device.refresh`

Re-read preset number, name, scene, grid and block states from the device. Menu: Device > Refresh Preset. Shortcut: F5.

### `device.cpu_load`

Ask the device for its current DSP load. Menu: Device > Read CPU Load.

### `device.poll`

Take in what the device sent by itself: tuner readings, tempo beats, and changes made on its front panel (which re-read the preset). Sends nothing unless a re-read is needed.

## tuner

### `tuner.on`

Switch the device's tuner on with MIDI controller 15 on the device's channel. Readings then arrive through device.poll.

### `tuner.off`

Switch the device's tuner off.

### `tuner.toggle`

Switch the device's tuner on or off. Menu: Device > Tuner. Shortcut: T.

## looper

### `looper.watch`

Ask the device to report what the looper is doing, so its buttons can show their state. Selecting the Looper block does this by itself.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `on` | boolean | no | false stops the reports; default true |

### `looper.press`

Press one of the looper's transport buttons. They are sent as the MIDI controllers the device assigns to the looper by default (record 28, play 29, once 30, stack 31, reverse 32, half 120, undo 121).

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `button` | string | yes | record, play, once, stack, reverse, half or undo |
| `on` | boolean | no | State to switch to; by default the button toggles |

## tempo

### `tempo.get`

Read the tempo of the current preset in beats per minute.

### `tempo.set`

Set the tempo in the edit buffer.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `bpm` | integer | yes | Beats per minute, 30 to 250 |

### `tempo.tap`

Tap twice within two seconds: the time between the taps becomes the tempo. Menu: Device > Tap Tempo. Shortcut: Space.

## preset

### `preset.select`

Load a preset on the device and read it back.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `number` | integer | yes | Zero-based preset number |

### `preset.next`

Load the preset after the current one. Menu: Preset > Next Preset. Shortcut: Ctrl+Right.

### `preset.previous`

Load the preset before the current one. Menu: Preset > Previous Preset. Shortcut: Ctrl+Left.

### `preset.rename`

Change the name of the preset in the edit buffer. Not saved until preset.store.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | yes | New name, 1 to 32 printable ASCII characters |

### `preset.store`

Write the edit buffer to a preset slot on the device, overwriting what is there. Menu: Preset > Save Preset. Shortcut: Ctrl+S.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `number` | integer | no | Destination slot; defaults to the current preset |

## scene

### `scene.select`

Switch to one of the preset's eight scenes.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `scene` | integer | yes | Scene number, 1 to 8 |

### `scene.next`

Switch to the next scene, wrapping from 8 to 1. Menu: Preset > Next Scene. Shortcut: Ctrl+Up.

### `scene.previous`

Switch to the previous scene, wrapping from 1 to 8. Menu: Preset > Previous Scene. Shortcut: Ctrl+Down.

## block

### `block.select`

Make a block of the current preset the edited one and read all its parameters. "Input Gate", "Output" and "Controllers" are part of every preset.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | yes | Block ID or name such as "Amp 1" |

### `block.deselect`

Close the block editor. Menu: Block > Deselect. Shortcut: Escape.

### `block.refresh`

Re-read every parameter of the selected block. Menu: Block > Refresh Parameters.

### `block.bypass`

Bypass a block.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

### `block.engage`

Take a block out of bypass.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

### `block.toggle_bypass`

Bypass the block if it is engaged, engage it if it is bypassed. Menu: Block > Toggle Bypass. Shortcut: B.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

### `block.set_xy`

Switch a block between its X and Y parameter sets.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `state` | string | yes | "X" or "Y" |

### `block.toggle_xy`

Switch the block to its other parameter set. Menu: Block > Toggle X > Y. Shortcut: X.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

## param

### `param.get`

Read one parameter: raw value plus the device's own display text.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |

### `param.set`

Set a parameter to a raw value and return what the device applied.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |
| `value` | integer | yes | Raw value, 0 to 65534; for enumerations, the choice index |

### `param.set_normalized`

Set a continuous parameter by knob position.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |
| `position` | number | yes | 0.0 (minimum) to 1.0 (maximum) |

### `param.nudge`

Move a continuous parameter up or down in steps of one percent of its range.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |
| `steps` | number | yes | -100 to 100; negative moves down |

### `param.set_choice`

Set an enumerated parameter by the text of the choice. Needs loaded definitions.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |
| `choice` | string | yes | Choice text, e.g. an amp model name |

## grid

### `grid.place_block`

Put a block in a grid cell of the edit buffer. A block that is already in the grid moves. Only the first column is cabled on arrival; use grid.connect for the rest.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | yes | Block ID or name |
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |

### `grid.place_shunt`

Put a shunt, a plain pass-through, in a grid cell.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |

### `grid.clear_cell`

Remove whatever block or shunt sits in a grid cell.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |

### `grid.move_block`

Move a block or shunt to an empty cell. Within a column its cables move with it; across columns the device drops them and they have to be drawn again.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |
| `to_column` | integer | yes | Destination column, 0 to 11 |
| `to_row` | integer | yes | Destination row, 0 to 3 |

### `grid.connect_to_input`

Run a cable from the input to a block along its row, adding shunts in the empty cells on the way.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |

### `grid.connect_to_output`

Run a cable from a block to the output along its row, adding shunts in the empty cells on the way.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |

### `grid.connect`

Cable a cell to a cell in the column on its left.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |
| `from_row` | integer | yes | Row, 0 to 3, of the source cell in the column to the left |

### `grid.disconnect`

Remove the cable between a cell and a cell in the column on its left.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `column` | integer | yes | 0 to 11, left to right |
| `row` | integer | yes | 0 to 3, top to bottom |
| `from_row` | integer | yes | Row, 0 to 3, of the source cell in the column to the left |

## definitions

### `definitions.load`

Load parameter ranges and choice lists from an effectDefinitions_NN.cache file.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | yes | Path of the cache file |

### `definitions.load_installed`

Load the definition cache an Axe-Edit installation left on this computer. Menu: File > Load Installed Definitions.

### `definitions.describe`

Ranges, steps and choice lists of every parameter of a block type.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name; defaults to the selected block |

## catalog

### `catalog.blocks`

Every block the device family offers, with its ID.

### `catalog.params`

Parameter IDs and symbolic names of a block type.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name; defaults to the selected block |

## sysex

### `sysex.inspect_file`

Check a .syx file: message count, checksums, and whether it is a preset or a cabinet IR.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | yes | Path of the .syx file |

## app

### `app.inspect`

The whole editor state as JSON: connection, preset, grid, block states, selected block.

### `app.commands`

Every command with its parameters and whether it can run right now.
