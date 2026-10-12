//! Request/reply conversation with one device.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use axegrid_model::Grid;
use axegrid_sysex::{
    function, BlockState, Dump, DumpError, DumpKind, Frame, Model, ModifierValue, ParamAddress, ParamInfo, ParamValue,
    Reply, Request, MODIFIER_FIELDS,
};

/// A bidirectional stream of complete SysEx messages.
pub trait Transport: Send {
    fn send(&mut self, message: &[u8]) -> io::Result<()>;
    /// Next complete message, or `None` if nothing arrived within `timeout`.
    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
}

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    /// No matching reply arrived in time.
    Timeout {
        function: u8,
    },
    /// The device answered with a non-zero status code.
    Rejected {
        function: u8,
        code: u8,
    },
    /// Something answered the probe with a model this editor does not know.
    UnsupportedModel(u8),
    UnexpectedReply {
        function: u8,
    },
    /// A preset or cabinet arrived damaged or incomplete.
    BadDump(DumpError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "MIDI I/O failed: {e}"),
            Self::Timeout { function } => write!(f, "no reply to function {function:#04X}"),
            Self::Rejected { function, code } => {
                write!(f, "device rejected function {function:#04X} with status {code}")
            }
            Self::UnsupportedModel(model) => write!(f, "unsupported device model {model:#04X}"),
            Self::UnexpectedReply { function } => {
                write!(f, "reply to function {function:#04X} had an unexpected shape")
            }
            Self::BadDump(e) => write!(f, "the device sent unusable data: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// What the editor shows about the current preset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetView {
    pub number: u16,
    pub name: String,
    pub scene: u8,
    pub grid: Grid,
    pub states: Vec<BlockState>,
}

pub struct Session<T> {
    transport: T,
    model: Model,
    timeout: Duration,
    /// Messages that arrived while waiting for something else: tuner data,
    /// tempo beats, front-panel notifications.
    unsolicited: VecDeque<Reply>,
}

impl<T: Transport> Session<T> {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);
    /// Unrequested messages kept until someone drains them.
    const MAX_UNSOLICITED: usize = 256;
    /// How long a list of choices may take to arrive. It is one message of
    /// several kilobytes, which a 31.25 kbaud DIN link carries in seconds.
    const LIST_TIMEOUT: Duration = Duration::from_secs(6);

    /// Probes for a device and adopts whichever model answers.
    pub fn connect(mut transport: T) -> Result<Self, SessionError> {
        transport.send(&Request::Probe.to_frame(0).to_bytes())?;
        let deadline = Instant::now() + Self::DEFAULT_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(bytes) = transport.receive(left)? else {
                return Err(SessionError::Timeout { function: function::PROBE });
            };
            if let Ok((model, Reply::Status { function: function::PROBE, code: 0 })) = Reply::parse(&bytes) {
                let model = Model::from_byte(model).ok_or(SessionError::UnsupportedModel(model))?;
                return Ok(Self { transport, model, timeout: Self::DEFAULT_TIMEOUT, unsolicited: VecDeque::new() });
            }
        }
    }

    pub fn model(&self) -> Model {
        self.model
    }

    /// Takes the messages that arrived without being asked for.
    pub fn drain_unsolicited(&mut self) -> impl Iterator<Item = Reply> + '_ {
        self.unsolicited.drain(..)
    }

    /// Collects pending input without sending anything.
    pub fn poll(&mut self) -> Result<(), SessionError> {
        // Bounded, so a device that never stops talking cannot hold the caller.
        for _ in 0..Self::MAX_UNSOLICITED {
            let Some(bytes) = self.transport.receive(Duration::ZERO)? else { break };
            if let Ok((_, reply)) = Reply::parse(&bytes) {
                self.keep(reply);
            }
        }
        Ok(())
    }

    /// Queues a message nobody asked for, dropping the oldest when full.
    fn keep(&mut self, reply: Reply) {
        if self.unsolicited.len() >= Self::MAX_UNSOLICITED {
            self.unsolicited.pop_front();
        }
        self.unsolicited.push_back(reply);
    }

    /// Sends bytes that are not a SysEx request, such as a controller
    /// change. Nothing is awaited.
    pub fn send_raw(&mut self, bytes: &[u8]) -> Result<(), SessionError> {
        Ok(self.transport.send(bytes)?)
    }

    /// Zero-based MIDI channel the device listens on.
    pub fn midi_channel(&mut self) -> Result<u8, SessionError> {
        match self.request(&Request::MidiChannel)? {
            Reply::MidiChannel(channel) => Ok(channel),
            _ => Err(SessionError::UnexpectedReply { function: function::MIDI_CHANNEL }),
        }
    }

    /// Sends a request and waits for the reply that completes it. Items of a
    /// multi-message list are returned together with the terminating reply.
    fn exchange(&mut self, request: &Request) -> Result<(Vec<Reply>, Reply), SessionError> {
        self.exchange_within(request, self.timeout)
    }

    /// [`Self::exchange`] with its own limit on how long the device may stay silent.
    fn exchange_within(&mut self, request: &Request, timeout: Duration) -> Result<(Vec<Reply>, Reply), SessionError> {
        let function = request.function();
        self.transport.send(&request.to_frame(self.model.byte()).to_bytes())?;
        let mut deadline = Instant::now() + timeout;
        let mut items = Vec::new();
        let dump = match request {
            Request::PresetDump(_) => Some(DumpKind::Preset),
            Request::CabDump(_) => Some(DumpKind::Cab),
            _ => None,
        };
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(bytes) = self.transport.receive(left)? else {
                return Err(SessionError::Timeout { function });
            };
            // A long list over a 31.25 kbaud DIN link takes seconds; the
            // timeout bounds silence, not the whole exchange.
            if !items.is_empty() {
                deadline = Instant::now() + timeout;
            }
            // Corrupt or foreign messages are dropped, as the original does.
            let Ok((_, reply)) = Reply::parse(&bytes) else { continue };
            match reply {
                Reply::Status { function: f, code } if f == function && code != 0 => {
                    return Err(SessionError::Rejected { function, code });
                }
                // Commands without a reply of their own are acknowledged
                // with a success status.
                Reply::Status { function: f, .. } if f == function => return Ok((items, reply)),
                Reply::ParamDumpItem(_) | Reply::ListStart if function == function::PARAM_DUMP => {
                    items.push(reply);
                }
                reply if request.is_answered_by(function_of(&reply)) => return Ok((items, reply)),
                // Header and data messages of a dump; its footer ends it above.
                Reply::Other(frame) if dump.is_some_and(|kind| kind.owns(frame.function)) => {
                    items.push(Reply::Other(frame));
                }
                other => self.keep(other),
            }
        }
    }

    pub fn request(&mut self, request: &Request) -> Result<Reply, SessionError> {
        self.exchange(request).map(|(_, reply)| reply)
    }

    pub fn firmware_version(&mut self) -> Result<(u8, u8), SessionError> {
        match self.request(&Request::FirmwareVersion)? {
            Reply::FirmwareVersion { major, minor } => Ok((major, minor)),
            _ => Err(SessionError::UnexpectedReply { function: function::FIRMWARE_VERSION }),
        }
    }

    /// Reads everything needed to draw the current preset.
    pub fn read_preset(&mut self) -> Result<PresetView, SessionError> {
        let mut view = PresetView::default();
        if let Reply::PresetNumber(number) = self.request(&Request::PresetNumber)? {
            view.number = number;
        }
        if let Reply::PresetName(name) = self.request(&Request::PresetName)? {
            view.name = name;
        }
        if let Reply::Grid(cells) = self.request(&Request::Grid)? {
            view.grid = Grid::from_cells(&cells);
        }
        // Scene reports queued by earlier exchanges are stale by now.
        self.unsolicited.retain(|reply| !matches!(reply, Reply::Scene(_)));
        if let Reply::BlockStates(states) = self.request(&Request::BlockStates)? {
            view.states = states;
        }
        // The device reports the scene right after the block list, so there
        // is no need to send a scene message just to read it.
        if let Some(scene) = self.await_scene()? {
            view.scene = scene;
        }
        Ok(view)
    }

    /// Waits briefly for the scene report that follows a block-state list.
    fn await_scene(&mut self) -> Result<Option<u8>, SessionError> {
        let queued = self.unsolicited.iter().position(|r| matches!(r, Reply::Scene(_)));
        if let Some(Reply::Scene(scene)) = queued.and_then(|at| self.unsolicited.remove(at)) {
            return Ok(Some(scene));
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(bytes) = self.transport.receive(left)? else { return Ok(None) };
            match Reply::parse(&bytes) {
                Ok((_, Reply::Scene(scene))) => return Ok(Some(scene)),
                Ok((_, other)) => self.keep(other),
                Err(_) => {}
            }
        }
    }

    pub fn get_param(&mut self, address: ParamAddress) -> Result<ParamValue, SessionError> {
        self.param_reply(&Request::GetParam(address))
    }

    /// Sets a parameter and returns the value the device actually applied.
    pub fn set_param(&mut self, address: ParamAddress, raw: u16) -> Result<ParamValue, SessionError> {
        self.param_reply(&Request::SetParam(address, raw))
    }

    /// Sets a parameter to a value in the unit it is displayed in and
    /// returns what the device applied; out-of-range values are limited by it.
    pub fn set_param_displayed(&mut self, address: ParamAddress, value: f32) -> Result<ParamValue, SessionError> {
        self.param_reply(&Request::SetParamDisplayed(address, value))
    }

    fn param_reply(&mut self, request: &Request) -> Result<ParamValue, SessionError> {
        match self.request(request)? {
            Reply::Param(value) => Ok(value),
            _ => Err(SessionError::UnexpectedReply { function: function::PARAM }),
        }
    }

    /// Current value of every parameter of one block.
    pub fn dump_block(&mut self, block: u16) -> Result<Vec<ParamValue>, SessionError> {
        let (items, _) = self.exchange(&Request::ParamDump { block })?;
        Ok(items
            .into_iter()
            .filter_map(|item| match item {
                Reply::ParamDumpItem(value) => Some(value),
                _ => None,
            })
            .collect())
    }

    /// The choices of an enumerated parameter, in the order of its raw
    /// values. The device refuses for a parameter that is not a list.
    pub fn param_strings(&mut self, address: ParamAddress) -> Result<Vec<String>, SessionError> {
        match self.exchange_within(&Request::ParamStrings(address), Self::LIST_TIMEOUT)?.1 {
            Reply::ParamStrings(strings) => Ok(strings),
            _ => Err(SessionError::UnexpectedReply { function: function::PARAM_STRINGS }),
        }
    }

    /// Reads one field of the modifier on a parameter.
    pub fn modifier(&mut self, address: ParamAddress, field: u16) -> Result<ModifierValue, SessionError> {
        self.modifier_reply(&Request::Modifier { address, field, value: None })
    }

    /// Sets one field of the modifier on a parameter and returns what the
    /// device applied.
    pub fn set_modifier(
        &mut self,
        address: ParamAddress,
        field: u16,
        value: u16,
    ) -> Result<ModifierValue, SessionError> {
        self.modifier_reply(&Request::Modifier { address, field, value: Some(value) })
    }

    fn modifier_reply(&mut self, request: &Request) -> Result<ModifierValue, SessionError> {
        // A unit asked for a field past the last one stopped answering for good.
        if matches!(request, Request::Modifier { field, .. } if *field >= MODIFIER_FIELDS) {
            return Err(SessionError::UnexpectedReply { function: function::MODIFIER });
        }
        match self.request(request)? {
            Reply::Modifier(value) => Ok(value),
            _ => Err(SessionError::UnexpectedReply { function: function::MODIFIER }),
        }
    }

    /// Type and range of a parameter.
    pub fn param_info(&mut self, address: ParamAddress) -> Result<ParamInfo, SessionError> {
        match self.request(&Request::ParamInfo(address))? {
            Reply::ParamInfo(info) => Ok(info),
            _ => Err(SessionError::UnexpectedReply { function: function::PARAM_INFO }),
        }
    }

    /// Name of a stored preset, without loading it.
    pub fn preset_name_at(&mut self, number: u16) -> Result<String, SessionError> {
        match self.request(&Request::PresetNameAt(number))? {
            Reply::PresetNameAt { name, .. } => Ok(name),
            _ => Err(SessionError::UnexpectedReply { function: function::PRESET_NAME_AT }),
        }
    }

    /// Name of a cabinet by its position in the device's cab list.
    pub fn cab_name(&mut self, index: u16) -> Result<String, SessionError> {
        match self.request(&Request::CabName(index))? {
            Reply::CabName { name, .. } => Ok(name),
            _ => Err(SessionError::UnexpectedReply { function: function::CAB_NAME }),
        }
    }

    /// Reads a stored preset or a user cab as a checked dump.
    pub fn read_dump(&mut self, request: &Request) -> Result<Dump, SessionError> {
        let (items, footer) = self.exchange(request)?;
        let frames = items.into_iter().chain([footer]).filter_map(|reply| match reply {
            Reply::Other(frame) => Some(frame),
            _ => None,
        });
        Dump::from_frames(frames).map_err(SessionError::BadDump)
    }

    /// Sends the messages of a dump, waiting for the device to accept each.
    pub fn send_dump(&mut self, frames: &[Frame]) -> Result<(), SessionError> {
        for frame in frames {
            let function = frame.function;
            self.transport.send(&frame.to_bytes())?;
            let deadline = Instant::now() + self.timeout;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let Some(bytes) = self.transport.receive(left)? else {
                    return Err(SessionError::Timeout { function });
                };
                match Reply::parse(&bytes) {
                    Ok((_, Reply::Status { function: f, code: 0 })) if f == function => break,
                    Ok((_, Reply::Status { function: f, code })) if f == function => {
                        return Err(SessionError::Rejected { function, code });
                    }
                    Ok((_, other)) => self.keep(other),
                    Err(_) => {}
                }
            }
        }
        Ok(())
    }

    /// Gives the transport back without telling the device anything.
    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Tells the device the editor is going away.
    pub fn disconnect(mut self) -> T {
        let _ = self.transport.send(&Request::Disconnect.to_frame(self.model.byte()).to_bytes());
        self.transport
    }
}

fn function_of(reply: &Reply) -> u8 {
    use function::*;
    match reply {
        Reply::Status { .. } => STATUS,
        Reply::FirmwareVersion { .. } => FIRMWARE_VERSION,
        Reply::PresetNumber(_) => PRESET_NUMBER,
        Reply::PresetName(_) => PRESET_NAME,
        Reply::PresetNameAt { .. } => PRESET_NAME_AT,
        Reply::CabName { .. } => CAB_NAME,
        Reply::Grid(_) => GRID,
        Reply::BlockStates(_) => BLOCK_STATES,
        Reply::Scene(_) => SCENE,
        Reply::CpuLoad(_) => CPU_LOAD,
        Reply::PresetEdited(_) => PRESET_EDITED,
        Reply::MidiChannel(_) => MIDI_CHANNEL,
        Reply::Param(_) => PARAM,
        Reply::ParamDumpItem(_) => PARAM_DUMP,
        Reply::ParamInfo(_) => PARAM_INFO,
        Reply::ParamStrings(_) => PARAM_STRINGS,
        Reply::Modifier(_) => MODIFIER,
        Reply::BlockXy { .. } => BLOCK_XY,
        Reply::Tuner { .. } => TUNER,
        Reply::TempoBeat => TEMPO_BEAT,
        Reply::FrontPanelChange => FRONT_PANEL_CHANGE,
        Reply::LooperStatus(_) => LOOPER_STATUS,
        Reply::ListStart => LIST_START,
        Reply::ListEnd => LIST_END,
        Reply::Other(frame) => frame.function,
    }
}

impl Transport for Box<dyn Transport> {
    fn send(&mut self, message: &[u8]) -> io::Result<()> {
        (**self).send(message)
    }

    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        (**self).receive(timeout)
    }
}
