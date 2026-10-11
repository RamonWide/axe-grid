//! An in-memory stand-in for an Axe-Fx II, for tests and for trying the
//! editor without hardware. It answers the requests this project sends; it
//! does not process audio or model a real unit's parameter behavior.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Duration;

use axegrid_model::{blocks, grid, layouts, params};
use axegrid_sysex::codec::{put_uint, Reader};
use axegrid_sysex::{function, Frame, Model};

use crate::session::Transport;

const STATUS_INVALID_BLOCK: u8 = 1;
/// Effect type of the controllers block, and the tempo a fresh preset has.
const SIM_CONTROLLERS: u8 = 37;
const SIM_TEMPO: u16 = 120;
/// Controller that switches the tuner, and how many readings one switch-on yields.
const TUNER_CONTROLLER: u8 = 15;
const TUNER_BURST: usize = 3;
/// Looper transport controllers, in the order of their status bits.
const LOOPER_CONTROLLERS: [u8; 6] = [28, 29, 30, 31, 32, 120];

/// Row the factory chains of the simulated presets sit on.
const CHAIN_ROW: usize = 1;

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
    values: BTreeMap<(usize, u16, u16), u16>,
    bypassed: BTreeMap<(usize, u16), bool>,
    on_y: BTreeMap<(usize, u16), bool>,
    outbox: VecDeque<Vec<u8>>,
    /// Looper transport state, one bit per button, and whether it is reported.
    looper: u8,
    looper_watched: bool,
    /// Number of messages received, for tests.
    pub received: usize,
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
            looper: 0,
            looper_watched: false,
            received: 0,
        }
    }

    fn reply(&mut self, function: u8, payload: Vec<u8>) {
        self.outbox.push_back(Frame::new(self.model.byte(), function, payload).to_bytes());
    }

    fn status(&mut self, function: u8, code: u8) {
        self.reply(function::STATUS, vec![function, code]);
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
        self.values.get(&(self.current, block, param)).copied().unwrap_or(default)
    }

    fn param_payload(&self, block: u16, param: u16) -> Vec<u8> {
        let raw = self.value(block, param);
        let mut payload = Vec::new();
        put_uint(&mut payload, block.into(), 2);
        put_uint(&mut payload, param.into(), 2);
        put_uint(&mut payload, raw.into(), 3);
        payload.extend_from_slice(&[0; 5]);
        let text = format!("{:.2}", f32::from(raw) / 6553.4);
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
                    let bypassed = self.bypassed.get(&(self.current, block)).copied().unwrap_or(false);
                    let on_y = self.on_y.get(&(self.current, block)).copied().unwrap_or(false);
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
            PRESET_EDITED => self.reply(PRESET_EDITED, vec![u8::from(!self.values.is_empty())]),
            PARAM => {
                let (block, param, value, set) = (r.u16(2)?, r.u16(2)?, r.u16(3)?, r.u8()?);
                if !self.in_preset(block) {
                    self.status(PARAM, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                if set != 0 {
                    self.values.insert((self.current, block, param), value);
                    if param == params::SHARED_BYPASS {
                        self.bypassed.insert((self.current, block), value != 0);
                    }
                }
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
            BLOCK_XY => {
                let (block, y, set) = (r.u16(2)?, r.u8()?, r.u8()?);
                if !self.in_preset(block) {
                    self.status(BLOCK_XY, STATUS_INVALID_BLOCK);
                    return Some(());
                }
                if set != 0 {
                    self.on_y.insert((self.current, block), y != 0);
                }
                let on_y = self.on_y.get(&(self.current, block)).copied().unwrap_or(false);
                let mut payload = Vec::new();
                put_uint(&mut payload, block.into(), 2);
                payload.push(u8::from(on_y));
                self.reply(BLOCK_XY, payload);
            }
            STORE_PRESET => self.status(STORE_PRESET, 0),
            LOOPER_STATUS => {
                self.looper_watched = r.u8()? != 0;
                self.status(LOOPER_STATUS, 0);
                if self.looper_watched {
                    self.reply(LOOPER_STATUS, vec![self.looper, 0]);
                }
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
                if self.looper_watched {
                    self.reply(function::LOOPER_STATUS, vec![self.looper, 0]);
                }
            }
            return Ok(());
        }
        if let Ok(frame) = Frame::parse(message) {
            let function = frame.function;
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
