# Clean-room rules

Axe-Grid is an independent project that interoperates with hardware made by
Fractal Audio Systems. It is written from a functional specification, not
from the original editor's code, and it ships none of the original's
artwork. This page explains how that is kept true, so that every
contributor works the same way.

## Two roles, two sets of material

| Role | May read | Produces |
|---|---|---|
| **Analyst** | The original program, decompiler output, captured MIDI | `spec/SPEC.md`: prose and tables describing behaviour and formats, each fact tagged with its source |
| **Implementer** | `spec/`, public documentation, this repository | Code, tests, docs |

An implementer does not open decompiler output. An analyst does not paste
code or pseudocode into the spec. Analysis material lives outside this
repository and is never committed; `.gitignore` excludes the folder names
used for it.

### How the first version was made

The first version of this repository was produced by a single AI agent in one
session that did both jobs: it analysed the original with REA and Ghidra,
wrote the spec, then wrote the code. The code was written from the spec and
contains no translated pseudocode, but this is not the strict two-team
separation a formal clean-room process uses. From here on, keep the roles in
separate sessions or with separate people. The preset and cabinet library
(spec sections 4.5 and 4.6) was added the same way as the first version: one
agent session read the message-layout tables recorded from the original,
confirmed what each message means with read-only requests to a unit, wrote
the spec sections, and then wrote the code from them. Anyone who wants stricter
provenance can have a fresh implementer, who has seen only `spec/`,
re-derive `axegrid-sysex` and `axegrid-model` and compare.

## Never in this repository

- Images, icons, fonts, skins, layout or configuration files, sample presets,
  cabinet IRs, firmware, or release notes from Fractal Audio software.
- Decompiler or disassembler output, or code translated from it.
- The original's user-interface text (prompts, tooltips, help) or artwork
  made by copying or tracing it. Control labels and page arrangement are the
  one exception, described below.
- Factory content read from a device: amp and cabinet model names, preset
  names. Axe-Grid shows these at run time because the user's own device or
  the user's own installed copy of the original editor supplies them; it does
  not ship them.

`cargo test` fails if a binary asset type appears in the tree without an
entry in `ATTRIBUTION.md`.

## Look and feel

Axe-Grid aims to feel familiar to people who know the original editor, with
artwork that is entirely its own.

- **Kept, because it is how the device works:** a 12 by 4 routing grid with
  the input on the left and the output on the right, cables between cells,
  a preset selector and eight scene buttons across the top, the selected
  block's controls underneath, X/Y and bypass switches on each block,
  rotary controls for continuous values and lists for choices.
- **Our own:** every pixel. The palette, tile shapes and colours, the knob,
  the lettering, icons and spacing live in `crates/axegrid-ui-egui/src/theme.rs`
  and `knob.rs` and are drawn with vector primitives at run time. There are
  no bitmaps. Fonts are the open ones bundled with egui.
- **Matched to the original, for familiarity:** the names of the controls
  and which page and row each one sits on
  (`crates/axegrid-model/data/layouts.json`). See below.
- **Not done:** matching the original's colours, knob artwork, fonts, logo or
  block abbreviations. Do not open the original's skin files or screenshots
  to "get closer".

Whoever works on the shell should not have the original editor's assets in
front of them, for the same reason the implementer does not read decompiler
output.

## Data that originates in the original

Two kinds of data in this repository come from the original program rather
than from this project's own writing. Both are listed in `ATTRIBUTION.md`.

**Parameter and block identifiers** (`crates/axegrid-model/data/blocks.json`,
`params.json`). These pair the numeric IDs the device's MIDI protocol uses
with short symbolic names such as `DISTORT_DRIVE`. The numbers are facts
required to talk to the hardware. The names are identifiers recovered from
the original program; they are kept because scripts and agents need stable
names. Nothing else depends on their spelling: they can be replaced with
other names while keeping the numbers. The friendly labels and block display
names in `blocks.rs` are this project's own.

**Control labels and page layouts** (`crates/axegrid-model/data/layouts.json`).
So that players need not relearn where things are, the control labels
("Input Drive", "Master Volume") and the grouping of controls into pages and
rows follow the original editor. The file was produced by converting the
control-layout description embedded in the original program: for each
control, the parameter it edits, its label, its kind (knob, list, switch,
fader), its page and row, and the condition under which it shows. Pixel
positions, skin references, graphics, tooltips and help text were not taken.
The labels are short functional names, most of which also appear on the
device's own screen and in its public manual. The file is replaceable: write
the pages independently (from the manual, or from how the device groups
parameters on its own screen) and the engine uses them; with no layout for a
block it falls back to a plain list of every parameter.

## Names and trademarks

"Axe-Fx", "Axe-Edit" and "Fractal Audio" are trademarks of Fractal Audio
Systems. They are used in this project only to say what the software works
with. Axe-Grid is not affiliated with, sponsored by or endorsed by Fractal
Audio Systems, and the documentation says so wherever the names appear
prominently.

## Third-party material

Anything not written for this project needs a permissive license, its license
file next to it, and a row in `ATTRIBUTION.md` in the same change.
