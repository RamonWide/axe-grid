# Changelog

All notable changes to Axe-Grid are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). While the version is below 1.0,
command ids and the control protocol may still change between minor versions; such changes are
listed under "Changed".

## [Unreleased]

### Added

- **Undo and redo** (Ctrl+Z and Ctrl+Y, the Undo and Redo buttons, the Edit menu) for parameter
  changes, bypass, X/Y, the preset name and everything done on the grid: placing, moving and
  removing blocks, and cables. Turning one knob is one step. An edit made in another scene, or on a
  block's other X/Y set, switches back to it first. Loading a preset empties the history; saving
  does not. Commands `edit.undo`, `edit.redo` and `edit.history`.
- **Lists are read from the unit.** The names in every list (amp models, effect types, cabs and
  the rest) now come from your own unit when you open a block, and match its firmware. Axe-Grid
  no longer needs the original editor to have been installed. Command `block.read_lists`;
  `param.set_choice` reads the list it needs by itself.
- **Preset manager** (the Presets button): the names of every preset slot, read without loading
  them, with a search box; load, copy to another slot, save the current sound to another slot,
  export to a `.syx` file, import a file into the edit buffer, and back up every preset to a
  folder. Commands `preset.names`, `preset.export`, `preset.export_range`, `preset.import` and
  `preset.copy`.
- **Cabinet manager** (the Cabs button) for the XL and XL+: the names of the 1024 user cabs,
  export to a `.syx` file, import a file into a slot, and "use in cab block" to hear a cab.
  Commands `cab.names`, `cab.export` and `cab.import`. Import has not yet been confirmed on a
  unit; the window says so.
- **Curves** beside the controls of thirteen block types: Graphic EQ, Filter, the delay's and
  reverb's EQ pages, the amp's Speaker page, the cab's cuts, Crossover, Compressor,
  Gate/Expander, the input gate, Multiband Compressor and the two ADSR envelopes, next to the
  Parametric EQ that had one. All are computed from the settings.
- **Type a value**: double-click the value under a knob or a fader, type the number you want and
  press Enter (`param.set_displayed`). The unit limits a value that is out of range.
- **The unit is found automatically** when the app starts: its USB port first, then every other
  MIDI port. "Find my Axe-Fx" does the same on demand (`device.connect_auto`).
- **The app icon** on the window, the taskbar and the Windows executables.
- **A Windows installer**, `axegrid-setup.exe`, in the Windows download: it copies the app into
  your user account, puts a shortcut on the desktop and in the Start menu, and lists Axe-Grid
  under Installed apps so it can be removed there. It needs no administrator rights.
- `sysex.inspect_file` reports the name stored in a preset or cabinet file and whether its
  checksum holds.

### Fixed

- Graphic EQ pages, and the amp's graphic EQ, showed no faders on a unit when the type name was
  longer than the page expected ("10 BAND VAR Q" against "10 Band").

### Changed

- **The window no longer waits for the unit.** The conversation with the unit runs on its own
  thread, so a unit that answers slowly, or not at all, cannot freeze the window. The status bar
  shows when the unit is being waited for. Dragging a knob stays smooth on a slow connection: the
  knob follows the pointer and the unit is sent the latest position as fast as it answers.
- The app starts at once and looks for the unit in the background.
- Axe-Grid no longer loads the original editor's definition file when it starts.
  `definitions.load` and `definitions.load_installed` remain for those who want them.
- `app.inspect` and `state.inspect` also report `history` and `lists`.
- Faders are twice as tall, and a fader's cap now follows the pointer over the length of its
  track.

- Updated the window toolkit (egui and eframe) from 0.31 to 0.36 and the MIDI library (midir) from
  0.10 to 0.11. Building from source now needs Rust 1.95 or newer.
- Requests on the control channel are answered while the window is minimized or hidden.

## [0.1.0] - 2026-10-11

The first public version: an early alpha.

### Added

- Desktop app for Windows, macOS and Linux with the 12×4 routing grid, preset and scene selection,
  rename and save, and a CPU meter.
- Block editor with pages for 38 block types: knobs, faders, switches and lists, the unit's display
  text under each control, bypass and X/Y, and the mixer controls on the right of every page.
- Grid editing: place, move and remove blocks and shunts, cable cells through input and output
  handles, and one-click runs to the input and the output.
- Input noise gate and output mixer, opened from the INPUT and OUTPUT bars.
- Tuner, tap tempo and BPM in the top bar; looper transport buttons; Tone Match capture buttons;
  a frequency-response curve for the Parametric EQ.
- A registry of 52 commands shared by the window, `axegrid-cli`, the loopback JSON control channel
  and the MCP server.
- A built-in simulated device.
- `spec/SPEC.md`, a functional specification of the Axe-Fx II SysEx messages used.

### Known limitations

- Tested on one Axe-Fx II XL (firmware 32.00) on Windows. The macOS and Linux builds, and the
  Axe-Fx II Mark I/II and XL+ models, are untested.
- No modifiers, preset or cab management, undo, meters or global settings. See `ROADMAP.md`.

[Unreleased]: https://github.com/RamonWide/axe-grid/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/RamonWide/axe-grid/releases/tag/v0.1.0
