//! The editor engine: device session, editor state, and the command
//! registry. Every front end (UI, CLI, control channel, MCP) acts only by
//! running commands from [`registry`].

pub mod command;
mod device_cmds;
mod edit_cmds;
mod file_cmds;
mod live_cmds;
mod preset_cmds;

use std::sync::OnceLock;

use axegrid_device::{PresetView, Session, Transport};
use axegrid_model::defs::RAW_MAX;
use axegrid_model::eq::{self, Band, Shape};
use axegrid_model::layouts::{self, Kind};
use axegrid_model::{blocks, grid, params, Definitions};
use axegrid_sysex::{Model, ParamValue};
use serde_json::{json, Value};

pub use axegrid_sysex::XyState;
pub use command::{Args, CommandError, CommandSpec, ParamDoc};
pub use live_cmds::TEMPO_RANGE;

/// Everything a front end may display. Changed only by commands.
#[derive(Debug, Default)]
pub struct State {
    pub model: Option<Model>,
    pub firmware: Option<(u8, u8)>,
    pub simulated: bool,
    pub preset: PresetView,
    pub selected_block: Option<u16>,
    /// Parameter values of the selected block.
    pub params: Vec<ParamValue>,
    pub cpu_load: Option<u8>,
    /// Zero-based MIDI channel of the device.
    pub midi_channel: u8,
    pub tuner_on: bool,
    /// Latest reading while the tuner is on.
    pub tuner: Option<TunerReading>,
    /// Tempo of the current preset, beats per minute.
    pub tempo: Option<u16>,
    /// Tempo beats the device has announced since connecting.
    pub beats: u64,
    /// What the looper is doing, one bit per transport button, once the
    /// device has been asked to report it.
    pub looper: Option<u8>,
}

/// What the device's tuner hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunerReading {
    /// Semitones above A.
    pub note: u8,
    pub string: u8,
    /// 0 to 127; 63 is in tune.
    pub cents: u8,
}

const NOTE_NAMES: [&str; 12] = ["A", "A#", "B", "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#"];
/// Tuner reading that means "in tune".
const TUNER_CENTER: i32 = 63;
/// Points along the frequency axis of an equaliser curve.
const CURVE_POINTS: usize = 96;
/// Effect type of the parametric equaliser, and how many bands it has.
const PARAMETRIC_EQ: u8 = 4;
const EQ_BANDS: usize = 5;
/// Controls per row when a block has no page layout.
const PLAIN_ROW: usize = 10;

#[derive(Default)]
pub struct Engine {
    session: Option<Session<Box<dyn Transport>>>,
    state: State,
    defs: Option<Definitions>,
    /// When tempo.tap last ran.
    last_tap: Option<std::time::Instant>,
}

/// All commands, in menu order.
pub fn registry() -> &'static [&'static CommandSpec] {
    static ALL: OnceLock<Vec<&'static CommandSpec>> = OnceLock::new();
    ALL.get_or_init(|| {
        [device_cmds::COMMANDS, live_cmds::COMMANDS, preset_cmds::COMMANDS, edit_cmds::COMMANDS, file_cmds::COMMANDS]
            .into_iter()
            .flatten()
            .collect()
    })
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    registry().iter().copied().find(|c| c.id == id)
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn definitions(&self) -> Option<&Definitions> {
        self.defs.as_ref()
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// Why `id` cannot run right now, or `None` if it can.
    pub fn disabled_reason(&self, id: &str) -> Option<&'static str> {
        match find(id) {
            Some(spec) => (spec.enabled)(self).err(),
            None => Some("unknown command"),
        }
    }

    /// Runs one command. This is the only way state changes.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        let spec = find(id).ok_or_else(|| CommandError::UnknownCommand(id.to_owned()))?;
        (spec.enabled)(self).map_err(|reason| CommandError::Disabled { id: spec.id, reason })?;
        if !(params.is_object() || params.is_null()) {
            return Err(CommandError::BadParam { name: "params", problem: "expected a JSON object".into() });
        }
        let result = (spec.run)(self, &Args(params));
        // A dead link must not leave the engine claiming to be connected.
        if let Err(CommandError::Device(axegrid_device::SessionError::Io(_))) = &result {
            self.session = None;
            self.state.model = None;
        }
        result
    }

    fn session(&mut self) -> Result<&mut Session<Box<dyn Transport>>, CommandError> {
        self.session.as_mut().ok_or(CommandError::Disabled { id: "device", reason: "no device connected" })
    }

    /// Adopts a freshly connected session and loads the current preset.
    fn attach(&mut self, session: Session<Box<dyn Transport>>, simulated: bool) -> Result<(), CommandError> {
        self.state = State { model: Some(session.model()), simulated, ..State::default() };
        self.session = Some(session);
        self.state.firmware = self.session()?.firmware_version().ok();
        self.state.midi_channel = self.session()?.midi_channel().unwrap_or(0);
        self.refresh_preset()
    }

    fn refresh_preset(&mut self) -> Result<(), CommandError> {
        self.state.preset = self.session()?.read_preset()?;
        // Load and tempo belong to the preset; neither is worth failing over.
        if let Ok(axegrid_sysex::Reply::CpuLoad(load)) = self.session()?.request(&axegrid_sysex::Request::CpuLoad) {
            self.state.cpu_load = Some(load);
        }
        let _ = self.refresh_tempo();
        let still_there = |id: &u16| blocks::is_fixed(*id) || self.state.preset.grid.blocks().contains(id);
        if self.state.selected_block.filter(still_there).is_none() {
            self.state.selected_block = None;
            self.state.params.clear();
        } else {
            self.refresh_params()?;
        }
        Ok(())
    }

    fn refresh_params(&mut self) -> Result<(), CommandError> {
        if let Some(block) = self.state.selected_block {
            self.state.params = self.session()?.dump_block(block)?;
        }
        Ok(())
    }

    /// Resolves a `block` argument: a numeric ID, or a display name such as
    /// `"Amp 1"` (case-insensitive).
    fn block_arg(&self, args: &Args<'_>) -> Result<u16, CommandError> {
        let bad = |problem: String| CommandError::BadParam { name: "block", problem };
        match args.raw("block") {
            None => self.state.selected_block.ok_or_else(|| bad("is required when no block is selected".into())),
            Some(Value::String(name)) => blocks::all()
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(name))
                .map(|b| b.id)
                .ok_or_else(|| bad(format!("no block is named {name:?}"))),
            Some(_) => Ok(args.int("block", u64::from(u16::MAX))? as u16),
        }
    }

    /// Resolves a `param` argument: a numeric ID or a symbolic name such as
    /// `"DISTORT_DRIVE"`.
    fn param_arg(&self, args: &Args<'_>, block: u16) -> Result<u16, CommandError> {
        match args.raw("param") {
            Some(Value::String(name)) => {
                blocks::by_id(block).and_then(|b| params::id(b.effect_type, name)).ok_or_else(|| {
                    CommandError::BadParam {
                        name: "param",
                        problem: format!("{name:?} is not a parameter of block {block}"),
                    }
                })
            }
            _ => Ok(args.int("param", u64::from(u16::MAX))? as u16),
        }
    }

    /// Describes one parameter value, with names and metadata where known.
    fn param_json(&self, value: &ParamValue) -> Value {
        let effect_type = blocks::by_id(value.address.block).map(|b| b.effect_type);
        let def =
            effect_type.and_then(|t| self.defs.as_ref()?.effect(t.into())).and_then(|e| e.param(value.address.param));
        json!({
            "block": value.address.block,
            "param": value.address.param,
            "name": effect_type.and_then(|t| params::name(t, value.address.param)),
            "label": effect_type.map(|t| params::label(t, value.address.param)),
            "value": value.value,
            "text": value.text,
            // Highest choice index; 0 for a continuous parameter.
            "choice_max": value.extra[1],
            "choices": def.filter(|d| d.is_enum()).map(|d| &d.strings),
        })
    }

    fn tuner_json(&self) -> Value {
        let reading = self.state.tuner.map(|t| {
            json!({ "note": NOTE_NAMES[usize::from(t.note) % NOTE_NAMES.len()], "string": t.string + 1,
                    "offset": i32::from(t.cents) - TUNER_CENTER })
        });
        json!({ "on": self.state.tuner_on, "reading": reading })
    }

    /// One control of the block editor, with its current value.
    fn control_json(&self, label: &str, kind: Kind, value: &ParamValue) -> Value {
        let mut described = self.param_json(value);
        let stepped_to = u32::from(value.extra[1]);
        let kind = match kind {
            Kind::Knob => "knob",
            Kind::Choice => "choice",
            Kind::Toggle => "toggle",
            Kind::Slider => "slider",
            Kind::Readout => "readout",
            Kind::Spacer => "spacer",
            Kind::Button => "button",
            Kind::Controller => "controller",
            Kind::Refresh => "refresh",
        };
        described["label"] = json!(label);
        described["kind"] = json!(kind);
        described["max"] = json!(if stepped_to > 0 { stepped_to } else { u32::from(RAW_MAX) });
        described
    }

    /// The curve a page draws beside its controls.
    fn graph_json(&self, kind: &str, effect: Option<u8>, values: &layouts::Values) -> Option<Value> {
        if kind != "eq" || effect != Some(PARAMETRIC_EQ) {
            return None;
        }
        let read = |name: String| values.get(&params::id(PARAMETRIC_EQ, &name)?);
        let number = |name: String| read(name).and_then(|(_, text)| eq::leading_number(text));
        let bands: Vec<Band> = (1..=EQ_BANDS)
            .filter_map(|band| {
                // The outer and the inner pairs choose their shape; the middle band is a peak.
                let (chooser, low_side) = match band {
                    1 => (Some("PEQ_LFTYPE"), true),
                    2 => (Some("PEQ_LMTYPE"), true),
                    4 => (Some("PEQ_HMTYPE"), false),
                    5 => (Some("PEQ_HFTYPE"), false),
                    _ => (None, true),
                };
                let chosen =
                    chooser.and_then(|name| read(name.to_owned())).map_or("PEAKING", |(_, text)| text.as_str());
                let shape = match (chosen.to_ascii_uppercase().as_str(), low_side) {
                    ("BLOCKING", true) => Shape::HighPass,
                    ("BLOCKING", false) => Shape::LowPass,
                    (shelf, true) if shelf.starts_with("SHELVING") => Shape::LowShelf,
                    (shelf, false) if shelf.starts_with("SHELVING") => Shape::HighShelf,
                    _ => Shape::Peaking,
                };
                Some(Band {
                    shape,
                    hz: number(format!("PEQ_FREQ{band}"))?,
                    q: number(format!("PEQ_Q{band}"))?,
                    gain_db: number(format!("PEQ_GAIN{band}"))?,
                })
            })
            .collect();
        let points: Vec<Value> = eq::response(&bands, CURVE_POINTS)
            .into_iter()
            .map(|(hz, db)| json!([(hz * 10.0).round() / 10.0, (db * 100.0).round() / 100.0]))
            .collect();
        let marks: Vec<Value> = bands.iter().map(|band| json!([band.hz, band.gain_db])).collect();
        Some(json!({ "kind": "eq", "points": points, "bands": marks }))
    }

    /// The selected block's controls arranged in pages and rows.
    pub fn editor(&self) -> Value {
        let Some(block) = self.state.selected_block else { return Value::Null };
        let effect = blocks::by_id(block).map(|b| b.effect_type);
        let values: layouts::Values =
            self.state.params.iter().map(|p| (p.address.param, (p.value, p.text.clone()))).collect();
        let find = |param: u16| self.state.params.iter().find(|p| p.address.param == param);
        let variant = effect.and_then(layouts::for_effect).and_then(|layout| layout.variant(&values));
        let pages: Vec<Value> = match variant {
            Some(variant) => variant
                .pages
                .iter()
                .filter(|page| page.visible(self.state.firmware, &values))
                .map(|page| {
                    let rows: Vec<Value> = page
                        .rows
                        .iter()
                        .map(|row| {
                            let shown = row.controls.iter().filter(|c| c.visible(self.state.firmware, &values));
                            let controls: Vec<Value> = shown
                                .filter_map(|control| match control.kind {
                                    Kind::Spacer => Some(json!({ "kind": "spacer" })),
                                    Kind::Refresh => Some(json!({ "kind": "refresh", "label": control.label })),
                                    Kind::Button => {
                                        Some(json!({ "kind": "button", "label": control.label, "sets": control.sets }))
                                    }
                                    Kind::Controller => control.controller.map(|number| {
                                        json!({ "kind": "controller", "label": control.label, "controller": number,
                                                "button": Self::looper_button(number),
                                                "once": control.once, "on": self.looper_lit(number) })
                                    }),
                                    kind => find(control.param).map(|v| self.control_json(&control.label, kind, v)),
                                })
                                .collect();
                            (row.mixer, controls)
                        })
                        .filter(|(_, controls)| controls.iter().any(|c| c["kind"] != "spacer"))
                        .map(|(mixer, controls)| json!({ "mixer": mixer, "controls": controls }))
                        .collect();
                    let graph = page.graph.as_deref().and_then(|kind| self.graph_json(kind, effect, &values));
                    json!({ "name": page.name, "rows": rows, "graph": graph })
                })
                .filter(|page| page["rows"].as_array().is_some_and(|rows| !rows.is_empty()))
                .collect(),
            // No layout for this block: every parameter, in ID order.
            None => {
                let rows: Vec<Value> = self
                    .state
                    .params
                    .chunks(PLAIN_ROW)
                    .map(|chunk| {
                        let controls: Vec<Value> = chunk
                            .iter()
                            .map(|v| {
                                let label = effect.map(|t| params::label(t, v.address.param)).unwrap_or_default();
                                let kind = if v.extra[1] > 0 { Kind::Choice } else { Kind::Knob };
                                self.control_json(&label, kind, v)
                            })
                            .collect();
                        json!({ "mixer": false, "controls": controls })
                    })
                    .collect();
                vec![json!({ "name": "Parameters", "rows": rows })]
            }
        };
        // The parameter that names the block's type or model, where it has one.
        let model = find(0)
            .filter(|_| effect.and_then(|t| params::name(t, 0)).is_some_and(|name| name.ends_with("_TYPE")))
            .map(|v| self.control_json("Type", Kind::Choice, v));
        json!({ "block": block, "name": blocks::label(block), "fixed": blocks::is_fixed(block),
                "variant": variant.map(|v| v.name.as_str()), "model": model, "pages": pages })
    }

    /// A JSON snapshot of the whole editor state.
    pub fn inspect(&self) -> Value {
        let s = &self.state;
        let view = &s.preset;
        let cells: Vec<Value> = (0..grid::COLUMNS)
            .flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)))
            .filter(|&(column, row)| view.grid.cell(column, row).block != blocks::EMPTY)
            .map(|(column, row)| {
                let cell = view.grid.cell(column, row);
                let inputs: Vec<usize> =
                    (0..grid::ROWS).filter(|&from| view.grid.is_fed_by(column, row, from)).collect();
                json!({ "column": column, "row": row, "block": cell.block,
                        "name": blocks::label(cell.block), "inputs_from_rows": inputs })
            })
            .collect();
        let states: Vec<Value> = view
            .states
            .iter()
            .map(|b| {
                json!({ "block": b.block, "name": blocks::label(b.block), "engaged": b.engaged,
                             "xy": if b.xy == XyState::X { "X" } else { "Y" } })
            })
            .collect();
        json!({
            "connected": self.is_connected(),
            "simulated": s.simulated,
            "model": s.model.map(Model::name),
            "firmware": s.firmware.map(|(major, minor)| format!("{major}.{minor:02}")),
            "cpu_load": s.cpu_load,
            "tempo": s.tempo,
            "tuner": self.tuner_json(),
            "looper": self.looper_json(),
            "definitions_loaded": self.defs.is_some(),
            "preset": { "number": view.number, "name": view.name, "scene": view.scene + 1 },
            "grid": cells,
            "blocks": states,
            "selected_block": s.selected_block,
            "params": s.params.iter().map(|p| self.param_json(p)).collect::<Vec<_>>(),
            "editor": self.editor(),
        })
    }
}

/// The registry as JSON, for discovery by scripts and agents.
pub fn command_list(engine: &Engine) -> Value {
    registry()
        .iter()
        .map(|c| {
            json!({
                "id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "doc": c.doc,
                "enabled": (c.enabled)(engine).is_ok(),
                "params": c.params.iter().map(|p| json!({
                    "name": p.name, "type": p.kind, "required": p.required, "doc": p.doc,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}
