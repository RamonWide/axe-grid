# Command reference

Generated from the registry by `scripts/gen-command-docs.py`. Do not edit by hand;
regenerate after adding or changing a command (see `CONTRIBUTING.md`).

86 commands.

## device

### `device.list_ports`

Names of the MIDI input and output ports on this computer.

### `device.connect`

Open a pair of MIDI ports, find the Axe-Fx II on them and read its current preset.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `input` | string | yes | MIDI input port name, as listed by device.list_ports |
| `output` | string | yes | MIDI output port name |

### `device.connect_auto`

Look for an Axe-Fx II and connect to the first one that answers: the unit's own USB port first,               then every other MIDI port that has both an input and an output. Menu: Device > Find My Axe-Fx.

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

Ask the device to report what the looper is doing, so its buttons can show their state and the editor how far the loop has played. Selecting the Looper block does this by itself. The position is read as public documentation describes it and has not yet been seen on a unit.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `on` | boolean | no | false stops the reports; default true |

### `looper.press`

Press one of the looper's transport buttons. They are sent as the MIDI controllers the device assigns to the looper by default (record 28, play 29, once 30, stack 31, reverse 32, half 120, undo 121).

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `button` | string | yes | record, play, once, stack, reverse, half or undo |
| `on` | boolean | no | State to switch to; by default the button toggles |

## tonematch

### `tonematch.capture`

Start or stop the Tone Match block's capture of the reference signal, of the local signal (the preset's own sound), or of both. While a capture runs the block averages what it hears.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `signal` | string | yes | reference, local or both |
| `on` | boolean | no | false stops the capture; default true |

### `tonematch.match`

Stop both captures and have the Tone Match block work out the correction that makes the local signal sound like the reference. Changes the edit buffer only. Starting a capture is confirmed on a unit; stopping one and matching are not yet.

### `tonematch.status`

Read whether the Tone Match block is capturing the reference or the local signal. Only reads.

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

### `scene.rename`

Give a scene of the current preset a name. The device has no scene names: they are kept on this computer, by preset slot, and shown while the preset in that slot keeps its name.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `scene` | integer | no | Scene number, 1 to 8; defaults to the current scene |
| `name` | string | yes | Up to 24 characters; empty takes the name away |

### `scene.names`

The eight scenes of the current preset with the names given to them on this computer.

### `scene.copy`

Remember what makes up the current scene: which blocks are bypassed, which X/Y set each is on, the output's main level for the scene and the two scene controllers. Only reads. Menu: Scene > Copy Scene. Shortcut: Ctrl+Shift+C.

### `scene.paste`

Make a scene the same as the one last copied, in the edit buffer: bypass and X/Y of every block both have, the main level and the scene controllers. Switches to that scene. Undo takes it back. Menu: Scene > Paste Scene. Shortcut: Ctrl+Shift+V.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `scene` | integer | no | Scene number, 1 to 8; defaults to the current scene |

## preset

### `preset.names`

Read the names of stored presets without loading them. Only reads.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `first` | integer | no | First slot to read; defaults to the first one |
| `count` | integer | no | How many slots to read; defaults to all that remain |

### `preset.export`

Save a stored preset as a .syx file. Exports what is stored in the slot, not unsaved edits.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | yes | File to write |
| `number` | integer | no | Zero-based preset slot; defaults to the current preset |

### `preset.export_range`

Save a run of stored presets into a folder, one .syx file each, named by slot and preset name.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `folder` | string | yes | Folder to write into; created if missing |
| `first` | integer | no | First slot to read; defaults to the first one |
| `count` | integer | no | How many slots to read; defaults to all that remain |
| `skip_empty` | boolean | no | Leave out unnamed (empty) slots; default true |

### `preset.import`

Load a preset .syx file into the edit buffer, replacing unsaved edits. Nothing is stored until preset.store.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | yes | Preset file to load |

### `preset.copy`

Copy a stored preset to another slot on the device, overwriting what is there, and select the copy. Unsaved edits are lost.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `from` | integer | yes | Zero-based slot to copy |
| `to` | integer | yes | Zero-based slot to overwrite |

## cab

### `cab.names`

Read the names of the user cabinet slots. Only reads. Models whose cabinet memory is not known are refused.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `first` | integer | no | First user cab to read, counted from 1; defaults to 1 |
| `count` | integer | no | How many slots to read; defaults to all that remain |

### `cab.export`

Save a user cabinet (impulse response) as a .syx file.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cab` | integer | yes | User cab number, counted from 1 |
| `path` | string | yes | File to write |

### `cab.import`

Write a cabinet .syx file into a user cab slot on the device, overwriting what is there. The slot addressing is taken from the message layout and public documentation and has not been confirmed on hardware.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cab` | integer | yes | User cab number to overwrite, counted from 1 |
| `path` | string | yes | Cabinet file to send |

## library

### `library.scan`

Go on checking the names of stored presets and cabs against the device, from where the last call stopped, and correct the ones that differ. The names shown right after connecting are the ones kept on this computer from last time; this is what makes them certain. "phase" in the result is "cached" or "reading" until every slot has been read, then "verified"; a call after that reads nothing. Only reads, and only slots the model has.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `budget` | integer | no | Most names to read in this call, one request each; 0 reads none. Without it everything that is left is read |
| `first` | string | no | Which list to go on with first: "presets" (default) or "cabs" |

### `library.rescan`

Start the check of preset and cab names over from the first slot, for when presets or cabs were changed on the device itself or by another program. The names stay on show meanwhile. With a budget the rest is left to library.scan. Only reads.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `budget` | integer | no | Most names to read in this call, one request each; 0 reads none. Without it everything that is left is read |
| `first` | string | no | Which list to go on with first: "presets" (default) or "cabs" |

## edit

### `edit.undo`

Take back the last edit to a parameter, bypass, X/Y state, the grid or the preset name, by sending the device what was there before. An edit made in another scene or on a block's other X/Y set switches back to it first. Loading a preset empties the history. Menu: Edit > Undo. Shortcut: Ctrl+Z.

### `edit.redo`

Make the edit that was last undone again. Menu: Edit > Redo. Shortcut: Ctrl+Y.

### `edit.history`

The edits that can be undone and redone, the next one first.

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

### `param.set_displayed`

Set a continuous parameter to a value in the unit the device displays it in, such as -40 for "-40.0 dB" or 1200 for "1200 Hz". The device limits values outside the parameter's range.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `param` | integer or string | yes | Parameter ID or symbolic name such as "DISTORT_DRIVE" |
| `value` | number | yes | The value as it should read on the device |

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

Set an enumerated parameter by the text of the choice. The list of choices is read from the device if it has not been yet.

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

### `grid.move_blocks`

Move several blocks and shunts together by the same number of columns and rows, as one edit that undoes in one step. All of them move or none does: it is refused if any would leave the grid or land on a block that is not itself being moved. Cables between the moved blocks go with them. A cable to a block that stays is kept when the move is only up or down, and dropped when the blocks change column, because a cable only joins neighbouring columns. Changes the grid of the edit buffer on the device; nothing is stored.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `cells` | array | yes | The cells to move, each as {"column": 0 to 11, "row": 0 to 3} |
| `dx` | integer | no | Columns to the right, -11 to 11; negative is left. Defaults to 0 |
| `dy` | integer | no | Rows down, -3 to 3; negative is up. Defaults to 0 |

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

## block

### `block.copy`

Remember every setting of a block, to paste into a block of the same kind in this or another preset. Bypass and modifiers are not part of it. Only reads, unless both sets are asked for: then the block is switched to its other X/Y set and back, which can be heard. Menu: Block > Copy Block. Shortcut: Ctrl+C.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `sets` | string | no | "current" (default): the X/Y set the block is on; "both": X and Y |

### `block.paste`

Give a block the settings last copied from a block of the same kind, in the edit buffer. One copied set goes to the X/Y set the block is on; a copy of both replaces both. Only parameters that differ are sent, the block's type first. Undo takes it back. Menu: Block > Paste Block. Shortcut: Ctrl+V.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

### `block.copy_x_to_y`

Make a block's Y set the same as its X set. The block ends on the set it was on. Menu: Block > Copy X to Y.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

### `block.copy_y_to_x`

Make a block's X set the same as its Y set. The block ends on the set it was on. Menu: Block > Copy Y to X.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |

## modifier

### `modifier.open`

Read the modifier of a parameter: its source, range, curve points and the rest, each with the device's own text, and a sketch of the curve. Only reads. Refused for a parameter that takes no modifier, and by the device once the preset holds 24 modifiers and this parameter has none.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Wah 1"; defaults to the selected block |
| `param` | integer or string | no | Parameter ID or symbolic name such as "WAH_CONTROL"; defaults to the parameter whose modifier is open |

### `modifier.close`

Close the modifier window.

### `modifier.attach`

Attach a controller to a parameter in the edit buffer by setting the modifier's source. The device names the sources; 0 is none, and the numbering is the device's own. Sources 1 to 23 are confirmed on a unit; none past 23 is sent.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Wah 1"; defaults to the selected block |
| `param` | integer or string | no | Parameter ID or symbolic name such as "WAH_CONTROL"; defaults to the parameter whose modifier is open |
| `source` | integer or string | yes | Source number from 1, or a name listed by modifier.sources |

### `modifier.remove`

Take the modifier off a parameter in the edit buffer by setting its source to none. The device puts the modifier's other fields back to their defaults; undoing the removal brings them back with the source.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Wah 1"; defaults to the selected block |
| `param` | integer or string | no | Parameter ID or symbolic name such as "WAH_CONTROL"; defaults to the parameter whose modifier is open |

### `modifier.set`

Set one field of a modifier to a raw value and return what the device applied, with its text. Setting the source to 0 removes the modifier, as modifier.remove does. The other fields need a source first: the device keeps none of them without one.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Wah 1"; defaults to the selected block |
| `param` | integer or string | no | Parameter ID or symbolic name such as "WAH_CONTROL"; defaults to the parameter whose modifier is open |
| `field` | string | yes | source, min, max, start, mid, end, slope, damping, auto_engage, pc_reset, off_value, scale or offset |
| `value` | integer | yes | Raw value, 0 to 65534; for source, auto_engage and pc_reset, the choice number |

### `modifier.sources`

The modifier sources the device has named so far in this session, by number. The device offers no list; a name is learned when a modifier that uses the source is read or set.

## block

### `block.read_lists`

Read from the device the choices of every list of a block: amp models, effect types, cabs and the like. Each list is read once and kept until the device disconnects. Only reads. The cab list is one request per cab, so over a 5-pin MIDI cable it takes a while.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `block` | integer or string | no | Block ID or name such as "Amp 1"; defaults to the selected block |
| `budget` | integer | no | Most requests to send in this call; "missing" in the result counts the lists still to read. Without it everything is read |

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

Check a .syx file: message count, checksums, whether it is a preset or a cabinet IR, and the name stored in it.

| Parameter | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | yes | Path of the .syx file |

## app

### `app.inspect`

The whole editor state as JSON: connection, preset, grid, block states, selected block.

### `app.commands`

Every command with its parameters and whether it can run right now.
