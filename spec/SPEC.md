# Axe-Edit 3.15 — functional specification

This document describes observable behavior and data formats of Fractal Audio's
Axe-Edit 3.15.00 for Windows (`Axe-Edit.exe`, SHA-256 `c37d3cc5…a646652`,
32-bit x86 PE). It is written in prose and tables, with no code taken from the
original, so that an implementation can be written from it alone.

Every statement carries a confidence tag:

- **[B]** read from the binary through REA + Ghidra 12.1.4. The REA Evidence ID
  (`ev_…`, abbreviated) is given; full records are kept with the analysis material, outside this repository.
- **[F]** observed in real files on disk (`.syx` exports, the app's own cache).
- **[P]** from Fractal's public MIDI documentation, recalled from memory and
  *not* confirmed in the binary. Treat as likely but unverified.
- **[H]** observed on hardware (Axe-Fx II XL, firmware 32.00, 2026-10-11).
- **[?]** unknown.

## 1. Architecture of the original

[B] The program is a JUCE application. Run-time type names show its parts:
a comm thread (`CommPortThread`, `MidiProcessing`, `SysExMultiMsgTimer`), a
device model (`EffectManager`, `FASEffectParam`, `PresetManager`,
`PresetStateMachine`, `PresetTranslator`), and views (`GridUnitContent`,
`EffectEditorContent`, `ControlUnitContent`, `PresetListContent`,
`BankManagerListContent`, `TunerContent`, `ModifierDlgContent`,
`SequencerContent`, `BlockLibraryManager`, `AxeChange*`, `Prefs*Content`).

[B] The editor UI is data-driven. A ZIP embedded in the executable holds
`__block_layout.xml` (which widget shows which parameter, per effect, per
page), `__components.xml` (skin geometry and colors), fonts and 415 images.
Those files are Fractal's creative work and are **not** reused here; only the
fact that pages are generated from parameter metadata is carried over.

[B] Supported device models, from the layout file's `Device` entries: `0x03`
(Axe-Fx II), `0x06` (Axe-Fx II XL), `0x07` (Axe-Fx II XL+), firmware major 25.

## 2. SysEx framing

[B ev_e0ec1fde] [F] Every message is

    F0 00 01 74 <model> <function> <payload…> <checksum> F7

`00 01 74` is the manufacturer ID. The checksum is the XOR of all bytes from
`F0` through the last payload byte, masked with `0x7F`.

[B ev_4659e67f] Messages with function `0x0D`, `0x0E` and `0x10` are accepted
without checksum verification; all others must match.

[B ev_2bc8ee80, ev_1e3af598] Multi-byte integers are split into 7-bit groups,
least significant group first: `byte[i] = (value >> 7·i) & 0x7F`.

[B ev_e0ec1fde] Before a device is identified the app sends function `0x00`
with model `0x7F` and an empty payload. [B ev_6e7f5c31] The answer is a
function-`0x64` status message; the app adopts the model byte of that answer.

## 3. Function `0x64`: status reply

[B] Payload: `<function being answered> <code>`. Code `0` is success.
The binary names the codes (order as stored, numeric values **[?]**):
BAD_CHKSUM, WRONG_MODEL_NUM, WRONG_SYSEX_ID, MSG_NOT_RECOGNIZED, BAD_ARGUMENT,
INVALID_PARAMID, INVALID_FXID, NO_MODIFIERS_LEFT, FX_NOT_IN_USE,
FX_NOT_ROUTABLE, WRONG_COUNT, DSP_OVERLOAD, BAD_GRID_POS, INVALID_PATCHNUM,
FUNCTION_FAIL, BAD_MSG_LENGTH, ILLEGAL_MSG, BAD_IMAGE_CHKSUM,
IMAGE_SIZE_INCORRECT, BUFFER_OVERRUN, NOT_RDY_FOR_FW_UPD, INVALID_MODIFIERID,
INVALID_CABNUM, FIRMWARE_ALREADY_CURRENT, INVALID_BANKNUM, NULL_DATA,
CMD_NOT_SUPPORTED, FLASH_WRITE_FAILED.

## 4. Messages

Offsets below are payload offsets (payload byte 0 is message byte 6). `n×7`
means an integer in *n* 7-bit groups. Layouts come from the per-message
descriptor tables in the binary [B ev_3105d541 for `02`; the batch records
`evidence/calls/batch_senders.json` and `batch_parsers.json` (ev_4218fc92)
for the rest].

### 4.1 Requests the app sends

| Fn | Payload | Meaning | Conf. |
|----|---------|---------|-------|
| `00` | — (model `7F`) | Who is there? | B |
| `01` | effect 2×7 | Begin parameter dump for a block; answered by `32` … items … `33` | B (purpose P) |
| `02` | effect 2×7, param 2×7, value 3×7, set 1 | Get (`set=0`) or set (`set=1`) a block parameter | B |
| `05` | block 2×7, cell 1 | Place a block in a grid cell | B layout, P meaning |
| `06` | a 1, b 1, c 1 | Connect/disconnect two grid cells | B layout, P meaning |
| `07` | effect 2×7, param 2×7, selector 2×7, value 3×7, set 1 | Get/set a modifier field | B |
| `08` | — | Firmware version | B |
| `09` | 32 characters | Set preset name | B |
| `0E` | — | Per-block state list | B |
| `0F` | — | Preset name | B |
| `11` | effect 2×7, xy 1, set 1 | Get/set a block's X/Y state | B layout, P meaning |
| `13` | — | CPU load | B |
| `14` | — | Current preset number | B |
| `16` | effect 2×7, param 2×7 | Parameter metadata | B |
| `17` | — | MIDI channel; reply is one byte, zero-based | B layout, H meaning |
| `1D` | a 1, b 1 | Store preset (number in two bytes) | B layout, P meaning |
| `20` | — | Grid layout and routing | B |
| `28` | effect 2×7, param 2×7 | Enum strings of a parameter | B |
| `29` | scene 1 (`7F` = query) | Get/set scene | B layout, P meaning |
| `2A` | — | Preset-edited flag | B layout, P meaning |
| `2E` | effect 2×7, param 2×7, value 5×7 | Set parameter with full-width value; answered by `02` | B |
| `3C` | a 1, b 1 | Select preset (number in two bytes) | B layout, P meaning |
| `42` | — | Disconnect | B layout, P meaning |
| `47` | — | Device capability words (five 2×7 values) | B layout, ? meaning |

Also sent, layout known but purpose not yet determined [B, ?]: `03`, `12`,
`15`, `18`, `19`, `1A`, `1B`, `1F`, `22`, `23`, `24`, `2B`, `2C`, `2F`, `30`,
`35`, `36`, `37`, `38`, `39`, `3A`, `3B`, `3E`, `41`, `46`, `5B`.

### 4.2 Replies and unsolicited messages

| Fn | Payload | Meaning | Conf. |
|----|---------|---------|-------|
| `01` | effect 2×7, param 2×7, value 3×7, flag 1, a 2×7, b 2×7, text… | One item of a parameter dump | B |
| `02` | same seven fields as `01` | Parameter value; `text` is the device's display string | B |
| `07` | effect 2×7, param 2×7, selector 2×7, value 3×7, text… | Modifier field | B |
| `08` | major 1, minor 1, four more bytes | Firmware version = major + minor/100 | B |
| `0D` | note 1, string 1, cents 1 (no checksum) | Tuner | B layout, P meaning |
| `0E` | N × (32-bit word as 5×7) (no checksum) | Per-block state, see 4.3 | B |
| `10` | — (no checksum) | Tempo beat | B |
| `13` | value 1 | CPU load | B |
| `14` | a 1, b 1 | Preset number; also sent unsolicited | B |
| `16` | type 5×7, min, max, scale, step (each an IEEE-754 single as 5×7) | Parameter metadata | B |
| `20` | 48 × (block 2×7, routing 2×7) | Grid, see 4.4 | B |
| `21` | — | Front panel changed something | B |
| `23` | status 1 | Looper status | B |
| `28` | NUL-terminated strings back to back | Enum strings | B |
| `29` | scene 1 | Scene | B |
| `2A` | flag 1 | Preset edited | B |
| `32` / `33` | count / — | Start / end of a multi-message list | B |
| `64` | function 1, code 1 | Status, section 3 | B |

[H] The preset number in `14` is sent most-significant 7 bits first, unlike
every other field. [H] `3C` uses the same order; [P] so does `1D`.

[H] Commands with no reply of their own (`05`, `09`, `3C`, `42`) are
acknowledged with a `64` status carrying the function and code 0. After `3C`
the device also pushes `14`, `0F`, `0E` and `29` for the new preset without
being asked. After a scene change it pushes `21`, `0E` and `29`.

[H] `05` (block 2×7, cell 1) puts a block instance into a grid cell of the
edit buffer; the cell index is column × 4 + row. A block placed in the first
column is connected to the input automatically.

[H] `11` with set = 1: xy 0 selects X, 1 selects Y; the reply (block 2×7,
xy 1) reports the state now in force. `09`, `29`, `05`, `02` and `11` change
only the edit buffer and raise the preset-edited flag; selecting the preset
again discards them.

[H] `02` with set = 1 applies the value and replies with the value in force
and its display text. In `01` and `02` replies the one-byte field after the value is the value
scaled to 7 bits, and the last 2×7 field is the highest choice index of an
enumeration (0 for continuous parameters). The `32` list header carries the
item count. Raw values run linearly from 0 to 65534, but the 3×7 field is
21 bits wide and a parameter the firmware does not define can read higher.

[B ev_4659e67f] Reply matching: a request for `2E` is satisfied by `02`; `3B`
by `07`; `0E` by `29`; `2B` by `2C`; `01`/`12`/`2B` by a `32` … `33` bracket.

[B] The tuner is not switched by a SysEx message. The original sends a MIDI
control change, controller 15, on the channel reported by `17`: value 127 to
open its tuner window, a low value to close it. [H] While the tuner is on
the device streams `0D` about 25 times a second and stops when it is
switched off. [P] In `0D`, note 0 is A and counts semitones up; the third
byte is 0 to 127 with 63 in tune.

[B] Tempo is the `CONTROLLERS_TEMPO` parameter of block 141; [H] its raw
value is beats per minute, 30 to 250, set with `02` like any parameter. The
original's tap button measures the time between two clicks (less than two
seconds apart) in the editor and sets the resulting tempo; there is no tap
message. `10` arrives on every beat.

[H] Blocks 139 (input gate), 140 (output mixer) and 141 (controllers)
answer `01` and `02` in every preset; they are not grid cells.

[H] `06` (from cell, to cell, connect 1 / disconnect 0) adds or removes the
cable between two cells of adjacent columns; `05` with block 0 empties a
cell and removes its cables on both sides, and placing a block that is
already in the grid moves it (cables are not carried along).

Shunts are blocks 200 to 235, and each one in a preset has its own ID; the
device's stored presets never repeat one. [?] A unit stopped answering MIDI
some seconds after ID 200 had been placed in eleven cells of one connected
row; the same row with eleven different IDs was stable. Never repeat a
shunt ID.

### 4.3 Block state word (function `0E`)

[B] Each item is one 32-bit word. [H] Bits 24–31: block ID. Bits 8–14:
bypass CC number. Bits 16–23: X/Y CC number. Bit 0: 1 = engaged,
0 = bypassed. Bit 1: 1 = X, 0 = Y. The device follows the list with a
function-`29` message carrying the current scene.

### 4.4 Grid (function `20`)

[B] 48 cells of 4 bytes: block ID then routing word. [H] Cells run column by
column (12 columns of 4 rows). Routing bits 0–3: the cell takes input from
row 1–4 of the column to its left. Block ID 0 is empty; 200–235 are shunts.

### 4.5 File transfer brackets

[B] [F] Presets travel as `77` (header) + N × `78` (data) + `79` (checksum
footer, 3 bytes). Data chunks carry a 2×7 count and that many 16-bit words as
3×7 each: 64 words per chunk on the II, 1024 on the XL/XL+. Cabinet IRs use
`7A`/`7B`/`7C` the same way, with a 5-byte footer; other brackets are
`74`/`75`/`76`, `61`/`62`/`63` and `67`/`68`/`69` [?].
[?] The layout of the words inside a preset is not yet mapped.

## 5. Identifiers

[B] Block IDs 100–170 and all per-effect parameter IDs are stored in the
binary as `{id, name}` tables and are reproduced in
`crates/axegrid-model/data/blocks.json` and `params.json`. Effect *types* (2 = Compressor … 39 = Tone Match) select
the parameter table; two instances of a block share a type. [H] ID 255 is the
bypass switch shared by all effects: 0 engaged, 1 bypassed. Setting it is how
a block is bypassed; the per-effect `*_BYPASS` names in the tables do not do
that on firmware 32 (the amp's reads back as an undefined parameter). Unknown names resolve to 666.

## 6. Definition cache file

[F] Layout worked out from the app's own cache file, which parses end to end
with it; the five numeric fields match the function-`16` reply and the string
list matches function `28` [B ev_4218fc92].
`%APPDATA%\Fractal Audio\Axe-Edit\effectDefinitions_<model>.cache`,
little-endian:

- 46-byte header; the last four bytes are the number of effect records.
- Per effect: `u32 type`, `u32 count`, then `count` parameter records.
- Per parameter: `u16 id`, `u32 type word`, `f32 min`, `f32 max`,
  `f32 scale`, `f32 step`, `u32 string count`, then for each string
  `u32 length` + bytes (no terminator), then 6 bytes [?] (zero in practice).

Type word `0x10` marks an enumeration whose raw value is an index into the
string list. Other type words encode unit and taper; bit meanings [?].
`scale` multiplies the engineering value for display (1000 for ms/Hz ranges).

## 7. Settings

[F] `Axe-Edit.settings` is an XML list of `<VALUE name= val=>` pairs, among
them `MIDI_IN`, `MIDI_OUT`, `MAIN_WINDOW_STATE`, the directory settings and
the prompt toggles.

## 8. Not covered

Preset word layout, block library files, Axe-Change, firmware update, IR
capture/conversion, Tone Match, the looper/sequencer UIs, auto-update.
