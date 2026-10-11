# Changelog

All notable changes to Axe-Grid are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). While the version is below 1.0,
command ids and the control protocol may still change between minor versions; such changes are
listed under "Changed".

## [Unreleased]

## [0.1.0] - not yet released

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
