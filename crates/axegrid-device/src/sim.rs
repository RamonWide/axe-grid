//! An in-memory stand-in for an Axe-Fx II, for tests and for trying the
//! editor without hardware. It answers the requests this project sends; it
//! does not process audio or model a real unit's parameter behavior.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Duration;

use axegrid_model::{blocks, defs, grid, layouts, params};
use axegrid_sysex::codec::{put_uint, Reader};
use axegrid_sysex::{function, Dump, DumpKind, Frame, Model, MODIFIER_FIELDS};

use crate::session::Transport;

const STATUS_INVALID_BLOCK: u8 = 1;
/// What a unit answers when asked for the choices of a parameter that has none.
const STATUS_NOT_A_LIST: u8 = 27;
/// The choices every list of the simulator offers.
const SIM_CHOICES: [&str; 3] = ["FIRST", "SECOND", "THIRD"];
/// The modifier sources the simulator offers; the first is none.
const SIM_SOURCES: [&str; 4] = ["NONE", "SIM LFO", "SIM ENVELOPE", "SIM PEDAL"];
/// Effect type of the cab block, and its parameters that pick a cab from the cab list.
const SIM_CAB: u8 = 6;
const SIM_CAB_PICKERS: [&str; 2] = ["CABINET_TYPEL", "CABINET_TYPER"];
/// Entries after the user cabs in the cab list.
const SIM_SPARE_CABS: u16 = 4;
/// Type word of a continuous parameter.
const SIM_TYPE_CONTINUOUS: u32 = 0x32;
/// Effect type of the controllers block, and the tempo a fresh preset has.
const SIM_CONTROLLERS: u8 = 37;
const SIM_TEMPO: u16 = 120;
/// Controller that switches the tuner, and how many readings one switch-on yields.
const TUNER_CONTROLLER: u8 = 15;
const TUNER_BURST: usize = 3;
/// Looper transport controllers, in the order of their status bits.
const LOOPER_CONTROLLERS: [u8; 6] = [28, 29, 30, 31, 32, 120];
/// Status bits of a loop that is recording or playing, and how far it
/// moves, in hundredths of its length, per message the simulator receives.
const LOOPER_MOVING: u8 = 0b11;
const LOOPER_STEP: u8 = 7;

/// Where the user cabs start in the cab list, and how many there are.
const SIM_FACTORY_CABS: u16 = 189;
const SIM_USER_CABS: u16 = 1024;
/// Words of a simulated impulse response, and of a data message.
const SIM_CAB_WORDS: usize = 64;
const SIM_CHUNK_WORDS: usize = 32;
/// First word of a preset dump, and where the grid starts in it.
const SIM_PRESET_MARK: u32 = 0x801;
const SIM_NAME_AT: usize = 2;
const SIM_GRID_AT: usize = 34;
/// Raw steps per unit of the value the simulator displays (0.00 to 10.00).
const SIM_DISPLAY_SCALE: f32 = 6553.4;
/// Row the factory chains of the simulated presets sit on.
const CHAIN_ROW: usize = 1;

#[derive(Clone)]
struct Preset {
    name: String,
    /// `(block, routing)` per cell, column by column.
    cells: Vec<(u16, u16)>,
}

impl Preset {
    /// Lays `chain` out along one row and pads it to the output with shunts.
    fn new(name: &str, chain: &[u16]) -> Self {
        let mut cells = vec![(blocks::EMPTY, 0); grid::CELLS];
        for column in 0..grid::COLUMNS {
            let block = chain.get(column).copied().unwrap_or(*blocks::SHUNT_IDS.start() + column as u16);
            cells[column * grid::ROWS + CHAIN_ROW] = (block, 1 << CHAIN_ROW);
        }
        Self { name: name.into(), cells }
    }

    /// The preset as the words of a dump: a mark, the name one character
    /// to a word, then block and routing of every cell.
    fn words(&self) -> Vec<u32> {
        let mut words = vec![SIM_PRESET_MARK, 0];
        words.extend(self.name.bytes().map(u32::from).chain(std::iter::repeat(0)).take(SIM_GRID_AT - SIM_NAME_AT));
        words.extend(self.cells.iter().flat_map(|&(block, routing)| [u32::from(block), u32::from(routing)]));
        words
    }

    fn from_words(words: &[u32]) -> Option<Self> {
        let name: Vec<u8> =
            words.get(SIM_NAME_AT..SIM_GRID_AT)?.iter().map(|&w| w as u8).take_while(|&b| b != 0).collect();
        let cells: Vec<(u16, u16)> =
            words.get(SIM_GRID_AT..)?.as_chunks::<2>().0.iter().map(|pair| (pair[0] as u16, pair[1] as u16)).collect();
        (cells.len() == grid::CELLS).then(|| Self { name: String::from_utf8_lossy(&name).into_owned(), cells })
    }

    /// Real blocks in grid order, without shunts.
    fn blocks(&self) -> Vec<u16> {
        let placed = self.cells.iter().map(|&(block, _)| block);
        placed.filter(|&id| id != blocks::EMPTY && !blocks::is_shunt(id)).collect()
    }
}

pub struct SimDevice {
    model: Model,
    presets: Vec<Preset>,
    current: usize,
    scene: u8,
    /// Parameter values by preset, block, parameter set (`true` is Y) and parameter.
    values: BTreeMap<(usize, u16, bool, u16), u16>,
    /// Bypass and X/Y belong to a scene: by preset, scene and block.
    bypassed: BTreeMap<(usize, u8, u16), bool>,
    on_y: BTreeMap<(usize, u8, u16), bool>,
    outbox: VecDeque<Vec<u8>>,
    /// Modifier fields by preset, block and parameter.
    modifiers: BTreeMap<(usize, u16, u16), [u16; MODIFIER_FIELDS as usize]>,
    /// User cabs that hold something: name and data words.
    cabs: BTreeMap<u16, Vec<u32>>,
    /// An upload in progress: its messages so far, and the cab slot it names.
    incoming: Vec<Frame>,
    upload_slot: Option<u16>,
    /// A preset upload replaced the edit buffer.
    uploaded: bool,
    /// Something in the edit buffer was changed.
    edited: bool,
    /// Presets written with a store message, as `(slot, name)`.
    pub stored: Vec<(u16, String)>,
    /// Looper transport state, one bit per button, and whether it is reported.
    looper: u8,
    looper_watched: bool,
    /// How far the loop has played, 0 to 99.
    looper_position: u8,
    /// Number of messages received, for tests.
    pub received: usize,
}

/// The words of a simulated cab: its name four characters to a word, then
/// a decaying ramp standing in for the impulse response.
fn cab_words(name: &str, seed: u32) -> Vec<u32> {
    let mut bytes: Vec<u8> = name.bytes().chain(std::iter::repeat(0)).take(32).collect();
    bytes[31] = 0;
    let mut words: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|chunk| u32::from_le_bytes(*chunk)).collect();
    words.extend((0..SIM_CAB_WORDS as u32 - 8).map(|n| (seed << 20) / (n + 1)));
    words
}

fn cab_name(words: &[u32]) -> String {
    let bytes: Vec<u8> = words.iter().take(8).flat_map(|word| word.to_le_bytes()).take_while(|&b| b != 0).collect();
    String::from_utf8_lossy(&bytes).trim_end().to_owned()
}

/// The user-cab number in a dump request or an upload header, as each model
/// lays it out.
fn cab_slot(model: Model, payload: &[u8]) -> Option<u16> {
    let byte = |at: usize| payload.get(at).map(|&b| u16::from(b));
    match model {
        Model::AxeFx2 => byte(0),
        Model::AxeFx2Xl => Some(byte(0)? << 7 | byte(1)?),
        Model::AxeFx2XlPlus => Some(byte(1)? << 7 | byte(0)?),
    }
}

/// The fields of a modifier nobody has touched: no source, the whole
/// range, a straight line.
fn modifier_defaults(block: u16, param: u16) -> [u16; MODIFIER_FIELDS as usize] {
    const TOP: u16 = 65534;
    const HALF: u16 = 32767;
    [0, 0, TOP, 0, HALF, TOP, HALF, 655, block, param, 0, 0, 0, HALF, HALF]
}

/// A modifier field as the simulator displays it.
fn modifier_text(field: u16, raw: u16) -> String {
    let share = f32::from(raw) / 65534.0;
    match field {
        0 => SIM_SOURCES.get(usize::from(raw)).copied().unwrap_or("?").to_owned(),
        1 | 2 => format!("{:.2}", f32::from(raw) / SIM_DISPLAY_SCALE),
        3..=6 | 12 => format!("{:.1} %", share * 100.0),
        7 => format!("{:.0} ms", share * 1000.0),
        // The block and the parameter the modifier belongs to carry no text.
        8 | 9 => String::new(),
        10 | 11 => if raw == 0 { "OFF" } else { "ON" }.to_owned(),
        13 => format!("{:.3}", share * 2.0),
        _ => format!("{:.1} %", share * 200.0 - 100.0),
    }
}

impl Default for SimDevice {
    fn default() -> Self {
        Self::new(Model::AxeFx2Xl)
    }
}

impl SimDevice {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            presets: vec![
                Preset::new("Clean Chorus", &[100, 106, 108, 116, 110]),
                Preset::new("Crunch", &[133, 106, 108, 112, 110]),
                Preset::new("Lead Delay", &[124, 133, 106, 108, 112, 110]),
            ],
            current: 0,
            scene: 0,
            values: BTreeMap::new(),
            bypassed: BTreeMap::new(),
            on_y: BTreeMap::new(),
            outbox: VecDeque::new(),
            modifiers: BTreeMap::new(),
            cabs: BTreeMap::from([(0, cab_words("Sim 4x12", 7))]),
            incoming: Vec::new(),
            upload_slot: None,
            uploaded: false,
            edited: false,
            stored: Vec::new(),
            looper: 0,
            looper_watched: false,
            looper_position: 0,
            received: 0,
        }
    }

    fn reply(&mut self, function: u8, payload: Vec<u8>) {
        self.outbox.push_back(Frame::new(self.model.byte(), function, payload).to_bytes());
    }

    fn status(&mut self, function: u8, code: u8) {
        self.reply(function::STATUS, vec![function, code]);
    }

    /// Queues a dump of `words` as header, data messages and footer.
    fn send_dump(&mut self, kind: DumpKind, header: Vec<u8>, words: &[u32]) {
        let (first, data, last) = kind.functions();
        let groups = if kind == DumpKind::Preset { 3 } else { 5 };
        self.reply(first, header);
        for chunk in words.chunks(SIM_CHUNK_WORDS) {
            let mut payload = Vec::new();
            put_uint(&mut payload, chunk.len() as u32, 2);
            chunk.iter().for_each(|&word| put_uint(&mut payload, word, groups));
            self.reply(data, payload);
        }
        let mut payload = Vec::new();
        put_uint(&mut payload, words.iter().fold(0, |acc, word| acc ^ word), groups);
        self.reply(last, payload);
    }

    /// Takes one message of an upload; the footer completes it.
    fn receive_dump(&mut self, frame: Frame) {
        let function = frame.function;
        if let Some(kind) = DumpKind::of_header(function) {
            self.incoming.clear();
            if kind == DumpKind::Cab {
                self.upload_slot = cab_slot(self.model, &frame.payload);
            }
        }
        let last = DumpKind::Preset.functions().2 == function || DumpKind::Cab.functions().2 == function;
        self.incoming.push(frame);
        if !last {
            return self.status(function, 0);
        }
        let Ok(dump) = Dump::from_frames(std::mem::take(&mut self.incoming)) else {
            return self.status(function, STATUS_INVALID_BLOCK);
        };
        let words: Vec<u32> = dump.words().iter().map(|&word| word as u32).collect();
        let slot = self.upload_slot.take().filter(|&slot| slot < SIM_USER_CABS);
        match (dump.kind, Preset::from_words(&words), slot) {
            (DumpKind::Preset, Some(preset), _) => {
                // An upload lands in the edit buffer of the current preset.
                self.presets[self.current] = preset;
                self.uploaded = true;
                self.status(function, 0);
            }
            (DumpKind::Cab, _, Some(slot)) => {
                self.cabs.insert(slot, words);
                self.status(function, 0);
            }
            _ => self.status(function, STATUS_INVALID_BLOCK),
        }
    }

    /// Name of the cab at a position in the cab list.
    fn cab_name(&self, index: u16) -> String {
        match index.checked_sub(SIM_FACTORY_CABS) {
            None => format!("Sim Factory {}", index + 1),
            // The entries after the user cabs have blank names.
            Some(slot) if slot >= SIM_USER_CABS => String::new(),
            Some(slot) => self.cabs.get(&slot).map_or_else(|| "EMPTY".to_owned(), |words| cab_name(words)),
        }
    }

    fn in_preset(&self, block: u16) -> bool {
        blocks::is_fixed(block) || self.presets[self.current].blocks().contains(&block)
    }

    fn value(&self, block: u16, param: u16) -> u16 {
        // An untouched parameter reads as mid-scale; an untouched block is engaged.
        let tempo = params::id(SIM_CONTROLLERS, "CONTROLLERS_TEMPO");
        let default = match (block, param) {
            (_, params::SHARED_BYPASS) => 0,
            (blocks::CONTROLLERS, param) if Some(param) == tempo => SIM_TEMPO,
            // Lists and switches rest on their first choice, knobs at mid-scale.
            _ if blocks::by_id(block).is_some_and(|b| layouts::is_stepped(b.effect_type, param)) => 0,
            _ => 0x8000,
        };
        if param == params::SHARED_BYPASS {
            return u16::from(self.is_bypassed(block));
        }
        self.values.get(&(self.current, block, self.is_on_y(block), param)).copied().unwrap_or(default)
    }

    fn is_bypassed(&self, block: u16) -> bool {
        self.bypassed.get(&(self.current, self.scene, block)).copied().unwrap_or(false)
    }

    fn is_on_y(&self, block: u16) -> bool {
        self.on_y.get(&(self.current, self.scene, block)).copied().unwrap_or(false)
    }

    fn set_value(&mut self, block: u16, param: u16, value: u16) {
        if param == params::SHARED_BYPASS {
            self.bypassed.insert((self.current, self.scene, block), value != 0);
        } else {
            self.values.insert((self.current, block, self.is_on_y(block), param), value);
        }
        self.edited = true;
    }

    /// Reports the looper's buttons and position, if anyone is watching.
    fn report_looper(&mut self) {
        if self.looper_watched {
            self.reply(function::LOOPER_STATUS, vec![self.looper, self.looper_position]);
        }
    }

    /// Whether the parameter picks a cab by its position in the cab list.
    fn picks_cab(block: u16, param: u16) -> bool {
        let cab = blocks::by_id(block).is_some_and(|b| b.effect_type == SIM_CAB);
        cab && SIM_CAB_PICKERS.iter().any(|name| params::id(SIM_CAB, name) == Some(param))
    }

    /// Whether the parameter is a list of [`SIM_CHOICES`].
    fn is_list(block: u16, param: u16) -> bool {
        let stepped = blocks::by_id(block).is_some_and(|b| layouts::is_stepped(b.effect_type, param));
        stepped && !Self::picks_cab(block, param)
    }

    fn param_payload(&self, block: u16, param: u16) -> Vec<u8> {
        let raw = self.value(block, param);
        let mut payload = Vec::new();
        put_uint(&mut payload, block.into(), 2);
        put_uint(&mut payload, param.into(), 2);
        put_uint(&mut payload, raw.into(), 3);
        payload.extend_from_slice(&[0; 3]);
        // A list reports its highest choice index; anything else reports 0.
        let highest = if Self::is_list(block, param) { SIM_CHOICES.len() as u32 - 1 } else { 0 };
        put_uint(&mut payload, highest, 2);
        let listed = SIM_CHOICES.get(usize::from(raw)).filter(|_| highest > 0);
        let text =
            listed.map_or_else(|| format!("{:.2}", f32::from(raw) / SIM_DISPLAY_SCALE), |name| (*name).to_owned());
        payload.extend_from_slice(text.as_bytes());
        payload.push(0);
        payload
    }

    fn handle(&mut self, frame: Frame) -> Option<()> {
        use function::*;
        let mut r = Reader::new(&frame.payload);
        match frame.function {
            PROBE => self.status(PROBE, 0),
            FIRMWARE_VERSION => self.reply(FIRMWARE_VERSION, vec![9, 3, 0, 0, 0, 0]),
            PRESET_NUMBER => {
                let n = self.current as u16;
                self.reply(PRESET_NUMBER, vec![(n >> 7) as u8, (n & 0x7F) as u8]);
            }
            SELECT_PRESET => {
                let number = usize::from(r.u8()?) << 7 | usize::from(r.u8()?);
                if number < self.presets.len() {
                    self.current = number;
                    self.scene = 0;
                    self.status(SELECT_PRESET, 0);
                } else {
                    self.status(SELECT_PRESET, STATUS_INVALID_BLOCK);
                }
            }
            PRESET_NAME => {
                let mut payload = Vec::new();
                axegrid_sysex::codec::put_fixed_text(&mut payload, &self.presets[self.current].name, 32);
                self.reply(PRESET_NAME, payload);
            }
            PRESET_NAME_AT | PRESET_DUMP => {
                let (high, low) = (r.u8()?, r.u8()?);
                let number = usize::from(high) << 7 | usize::from(low);
                let Some(preset) = self.presets.get(number).cloned() else {
                    self.status(frame.function, STATUS_INVALID_BLOCK);
                    return Some(());
                };
                if frame.function == PRESET_DUMP {
                    let words = preset.words();
                    let mut header = vec![0x7F, 0x00];
                    put_uint(&mut header, words.len() as u32, 2);
                    self.send_dump(DumpKind::Preset, header, &words);
                } else {
                    let mut payload = vec![high, low];
                    payload.extend_from_slice(preset.name.as_bytes());
                    payload.push(0);
                    self.reply(PRESET_NAME_AT, payload);
                }
            }
            CAB_NAME => {
                let index = r.u16(2)?;
                let mut payload = Vec::new();
                put_uint(&mut payload, index.into(), 2);
                payload.extend_from_slice(self.cab_name(index).as_bytes());
                payload.push(0);
                self.reply(CAB_NAME, payload);
            }
            CAB_DUMP => {
                let slot = cab_slot(self.model, &frame.payload).filter(|&slot| slot < SIM_USER_CABS);
                let Some(slot) = slot else {
                    self.status(CAB_DUMP, STATUS_INVALID_BLOCK);
                    return Some(());
                };
                // An empty slot reads as erased memory.
                let words = self.cabs.get(&slot).cloned().unwrap_or_else(|| vec![u32::MAX; SIM_CAB_WORDS]);
                let mut header = vec![0x7F, 0x7F];
                put_uint(&mut header, words.len() as u32, 2);
                self.send_dump(DumpKind::Cab, header, &words);
            }
            function if DumpKind::Preset.owns(function) || DumpKind::Cab.owns(function) => self.receive_dump(frame),
            SET_PRESET_NAME => {
                self.presets[self.current].name = r.padded_text();
                self.status(SET_PRESET_NAME, 0);
            }
            GRID => {
                let mut payload = Vec::new();
                for &(block, routing) in &self.presets[self.current].cells {
                    put_uint(&mut payload, block.into(), 2);
                    put_uint(&mut payload, routing.into(), 2);
                }
                self.reply(GRID, payload);
            }
            PLACE_BLOCK => {
                let (block, cell) = (r.u16(2)?, usize::from(r.u8()?));
                let known = block == blocks::EMPTY || blocks::is_shunt(block) || blocks::by_id(block).is_some();
                if !known || cell >= grid::CELLS {
                    self.status(PLACE_BLOCK, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                let cells = &mut self.presets[self.current].cells;
                // A block exists once: placing it again moves it.
                let mut emptied: Vec<usize> = Vec::new();
                if blocks::by_id(block).is_some() {
                    emptied.extend((0..grid::CELLS).filter(|&at| cells[at].0 == block && at != cell));
                }
                if block == blocks::EMPTY {
                    emptied.push(cell);
                }
                // Cables do not outlive an emptied cell: none into it, none out of it.
                for at in emptied {
                    cells[at] = (blocks::EMPTY, 0);
                    let (column, row) = (at / grid::ROWS, at % grid::ROWS);
                    if column + 1 < grid::COLUMNS {
                        for taker in 0..grid::ROWS {
                            cells[(column + 1) * grid::ROWS + taker].1 &= !(1 << row);
                        }
                    }
                }
                // Only the first column is cabled (to the input) on arrival.
                let routing = if block != blocks::EMPTY && cell < grid::ROWS { 1 << cell } else { 0 };
                cells[cell] = (block, routing);
                self.status(PLACE_BLOCK, 0);
            }
            CONNECT => {
                let (from, to, connect) = (usize::from(r.u8()?), usize::from(r.u8()?), r.u8()? != 0);
                if from >= grid::CELLS || to >= grid::CELLS || to / grid::ROWS != from / grid::ROWS + 1 {
                    self.status(CONNECT, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                let bit = 1 << (from % grid::ROWS);
                let routing = &mut self.presets[self.current].cells[to].1;
                *routing = if connect { *routing | bit } else { *routing & !bit };
                self.status(CONNECT, 0);
            }
            BLOCK_STATES => {
                let mut message = vec![0xF0, 0x00, 0x01, 0x74, self.model.byte(), BLOCK_STATES];
                for block in self.presets[self.current].blocks() {
                    let (bypassed, on_y) = (self.is_bypassed(block), self.is_on_y(block));
                    let flags = u32::from(!on_y) << 1 | u32::from(!bypassed);
                    put_uint(&mut message, u32::from(block) << 24 | flags, 5);
                }
                message.push(0xF7);
                self.outbox.push_back(message);
                // A real unit follows the list with its current scene.
                self.reply(SCENE, vec![self.scene]);
            }
            SCENE => {
                let scene = r.u8()?;
                if scene < 8 {
                    self.scene = scene;
                }
                self.reply(SCENE, vec![self.scene]);
            }
            CPU_LOAD => self.reply(CPU_LOAD, vec![42]),
            MIDI_CHANNEL => self.reply(MIDI_CHANNEL, vec![0]),
            PRESET_EDITED => self.reply(PRESET_EDITED, vec![u8::from(self.uploaded || self.edited)]),
            PARAM => {
                let (block, param, value, set) = (r.u16(2)?, r.u16(2)?, r.u16(3)?, r.u8()?);
                if !self.in_preset(block) {
                    self.status(PARAM, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                if set != 0 {
                    self.set_value(block, param, value);
                }
                let payload = self.param_payload(block, param);
                self.reply(PARAM, payload);
            }
            PARAM_WIDE => {
                let (block, param, shown) = (r.u16(2)?, r.u16(2)?, r.f32()?);
                if !self.in_preset(block) {
                    self.status(PARAM_WIDE, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                // The simulator shows every value as raw / 6553.4.
                let raw = (shown * SIM_DISPLAY_SCALE).round().clamp(0.0, 65534.0) as u16;
                self.set_value(block, param, raw);
                let payload = self.param_payload(block, param);
                self.reply(PARAM, payload);
            }
            PARAM_DUMP => {
                let block = r.u16(2)?;
                let Some(info) = blocks::by_id(block).filter(|_| self.in_preset(block)) else {
                    self.status(PARAM_DUMP, STATUS_INVALID_BLOCK);
                    return Some(());
                };
                self.reply(LIST_START, vec![0, 0, 0]);
                let ids = (0..u16::MAX).take_while(|&p| params::name(info.effect_type, p).is_some());
                for param in ids.collect::<Vec<_>>() {
                    let payload = self.param_payload(block, param);
                    self.reply(PARAM_DUMP, payload);
                }
                self.reply(LIST_END, vec![]);
            }
            PARAM_STRINGS | PARAM_INFO => {
                let (block, param) = (r.u16(2)?, r.u16(2)?);
                // Like a unit, this answers for any block type, placed or not.
                if blocks::by_id(block).is_none() {
                    self.status(frame.function, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                let list = Self::is_list(block, param);
                if frame.function == PARAM_STRINGS {
                    if list {
                        let text: Vec<u8> = SIM_CHOICES.iter().flat_map(|name| name.bytes().chain([0])).collect();
                        self.reply(PARAM_STRINGS, text);
                    } else {
                        self.status(PARAM_STRINGS, STATUS_NOT_A_LIST);
                    }
                    return Some(());
                }
                let cabs = SIM_FACTORY_CABS + SIM_USER_CABS + SIM_SPARE_CABS;
                let (type_word, highest, step) = match (list, Self::picks_cab(block, param)) {
                    (true, _) => (defs::TYPE_ENUM, SIM_CHOICES.len() as f32 - 1.0, 0.0),
                    (_, true) => (defs::TYPE_CAB_LIST, f32::from(cabs - 1), 1.0),
                    _ => (SIM_TYPE_CONTINUOUS, 10.0, 0.001),
                };
                let mut payload = Vec::new();
                put_uint(&mut payload, type_word, 5);
                for value in [0.0, highest, 1.0, step] {
                    axegrid_sysex::codec::put_f32(&mut payload, value);
                }
                self.reply(PARAM_INFO, payload);
            }
            MODIFIER => {
                let (block, param, field) = (r.u16(2)?, r.u16(2)?, r.u16(2)?);
                let (value, set) = (r.u16(3)?, r.u8()?);
                // Lists and switches take no modifier; a real unit must never be asked past the last field.
                let stepped = blocks::by_id(block).is_some_and(|b| layouts::is_stepped(b.effect_type, param));
                if !self.in_preset(block) || stepped || field >= MODIFIER_FIELDS {
                    self.status(MODIFIER, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                let key = (self.current, block, param);
                let fields = self.modifiers.entry(key).or_insert_with(|| modifier_defaults(block, param));
                if set != 0 {
                    // A source past the last one is the last one.
                    fields[usize::from(field)] =
                        if field == 0 { value.min(SIM_SOURCES.len() as u16 - 1) } else { value };
                    // As on a unit: with the source gone, the rest is as new.
                    if (field, value) == (0, 0) {
                        *fields = modifier_defaults(block, param);
                    }
                    self.edited = true;
                }
                let raw = fields[usize::from(field)];
                let mut payload = Vec::new();
                for part in [block, param, field] {
                    put_uint(&mut payload, part.into(), 2);
                }
                put_uint(&mut payload, raw.into(), 3);
                payload.extend_from_slice(modifier_text(field, raw).as_bytes());
                payload.push(0);
                self.reply(MODIFIER, payload);
            }
            BLOCK_XY => {
                let (block, y, set) = (r.u16(2)?, r.u8()?, r.u8()?);
                if !self.in_preset(block) {
                    self.status(BLOCK_XY, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                if set != 0 {
                    self.on_y.insert((self.current, self.scene, block), y != 0);
                    self.edited = true;
                }
                let on_y = self.is_on_y(block);
                let mut payload = Vec::new();
                put_uint(&mut payload, block.into(), 2);
                payload.push(u8::from(on_y));
                self.reply(BLOCK_XY, payload);
            }
            STORE_PRESET => {
                let number = usize::from(r.u8()?) << 7 | usize::from(r.u8()?);
                let edit = self.presets[self.current].clone();
                self.stored.push((number as u16, edit.name.clone()));
                // Slots past the end are created, so tests can store anywhere nearby.
                if number < self.presets.len() {
                    self.presets[number] = edit;
                } else if number == self.presets.len() {
                    self.presets.push(edit);
                }
                self.uploaded = false;
                self.edited = false;
                self.status(STORE_PRESET, 0);
            }
            LOOPER_STATUS => {
                self.looper_watched = r.u8()? != 0;
                self.status(LOOPER_STATUS, 0);
                self.report_looper();
            }
            DISCONNECT => self.status(DISCONNECT, 0),
            other => self.status(other, STATUS_INVALID_BLOCK),
        }
        Some(())
    }
}

impl Transport for SimDevice {
    fn send(&mut self, message: &[u8]) -> io::Result<()> {
        self.received += 1;
        if let [status, TUNER_CONTROLLER, value] = *message {
            if status & 0xF0 == 0xB0 && value >= 64 {
                // A slightly flat A on the fifth string.
                for _ in 0..TUNER_BURST {
                    let reading = vec![0xF0, 0x00, 0x01, 0x74, self.model.byte(), function::TUNER, 0, 4, 58, 0xF7];
                    self.outbox.push_back(reading);
                }
            }
            return Ok(());
        }
        if let [status, controller, value] = *message {
            let button = LOOPER_CONTROLLERS.iter().position(|&number| number == controller);
            if let (0xB0, Some(bit)) = (status & 0xF0, button) {
                let mask = 1 << bit;
                self.looper = if value >= 64 { self.looper | mask } else { self.looper & !mask };
                if self.looper & LOOPER_MOVING == 0 {
                    self.looper_position = 0;
                }
                self.report_looper();
            }
            return Ok(());
        }
        if let Ok(frame) = Frame::parse(message) {
            let function = frame.function;
            // Time passes for a running loop whenever the device is spoken to.
            if self.looper & LOOPER_MOVING != 0 {
                self.looper_position = (self.looper_position + LOOPER_STEP) % 100;
                self.report_looper();
            }
            if self.handle(frame).is_none() {
                self.status(function, STATUS_INVALID_BLOCK);
            }
        }
        Ok(())
    }

    fn receive(&mut self, _timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        Ok(self.outbox.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Session, SessionError};
    use axegrid_sysex::{ParamAddress, Reply, Request};

    #[test]
    fn connect_identifies_the_model() {
        let session = Session::connect(SimDevice::new(Model::AxeFx2XlPlus)).unwrap();
        assert_eq!(session.model(), Model::AxeFx2XlPlus);
    }

    #[test]
    fn reads_a_whole_preset() {
        let mut session = Session::connect(SimDevice::default()).unwrap();
        assert_eq!(session.firmware_version().unwrap(), (9, 3));
        let view = session.read_preset().unwrap();
        assert_eq!((view.number, view.name.as_str(), view.scene), (0, "Clean Chorus", 0));
        assert_eq!(view.grid.blocks(), [100, 106, 108, 116, 110]);
        assert_eq!(view.states.len(), 5);
        assert!(view.states.iter().all(|s| s.engaged));
        assert!(view.grid.is_fed_by(1, 1, 1));
    }

    #[test]
    fn set_param_reads_back() {
        let mut session = Session::connect(SimDevice::default()).unwrap();
        let drive = ParamAddress { block: 106, param: 1 };
        assert_eq!(session.get_param(drive).unwrap().value, 0x8000);
        let applied = session.set_param(drive, 65534).unwrap();
        assert_eq!((applied.value, applied.text.as_str()), (65534, "10.00"));
        assert_eq!(session.get_param(drive).unwrap().value, 65534);
    }

    #[test]
    fn rejection_surfaces_as_an_error() {
        let mut session = Session::connect(SimDevice::default()).unwrap();
        // Preset 0 has no Wah block.
        let err = session.get_param(ParamAddress { block: 124, param: 0 }).unwrap_err();
        assert!(matches!(err, SessionError::Rejected { function: function::PARAM, code: 1 }));
        // The session is still usable afterwards.
        assert!(session.read_preset().is_ok());
    }

    #[test]
    fn dump_returns_every_named_parameter() {
        let mut session = Session::connect(SimDevice::default()).unwrap();
        let values = session.dump_block(106).unwrap();
        assert_eq!(values.len(), 118);
        assert_eq!(values[1].address, ParamAddress { block: 106, param: 1 });
    }

    #[test]
    fn switching_presets_changes_the_grid() {
        let mut session = Session::connect(SimDevice::default()).unwrap();
        let ack = session.request(&Request::SelectPreset(2)).unwrap();
        assert_eq!(ack, Reply::Status { function: function::SELECT_PRESET, code: 0 });
        let err = session.request(&Request::SelectPreset(99)).unwrap_err();
        assert!(matches!(err, SessionError::Rejected { function: function::SELECT_PRESET, .. }));
        let view = session.read_preset().unwrap();
        assert_eq!((view.number, view.name.as_str()), (2, "Lead Delay"));
        assert_eq!(view.grid.blocks().first(), Some(&124));
    }

    #[test]
    fn silence_times_out() {
        struct Mute;
        impl Transport for Mute {
            fn send(&mut self, _: &[u8]) -> io::Result<()> {
                Ok(())
            }
            fn receive(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
                Ok(None)
            }
        }
        assert!(matches!(Session::connect(Mute), Err(SessionError::Timeout { function: 0 })));
    }
}
