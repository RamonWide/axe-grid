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
| `03` | a 1, b 1 (preset number in two bytes) | Send a stored preset as a dump; answered by `77` … `78` … `79` | B layout, H meaning |
| `05` | block 2×7, cell 1 | Place a block in a grid cell | B layout, P meaning |
| `06` | a 1, b 1, c 1 | Connect/disconnect two grid cells | B layout, P meaning |
| `07` | effect 2×7, param 2×7, selector 2×7, value 3×7, set 1 | Get (`set=0`) or set (`set=1`) a modifier field, see 4.7 | B layout, H get, P set |
| `08` | — | Firmware version | B |
| `09` | 32 characters | Set preset name | B |
| `0E` | — | Per-block state list | B |
| `0F` | — | Preset name | B |
| `11` | effect 2×7, xy 1, set 1 | Get/set a block's X/Y state | B layout, P meaning |
| `12` | index 2×7 | Name of a cabinet by its position in the cab list | B layout, H meaning |
| `13` | — | CPU load | B |
| `14` | — | Current preset number | B |
| `15` | a 1, b 1 (preset number in two bytes) | Name of a stored preset, without loading it | B layout, H meaning |
| `16` | effect 2×7, param 2×7 | Parameter metadata | B layout, H meaning |
| `17` | — | MIDI channel; reply is one byte, zero-based | B layout, H meaning |
| `19` | user-cab number, laid out per model (4.5) | Send a user cab as a dump; answered by `7A` … `7B` … `7C` | B layout, H meaning (XL) |
| `1D` | a 1, b 1 | Store preset (number in two bytes) | B layout, P meaning |
| `20` | — | Grid layout and routing | B |
| `28` | effect 2×7, param 2×7 | Enum strings of a parameter | B layout, H meaning |
| `29` | scene 1 (`7F` = query) | Get/set scene | B layout, P meaning |
| `2A` | — | Preset-edited flag | B layout, P meaning |
| `2E` | effect 2×7, param 2×7, value 5×7 (an IEEE-754 single) | Set a parameter to a value in its displayed unit; answered by `02` | B layout, H meaning |
| `3C` | a 1, b 1 | Select preset (number in two bytes) | B layout, P meaning |
| `42` | — | Disconnect | B layout, P meaning |
| `47` | — | Device capability words (five 2×7 values) | B layout, ? meaning |

Also sent, layout known but purpose not yet determined [B, ?]: `18`, `1A`,
`1B`, `1F`, `22`, `23`, `24`, `2B`, `2C`, `2F`, `30`,
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
| `12` | index 2×7, name (32 characters, NUL-terminated) | Cabinet name | B layout, H meaning |
| `13` | value 1 | CPU load | B |
| `14` | a 1, b 1 | Preset number; also sent unsolicited | B |
| `15` | a 1, b 1, name (32 bytes) | Name of a stored preset | B layout, H meaning |
| `16` | type 5×7, min, max, scale, step (each an IEEE-754 single as 5×7) | Parameter metadata | B |
| `20` | 48 × (block 2×7, routing 2×7) | Grid, see 4.4 | B |
| `21` | — | Front panel changed something | B |
| `23` | status 1, position 1 | Looper status, see 4.8 | B layout, H status, P position |
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

[H] `2E` takes the number as the device displays it: -40.0 sets a level
shown as "-40.0 dB", 100.0 a time shown as "100.0 ms", 3.0 a ratio shown as
"3.00". A value outside the parameter's range is limited to the nearest end
(a threshold sent as -200 read back as "OFF", raw 0). The reply is the `02`
message with the raw value and text now in force.

[H] `28` answers with every choice of an enumerated parameter in one
message, in the order of its raw values: on firmware 32 the amp type has 266
names in 3.6 kB, most lists a handful. It answers for a block that is not in
the current preset as well. For a parameter that is not an enumeration the
device answers with status code 27, and for a parameter ID it does not have
with code 7. `16` reports type word `0x10` for an enumeration (`0xB0` for a
block's bypass mode, which has strings too), with `max` its highest choice
index.

[H] The cab block's two cab parameters are not enumerations. `16` reports
type word `0x20` and `max` 1216 for them, `01` and `02` report no highest
choice index, and `28` is refused with code 27. Their raw value is a position
in the cab list that `12` reads (4.6).

[H] A block removed from the grid (`05` with block 0) and placed again in
the same edit buffer came back with the parameter value it had (seen with
one Parametric EQ and one parameter).

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

### 4.5 Dumps: presets and cabinets

[B] [F] [H] A preset travels as `77` (header) + N × `78` (data) + `79`
(footer). A cabinet impulse response travels as `7A` + N × `7B` + `7C`. The
same messages are what a `.syx` file holds, what the device sends when asked
for a dump, and what is sent to the device to load one. Other brackets exist
(`74`/`75`/`76`, `61`/`62`/`63`, `67`/`68`/`69`) [?].

| | Preset | Cabinet |
|---|---|---|
| Header payload | a 1, b 1, word count 2×7 [H]; the binary also has a form with a 3×7 count [B, ?] | slot, word count 2×7 |
| Data payload | count 2×7, then that many words | count 2×7, then that many words |
| One word | 3×7 (up to 21 bits) | 5×7 (a 32-bit sample; the spare bits are not data) |
| Footer payload | XOR of all words, 3×7 | XOR of all words, 5×7 |
| Seen on an XL [H] | 4096 words, 64 per data message | 2048 words, 32 per data message |

[H] The XOR in the footer was checked against a preset dump and a cabinet
dump read from an XL, and [F] against a cabinet file made for an XL+. For a
cabinet only the low 32 bits of each word and of the footer agree between a
file and a device dump of the same cabinet; the top three bits differ.

[H] **Preset header.** A dump the device sends carries `7F 00` in the two
number bytes, whichever slot was asked for. A dump sent to the device with
`7F 00` there is loaded into the edit buffer and nothing is stored: the
preset-edited flag is raised, and selecting a preset discards it. To keep it,
store it (`1D`). [?] Whether a header that names a slot writes that slot
directly is not known and is not used.

[H] **Receiving.** The device acknowledges every message of a dump it is
sent with a `64` status carrying that message's function and code 0, within
a few milliseconds; the footer's acknowledgement takes about 0.1 s.

[H] **Asking for a preset.** `03` with a slot number sends what is stored in
that slot; unsaved edits are not included. With `7F 00` as the number the
device answered with an empty preset, so the edit buffer cannot be read this
way. [H] Asking does not change the current preset or its edited flag.

[H] **Preset words.** Word 0 was `0x801` in every dump seen. Words 2 to 33
hold the name, one character per word, padded with zeros; an empty slot has
all zeros there. [?] The rest of the layout is not mapped.

[H] [F] **Cabinet words.** The first eight words hold the name, four
characters per word with the first character in the low byte. An empty user
slot reads as all bits set.

**Cabinet slot numbers.** The user-cab number is counted from 0. Each model
lays it out differently in `19` and in the `7A` header [B]:

| Model | In `19` | In the `7A` header | Conf. |
|---|---|---|---|
| II (`03`) | 1 byte | 1 byte, then the count | B layout |
| XL (`06`) | 2 bytes, high 7 bits first | the same 2 bytes, then the count | B layout; H for `19` |
| XL+ (`07`) | 2×7, low group first | the same 2×7, then the count | B layout; F for the header |

[H] A dump the XL sends carries `7F 7F` in the slot bytes, as [F] cabinet
files do. [P] Sending a cabinet with a slot number in the header writes that
user slot; **this has not been done on hardware.** [H] `19` with a number
that is not a user slot is refused with status code 22.

### 4.6 What a unit stores

[H] `15` answers for preset numbers 0 to 767 on an XL; an empty slot is named
`<EMPTY>` and the bytes after the terminating NUL are leftovers. [P] The
Mark I/II has 384 preset slots and the XL+ 768.

[H] On an XL the cab list that `12` indexes runs: 0 to 188 factory cabs,
189 to 1212 the 1024 user cabs (an unused one is named `EMPTY`), 1213 to
1216 four more entries with blank names [?]. This matches the largest value
of the cab block's cab parameter in the definition cache (1216) [F]. [P] The
XL+ has the same cabinet memory. [?] The number of factory cabs on a Mark
I/II is not known here, so its user cabs cannot be located in the list.

### 4.7 Modifiers (function `07`)

A modifier ties a controller (an LFO, an envelope, a pedal and so on) to one
parameter. It is addressed by the parameter it belongs to plus a selector
that names one of its fields. [B] The selector names are stored in the binary
and reproduced in `params.json` under `modifier`.

[H] Read with `set = 0` for Amp 1's input drive, a parameter with no
modifier, on firmware 32.00. Each reply repeats effect, parameter and
selector, then the value and the device's text for it:

| Selector | Name | Raw value read | Text read | Meaning |
|---|---|---|---|---|
| 0 | `MOD_CTRLID` | 0 | `NONE` | Source; 0 is no modifier [H text, P meaning] |
| 1 | `MOD_MIN` | 0 | `0.00` | Lowest parameter value, in the parameter's own unit |
| 2 | `MOD_MAX` | 65534 | `10.00` | Highest parameter value |
| 3 | `MOD_STARTPT` | 0 | `0.0 %` | Curve start |
| 4 | `MOD_MIDPT` | 32767 | `50.0 %` | Curve middle |
| 5 | `MOD_ENDPT` | 65534 | `100.0 %` | Curve end |
| 6 | `MOD_SLOPE` | 32767 | `50.0 %` | Curve slope |
| 7 | `MOD_DAMPING` | 655 | `10 ms` | Damping |
| 8 | `MOD_EFFECTID` | 106 | (none) | The block the modifier belongs to |
| 9 | `MOD_PARAM` | 1 | (none) | The parameter it belongs to |
| 10 | `MOD_AUTOENGAGE` | 0 | `OFF` | Auto engage |
| 11 | `MOD_PCRESET` | 0 | `OFF` | Reset on preset change |
| 12 | `MOD_OFFVAL` | 0 | `5.0 %` | Off value for auto engage |
| 13 | `MOD_SCALE` | 32767 | `1.000` | Scale |
| 14 | `MOD_OFFSET` | 32767 | `0.0 %` | Offset |

[H] **Selector 15 must never be sent.** `MOD_END` in the table is an end
marker, not a field. A unit asked for it with `set = 0` did not answer, and
answered no other message afterwards until it was switched off and on. The
Windows MIDI service was left unable to open ports as well. Nothing on the
unit was changed. Only selectors 0 to 14 exist.

[H] With `set = 1` the field takes the value and the reply reports what is in
force, with its text, as `02` does for a parameter. Sent to an XL on firmware
32.00 for Amp 1's input drive, in the edit buffer of an empty preset, one
message per connection:

| Sent | Reply | Read back afterwards |
|---|---|---|
| selector 0 = 1 | 1, `LFO 1A` | 1, `LFO 1A`; every other field as in the table above |
| selector 4 = 40000 | 40000, `61.0 %` | the same |
| selector 10 = 1 | 1, `SLOW SPD` | the same |
| selector 0 = 0 | 0, `NONE` | 0, `NONE`; selector 4 back at 32767, selector 10 back at 0 |

So a modifier is attached by setting selector 0 to a source other than 0 and
removed by setting it back to 0, and **removing it puts its other fields back
to their defaults**. The preset-edited flag (`2A`) is set afterwards.

[?] The numbering of the sources past the first and their number, whether a
field can be set while the source is none, what the device answers for a
parameter that cannot take a modifier, and how many modifiers a preset may
hold (status `NO_MODIFIERS_LEFT` exists).

[P] The owner's manual lists 23 sources besides none (four LFO outputs, two
ADSRs, the sequencer, the envelope follower, the pitch detector, twelve
external controllers and two scene controllers), seven auto-engage settings
and two for the reset on preset change. Until a unit has shown what it does
with a choice past the last, an editor should send selector 0 no higher than
23, selector 10 no higher than 6 and selector 11 no higher than 1.

[?] How the device shapes the controller with start, middle, end, slope,
scale and offset is not published. The editor's curve is its own sketch: a
line through the three points, bowed by the slope, then scaled and shifted.

### 4.8 Looper and scenes

[H] `23` with payload 1 is acknowledged with a `64` status and switches on
status reports; with 0 it switches them off. A report's first byte has one
bit per transport state: bit 0 record, 1 play, 2 once, 3 overdub, 4 reverse,
5 half speed (record and play were seen on a unit). [P] The second byte is
how far playback is through the loop, 0 to 99. [?] How often a report is sent
while a loop plays.

[H] The transport is driven with MIDI control changes on the device's
channel: record 28, play 29, once 30, overdub 31, reverse 32, half speed 120,
undo 121, the device's default assignments.

[P] A scene has no name on this device family; the preset holds no text for
one. What belongs to a scene: each block's bypass state and X/Y state (4.3),
[F] the output mixer's main level for the scene (`OUTPUT_MAIN1` to
`OUTPUT_MAIN8`) and the values of the two scene controllers
(`CONTROLLERS_SCENE1_VAL1` to `VAL8`, `CONTROLLERS_SCENE2_VAL1` to `VAL8`).
Those are parameters with one ID per scene, so one scene's values can be read
and written while another is selected.

[H] `01` lists a block's parameters for the X/Y set the block is on. Reading
the other set means switching to it with `11`.

[F] The Tone Match block starts a capture when `EQMATCH_CAPTUREREF` (the
reference) or `EQMATCH_CAPTUREUSER` (the preset's own sound) is set to 1 and
[H] both read back as 1 afterwards. [P] Setting them to 0 ends a capture and
setting `EQMATCH_MATCH` to 1 computes the correction. [?] Whether a capture
ends by itself, and the message that carries the measured spectra.

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
string list; [H] `0x20` a position in the cab list (4.2). Other type words
encode unit and taper; bit meanings [?].
`scale` multiplies the engineering value for display (1000 for ms/Hz ranges).

## 7. Settings

[F] `Axe-Edit.settings` is an XML list of `<VALUE name= val=>` pairs, among
them `MIDI_IN`, `MIDI_OUT`, `MAIN_WINDOW_STATE`, the directory settings and
the prompt toggles.

## 8. Not covered

Most of the preset word layout, bank and bundle files, block library files,
Axe-Change, firmware update, IR capture/conversion, the Tone Match spectrum
display, the sequencer UI, auto-update. How the original resets a block to
its defaults, and how it lists modifier sources.
