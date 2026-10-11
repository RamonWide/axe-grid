//! Typed requests and replies. Section numbers refer to `spec/SPEC.md`.

use crate::codec::{put_fixed_text, put_uint, Reader};
use crate::frame::{Frame, FrameError};
use crate::transfer::DumpKind;

/// Function IDs (spec section 4).
pub mod function {
    pub const PROBE: u8 = 0x00;
    pub const PARAM_DUMP: u8 = 0x01;
    pub const PARAM: u8 = 0x02;
    pub const PRESET_DUMP: u8 = 0x03;
    pub const PLACE_BLOCK: u8 = 0x05;
    pub const CONNECT: u8 = 0x06;
    pub const MODIFIER: u8 = 0x07;
    pub const FIRMWARE_VERSION: u8 = 0x08;
    pub const SET_PRESET_NAME: u8 = 0x09;
    pub const TUNER: u8 = 0x0D;
    pub const BLOCK_STATES: u8 = 0x0E;
    pub const PRESET_NAME: u8 = 0x0F;
    pub const TEMPO_BEAT: u8 = 0x10;
    pub const BLOCK_XY: u8 = 0x11;
    pub const CAB_NAME: u8 = 0x12;
    pub const CPU_LOAD: u8 = 0x13;
    pub const PRESET_NUMBER: u8 = 0x14;
    pub const PRESET_NAME_AT: u8 = 0x15;
    pub const PARAM_INFO: u8 = 0x16;
    pub const MIDI_CHANNEL: u8 = 0x17;
    pub const CAB_DUMP: u8 = 0x19;
    pub const STORE_PRESET: u8 = 0x1D;
    pub const GRID: u8 = 0x20;
    pub const FRONT_PANEL_CHANGE: u8 = 0x21;
    pub const LOOPER_STATUS: u8 = 0x23;
    pub const PARAM_STRINGS: u8 = 0x28;
    pub const SCENE: u8 = 0x29;
    pub const PRESET_EDITED: u8 = 0x2A;
    pub const PARAM_WIDE: u8 = 0x2E;
    pub const LIST_START: u8 = 0x32;
    pub const LIST_END: u8 = 0x33;
    pub const SELECT_PRESET: u8 = 0x3C;
    pub const DISCONNECT: u8 = 0x42;
    pub const STATUS: u8 = 0x64;
}

/// Model byte used before a device has identified itself.
pub const MODEL_ANY: u8 = 0x7F;
/// Scene value that asks for the current scene instead of setting one.
const SCENE_QUERY: u8 = 0x7F;
pub const PRESET_NAME_LEN: usize = 32;

/// The device families the editor supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    AxeFx2,
    AxeFx2Xl,
    AxeFx2XlPlus,
}

impl Model {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x03 => Some(Self::AxeFx2),
            0x06 => Some(Self::AxeFx2Xl),
            0x07 => Some(Self::AxeFx2XlPlus),
            _ => None,
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::AxeFx2 => 0x03,
            Self::AxeFx2Xl => 0x06,
            Self::AxeFx2XlPlus => 0x07,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::AxeFx2 => "Axe-Fx II",
            Self::AxeFx2Xl => "Axe-Fx II XL",
            Self::AxeFx2XlPlus => "Axe-Fx II XL+",
        }
    }
}

/// Address of one parameter: a block instance and a parameter within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamAddress {
    pub block: u16,
    pub param: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XyState {
    X,
    Y,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Probe,
    FirmwareVersion,
    PresetNumber,
    PresetName,
    /// Name of a stored preset, without loading it.
    PresetNameAt(u16),
    /// Asks for a stored preset as a dump (header, data, footer).
    PresetDump(u16),
    /// Name of a cabinet by its position in the cab list: factory cabs
    /// first, then user cabs.
    CabName(u16),
    /// Asks for a user cab (zero-based) as a dump.
    CabDump(u16),
    SetPresetName(String),
    SelectPreset(u16),
    StorePreset(u16),
    Grid,
    BlockStates,
    /// `None` queries the current scene.
    Scene(Option<u8>),
    CpuLoad,
    PresetEdited,
    /// The channel the device listens on for channel messages.
    MidiChannel,
    /// Asks the device to report (or stop reporting) what the looper is doing.
    LooperStatus(bool),
    GetParam(ParamAddress),
    SetParam(ParamAddress, u16),
    /// Sets a parameter to a value in the unit it is displayed in, such as
    /// decibels or hertz. Answered like [`Request::SetParam`].
    SetParamDisplayed(ParamAddress, f32),
    ParamInfo(ParamAddress),
    ParamStrings(ParamAddress),
    /// `None` queries the block's X/Y state.
    BlockXy(u16, Option<XyState>),
    ParamDump {
        block: u16,
    },
    PlaceBlock {
        block: u16,
        cell: u8,
    },
    /// Adds or removes the cable between two cells of adjacent columns.
    Cable {
        from: u8,
        to: u8,
        connect: bool,
    },
    Disconnect,
}

impl Request {
    pub fn function(&self) -> u8 {
        use function::*;
        match self {
            Self::Probe => PROBE,
            Self::FirmwareVersion => FIRMWARE_VERSION,
            Self::PresetNumber => PRESET_NUMBER,
            Self::PresetName => PRESET_NAME,
            Self::PresetNameAt(_) => PRESET_NAME_AT,
            Self::PresetDump(_) => PRESET_DUMP,
            Self::CabName(_) => CAB_NAME,
            Self::CabDump(_) => CAB_DUMP,
            Self::SetPresetName(_) => SET_PRESET_NAME,
            Self::SelectPreset(_) => SELECT_PRESET,
            Self::StorePreset(_) => STORE_PRESET,
            Self::Grid => GRID,
            Self::BlockStates => BLOCK_STATES,
            Self::Scene(_) => SCENE,
            Self::CpuLoad => CPU_LOAD,
            Self::PresetEdited => PRESET_EDITED,
            Self::MidiChannel => MIDI_CHANNEL,
            Self::LooperStatus(_) => LOOPER_STATUS,
            Self::GetParam(_) | Self::SetParam(..) => PARAM,
            Self::SetParamDisplayed(..) => PARAM_WIDE,
            Self::ParamInfo(_) => PARAM_INFO,
            Self::ParamStrings(_) => PARAM_STRINGS,
            Self::BlockXy(..) => BLOCK_XY,
            Self::ParamDump { .. } => PARAM_DUMP,
            Self::PlaceBlock { .. } => PLACE_BLOCK,
            Self::Cable { .. } => CONNECT,
            Self::Disconnect => DISCONNECT,
        }
    }

    /// Builds the message for a device of the given model byte. A probe is
    /// always addressed to [`MODEL_ANY`].
    pub fn to_frame(&self, model: u8) -> Frame {
        let mut payload = Vec::new();
        let address = |out: &mut Vec<u8>, a: &ParamAddress| {
            put_uint(out, a.block.into(), 2);
            put_uint(out, a.param.into(), 2);
        };
        match self {
            Self::Probe
            | Self::FirmwareVersion
            | Self::PresetNumber
            | Self::PresetName
            | Self::Grid
            | Self::BlockStates
            | Self::CpuLoad
            | Self::PresetEdited
            | Self::MidiChannel
            | Self::Disconnect => {}
            Self::SetPresetName(name) => put_fixed_text(&mut payload, name, PRESET_NAME_LEN),
            Self::CabName(index) => put_uint(&mut payload, (*index).into(), 2),
            Self::CabDump(cab) => {
                let target = Model::from_byte(model).unwrap_or(Model::AxeFx2Xl);
                payload = crate::transfer::cab_number(target, *cab);
            }
            Self::SelectPreset(number)
            | Self::StorePreset(number)
            | Self::PresetNameAt(number)
            | Self::PresetDump(number) => {
                // Unlike other fields, preset numbers go high group first.
                payload.push(((number >> 7) & 0x7F) as u8);
                payload.push((number & 0x7F) as u8);
            }
            Self::Scene(scene) => payload.push(scene.unwrap_or(SCENE_QUERY) & 0x7F),
            Self::LooperStatus(on) => payload.push(u8::from(*on)),
            Self::GetParam(a) => {
                address(&mut payload, a);
                put_uint(&mut payload, 0, 3);
                payload.push(0);
            }
            Self::SetParam(a, value) => {
                address(&mut payload, a);
                put_uint(&mut payload, (*value).into(), 3);
                payload.push(1);
            }
            Self::SetParamDisplayed(a, value) => {
                address(&mut payload, a);
                crate::codec::put_f32(&mut payload, *value);
            }
            Self::ParamInfo(a) | Self::ParamStrings(a) => address(&mut payload, a),
            Self::BlockXy(block, state) => {
                put_uint(&mut payload, (*block).into(), 2);
                payload.push(u8::from(*state == Some(XyState::Y)));
                payload.push(u8::from(state.is_some()));
            }
            Self::ParamDump { block } => put_uint(&mut payload, (*block).into(), 2),
            Self::PlaceBlock { block, cell } => {
                put_uint(&mut payload, (*block).into(), 2);
                payload.push(cell & 0x7F);
            }
            Self::Cable { from, to, connect } => {
                payload.extend_from_slice(&[from & 0x7F, to & 0x7F, u8::from(*connect)]);
            }
        }
        let model = if matches!(self, Self::Probe) { MODEL_ANY } else { model };
        Frame::new(model, self.function(), payload)
    }

    /// Whether a message with function `reply` completes this request
    /// (spec section 4.2, reply matching).
    pub fn is_answered_by(&self, reply: u8) -> bool {
        use function::*;
        match self.function() {
            PROBE => reply == STATUS,
            PARAM_DUMP => reply == LIST_END,
            PARAM_WIDE => reply == PARAM,
            PRESET_DUMP => reply == DumpKind::Preset.functions().2,
            CAB_DUMP => reply == DumpKind::Cab.functions().2,
            own => reply == own,
        }
    }
}

/// One parameter value as reported by the device.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamValue {
    pub address: ParamAddress,
    /// Three 7-bit groups on the wire, so it can exceed 16 bits.
    pub value: u32,
    pub flag: u8,
    pub extra: [u16; 2],
    /// The value formatted by the device, e.g. `"4.50"` or `"PLEXI 100W"`.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamInfo {
    pub type_word: u32,
    pub min: f32,
    pub max: f32,
    pub scale: f32,
    pub step: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCell {
    pub block: u16,
    pub routing: u16,
}

/// Decoded state word of one block in the current preset (spec 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockState {
    pub block: u16,
    pub engaged: bool,
    pub xy: XyState,
    pub bypass_cc: u8,
    pub xy_cc: u8,
}

impl BlockState {
    pub fn from_word(word: u32) -> Self {
        Self {
            block: (word >> 24) as u16,
            engaged: word & 1 != 0,
            xy: if word & 2 != 0 { XyState::X } else { XyState::Y },
            bypass_cc: ((word >> 8) & 0x7F) as u8,
            xy_cc: (word >> 16) as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `code == 0` is success.
    Status {
        function: u8,
        code: u8,
    },
    FirmwareVersion {
        major: u8,
        minor: u8,
    },
    PresetNumber(u16),
    PresetName(String),
    PresetNameAt {
        number: u16,
        name: String,
    },
    CabName {
        index: u16,
        name: String,
    },
    Grid(Vec<GridCell>),
    BlockStates(Vec<BlockState>),
    Scene(u8),
    CpuLoad(u8),
    PresetEdited(bool),
    /// Zero-based channel the device listens on for channel messages.
    MidiChannel(u8),
    Param(ParamValue),
    /// One entry of a parameter dump; same content as [`Reply::Param`].
    ParamDumpItem(ParamValue),
    ParamInfo(ParamInfo),
    ParamStrings(Vec<String>),
    BlockXy {
        block: u16,
        xy: XyState,
    },
    Tuner {
        note: u8,
        string: u8,
        cents: u8,
    },
    TempoBeat,
    FrontPanelChange,
    LooperStatus(u8),
    ListStart,
    ListEnd,
    /// Well-formed message this crate does not interpret.
    Other(Frame),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyError {
    Frame(FrameError),
    /// The payload is shorter than the function's layout requires.
    Truncated {
        function: u8,
    },
}

impl std::fmt::Display for ReplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame(e) => e.fmt(f),
            Self::Truncated { function } => {
                write!(f, "payload of function {function:#04X} is too short")
            }
        }
    }
}

impl std::error::Error for ReplyError {}

impl From<FrameError> for ReplyError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// Functions the device sends without a checksum byte.
fn has_checksum(function: u8) -> bool {
    !matches!(function, function::TUNER | function::BLOCK_STATES | function::TEMPO_BEAT)
}

impl Reply {
    /// Parses one complete message. Returns the sender's model byte too.
    pub fn parse(bytes: &[u8]) -> Result<(u8, Self), ReplyError> {
        let frame = match bytes.get(5) {
            Some(&function) if !has_checksum(function) => Frame::parse_unchecked(bytes)?,
            _ => Frame::parse(bytes)?,
        };
        let model = frame.model;
        let reply = Self::from_frame(frame.clone()).ok_or(ReplyError::Truncated { function: frame.function })?;
        Ok((model, reply))
    }

    fn from_frame(frame: Frame) -> Option<Self> {
        use function::*;
        let mut r = Reader::new(&frame.payload);
        Some(match frame.function {
            STATUS => Self::Status { function: r.u8()?, code: r.u8()? },
            FIRMWARE_VERSION => Self::FirmwareVersion { major: r.u8()?, minor: r.u8()? },
            PRESET_NUMBER => {
                let high = u16::from(r.u8()?);
                Self::PresetNumber(high << 7 | u16::from(r.u8()?))
            }
            PRESET_NAME => Self::PresetName(r.padded_text()),
            PRESET_NAME_AT => {
                let high = u16::from(r.u8()?);
                // The name ends at its first NUL; what follows is not part of it.
                Self::PresetNameAt { number: high << 7 | u16::from(r.u8()?), name: r.text().trim_end().to_owned() }
            }
            CAB_NAME => Self::CabName { index: r.u16(2)?, name: r.text().trim_end().to_owned() },
            GRID => {
                let mut cells = Vec::with_capacity(r.remaining() / 4);
                while r.remaining() >= 4 {
                    cells.push(GridCell { block: r.u16(2)?, routing: r.u16(2)? });
                }
                Self::Grid(cells)
            }
            BLOCK_STATES => {
                let mut states = Vec::with_capacity(r.remaining() / 5);
                while r.remaining() >= 5 {
                    states.push(BlockState::from_word(r.uint(5)?));
                }
                Self::BlockStates(states)
            }
            SCENE => Self::Scene(r.u8()?),
            CPU_LOAD => Self::CpuLoad(r.u8()?),
            PRESET_EDITED => Self::PresetEdited(r.u8()? != 0),
            MIDI_CHANNEL => Self::MidiChannel(r.u8()? & 0x0F),
            PARAM => Self::Param(param_value(&mut r)?),
            PARAM_DUMP => Self::ParamDumpItem(param_value(&mut r)?),
            PARAM_INFO => Self::ParamInfo(ParamInfo {
                type_word: r.uint(5)?,
                min: r.f32()?,
                max: r.f32()?,
                scale: r.f32()?,
                step: r.f32()?,
            }),
            PARAM_STRINGS => {
                let mut strings = Vec::new();
                while r.remaining() > 0 {
                    strings.push(r.text());
                }
                Self::ParamStrings(strings)
            }
            BLOCK_XY => Self::BlockXy { block: r.u16(2)?, xy: if r.u8()? == 0 { XyState::X } else { XyState::Y } },
            TUNER => Self::Tuner { note: r.u8()?, string: r.u8()?, cents: r.u8()? },
            TEMPO_BEAT => Self::TempoBeat,
            FRONT_PANEL_CHANGE => Self::FrontPanelChange,
            LOOPER_STATUS => Self::LooperStatus(r.u8()?),
            LIST_START => Self::ListStart,
            LIST_END => Self::ListEnd,
            _ => Self::Other(frame),
        })
    }
}

fn param_value(r: &mut Reader<'_>) -> Option<ParamValue> {
    Some(ParamValue {
        address: ParamAddress { block: r.u16(2)?, param: r.u16(2)? },
        value: r.uint(3)?,
        flag: r.u8()?,
        extra: [r.u16(2)?, r.u16(2)?],
        text: r.text(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMP1: u16 = 106;

    #[test]
    fn probe_is_addressed_to_any_model() {
        let bytes = Request::Probe.to_frame(0x06).to_bytes();
        assert_eq!(bytes, [0xF0, 0x00, 0x01, 0x74, 0x7F, 0x00, 0x7A, 0xF7]);
    }

    #[test]
    fn set_param_layout() {
        let request = Request::SetParam(ParamAddress { block: AMP1, param: 1 }, 0x8000);
        let frame = request.to_frame(0x03);
        assert_eq!(frame.function, function::PARAM);
        // block 2x7, param 2x7, value 3x7, set flag
        assert_eq!(frame.payload, [0x6A, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x01]);

        let query = Request::GetParam(ParamAddress { block: AMP1, param: 1 }).to_frame(0x03);
        assert_eq!(query.payload, [0x6A, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn preset_number_is_high_group_first() {
        assert_eq!(Request::SelectPreset(300).to_frame(0x03).payload, [0x02, 0x2C]);
        let reply = Frame::new(0x03, function::PRESET_NUMBER, [0x02, 0x2C]).to_bytes();
        assert_eq!(Reply::parse(&reply).unwrap().1, Reply::PresetNumber(300));
    }

    #[test]
    fn scene_query_uses_sentinel() {
        assert_eq!(Request::Scene(None).to_frame(0x03).payload, [0x7F]);
        assert_eq!(Request::Scene(Some(3)).to_frame(0x03).payload, [0x03]);
    }

    #[test]
    fn parses_param_reply_with_text() {
        let mut payload = vec![0x6A, 0x00, 0x01, 0x00, 0x7E, 0x7F, 0x03, 0x00, 0, 0, 0, 0];
        payload.extend_from_slice(b"10.00\0");
        let bytes = Frame::new(0x03, function::PARAM, payload).to_bytes();
        let (model, reply) = Reply::parse(&bytes).unwrap();
        assert_eq!(model, 0x03);
        let Reply::Param(value) = reply else { panic!("wrong variant") };
        assert_eq!(value.address, ParamAddress { block: AMP1, param: 1 });
        assert_eq!(value.value, 0xFFFE);
        assert_eq!(value.text, "10.00");
    }

    #[test]
    fn parses_param_info_and_strings() {
        let mut payload = Vec::new();
        put_uint(&mut payload, 0x131, 5);
        for v in [-80.0f32, 0.0, 1.0, 0.1] {
            crate::codec::put_f32(&mut payload, v);
        }
        let bytes = Frame::new(0x06, function::PARAM_INFO, payload).to_bytes();
        assert_eq!(
            Reply::parse(&bytes).unwrap().1,
            Reply::ParamInfo(ParamInfo { type_word: 0x131, min: -80.0, max: 0.0, scale: 1.0, step: 0.1 })
        );

        let bytes = Frame::new(0x06, function::PARAM_STRINGS, *b"OFF\0ON\0").to_bytes();
        assert_eq!(Reply::parse(&bytes).unwrap().1, Reply::ParamStrings(vec!["OFF".into(), "ON".into()]));
    }

    #[test]
    fn tuner_and_block_states_have_no_checksum() {
        let tuner = [0xF0, 0x00, 0x01, 0x74, 0x03, 0x0D, 0x09, 0x05, 0x3F, 0xF7];
        assert_eq!(Reply::parse(&tuner).unwrap().1, Reply::Tuner { note: 9, string: 5, cents: 63 });

        // One block: ID 106 in bits 24..31, engaged, X.
        let word: u32 = 106 << 24 | 0b11;
        let mut bytes = vec![0xF0, 0x00, 0x01, 0x74, 0x03, 0x0E];
        put_uint(&mut bytes, word, 5);
        bytes.push(0xF7);
        let Reply::BlockStates(states) = Reply::parse(&bytes).unwrap().1 else { panic!() };
        assert_eq!(states.len(), 1);
        assert_eq!((states[0].block, states[0].engaged, states[0].xy), (106, true, XyState::X));
    }

    /// Captured from an Axe-Fx II XL: Amp 1 with its factory-default
    /// controller assignments (bypass on CC 37, X/Y on CC 100).
    #[test]
    fn block_state_word_from_hardware() {
        let bytes = [0xF0, 0x00, 0x01, 0x74, 0x06, 0x0E, 0x02, 0x4A, 0x10, 0x53, 0x06, 0xF7];
        let Reply::BlockStates(states) = Reply::parse(&bytes).unwrap().1 else { panic!() };
        let expected = BlockState { block: 106, engaged: false, xy: XyState::X, bypass_cc: 37, xy_cc: 100 };
        assert_eq!(states, [expected]);
    }

    /// Captured from an Axe-Fx II XL.
    #[test]
    fn replies_from_hardware() {
        let probe = [0xF0, 0x00, 0x01, 0x74, 0x06, 0x64, 0x00, 0x00, 0x67, 0xF7];
        assert_eq!(Reply::parse(&probe).unwrap(), (0x06, Reply::Status { function: 0, code: 0 }));
        let number = [0xF0, 0x00, 0x01, 0x74, 0x06, 0x14, 0x00, 0x29, 0x3E, 0xF7];
        assert_eq!(Reply::parse(&number).unwrap().1, Reply::PresetNumber(41));
        let drive = [
            0xF0, 0x00, 0x01, 0x74, 0x06, 0x02, 0x6A, 0x00, 0x01, 0x00, 0x4C, 0x19, 0x02, 0x45, 0x00, 0x00, 0x00, 0x00,
            0x35, 0x2E, 0x35, 0x30, 0x00, 0x66, 0xF7,
        ];
        let Reply::Param(value) = Reply::parse(&drive).unwrap().1 else { panic!() };
        assert_eq!((value.address.block, value.address.param, value.value), (106, 1, 36044));
        assert_eq!(value.text, "5.50");
        assert_eq!(Reply::parse(&[0xF0, 0x00, 0x01, 0x74, 0x06, 0x10, 0xF7]).unwrap().1, Reply::TempoBeat);
    }

    #[test]
    fn grid_reply_has_one_cell_per_four_bytes() {
        let mut payload = Vec::new();
        for cell in 0..48u32 {
            put_uint(&mut payload, if cell == 4 { 106 } else { 0 }, 2);
            put_uint(&mut payload, if cell == 4 { 0b0010 } else { 0 }, 2);
        }
        let bytes = Frame::new(0x03, function::GRID, payload).to_bytes();
        let Reply::Grid(cells) = Reply::parse(&bytes).unwrap().1 else { panic!() };
        assert_eq!(cells.len(), 48);
        assert_eq!(cells[4], GridCell { block: 106, routing: 2 });
    }

    #[test]
    fn short_payload_is_reported() {
        let bytes = Frame::new(0x03, function::PARAM, [0x6A, 0x00]).to_bytes();
        assert_eq!(Reply::parse(&bytes), Err(ReplyError::Truncated { function: function::PARAM }));
    }

    #[test]
    fn requests_the_hardware_accepted() {
        let bytes = |request: Request| request.to_frame(0x06).to_bytes();
        assert_eq!(bytes(Request::SelectPreset(500)), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x3C, 0x03, 0x74, 0x48, 0xF7]);
        assert_eq!(
            bytes(Request::PlaceBlock { block: AMP1, cell: 1 }),
            [0xF0, 0x00, 0x01, 0x74, 0x06, 0x05, 0x6A, 0x00, 0x01, 0x6D, 0xF7]
        );
        assert_eq!(
            bytes(Request::BlockXy(AMP1, Some(XyState::Y))),
            [0xF0, 0x00, 0x01, 0x74, 0x06, 0x11, 0x6A, 0x00, 0x01, 0x01, 0x78, 0xF7]
        );
        // Bypass: shared parameter 255 set to 1.
        assert_eq!(
            bytes(Request::SetParam(ParamAddress { block: AMP1, param: 255 }, 1)),
            [0xF0, 0x00, 0x01, 0x74, 0x06, 0x02, 0x6A, 0x00, 0x7F, 0x01, 0x01, 0x00, 0x00, 0x01, 0x15, 0xF7]
        );
        let cable = Request::Cable { from: 1, to: 5, connect: true }.to_frame(0x06);
        assert_eq!((cable.function, cable.payload.as_slice()), (function::CONNECT, &[1, 5, 1][..]));
    }

    #[test]
    fn value_wider_than_sixteen_bits_survives() {
        // Captured from an Axe-Fx II XL: a parameter the firmware leaves undefined.
        let bytes = [
            0xF0, 0x00, 0x01, 0x74, 0x06, 0x02, 0x6A, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x04, 0x3F, 0x00, 0x00, 0x00, 0x00,
            0x31, 0x2E, 0x30, 0x30, 0x00, 0x53, 0xF7,
        ];
        let Reply::Param(value) = Reply::parse(&bytes).unwrap().1 else { panic!() };
        assert_eq!((value.address.param, value.value, value.text.as_str()), (28, 65536, "1.00"));
    }

    #[test]
    fn midi_channel_from_hardware() {
        let bytes = [0xF0, 0x00, 0x01, 0x74, 0x06, 0x17, 0x00, 0x14, 0xF7];
        assert_eq!(Reply::parse(&bytes).unwrap().1, Reply::MidiChannel(0));
        assert_eq!(Request::MidiChannel.to_frame(0x06).to_bytes(), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x17, 0x14, 0xF7]);
        let tuner = [0xF0, 0x00, 0x01, 0x74, 0x06, 0x0D, 0x00, 0x05, 0x0E, 0xF7];
        assert_eq!(Reply::parse(&tuner).unwrap().1, Reply::Tuner { note: 0, string: 5, cents: 14 });
    }

    /// Captured from an Axe-Fx II XL.
    #[test]
    fn library_messages_from_hardware() {
        let bytes = |request: Request| request.to_frame(0x06).to_bytes();
        assert_eq!(bytes(Request::PresetNameAt(500)), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x15, 0x03, 0x74, 0x61, 0xF7]);
        assert_eq!(bytes(Request::PresetDump(500)), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x03, 0x03, 0x74, 0x77, 0xF7]);
        assert_eq!(bytes(Request::CabName(0)), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x12, 0x00, 0x00, 0x11, 0xF7]);
        assert_eq!(bytes(Request::CabDump(3)), [0xF0, 0x00, 0x01, 0x74, 0x06, 0x19, 0x00, 0x03, 0x19, 0xF7]);
        // The other models lay the cab number out differently.
        assert_eq!(Request::CabDump(300).to_frame(0x07).payload, [0x2C, 0x02]);
        assert_eq!(Request::CabDump(5).to_frame(0x03).payload, [5]);

        // An empty slot: the name ends at the NUL and the rest is leftovers.
        let mut payload = vec![0x03, 0x74];
        payload.extend_from_slice(b"<EMPTY>");
        payload.extend_from_slice(&[0, 0x7D, 0, 0x02, 0, 0, 0x7F, 0x7F, 0x56, 0x45, 0x52, 0x01]);
        let reply = Frame::new(0x06, function::PRESET_NAME_AT, payload).to_bytes();
        assert_eq!(Reply::parse(&reply).unwrap().1, Reply::PresetNameAt { number: 500, name: "<EMPTY>".into() });
        let mut payload = vec![0x40, 0x01];
        payload.extend_from_slice(b"Magic 160                      ");
        payload.push(0);
        let reply = Frame::new(0x06, function::CAB_NAME, payload).to_bytes();
        assert_eq!(Reply::parse(&reply).unwrap().1, Reply::CabName { index: 192, name: "Magic 160".into() });
    }

    #[test]
    fn reply_matching() {
        assert!(Request::PresetDump(0).is_answered_by(0x79));
        assert!(Request::CabDump(0).is_answered_by(0x7C));
        assert!(Request::Probe.is_answered_by(function::STATUS));
        assert!(Request::Grid.is_answered_by(function::GRID));
        assert!(Request::ParamDump { block: AMP1 }.is_answered_by(function::LIST_END));
        assert!(!Request::ParamDump { block: AMP1 }.is_answered_by(function::PARAM_DUMP));
    }
}
