//! The egui shell. It draws from the [`axegrid_engine::View`] the engine
//! last published and acts only by running commands: every menu item,
//! button, knob and shortcut below ends in [`Shell::run`]. Menus and
//! shortcuts are generated from the command registry, so registering a
//! command with a menu path is all it takes to surface it.
//!
//! The engine, and with it the conversation with the device, lives on a
//! thread of its own ([`axegrid_engine::Worker`]). A command is handed over
//! and its outcome picked up on a later frame, so a unit that answers
//! slowly, or not at all, never stops the window from drawing.
//!
//! The window is arranged the way hardware-editor users expect: preset and
//! scenes across the top, the routing grid under it, the selected block's
//! controls at the bottom. The look itself lives in [`theme`] and [`knob`].

pub mod knob;
mod library;
mod picker;
mod selection;
pub mod theme;

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axegrid_engine::{registry, CommandError, Engine, View, Worker, XyState};
use axegrid_model::{blocks, grid};
use egui::epaint::CubicBezierShape;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Rect, RichText,
};
use egui::{Sense, Stroke, StrokeKind, Vec2};
use selection::Selection;
use serde_json::{json, Value};
use theme::Category;

/// Smallest window the layout is designed for.
pub const MIN_WINDOW: [f32; 2] = [1120.0, 680.0];

const TILE: Vec2 = vec2(68.0, 46.0);
const TILE_GAP: Vec2 = vec2(28.0, 14.0);
/// Width of the input and output bars at the ends of the grid.
const RAIL: f32 = 26.0;
/// How far the grid may shrink or grow with the window.
const GRID_SCALE: (f32, f32) = (0.6, 2.2);
/// Radius of the cable handles on a tile's edges, at scale 1.
const HANDLE: f32 = 8.0;
/// Commands the shell asks about before running.
const CONFIRMED: &[&str] = &["preset.store", "preset.copy", "cab.import"];
/// How often the unit is listened to for what it sends by itself, with
/// the tuner off and on.
const POLL_PERIOD: Duration = Duration::from_millis(100);
const TUNER_POLL_PERIOD: Duration = Duration::from_millis(40);
/// Requests spent on choice lists per turn, so that an edit made meanwhile
/// does not wait long behind them.
const LIST_BUDGET: u64 = 8;
/// A command that has kept the unit this long is shown as being waited for.
const SLOW: Duration = Duration::from_millis(400);
const SEARCHING: &str = "Looking for your Axe-Fx…";
/// Height of a row of the header, which is that of its buttons.
const HEADER_ROW: f32 = 24.0;
/// The preset's name in the header: its text size, the height of its row,
/// the width of each arrow beside it and the least width the name keeps.
const PRESET_FONT: f32 = 20.0;
const PRESET_HEIGHT: f32 = 34.0;
const PRESET_ARROW: f32 = 38.0;
const PRESET_NAME_MIN: f32 = 120.0;
/// Width of one character of the small text in a control cell, roughly.
const CHOICE_CHAR_WIDTH: f32 = 7.4;
/// A fader's share of a cell's width.
const FADER_WIDTH: f32 = 0.62;
/// How many rows of knobs a row of faders is as tall as.
const FADER_ROWS: f32 = 2.0;
/// Width of a page's curve, in cells, and the frequencies and gain steps it shows.
const GRAPH_UNITS: f32 = 4.0;
const GRAPH_HZ: (f32, f32) = (20.0, 20_000.0);
const GRAPH_STEP_DB: f32 = 6.0;
/// Lines across a level curve, in decibels.
const LEVEL_STEP_DB: f32 = 20.0;
/// Space around the line that sets the output section apart.
const MIXER_GAP: f32 = 18.0;
/// Most of the window's height the block editor may take.
const EDITOR_SHARE: f32 = 0.62;
/// Height of the block editor apart from its rows of controls.
const EDITOR_CHROME: f32 = 66.0;
/// How often the load meter is refreshed.
const CPU_PERIOD: Duration = Duration::from_secs(2);
/// How long the tempo lamp stays lit after a beat.
const BEAT_FLASH: Duration = Duration::from_millis(110);
/// Tuner offsets within this many steps of centre read as in tune.
const IN_TUNE: i64 = 2;
/// Height of the strip the looper and the tone match show above their controls.
const STRIP: f32 = 34.0;
/// How often a running tone match capture is read back.
const TONE_PERIOD: Duration = Duration::from_millis(500);
/// Knobs per row of the modifier window, and the side of its curve.
const MODIFIER_ROW: usize = 5;
const MODIFIER_GRAPH: f32 = 190.0;
const TUNER_SPAN: f32 = 63.0;

/// A command the user asked for during this frame.
type Action = (&'static str, Value);

/// A command the engine has finished.
struct Finished {
    id: &'static str,
    params: Value,
    /// Housekeeping: only a failure reaches the status bar.
    quiet: bool,
    outcome: Result<Value, CommandError>,
}

/// What a command that sets one value is keyed by. A control being dragged
/// issues such a command every frame; only the latest one waiting is worth
/// sending once the unit has answered the one before.
fn value_key(id: &str, params: &Value) -> Option<String> {
    match id {
        "param.set" => Some(format!("{}/{}", params["block"], params["param"])),
        "tempo.set" => Some(TEMPO_KEY.to_owned()),
        "modifier.set" => Some(modifier_key(params["field"].as_str().unwrap_or_default())),
        _ => None,
    }
}

const TEMPO_KEY: &str = "tempo";

/// What a field of the open modifier is keyed by while it is being set.
fn modifier_key(field: &str) -> String {
    format!("modifier/{field}")
}

/// One end of a cable being drawn: a cell, and whether it is its output side.
type CableEnd = (usize, usize, bool);

/// What the user is in the middle of on the grid.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct GridGesture {
    /// The cable end clicked first, while a cable is being drawn.
    cable: Option<CableEnd>,
    /// The cell the selected blocks are being dragged by.
    dragging: Option<(usize, usize)>,
    /// The cells picked to be moved together.
    selection: Selection,
    /// Where a rubber band was started, while one is being pulled.
    band: Option<Pos2>,
    /// A move is on its way to the unit; the selection follows it once it has landed.
    moving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rail {
    Input,
    Output,
}

/// How much room a page of the block editor needs.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct PageShape {
    /// Height of each section in rows of knobs; a row of faders is taller.
    param_rows: f32,
    mixer_rows: f32,
    /// Widest row of each section, in cells.
    param_units: f32,
    mixer_units: f32,
    graph: bool,
}

impl PageShape {
    fn of(page: &Value) -> Self {
        let mut shape = Self { graph: page["graph"].is_object(), ..Self::default() };
        for row in page["rows"].as_array().into_iter().flatten() {
            let controls = || row["controls"].as_array().into_iter().flatten();
            let units: f32 = controls().map(|c| if c["kind"] == "slider" { FADER_WIDTH } else { 1.0 }).sum();
            let height = if controls().any(|c| c["kind"] == "slider") { FADER_ROWS } else { 1.0 };
            if row["mixer"] == true {
                shape.mixer_rows += height;
                shape.mixer_units = shape.mixer_units.max(units);
            } else {
                shape.param_rows += height;
                shape.param_units = shape.param_units.max(units);
            }
        }
        shape
    }

    fn rows(&self) -> f32 {
        self.param_rows.max(self.mixer_rows).max(if self.graph { 2.0 } else { 1.0 })
    }

    fn units(&self) -> f32 {
        let graph = if self.graph { GRAPH_UNITS + 0.15 } else { 0.0 };
        (self.param_units + graph + self.mixer_units).max(1.0)
    }
}

/// One control of the selected block, as the engine describes it.
struct ControlView {
    kind: String,
    id: u64,
    name: String,
    label: String,
    raw: u32,
    max: u32,
    text: String,
    choices: Option<Vec<String>>,
    /// Parameters a push button sets.
    sets: Vec<u64>,
    /// The parameter takes a modifier.
    modifier: bool,
    /// Its value is a position in the unit's cab list.
    cabs: bool,
}

impl ControlView {
    fn from_json(value: &Value) -> Self {
        let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        // A looper button carries its command name and whether it is lit.
        let transport = value["kind"] == "controller";
        Self {
            kind: text("kind"),
            id: value["param"].as_u64().unwrap_or_default(),
            name: if transport { text("button") } else { text("name") },
            label: text("label"),
            raw: value["value"].as_u64().unwrap_or(u64::from(value["on"] == true)) as u32,
            max: value["max"].as_u64().unwrap_or(1) as u32,
            text: text("text"),
            choices: serde_json::from_value(value["choices"].clone()).ok().flatten(),
            sets: value["sets"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect(),
            modifier: value["modifier"] == true,
            cabs: value["cab_list"].is_object(),
        }
    }
}

pub struct Shell {
    worker: Worker,
    /// The engine's state after the last command it finished: what is drawn.
    view: Arc<View>,
    finished: Receiver<Finished>,
    report: Sender<Finished>,
    /// Values set and not yet answered by the unit, shown meanwhile so that
    /// a control follows the pointer, by [`value_key`].
    unconfirmed: HashMap<String, u64>,
    /// Sets on their way to the unit, each with the latest one waiting behind it.
    in_flight: HashMap<String, Option<Value>>,
    polling: bool,
    polled_at: Option<Instant>,
    /// The unit is being looked for.
    searching: bool,
    reading_lists: bool,
    /// The block whose choice lists could not be read, so as not to keep asking.
    lists_failed: Option<u16>,
    status: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    input: String,
    output: String,
    /// Text of the preset-number box.
    goto: String,
    /// The name being typed while the preset is renamed.
    rename: Option<String>,
    rename_needs_focus: bool,
    page: usize,
    page_block: Option<u16>,
    /// A command that overwrites something, waiting for a yes.
    confirm: Option<Action>,
    library: library::LibraryView,
    themed: bool,
    gesture: GridGesture,
    cpu_read_at: Option<Instant>,
    beats_seen: u64,
    beat_at: Option<Instant>,
    /// The scene being named, with the text typed so far.
    scene_rename: Option<(u8, String)>,
    scene_rename_needs_focus: bool,
    tone_read_at: Option<Instant>,
    /// The browser of a long choice list, while it is open.
    picker: Option<picker::Picker>,
}

/// Turns a registry shortcut such as `Ctrl+Right` or `F5` into an egui one.
fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        match part {
            "Ctrl" => modifiers |= Modifiers::COMMAND,
            "Shift" => modifiers |= Modifiers::SHIFT,
            "Alt" => modifiers |= Modifiers::ALT,
            "Left" | "Right" | "Up" | "Down" => key = Key::from_name(&format!("Arrow{part}")),
            name => key = Key::from_name(name),
        }
    }
    key.map(|key| KeyboardShortcut::new(modifiers, key))
}

/// Cuts `text` to what fits in `width` points of a control cell.
fn shorten(text: &str, width: f32) -> String {
    let fits = ((width / CHOICE_CHAR_WIDTH) as usize).max(4);
    if text.chars().count() <= fits {
        return text.to_owned();
    }
    text.chars().take(fits - 1).chain(['…']).collect()
}

/// The size the grid is drawn at so that it fits `available`.
fn grid_scale(available: Vec2, natural: Vec2) -> f32 {
    (available.x / natural.x).min(available.y / natural.y).clamp(GRID_SCALE.0, GRID_SCALE.1)
}

/// The command that a click on a cable end issues, given the end clicked
/// before it, as `(column, row, from_row)`. `None` means the click starts
/// (or restarts) a cable.
fn cable_pair(pending: Option<CableEnd>, clicked: CableEnd) -> Option<(usize, usize, usize)> {
    let (column, row, output) = pending?;
    let (to_column, to_row, to_output) = clicked;
    match (output, to_output) {
        (true, false) if to_column == column + 1 => Some((to_column, to_row, row)),
        (false, true) if column == to_column + 1 => Some((column, row, to_row)),
        _ => None,
    }
}

/// A button that reads as lit when `on`.
fn lit_button(ui: &mut egui::Ui, text: &str, on: bool, lit: Color32) -> egui::Response {
    let (fill, ink) = if on { (lit, theme::text_on(lit)) } else { (theme::RAISED, theme::TEXT_DIM) };
    ui.add(egui::Button::new(RichText::new(text).color(ink).strong()).fill(fill).min_size(vec2(28.0, 24.0)))
}

/// A button filled with `fill`, its text in the colour that reads on it.
fn filled_button(text: &str, fill: Color32) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(theme::text_on(fill)).strong()).fill(fill)
}

/// Paints `text` with the middle of its ink, not of its line box, on
/// `centre`. A line box leaves room for descenders, so a capital centred by
/// it sits off the middle of a small circle or bar.
fn centred_text(painter: &egui::Painter, centre: Pos2, text: &str, font: FontId, ink: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, ink);
    let bounds = galley.mesh_bounds;
    let middle = if bounds.is_finite() && bounds.is_positive() { bounds.center() } else { galley.rect.center() };
    painter.galley(centre - middle.to_vec2(), galley, ink);
}

/// Cuts `text`, which is `drawn` points wide, to what fits in `room` points.
fn fitted(text: &str, drawn: f32, room: f32) -> String {
    let letters = text.chars().count();
    if drawn <= room || letters == 0 {
        return text.to_owned();
    }
    // `shorten` counts in letters of its own width, so hand it this text's.
    shorten(text, room / (drawn / letters as f32) * CHOICE_CHAR_WIDTH)
}

impl Shell {
    /// Starts the engine on its own thread and looks for the unit. `ctx` is
    /// repainted whenever the engine has finished a command.
    pub fn new(ctx: egui::Context) -> std::io::Result<Self> {
        let worker = Worker::spawn(Engine::for_this_computer(), move || ctx.request_repaint())?;
        let (report, finished) = mpsc::channel();
        let mut shell = Self {
            view: worker.view(),
            worker,
            finished,
            report,
            unconfirmed: HashMap::new(),
            in_flight: HashMap::new(),
            polling: false,
            polled_at: None,
            searching: false,
            reading_lists: false,
            lists_failed: None,
            status: String::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            input: String::new(),
            output: String::new(),
            goto: String::new(),
            rename: None,
            rename_needs_focus: false,
            page: 0,
            page_block: None,
            confirm: None,
            library: library::LibraryView::default(),
            themed: false,
            gesture: GridGesture::default(),
            cpu_read_at: None,
            beats_seen: 0,
            beat_at: None,
            scene_rename: None,
            scene_rename_needs_focus: false,
            tone_read_at: None,
            picker: None,
        };
        shell.submit("device.list_ports", Value::Null, true);
        shell.find_unit();
        Ok(shell)
    }

    /// The thread the engine runs on, for callers that drive it too.
    pub fn worker(&self) -> &Worker {
        &self.worker
    }

    fn submit(&self, id: &'static str, params: Value, quiet: bool) {
        let report = self.report.clone();
        let asked = params.clone();
        self.worker.run(id, params, move |outcome| {
            // Nobody is listening once the window has closed.
            let _ = report.send(Finished { id, params: asked, quiet, outcome });
        });
    }

    /// Hands a command to the engine. Its outcome shows in the status bar
    /// once the unit has answered.
    pub fn run(&mut self, id: &'static str, params: Value) {
        if let Some(key) = value_key(id, &params) {
            if let Some(value) = params["value"].as_u64().or_else(|| params["bpm"].as_u64()) {
                self.unconfirmed.insert(key.clone(), value);
            }
            if let Some(waiting) = self.in_flight.get_mut(&key) {
                *waiting = Some(params);
                return;
            }
            self.in_flight.insert(key, None);
        }
        self.submit(id, params, false);
    }

    /// Takes in the outcome of a command the engine has finished.
    fn settle(&mut self, done: Finished) {
        if let Some(key) = value_key(done.id, &done.params) {
            match self.in_flight.remove(&key).flatten() {
                Some(next) => {
                    self.in_flight.insert(key, None);
                    self.submit(done.id, next, false);
                }
                // The unit has the last word on the value now.
                None => drop(self.unconfirmed.remove(&key)),
            }
        }
        match (done.id, &done.outcome) {
            ("device.poll", _) => self.polling = false,
            ("device.connect_auto", _) => self.searching = false,
            ("block.select", _) => self.lists_failed = None,
            ("grid.move_block" | "grid.move_blocks", outcome) => {
                self.gesture.moving = false;
                // The selection stays on the blocks it was on.
                if let (Ok(_), Some(moved)) = (outcome, Selection::after(done.id, &done.params)) {
                    self.gesture.selection = moved;
                }
            }
            ("block.read_lists", outcome) => {
                self.reading_lists = false;
                if outcome.is_err() {
                    self.lists_failed = self.view.state.selected_block;
                }
            }
            ("device.list_ports", Ok(result)) => {
                let names =
                    |key: &str| -> Vec<String> { serde_json::from_value(result[key].clone()).unwrap_or_default() };
                (self.inputs, self.outputs) = (names("inputs"), names("outputs"));
            }
            ("library.scan" | "preset.export_range", _) if done.quiet => {
                return self.library_done(done.id, &done.params, &done.outcome);
            }
            _ => {}
        }
        let did = |result: &Value, key: &str| result[key].as_str().unwrap_or_default().to_owned();
        match (&done.outcome, done.id) {
            (Ok(found), "device.connect_auto") => {
                self.status = format!("Connected on {}", found["port"].as_str().unwrap_or("a MIDI port"));
            }
            (Err(_), "device.connect_auto") => {
                self.status = "No Axe-Fx found. Switch it on and press Find my Axe-Fx, or pick the ports.".into();
            }
            (Ok(result), "edit.undo") => self.status = format!("Undid {}", did(result, "undone")),
            (Ok(result), "edit.redo") => self.status = format!("Redid {}", did(result, "redone")),
            (Ok(_), id) if !done.quiet => self.status = format!("{id}: ok"),
            (Err(error), _) => self.status = error.to_string(),
            _ => {}
        }
    }

    /// Takes in what the engine has finished, and keeps tuner, tempo lamp,
    /// load meter, choice lists and front-panel changes current.
    fn housekeeping(&mut self, ctx: &egui::Context) {
        // Outcomes before the view: the view is then at least as new as each of them.
        while let Ok(done) = self.finished.try_recv() {
            self.settle(done);
        }
        self.view = self.worker.view();
        if self.worker.pending() > 0 {
            // So that a long wait shows as one.
            ctx.request_repaint_after(SLOW / 2);
        }
        if !self.view.connected {
            self.gesture = GridGesture::default();
            self.library.reset();
            self.unconfirmed.clear();
            self.in_flight.clear();
            (self.polling, self.reading_lists) = (false, false);
            return;
        }
        // Housekeeping waits its turn behind whatever the user asked for.
        let free = self.worker.pending() == 0;
        let tuning = self.view.state.tuner_on;
        let period = if tuning { TUNER_POLL_PERIOD } else { POLL_PERIOD };
        if !self.polling && self.polled_at.is_none_or(|at| at.elapsed() >= period) {
            (self.polling, self.polled_at) = (true, Some(Instant::now()));
            self.submit("device.poll", Value::Null, true);
        }
        let beats = self.view.state.beats;
        if beats != std::mem::replace(&mut self.beats_seen, beats) {
            self.beat_at = Some(Instant::now());
        }
        // Not while a control is being dragged: its edits would queue behind these.
        let idle = free && !ctx.input(|input| input.pointer.any_down());
        if idle && self.cpu_read_at.is_none_or(|at| at.elapsed() >= CPU_PERIOD) {
            self.cpu_read_at = Some(Instant::now());
            self.submit("device.cpu_load", Value::Null, true);
        }
        let lists_wanted = self.view.lists_missing > 0 && self.lists_failed != self.view.state.selected_block;
        if idle && lists_wanted && !self.reading_lists {
            self.reading_lists = true;
            self.submit("block.read_lists", json!({ "budget": LIST_BUDGET }), true);
        }
        let capturing =
            ["capturing_reference", "capturing_local"].iter().any(|key| self.view.editor["tone_match"][*key] == true);
        if idle && capturing && self.tone_read_at.is_none_or(|at| at.elapsed() >= TONE_PERIOD) {
            self.tone_read_at = Some(Instant::now());
            self.submit("tonematch.status", Value::Null, true);
        }
        if idle {
            self.library_work();
        }
        let soon = if self.library.busy(&self.view.state.name_scan) || lists_wanted {
            5
        } else if tuning {
            40
        } else {
            120
        };
        ctx.request_repaint_after(Duration::from_millis(soon));
    }

    /// Connects to the first unit that answers, USB before other MIDI ports.
    fn find_unit(&mut self) {
        self.searching = true;
        self.status = SEARCHING.into();
        self.submit("device.connect_auto", Value::Null, false);
    }

    fn shortcuts(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // Copy and paste arrive as events of their own, not as key presses.
        let (copied, pasted) = ctx.input(|input| {
            let has = |wanted: fn(&egui::Event) -> bool| input.events.iter().any(wanted);
            (has(|event| matches!(event, egui::Event::Copy)), has(|event| matches!(event, egui::Event::Paste(_))))
        });
        if copied && self.view.disabled_reason("block.copy").is_none() {
            actions.push(("block.copy", Value::Null));
            // Something on the system clipboard, so that a paste is offered back.
            let name = self.view.state.selected_block.map(blocks::label).unwrap_or_default();
            ctx.copy_text(format!("Axe-Grid block: {name}"));
        }
        if pasted && self.view.disabled_reason("block.paste").is_none() {
            actions.push(("block.paste", Value::Null));
        }
        for command in registry().iter().filter(|c| !c.shortcut.is_empty()) {
            if matches!(command.id, "block.copy" | "block.paste") {
                continue;
            }
            let Some(shortcut) = parse_shortcut(command.shortcut) else { continue };
            let enabled = self.view.disabled_reason(command.id).is_none();
            if enabled && ctx.input_mut(|input| input.consume_shortcut(&shortcut)) {
                actions.push((command.id, Value::Null));
            }
        }
    }

    fn menu_bar(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut menus: BTreeMap<&str, Vec<(&str, &'static axegrid_engine::CommandSpec)>> = BTreeMap::new();
        for command in registry().iter().copied() {
            if let Some((menu, item)) = command.menu.split_once('/') {
                menus.entry(menu).or_default().push((item, command));
            }
        }
        egui::MenuBar::new().ui(ui, |ui| {
            for (menu, items) in menus {
                ui.menu_button(menu, |ui| {
                    for (item, command) in items {
                        let reason = self.view.disabled_reason(command.id);
                        // Undo and Redo say what they would act on.
                        let acts_on = match command.id {
                            "edit.undo" => self.view.next_undo.as_deref(),
                            "edit.redo" => self.view.next_redo.as_deref(),
                            _ => None,
                        };
                        let item = acts_on.map_or_else(|| item.to_owned(), |edit| format!("{item} {edit}"));
                        let button = egui::Button::new(item).shortcut_text(command.shortcut);
                        let response = ui.add_enabled(reason.is_none(), button);
                        if response
                            .on_hover_text(command.doc)
                            .on_disabled_hover_text(reason.unwrap_or_default())
                            .clicked()
                        {
                            actions.push((command.id, Value::Null));
                            ui.close();
                        }
                    }
                });
            }
        });
    }

    fn connection_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let port = |ui: &mut egui::Ui, id: &str, names: &[String], chosen: &mut String| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(if chosen.is_empty() { "choose a port" } else { chosen.as_str() })
                .show_ui(ui, |ui| {
                    for name in names {
                        ui.selectable_value(chosen, name.clone(), name);
                    }
                });
        };
        let find = filled_button("Find my Axe-Fx", theme::ACCENT);
        if ui.add(find).on_hover_text("Look on USB first, then on every MIDI port").clicked() {
            actions.push(("device.connect_auto", Value::Null));
        }
        ui.separator();
        ui.label(RichText::new("MIDI IN").small().color(theme::TEXT_DIM));
        port(ui, "midi-in", &self.inputs, &mut self.input);
        ui.label(RichText::new("OUT").small().color(theme::TEXT_DIM));
        port(ui, "midi-out", &self.outputs, &mut self.output);
        if ui.button("Rescan").clicked() {
            actions.push(("device.list_ports", Value::Null));
        }
        let ready = !self.input.is_empty() && !self.output.is_empty();
        if ui.add_enabled(ready, egui::Button::new("Connect")).clicked() {
            actions.push(("device.connect", json!({ "input": self.input, "output": self.output })));
        }
        if ui.button("Simulator").on_hover_text("Try the editor without hardware").clicked() {
            actions.push(("device.connect_simulator", Value::Null));
        }
    }

    /// The preset's number and name between the previous and next arrows.
    /// It fills the width it is given: the name gets what the rest leaves.
    fn preset_name(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let view = self.view.clone();
        let state = &view.state;
        let arrow = |ui: &mut egui::Ui, glyph: &str| {
            let text = RichText::new(glyph).size(PRESET_FONT);
            ui.add(egui::Button::new(text).min_size(vec2(PRESET_ARROW, PRESET_HEIGHT)))
        };
        if arrow(ui, "◀").on_hover_text("Previous preset").clicked() {
            actions.push(("preset.previous", Value::Null));
        }
        let number = egui::TextEdit::singleline(&mut self.goto)
            .hint_text(format!("{:03}", state.preset.number))
            .desired_width(42.0)
            .font(FontId::monospace(17.0));
        let number = ui.add(number).on_hover_text("Type a preset number and press Enter");
        // Everything left but the next arrow and the gap before it.
        let width = (ui.available_width() - PRESET_ARROW - ui.spacing().item_spacing.x).max(PRESET_NAME_MIN);
        let font = FontId::proportional(PRESET_FONT);
        if number.lost_focus() {
            if let (true, Ok(wanted)) = (ui.input(|i| i.key_pressed(Key::Enter)), self.goto.trim().parse::<u16>()) {
                actions.push(("preset.select", json!({ "number": wanted })));
            }
            self.goto.clear();
        }
        match &mut self.rename {
            Some(name) => {
                // The box is its text plus a margin of 4 points on each side.
                let field = egui::TextEdit::singleline(name).desired_width(width - 8.0).font(font).char_limit(32);
                let edit = ui.add(field);
                if std::mem::take(&mut self.rename_needs_focus) {
                    edit.request_focus();
                }
                if edit.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty() {
                        actions.push(("preset.rename", json!({ "name": name.trim() })));
                    }
                    self.rename = None;
                }
            }
            None => {
                let shown = if state.preset.name.is_empty() { "(unnamed)" } else { state.preset.name.as_str() };
                let drawn = ui.fonts_mut(|fonts| fonts.layout_no_wrap(shown.to_owned(), font, theme::TEXT)).size().x;
                // A little under the button's inside, as letters differ in width.
                let room = (width - 2.0 * ui.spacing().button_padding.x) * 0.94;
                let cut = fitted(shown, drawn, room);
                let hint =
                    if cut == shown { "Click to rename".to_owned() } else { format!("{shown}\nClick to rename") };
                let name = egui::Button::new(RichText::new(cut).size(PRESET_FONT).strong())
                    .min_size(vec2(width, PRESET_HEIGHT));
                if ui.add(name).on_hover_text(hint).clicked() {
                    self.rename = Some(state.preset.name.clone());
                    self.rename_needs_focus = true;
                }
            }
        }
        if arrow(ui, "▶").on_hover_text("Next preset").clicked() {
            actions.push(("preset.next", Value::Null));
        }
    }

    fn preset_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let view = self.view.clone();
        // Laid out from the right: the buttons first, so that the preset's
        // name can take all the room they leave.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Disconnect").clicked() {
                actions.push(("device.disconnect", Value::Null));
            }
            let save = filled_button("Save", theme::ACCENT);
            if ui.add(save).on_hover_text("Write the edit buffer to the device (asks first)").clicked() {
                actions.push(("preset.store", Value::Null));
            }
            ui.separator();
            let cabs = lit_button(ui, "Cabs", self.library.cabs_open, theme::ACCENT);
            if cabs.on_hover_text("User cabs: list, export, import").clicked() {
                self.library.cabs_open = !self.library.cabs_open;
            }
            let presets = lit_button(ui, "Presets", self.library.presets_open, theme::ACCENT);
            if presets.on_hover_text("All presets: browse, copy, export, import, back up").clicked() {
                self.library.presets_open = !self.library.presets_open;
            }
            ui.separator();
            // Laid out from the right, so Redo comes first.
            for (name, command, edit) in
                [("Redo", "edit.redo", &view.next_redo), ("Undo", "edit.undo", &view.next_undo)]
            {
                let hint = edit
                    .as_ref()
                    .map_or_else(|| format!("Nothing to {}", name.to_lowercase()), |e| format!("{name} {e}"));
                let button = ui.add_enabled(edit.is_some(), egui::Button::new(name));
                if button.on_hover_text(&hint).on_disabled_hover_text(hint).clicked() {
                    actions.push((command, Value::Null));
                }
            }
            ui.separator();
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| self.preset_name(ui, actions));
        });
    }

    /// Second row of the header: scenes, tuner, tempo and the load meter.
    fn performance_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let view = self.view.clone();
        let state = &view.state;
        ui.label(RichText::new("SCENE").small().color(theme::TEXT_DIM));
        for scene in 0..8u8 {
            let name = state.scene_names.get(usize::from(scene)).map(String::as_str).unwrap_or_default();
            let text = if name.is_empty() { (scene + 1).to_string() } else { format!("{} {name}", scene + 1) };
            let button = lit_button(ui, &text, state.preset.scene == scene, theme::ACCENT);
            if button.clicked() {
                actions.push(("scene.select", json!({ "scene": scene + 1 })));
            }
            button.on_hover_text("Click to switch; right-click to name, copy or paste").context_menu(|ui| {
                if ui.button("Name…").clicked() {
                    self.scene_rename = Some((scene, name.to_owned()));
                    self.scene_rename_needs_focus = true;
                    ui.close();
                }
                if ui
                    .button("Copy")
                    .on_hover_text("Bypass and X/Y of every block, main level, scene controllers")
                    .clicked()
                {
                    if state.preset.scene != scene {
                        actions.push(("scene.select", json!({ "scene": scene + 1 })));
                    }
                    actions.push(("scene.copy", Value::Null));
                    ui.close();
                }
                if ui.add_enabled(view.scene_copied, egui::Button::new("Paste")).clicked() {
                    actions.push(("scene.paste", json!({ "scene": scene + 1 })));
                    ui.close();
                }
            });
        }
        ui.separator();
        let tuner = lit_button(ui, "Tuner", state.tuner_on, theme::LAMP_ON);
        if tuner.on_hover_text("Switch the unit's tuner on or off (T)").clicked() {
            actions.push(("tuner.toggle", Value::Null));
        }
        ui.separator();
        let (lamp, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        let flashing = self.beat_at.is_some_and(|at| at.elapsed() < BEAT_FLASH);
        ui.painter().circle_filled(lamp.center(), 4.5, if flashing { theme::ACCENT } else { theme::LAMP_OFF });
        if ui.button("Tap").on_hover_text("Tap twice in time to set the tempo (Space)").clicked() {
            actions.push(("tempo.tap", Value::Null));
        }
        if let Some(tempo) = state.tempo {
            // While it is dragged the box shows what was asked for, not the unit's answer so far.
            let tempo = self.unconfirmed.get(TEMPO_KEY).map_or(tempo, |asked| *asked as u16);
            let mut bpm = tempo;
            let (slowest, fastest) = axegrid_engine::TEMPO_RANGE;
            let field =
                egui::DragValue::new(&mut bpm).range(slowest as u16..=fastest as u16).suffix(" BPM").speed(0.25);
            if ui.add(field).on_hover_text("Drag, or double-click to type a tempo").changed() && bpm != tempo {
                actions.push(("tempo.set", json!({ "bpm": bpm })));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(load) = state.cpu_load {
                let (meter, _) = ui.allocate_exact_size(vec2(90.0, 8.0), Sense::hover());
                let filled = meter.width() * f32::from(load.min(100)) / 100.0;
                let ink = if load >= 90 { theme::DANGER } else { theme::LAMP_ON };
                ui.painter().rect_filled(meter, 4.0_f32, theme::EDGE);
                ui.painter().rect_filled(Rect::from_min_size(meter.min, vec2(filled, meter.height())), 4.0_f32, ink);
                ui.label(RichText::new(format!("CPU {load}%")).small().color(theme::TEXT_DIM));
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        // A row is given its full height up front: one that grows as taller
        // things are added leaves the shorter ones above its middle.
        let row =
            |ui: &egui::Ui, height: f32| (vec2(ui.available_width(), height), Layout::left_to_right(Align::Center));
        let (size, layout) = row(ui, if self.view.connected { PRESET_HEIGHT } else { HEADER_ROW });
        ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.label(RichText::new("AXE·GRID").size(18.0).strong().color(theme::ACCENT));
            let view = self.view.clone();
            let state = &view.state;
            let (lamp, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            let lit = if view.connected { theme::LAMP_ON } else { theme::LAMP_OFF };
            ui.painter().circle_filled(lamp.center(), 4.5, lit);
            let model = state.model.map_or("offline", |m| m.name());
            let firmware = state.firmware.map(|(a, b)| format!("  fw {a}.{b:02}")).unwrap_or_default();
            let tag = if state.simulated { "  simulated" } else { "" };
            ui.label(RichText::new(format!("{model}{firmware}{tag}")).small().color(theme::TEXT_DIM));
            ui.separator();
            if view.connected {
                self.preset_controls(ui, actions);
            } else {
                if self.searching {
                    ui.spinner();
                }
                // One search at a time; what is clicked meanwhile would only queue behind it.
                ui.add_enabled_ui(!self.searching, |ui| self.connection_controls(ui, actions));
            }
        });
        if self.view.connected {
            ui.add_space(4.0);
            let (size, layout) = row(ui, HEADER_ROW);
            ui.allocate_ui_with_layout(size, layout, |ui| self.performance_controls(ui, actions));
        }
    }

    /// The right-click menu of one grid cell.
    fn cell_menu(&self, ui: &mut egui::Ui, column: usize, row: usize, actions: &mut Vec<Action>) {
        let view = &self.view.state.preset;
        let id = view.grid.cell(column, row).block;
        let cell = json!({ "column": column, "row": row });
        let with_cell = |extra: Value| {
            let mut params = cell.clone();
            if let (Some(target), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
                target.extend(extra.clone());
            }
            params
        };
        if blocks::by_id(id).is_some() {
            let live = view.states.iter().find(|s| s.block == id);
            if ui.button("Edit").clicked() {
                actions.push(("block.select", json!({ "block": id })));
                ui.close();
            }
            let engaged = live.is_none_or(|s| s.engaged);
            if ui.button(if engaged { "Bypass" } else { "Engage" }).clicked() {
                actions.push((if engaged { "block.bypass" } else { "block.engage" }, json!({ "block": id })));
                ui.close();
            }
            let on_x = live.is_none_or(|s| s.xy == XyState::X);
            if ui.button(if on_x { "Switch to Y" } else { "Switch to X" }).clicked() {
                actions.push(("block.set_xy", json!({ "block": id, "state": if on_x { "Y" } else { "X" } })));
                ui.close();
            }
            ui.separator();
            if ui.button("Copy").on_hover_text("Copy the settings of the X/Y set it is on").clicked() {
                actions.push(("block.copy", json!({ "block": id })));
                ui.close();
            }
            let both = ui.button("Copy X and Y");
            if both.on_hover_text("Switches the block to its other set and back, which can be heard").clicked() {
                actions.push(("block.copy", json!({ "block": id, "sets": "both" })));
                ui.close();
            }
            let clip = &self.view.block_clip;
            let copied = clip["block"].as_u64().and_then(|from| blocks::by_id(from as u16));
            let fits = copied.zip(blocks::by_id(id)).is_some_and(|(from, to)| from.effect_type == to.effect_type);
            let paste = clip["name"].as_str().map_or_else(|| "Paste".to_owned(), |name| format!("Paste {name}"));
            let paste = ui.add_enabled(fits, egui::Button::new(paste));
            if paste.on_disabled_hover_text("Copy a block of this kind first").clicked() {
                actions.push(("block.paste", json!({ "block": id })));
                ui.close();
            }
            for (name, command) in [("Copy X to Y", "block.copy_x_to_y"), ("Copy Y to X", "block.copy_y_to_x")] {
                if ui.button(name).clicked() {
                    actions.push((command, json!({ "block": id })));
                    ui.close();
                }
            }
            ui.separator();
        }
        if id == blocks::EMPTY {
            let placed = view.grid.blocks();
            for category in Category::ALL {
                ui.menu_button(category.title(), |ui| {
                    let free = blocks::all().iter().filter(|b| Category::of(b.effect_type) == category);
                    for block in free.filter(|b| !placed.contains(&b.id) && !blocks::is_fixed(b.id)) {
                        if ui.button(&block.name).clicked() {
                            actions.push(("grid.place_block", with_cell(json!({ "block": block.id }))));
                            ui.close();
                        }
                    }
                });
            }
            if ui.button("Shunt").on_hover_text("A plain pass-through").clicked() {
                actions.push(("grid.place_shunt", cell.clone()));
                ui.close();
            }
            return;
        }
        if column > 0 {
            for from in (0..grid::ROWS).filter(|&from| view.grid.cell(column - 1, from).block != blocks::EMPTY) {
                let fed = view.grid.is_fed_by(column, row, from);
                let source = blocks::label(view.grid.cell(column - 1, from).block);
                let mark = if fed { "✔ " } else { "    " };
                if ui.button(format!("{mark}Cable from row {} ({source})", from + 1)).clicked() {
                    let command = if fed { "grid.disconnect" } else { "grid.connect" };
                    actions.push((command, with_cell(json!({ "from_row": from }))));
                    ui.close();
                }
            }
            ui.separator();
        }
        if ui.button(RichText::new("Remove").color(theme::DANGER)).clicked() {
            actions.push(("grid.clear_cell", cell.clone()));
            ui.close();
        }
    }

    /// One of the bars at the ends of the grid. A click opens its block, or,
    /// while a cable is being drawn from the matching side of a block, runs
    /// the cable to it. Returns whether it took the cable.
    fn rail(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        rail: Rail,
        scale: f32,
        pending: Option<CableEnd>,
        actions: &mut Vec<Action>,
    ) -> bool {
        let (name, block, command) = match rail {
            Rail::Input => ("INPUT", blocks::INPUT_GATE, "grid.connect_to_input"),
            Rail::Output => ("OUTPUT", blocks::OUTPUT, "grid.connect_to_output"),
        };
        // An input end goes to the input bar, an output end to the output bar.
        let takes = pending.filter(|end| end.2 == (rail == Rail::Output));
        let response = ui.interact(rect, ui.id().with(("rail", block)), Sense::click());
        let selected = self.view.state.selected_block == Some(block);
        let fill = if selected || response.hovered() { theme::RAISED_HOT } else { theme::RAISED };
        let painter = ui.painter();
        painter.rect_filled(rect, theme::RADIUS, fill);
        if takes.is_some() || selected {
            painter.rect_stroke(rect, theme::RADIUS, Stroke::new(2.0_f32, theme::ACCENT), StrokeKind::Outside);
        }
        // One letter per line, so the name reads down the bar.
        let font = FontId::proportional(10.0 * scale);
        let step = 12.0 * scale;
        let top = rect.center().y - step * (name.len() as f32 - 1.0) / 2.0;
        let ink = if takes.is_some() { theme::ACCENT } else { theme::TEXT_DIM };
        for (index, letter) in name.chars().enumerate() {
            let at = pos2(rect.center().x, top + step * index as f32);
            centred_text(painter, at, letter.encode_utf8(&mut [0; 4]), font.clone(), ink);
        }
        let hint = match takes {
            Some(_) => format!("Click to run the cable to the {}", name.to_lowercase()),
            None => format!("{}: click to edit", blocks::label(block)),
        };
        if !response.on_hover_text(hint).clicked() {
            return false;
        }
        match takes {
            Some((column, row, _)) => actions.push((command, json!({ "column": column, "row": row }))),
            None => actions.push(("block.select", json!({ "block": block }))),
        }
        takes.is_some()
    }

    /// Draws the grid and returns what is still in progress on it.
    fn grid(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) -> GridGesture {
        let state = &self.view.state;
        let view = &state.preset;
        let natural = vec2(
            2.0 * (RAIL + TILE_GAP.x) + grid::COLUMNS as f32 * (TILE.x + TILE_GAP.x) - TILE_GAP.x,
            grid::ROWS as f32 * (TILE.y + TILE_GAP.y) - TILE_GAP.y,
        );
        let available = ui.available_size();
        let scale = grid_scale(available, natural);
        let (outer, backdrop) = ui.allocate_exact_size(available.max(natural * scale), Sense::click_and_drag());
        let area = Rect::from_center_size(outer.center(), natural * scale);
        let (tile_size, pitch) = (TILE * scale, (TILE + TILE_GAP) * scale);
        let lead = (RAIL + TILE_GAP.x) * scale;
        let tile = |column: usize, row: usize| {
            Rect::from_min_size(area.min + vec2(lead + column as f32 * pitch.x, row as f32 * pitch.y), tile_size)
        };
        let handle = |end: CableEnd| {
            let rect = tile(end.0, end.1);
            if end.2 {
                rect.right_center()
            } else {
                rect.left_center()
            }
        };
        let painter = ui.painter_at(outer);
        let cable = Stroke::new(2.0 * scale, theme::CABLE);
        let GridGesture { cable: mut pending, mut dragging, mut selection, mut band, mut moving } = self.gesture;
        let occupied = |(column, row): selection::Cell| view.grid.cell(column, row).block != blocks::EMPTY;
        // A block that has gone, with another preset or an undo, is not selected any more.
        selection = selection.pruned(occupied);
        let escaped = ui.input(|i| i.key_pressed(Key::Escape));
        let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        if backdrop.clicked() || backdrop.secondary_clicked() || escaped {
            pending = None;
        }
        if backdrop.clicked() || escaped {
            // Escape also lets go of blocks that are being dragged.
            (selection, dragging, band) = (Selection::default(), None, None);
        }
        if backdrop.drag_started() {
            band = ui.input(|i| i.pointer.press_origin());
        }

        let rail_size = vec2(RAIL * scale, area.height());
        let rails = [
            Rect::from_min_size(area.min, rail_size),
            Rect::from_min_size(pos2(area.right() - rail_size.x, area.top()), rail_size),
        ];
        for (rect, rail) in [(rails[0], Rail::Input), (rails[1], Rail::Output)] {
            if self.rail(ui, rect, rail, scale, pending, actions) {
                pending = None;
            }
        }

        // Cables first, so tiles sit on top of them.
        for row in 0..grid::ROWS {
            if view.grid.cell(0, row).block != blocks::EMPTY {
                let end = tile(0, row).left_center();
                painter.line_segment([pos2(rails[0].right(), end.y), end], cable);
            }
            let last = grid::COLUMNS - 1;
            if view.grid.cell(last, row).block != blocks::EMPTY {
                let start = tile(last, row).right_center();
                painter.line_segment([start, pos2(rails[1].left(), start.y)], cable);
            }
        }
        let curve = |a: Pos2, b: Pos2, stroke: Stroke| {
            let bend = vec2((b.x - a.x) / 2.0, 0.0);
            CubicBezierShape::from_points_stroke([a, a + bend, b - bend, b], false, Color32::TRANSPARENT, stroke)
        };
        for column in 1..grid::COLUMNS {
            for row in 0..grid::ROWS {
                for from in (0..grid::ROWS).filter(|&from| view.grid.is_fed_by(column, row, from)) {
                    painter.add(curve(tile(column - 1, from).right_center(), tile(column, row).left_center(), cable));
                }
            }
        }

        let pointer = ui.input(|i| i.pointer.hover_pos());
        // The cell a point is nearest to, counted on beyond the grid's edges:
        // each cell owns its tile and half the gap around it.
        let cell_near = |at: Pos2| {
            let from = at - area.min - vec2(lead, 0.0) + TILE_GAP * scale / 2.0;
            ((from.x / pitch.x).floor() as isize, (from.y / pitch.y).floor() as isize)
        };
        for column in 0..grid::COLUMNS {
            for row in 0..grid::ROWS {
                let rect = tile(column, row);
                let id = view.grid.cell(column, row).block;
                // An empty cell is dragged from to pull a rubber band.
                let response = ui.interact(rect, ui.id().with(("cell", column, row)), Sense::click_and_drag());
                let block = blocks::by_id(id);
                let picked = selection.contains((column, row));
                let lifted = dragging.is_some() && picked;
                if id == blocks::EMPTY {
                    let edge = if response.hovered() { theme::TEXT_DIM } else { theme::RAISED };
                    painter.rect_stroke(rect, theme::RADIUS, Stroke::new(1.0_f32, edge), StrokeKind::Inside);
                } else if let Some(block) = block {
                    let live = view.states.iter().find(|s| s.block == id);
                    let engaged = live.is_none_or(|s| s.engaged) && !lifted;
                    let color = Category::of(block.effect_type).color();
                    painter.rect_filled(rect, theme::RADIUS, if engaged { color } else { theme::muted(color) });
                    let ink = if engaged { theme::text_on(color) } else { theme::TEXT_DIM };
                    // Shrink the lettering until the longest word fits, so that
                    // a name only ever breaks between words.
                    let room = tile_size.x - 8.0 * scale;
                    let longest = block.name.split(' ').max_by_key(|word| word.len()).unwrap_or_default();
                    let fits = |size: &f32| {
                        let font = FontId::proportional(size * scale);
                        ui.fonts_mut(|fonts| fonts.layout_no_wrap(longest.to_owned(), font, ink)).size().x <= room
                    };
                    let size = [11.0_f32, 10.0, 9.0].into_iter().find(fits).unwrap_or(8.5);
                    let font = FontId::proportional(size * scale);
                    let mut job = egui::text::LayoutJob::simple(block.name.clone(), font, ink, room);
                    job.halign = Align::Center;
                    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    let top = rect.center().y - galley.size().y / 2.0;
                    painter.galley(pos2(rect.center().x, top), galley, ink);
                    if live.is_some_and(|s| s.xy == XyState::Y) {
                        let corner = rect.right_top() + vec2(-8.0, 8.0) * scale;
                        painter.text(
                            corner,
                            Align2::CENTER_CENTER,
                            "Y",
                            FontId::proportional(9.0 * scale),
                            if engaged { ink } else { theme::ACCENT },
                        );
                    }
                    if state.selected_block == Some(id) {
                        painter.rect_stroke(
                            rect,
                            theme::RADIUS,
                            Stroke::new(2.0_f32, theme::ACCENT),
                            StrokeKind::Outside,
                        );
                    } else if response.hovered() {
                        painter.rect_stroke(
                            rect,
                            theme::RADIUS,
                            Stroke::new(1.0_f32, theme::TEXT),
                            StrokeKind::Outside,
                        );
                    }
                } else {
                    // A shunt, or a block this build does not know: a pass-through bar.
                    let bar = Rect::from_center_size(rect.center(), vec2(tile_size.x, 8.0 * scale));
                    let hot = response.hovered() || lifted;
                    painter.rect_filled(bar, 4.0_f32, if hot { theme::TEXT_DIM } else { theme::EDGE });
                }
                if picked {
                    // Selected to be moved: a shunt gets a frame around its bar.
                    let frame = if block.is_some() { rect } else { rect.shrink2(vec2(0.0, tile_size.y * 0.22)) };
                    painter.rect_filled(frame, theme::RADIUS, theme::ACCENT.gamma_multiply(0.16));
                    painter.rect_stroke(frame, theme::RADIUS, Stroke::new(2.0_f32, theme::ACCENT), StrokeKind::Outside);
                }
                if response.clicked() && pending.is_none() {
                    let cell = (column, row);
                    if id == blocks::EMPTY {
                        selection = Selection::default();
                    } else if ctrl {
                        selection.toggle(cell);
                    } else if shift {
                        selection.extend_to(cell, occupied);
                    } else {
                        selection = Selection::only(cell);
                    }
                    // A plain click opens the block's editor. While more are
                    // being picked, an editor that is open follows the block
                    // picked last, but none opens: it would shift the grid.
                    let follows =
                        selection.contains(cell) && state.selected_block.is_some() && state.selected_block != Some(id);
                    if block.is_some() && (!(ctrl || shift) || follows) {
                        actions.push(("block.select", json!({ "block": id })));
                    }
                }
                if response.drag_started() {
                    pending = None;
                    if id == blocks::EMPTY {
                        band = ui.input(|i| i.pointer.press_origin());
                    } else {
                        // What is dragged is the selection; a block outside it becomes it.
                        if !picked && ctrl {
                            selection.toggle((column, row));
                        } else if !picked {
                            selection = Selection::only((column, row));
                        }
                        dragging = Some((column, row));
                    }
                }
                if response.drag_stopped() && dragging == Some((column, row)) {
                    let (dx, dy) = pointer.map(|p| selection::offset((column, row), cell_near(p))).unwrap_or_default();
                    if !moving && selection.can_move(dx, dy, occupied) {
                        if let Some(action) = selection.move_action(dx, dy) {
                            actions.push(action);
                            moving = true;
                        }
                    }
                    dragging = None;
                }
                let hint = match id {
                    blocks::EMPTY => "Right-click to insert, drag to select".to_owned(),
                    id => format!(
                        "{}: click to edit, drag to move.\nCtrl+click or Shift+click selects more; \
                         Alt+arrow keys move what is selected",
                        blocks::label(id)
                    ),
                };
                if dragging.is_none() {
                    response.on_hover_text(hint).context_menu(|ui| self.cell_menu(ui, column, row, actions));
                }
                if id == blocks::EMPTY || dragging.is_some() {
                    continue;
                }

                // Cable handles: I on the left, O on the right. They show on
                // the tile under the pointer, and on every tile that could
                // take the other end while a cable is being drawn.
                let near = pointer.is_some_and(|p| rect.expand(HANDLE * scale * 1.5).contains(p));
                for (output, letter) in [(false, "I"), (true, "O")] {
                    let end: CableEnd = (column, row, output);
                    let completes = cable_pair(pending, end).is_some();
                    if !((near && pending.is_none()) || completes || pending == Some(end)) {
                        continue;
                    }
                    let centre = handle(end);
                    let spot = Rect::from_center_size(centre, Vec2::splat(HANDLE * scale * 2.4));
                    let touch = ui.interact(spot, ui.id().with(("handle", column, row, output)), Sense::click());
                    let lit = pending == Some(end) || touch.hovered();
                    painter.circle_filled(centre, HANDLE * scale, if lit { theme::ACCENT } else { theme::PANEL });
                    painter.circle_stroke(centre, HANDLE * scale, Stroke::new(1.5_f32, theme::ACCENT));
                    let ink = if lit { theme::text_on(theme::ACCENT) } else { theme::ACCENT };
                    centred_text(&painter, centre, letter, FontId::proportional(10.0 * scale), ink);
                    let touch = touch.on_hover_text(if completes {
                        "Click to add or remove this cable"
                    } else if output {
                        "Output: click, then click the I of a block in the next column, or the OUTPUT bar"
                    } else {
                        "Input: click, then click the O of a block in the previous column, or the INPUT bar"
                    });
                    if touch.clicked() {
                        match cable_pair(pending, end) {
                            Some((to_column, to_row, from_row)) => {
                                let fed = view.grid.is_fed_by(to_column, to_row, from_row);
                                let command = if fed { "grid.disconnect" } else { "grid.connect" };
                                actions.push((
                                    command,
                                    json!({ "column": to_column, "row": to_row, "from_row": from_row }),
                                ));
                                pending = None;
                            }
                            None => pending = if pending == Some(end) { None } else { Some(end) },
                        }
                    }
                }
            }
        }
        if let (Some(end), Some(pointer)) = (pending, pointer) {
            let (a, b) = if end.2 { (handle(end), pointer) } else { (pointer, handle(end)) };
            painter.add(curve(a, b, Stroke::new(2.0 * scale, theme::ACCENT)));
        }
        // The blocks being moved show where they would land: in the accent
        // colour where all of them fit, in red where one of them does not.
        let shift_by = dragging.zip(pointer).map(|(from, pointer)| selection::offset(from, cell_near(pointer)));
        if let Some((dx, dy)) = shift_by.filter(|&by| by != (0, 0)) {
            let edge = if selection.can_move(dx, dy, occupied) { theme::ACCENT } else { theme::DANGER };
            let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.id().with("ghost")));
            for (column, row) in selection.cells() {
                let Some((to_column, to_row)) = selection::shifted((column, row), dx, dy) else { continue };
                let ghost = tile(to_column, to_row);
                let name = blocks::label(view.grid.cell(column, row).block);
                top.rect_filled(ghost, theme::RADIUS, theme::RAISED_HOT.gamma_multiply(0.92));
                top.rect_stroke(ghost, theme::RADIUS, Stroke::new(2.0_f32, edge), StrokeKind::Outside);
                top.text(ghost.center(), Align2::CENTER_CENTER, name, FontId::proportional(10.0 * scale), theme::TEXT);
            }
        }
        // A rubber band selects what it touches.
        if let (Some(start), Some(pointer)) = (band, pointer) {
            let pulled = Rect::from_two_pos(start, pointer);
            let cells = (0..grid::COLUMNS).flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)));
            selection = Selection::of(cells.filter(|&cell| occupied(cell) && tile(cell.0, cell.1).intersects(pulled)));
            painter.rect_filled(pulled, 2.0_f32, theme::ACCENT.gamma_multiply(0.10));
            painter.rect_stroke(pulled, 2.0_f32, Stroke::new(1.0_f32, theme::ACCENT), StrokeKind::Inside);
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            (dragging, band) = (None, None);
        }
        // Alt and an arrow key move the selection one cell.
        let idle = dragging.is_none() && !moving && !ui.ctx().egui_wants_keyboard_input();
        if idle && !selection.is_empty() {
            let arrows =
                [(Key::ArrowLeft, -1, 0), (Key::ArrowRight, 1, 0), (Key::ArrowUp, 0, -1), (Key::ArrowDown, 0, 1)];
            for (key, dx, dy) in arrows {
                let pressed = ui.input_mut(|i| i.consume_key(Modifiers::ALT, key));
                if pressed && !moving && selection.can_move(dx, dy, occupied) {
                    if let Some(action) = selection.move_action(dx, dy) {
                        actions.push(action);
                        moving = true;
                    }
                }
            }
        }
        GridGesture { cable: pending, dragging, selection, band, moving }
    }

    /// One control cell of the block editor.
    fn control(
        ui: &mut egui::Ui,
        block: u16,
        cell: Vec2,
        control: &ControlView,
        picker: &mut Option<picker::Picker>,
        actions: &mut Vec<Action>,
    ) {
        let set = |value: u32| ("param.set", json!({ "block": block, "param": control.id, "value": value }));
        // A knob or a fader is either moved to a raw value or given a typed one.
        let modifier = || ("modifier.open", json!({ "block": block, "param": control.id }));
        let changed = |change: knob::Change| match change {
            knob::Change::Raw(value) => Some(set(value)),
            knob::Change::Typed(value) => {
                Some(("param.set_displayed", json!({ "block": block, "param": control.id, "value": value })))
            }
            knob::Change::More => control.modifier.then(modifier),
        };
        // A switch or a list that takes a modifier opens it the same way,
        // without the right click being taken from the control itself.
        let over = ui.rect_contains_pointer(Rect::from_min_size(ui.cursor().min, cell));
        let stepped = matches!(control.kind.as_str(), "toggle" | "choice");
        if control.modifier && stepped && over && ui.input(|input| input.pointer.secondary_clicked()) {
            actions.push(modifier());
        }
        match control.kind.as_str() {
            "spacer" => {
                ui.allocate_exact_size(cell, Sense::hover());
            }
            "knob" => {
                let change = knob::knob(ui, cell, &control.label, &control.text, control.raw, control.max);
                actions.extend(change.and_then(changed));
            }
            "slider" => {
                let size = vec2(cell.x * FADER_WIDTH, cell.y * FADER_ROWS);
                let change = knob::fader(ui, size, &control.label, &control.text, control.raw, control.max);
                actions.extend(change.and_then(changed));
            }
            "toggle" => {
                if let Some(on) = knob::switch(ui, cell, &control.label, control.raw != 0) {
                    actions.push(set(u32::from(on)));
                }
            }
            kind => {
                let (rect, response) = ui.allocate_exact_size(cell, Sense::hover());
                knob::label(ui, rect, &control.label, response.contains_pointer());
                let body = Rect::from_min_max(
                    pos2(rect.left() + 3.0, rect.top() + knob::LABEL_BAND + 8.0),
                    rect.max - vec2(3.0, 4.0),
                );
                let mut inner =
                    ui.new_child(egui::UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Center)));
                let ui = &mut inner;
                match (kind, &control.choices) {
                    ("readout", _) => {
                        ui.label(RichText::new(&control.text).monospace().size(11.0));
                    }
                    ("refresh", _) => {
                        if ui.button("Refresh").on_hover_text("Read the block again from the device").clicked() {
                            actions.push(("block.refresh", Value::Null));
                        }
                    }
                    ("button", _) => {
                        if ui.button("Press").on_hover_text(&control.label).clicked() {
                            for param in &control.sets {
                                actions.push(("param.set", json!({ "block": block, "param": param, "value": 1 })));
                            }
                        }
                    }
                    ("controller", _) => {
                        let on = control.raw != 0;
                        if lit_button(ui, if on { "ON" } else { "OFF" }, on, theme::LAMP_ON).clicked() {
                            actions.push(("looper.press", json!({ "button": control.name })));
                        }
                    }
                    // A long list, or the unit's cabs, is browsed in a window of its own.
                    (_, choices) if picker::wanted(choices.as_ref().map(Vec::len), control.cabs) => {
                        let listed = choices.as_ref().and_then(|choices| choices.get(control.raw as usize));
                        let current = listed.map_or(control.text.as_str(), String::as_str);
                        let name = RichText::new(shorten(current, body.width() - 10.0)).size(11.0);
                        let hint = format!("{current}\nClick to browse the list");
                        if picker::button(ui, name, body.width()).on_hover_text(hint).clicked() {
                            *picker = Some(picker::Picker::new(block, control.id, &control.label));
                        }
                    }
                    (_, Some(choices)) => {
                        let current = choices.get(control.raw as usize).map_or(control.text.as_str(), String::as_str);
                        egui::ComboBox::from_id_salt(("choice", control.id))
                            .selected_text(RichText::new(shorten(current, body.width())).size(11.0))
                            .width(body.width())
                            .show_ui(ui, |ui| {
                                for (index, choice) in choices.iter().enumerate() {
                                    if ui.selectable_label(index == control.raw as usize, choice).clicked() {
                                        actions.push(set(index as u32));
                                    }
                                }
                            })
                            .response
                            .on_hover_text(current);
                    }
                    (_, None) => {
                        ui.label(RichText::new(shorten(&control.text, body.width())).monospace().size(11.0))
                            .on_hover_text(&control.text);
                        ui.horizontal(|ui| {
                            ui.add_space((body.width() - 52.0).max(0.0) / 2.0);
                            if ui.add_enabled(control.raw > 0, egui::Button::new("◀").small()).clicked() {
                                actions.push(set(control.raw - 1));
                            }
                            if ui.add_enabled(control.raw < control.max, egui::Button::new("▶").small()).clicked() {
                                actions.push(set(control.raw + 1));
                            }
                        });
                    }
                }
                // The wheel steps through a list, as it turns a knob.
                let steps_through = matches!(kind, "choice" | "knob") && control.max > 0;
                let turned = if response.contains_pointer() { knob::wheel_turned(ui) } else { 0.0 };
                if steps_through && turned != 0.0 {
                    ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
                    let limit = control.choices.as_ref().map_or(control.max, |c| c.len().saturating_sub(1) as u32);
                    let next = knob::wheeled_value(control.raw, limit, if turned > 0.0 { 1 } else { -1 }, false);
                    if next != control.raw {
                        actions.push(set(next));
                    }
                }
            }
        }
    }

    /// Draws the curve of a page, whichever kind it is.
    fn graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        match graph["kind"].as_str() {
            Some("dynamics") => Self::level_graph(ui, size, graph),
            Some("envelope") => Self::envelope_graph(ui, size, graph),
            Some("modifier") => Self::modifier_graph(ui, size, graph),
            _ => Self::eq_graph(ui, size, graph),
        }
    }

    /// The pairs of numbers under `key` of a graph.
    fn pairs(graph: &Value, key: &str) -> Vec<(f32, f32)> {
        let list = graph[key].as_array().map(Vec::as_slice).unwrap_or_default();
        list.iter().filter_map(|p| Some((p[0].as_f64()? as f32, p[1].as_f64()? as f32))).collect()
    }

    /// Draws an equaliser curve: gain against frequency on a logarithmic axis.
    fn eq_graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let plot = rect.shrink2(vec2(8.0, 10.0));
        let (points, also, bands) =
            (Self::pairs(graph, "points"), Self::pairs(graph, "also"), Self::pairs(graph, "bands"));
        // Scaled to the boosts and cuts dialled in; a filter's steep slope may run off the edge.
        let loudest = bands.iter().map(|p| p.1.abs()).fold(0.0_f32, f32::max);
        let span = ((loudest / GRAPH_STEP_DB).ceil() * GRAPH_STEP_DB).clamp(2.0 * GRAPH_STEP_DB, 4.0 * GRAPH_STEP_DB);
        let (low, high) = GRAPH_HZ;
        let x = |hz: f32| plot.left() + plot.width() * (hz.clamp(low, high) / low).ln() / (high / low).ln();
        let y = |db: f32| plot.center().y - plot.height() / 2.0 * (db / span).clamp(-1.0, 1.0);

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, theme::RADIUS, theme::BACKDROP);
        let faint = Stroke::new(1.0_f32, theme::RAISED);
        let small = FontId::proportional(9.0);
        for (hz, name) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
            painter.line_segment([pos2(x(hz), plot.top()), pos2(x(hz), plot.bottom())], faint);
            painter.text(pos2(x(hz) + 3.0, plot.bottom()), Align2::LEFT_BOTTOM, name, small.clone(), theme::TEXT_DIM);
        }
        for db in [-span, -span / 2.0, span / 2.0, span] {
            painter.line_segment([pos2(plot.left(), y(db)), pos2(plot.right(), y(db))], faint);
            painter.text(
                pos2(plot.left(), y(db)),
                Align2::LEFT_TOP,
                format!("{db:+.0}"),
                small.clone(),
                theme::TEXT_DIM,
            );
        }
        painter
            .line_segment([pos2(plot.left(), y(0.0)), pos2(plot.right(), y(0.0))], Stroke::new(1.0_f32, theme::EDGE));
        // A second curve, where a block has two, sits behind the main one.
        for (curve, ink) in [(also, theme::CABLE), (points, theme::ACCENT)] {
            let line: Vec<Pos2> = curve.iter().map(|&(hz, db)| pos2(x(hz), y(db))).collect();
            painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, ink)));
        }
        for (index, &(hz, db)) in bands.iter().enumerate() {
            let at = pos2(x(hz), y(db));
            painter.circle_filled(at, 7.0, theme::PANEL);
            painter.circle_stroke(at, 7.0, Stroke::new(1.5_f32, theme::ACCENT));
            painter.text(at, Align2::CENTER_CENTER, (index + 1).to_string(), small.clone(), theme::TEXT);
        }
    }

    /// Draws what a compressor or a gate does: level out against level in.
    fn level_graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        // Square, so that "unchanged" is the diagonal.
        let side = (rect.width() - 16.0).min(rect.height() - 20.0).max(10.0);
        let plot = Rect::from_center_size(rect.center(), Vec2::splat(side));
        let floor = (graph["floor"].as_f64().unwrap_or(-80.0) as f32).min(-1.0);
        let along = |db: f32| 1.0 - db.clamp(floor, 0.0) / floor;
        let x = |db: f32| plot.left() + plot.width() * along(db);
        let y = |db: f32| plot.bottom() - plot.height() * along(db);

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, theme::RADIUS, theme::BACKDROP);
        let faint = Stroke::new(1.0_f32, theme::RAISED);
        let small = FontId::proportional(9.0);
        let mut db = floor;
        while db <= 0.0 {
            painter.line_segment([pos2(x(db), plot.top()), pos2(x(db), plot.bottom())], faint);
            painter.line_segment([pos2(plot.left(), y(db)), pos2(plot.right(), y(db))], faint);
            let label = format!("{db:.0}");
            painter.text(pos2(x(db) + 2.0, plot.bottom()), Align2::LEFT_BOTTOM, label, small.clone(), theme::TEXT_DIM);
            db += LEVEL_STEP_DB;
        }
        painter.line_segment([pos2(x(floor), y(floor)), pos2(x(0.0), y(0.0))], Stroke::new(1.0_f32, theme::EDGE));
        let threshold = graph["threshold"].as_f64().map(|t| t as f32).filter(|t| (floor..=0.0).contains(t));
        if let Some(threshold) = threshold {
            let mark = Stroke::new(1.0_f32, theme::TEXT_DIM);
            painter.line_segment([pos2(x(threshold), plot.top()), pos2(x(threshold), plot.bottom())], mark);
        }
        let line: Vec<Pos2> =
            Self::pairs(graph, "points").iter().map(|&(input, output)| pos2(x(input), y(output))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, theme::ACCENT)));
        painter.text(plot.left_top() + vec2(3.0, 2.0), Align2::LEFT_TOP, "out / in dB", small, theme::TEXT_DIM);
    }

    /// Draws an envelope: level against time.
    fn envelope_graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let plot = rect.shrink2(vec2(10.0, 14.0));
        let length = (graph["length_ms"].as_f64().unwrap_or(1.0) as f32).max(1.0);
        let at = |(ms, level): (f32, f32)| {
            let across = plot.width() * (ms / length).clamp(0.0, 1.0);
            pos2(plot.left() + across, plot.bottom() - plot.height() * level.clamp(0.0, 1.0))
        };
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, theme::RADIUS, theme::BACKDROP);
        painter.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0_f32, theme::EDGE));
        let corners: Vec<Pos2> = Self::pairs(graph, "points").into_iter().map(at).collect();
        painter.add(egui::Shape::line(corners.clone(), Stroke::new(2.0_f32, theme::ACCENT)));
        for corner in corners.iter().skip(1) {
            painter.circle_filled(*corner, 3.0, theme::ACCENT);
        }
        let small = FontId::proportional(9.0);
        let label = format!("{length:.0} ms");
        painter.text(plot.right_bottom() + vec2(0.0, 2.0), Align2::RIGHT_TOP, label, small, theme::TEXT_DIM);
    }

    /// Draws what a modifier makes of its controller: the parameter, from
    /// its lowest to its highest value, against the controller's travel.
    fn modifier_graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let plot = rect.shrink2(vec2(12.0, 18.0));
        let at = |(x, y): (f32, f32)| {
            pos2(plot.left() + plot.width() * x.clamp(0.0, 1.0), plot.bottom() - plot.height() * y.clamp(0.0, 1.0))
        };
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, theme::RADIUS, theme::BACKDROP);
        let faint = Stroke::new(1.0_f32, theme::RAISED);
        for step in [0.0_f32, 0.5, 1.0] {
            painter.line_segment([at((step, 0.0)), at((step, 1.0))], faint);
            painter.line_segment([at((0.0, step)), at((1.0, step))], faint);
        }
        let line: Vec<Pos2> = Self::pairs(graph, "points").into_iter().map(at).collect();
        painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, theme::ACCENT)));
        let small = FontId::proportional(9.0);
        let text = |key: &str| graph[key].as_str().unwrap_or_default().to_owned();
        painter.text(rect.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, text("max"), small.clone(), theme::TEXT_DIM);
        let low = rect.left_bottom() + vec2(4.0, -3.0);
        painter.text(low, Align2::LEFT_BOTTOM, text("min"), small.clone(), theme::TEXT_DIM);
        let travel = rect.right_bottom() + vec2(-4.0, -3.0);
        painter.text(travel, Align2::RIGHT_BOTTOM, "controller →", small, theme::TEXT_DIM);
    }

    /// How far the loop has played, as a bar above the looper's controls.
    fn looper_bar(ui: &mut egui::Ui, looper: &Value) {
        if !looper.is_object() {
            return;
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP - 8.0), Sense::hover());
        let track = Rect::from_center_size(bar.center(), vec2(bar.width(), 8.0));
        ui.painter().rect_filled(track, 4.0_f32, theme::EDGE);
        if let Some(position) = looper["position"].as_u64() {
            let filled = track.width() * (position.min(99) as f32 + 1.0) / 100.0;
            let played = Rect::from_min_size(track.min, vec2(filled, track.height()));
            ui.painter().rect_filled(played, 4.0_f32, theme::LAMP_ON);
        }
        ui.add_space(8.0);
    }

    /// The three steps of a tone match, above the block's controls.
    fn tone_match_steps(ui: &mut egui::Ui, tone: &Value, actions: &mut Vec<Action>) {
        if !tone.is_object() {
            return;
        }
        ui.allocate_ui(vec2(ui.available_width(), STRIP - 8.0), |ui| {
            ui.horizontal(|ui| {
                let steps = [
                    ("1  Capture the reference", "capturing_reference", "reference"),
                    ("2  Capture your own sound", "capturing_local", "local"),
                ];
                for (title, key, signal) in steps {
                    let on = tone[key] == true;
                    ui.label(RichText::new(title).small().color(theme::TEXT_DIM));
                    let hint =
                        if on { "Click to stop" } else { "Starts averaging what the block hears; play while it runs" };
                    let button = lit_button(ui, if on { "Capturing…" } else { "Start" }, on, theme::LAMP_ON);
                    if button.on_hover_text(hint).clicked() {
                        actions.push(("tonematch.capture", json!({ "signal": signal, "on": !on })));
                    }
                    ui.separator();
                }
                ui.label(RichText::new("3").small().color(theme::TEXT_DIM));
                if ui.button("Match").on_hover_text("Stop both captures and work out the correction").clicked() {
                    actions.push(("tonematch.match", Value::Null));
                }
            });
        });
        ui.add_space(8.0);
    }

    /// The window of the modifier that is open.
    fn modifier_window(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let view = self.view.clone();
        let modifier = &view.modifier;
        if !modifier.is_object() {
            return;
        }
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let title = format!("Modifier: {} {}", text(&modifier["block_name"]), text(&modifier["label"]));
        let empty = Vec::new();
        let fields = modifier["fields"].as_array().unwrap_or(&empty);
        let set = |key: &str, value: u64| ("modifier.set", json!({ "field": key, "value": value }));
        let attached = modifier["attached"] == true;
        let mut open = true;
        let window = egui::Window::new(title).id(egui::Id::new("modifier")).collapsible(false).resizable(false);
        window.open(&mut open).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for field in fields.iter().filter(|field| field["kind"] == "step") {
                    let (key, raw) = (text(&field["key"]), field["value"].as_u64().unwrap_or(0));
                    ui.label(RichText::new(text(&field["label"])).small().color(theme::TEXT_DIM));
                    // Only the source can be set before there is one.
                    let usable = attached || key == "source";
                    if ui.add_enabled(usable && raw > 0, egui::Button::new("◀").small()).clicked() {
                        actions.push(set(&key, raw - 1));
                    }
                    ui.label(RichText::new(text(&field["text"])).monospace());
                    let last = field["max"].as_u64().unwrap_or(0);
                    if ui.add_enabled(usable && raw < last, egui::Button::new("▶").small()).clicked() {
                        actions.push(set(&key, raw + 1));
                    }
                    ui.separator();
                }
            });
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    if !attached {
                        ui.disable();
                    }
                    let knobs: Vec<&Value> = fields.iter().filter(|field| field["kind"] == "knob").collect();
                    for row in knobs.chunks(MODIFIER_ROW) {
                        ui.horizontal(|ui| {
                            for field in row {
                                let key = text(&field["key"]);
                                // What was just set, until the unit has answered.
                                let asked = self.unconfirmed.get(&modifier_key(&key)).copied();
                                let raw = asked.or(field["value"].as_u64()).unwrap_or(0) as u32;
                                let max = field["max"].as_u64().unwrap_or(1) as u32;
                                let (label, shown) = (text(&field["label"]), text(&field["text"]));
                                if let Some(knob::Change::Raw(value)) =
                                    knob::knob(ui, knob::CELL, &label, &shown, raw, max)
                                {
                                    actions.push(set(&key, value.into()));
                                }
                            }
                        });
                    }
                });
                if modifier["graph"].is_object() {
                    Self::modifier_graph(ui, Vec2::splat(MODIFIER_GRAPH), &modifier["graph"]);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if modifier["attached"] == true {
                    if ui.button(RichText::new("Remove modifier").color(theme::DANGER)).clicked() {
                        actions.push(("modifier.remove", Value::Null));
                    }
                } else {
                    let hint = "No source yet: step Source to a controller to attach the modifier.";
                    ui.label(RichText::new(hint).small().color(theme::TEXT_DIM));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("The curve is a sketch of the settings").small().color(theme::TEXT_DIM));
                });
            });
        });
        if !open {
            actions.push(("modifier.close", Value::Null));
        }
    }

    /// The box a scene's name is typed in.
    fn scene_rename_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some((scene, mut name)) = self.scene_rename.take() else { return };
        let mut keep = true;
        let window = egui::Window::new(format!("Name scene {}", scene + 1)).collapsible(false).resizable(false);
        window.anchor(Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
            let field = egui::TextEdit::singleline(&mut name).desired_width(240.0).char_limit(24).hint_text("no name");
            let edit = ui.add(field);
            if std::mem::take(&mut self.scene_rename_needs_focus) {
                edit.request_focus();
            }
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.label(
                RichText::new("Kept on this computer; the unit has no scene names.").small().color(theme::TEXT_DIM),
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("OK").clicked() || entered {
                    actions.push(("scene.rename", json!({ "scene": scene + 1, "name": name.trim() })));
                    keep = false;
                }
            });
        });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            keep = false;
        }
        if keep {
            self.scene_rename = Some((scene, name));
        }
    }

    fn block_editor(&mut self, ui: &mut egui::Ui, block: u16, actions: &mut Vec<Action>) {
        if self.page_block != Some(block) {
            (self.page_block, self.page) = (Some(block), 0);
        }
        let view = self.view.clone();
        let live = view.state.preset.states.iter().find(|s| s.block == block);
        let editor = &view.editor;
        let unconfirmed = &self.unconfirmed;
        let empty = Vec::new();
        let pages = editor["pages"].as_array().unwrap_or(&empty);
        let fixed = blocks::is_fixed(block);
        let category = blocks::by_id(block).map_or(Category::Utility, |b| Category::of(b.effect_type));

        ui.horizontal(|ui| {
            let (chip, _) = ui.allocate_exact_size(vec2(6.0, 24.0), Sense::hover());
            ui.painter().rect_filled(chip, 3.0_f32, category.color());
            ui.label(RichText::new(blocks::label(block)).size(17.0).strong());
            // The model selector, where a block has one, sits in the title row.
            let model = editor["model"].as_object().map(|_| ControlView::from_json(&editor["model"]));
            if let Some((param, choices)) = model.as_ref().and_then(|p| Some((p, p.choices.as_ref()?))) {
                let current = choices.get(param.raw as usize).map_or(param.text.as_str(), String::as_str);
                if picker::wanted(Some(choices.len()), param.cabs) {
                    // Too many models for a drop-down: they are browsed in a window.
                    let hint = "Click to browse the models";
                    if picker::button(ui, RichText::new(current), 240.0).on_hover_text(hint).clicked() {
                        self.picker = Some(picker::Picker::new(block, param.id, &param.label));
                    }
                } else {
                    egui::ComboBox::from_id_salt("block-model").selected_text(current).width(240.0).show_ui(ui, |ui| {
                        for (index, choice) in choices.iter().enumerate() {
                            if ui.selectable_label(index == param.raw as usize, choice).clicked() {
                                actions
                                    .push(("param.set", json!({ "block": block, "param": param.id, "value": index })));
                            }
                        }
                    });
                }
            }
            if !fixed {
                ui.separator();
                let on_x = live.is_none_or(|s| s.xy == XyState::X);
                for (name, lit) in [("X", on_x), ("Y", !on_x)] {
                    if lit_button(ui, name, lit, theme::ACCENT).clicked() && !lit {
                        actions.push(("block.set_xy", json!({ "block": block, "state": name })));
                    }
                }
                ui.separator();
                let engaged = live.is_none_or(|s| s.engaged);
                let (text, hint) =
                    if engaged { ("Engaged", "Click to bypass") } else { ("Bypassed", "Click to engage") };
                if lit_button(ui, text, engaged, theme::LAMP_ON).on_hover_text(hint).clicked() {
                    actions.push(("block.toggle_bypass", Value::Null));
                }
            }
            ui.separator();
            for (index, page) in pages.iter().enumerate() {
                let title = page["name"].as_str().unwrap_or_default();
                if ui.selectable_label(self.page == index, title).clicked() {
                    self.page = index;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Close").on_hover_text("Close the block editor").clicked() {
                    actions.push(("block.deselect", Value::Null));
                }
                if ui.button("Refresh").on_hover_text("Read the block again from the device").clicked() {
                    actions.push(("block.refresh", Value::Null));
                }
            });
        });
        ui.separator();
        Self::looper_bar(ui, &editor["looper"]);
        Self::tone_match_steps(ui, &editor["tone_match"], actions);

        self.page = self.page.min(pages.len().saturating_sub(1));
        let Some(page) = pages.get(self.page) else {
            ui.label(RichText::new("This block has no controls on this firmware.").color(theme::TEXT_DIM));
            return;
        };
        let shape = PageShape::of(page);
        let available = ui.available_size();
        let rows = shape.rows();
        // Cells shrink before anything has to scroll.
        let cell = vec2(
            ((available.x - MIXER_GAP) / shape.units()).clamp(knob::MIN_CELL.x, knob::CELL.x),
            (available.y / rows).clamp(knob::MIN_CELL.y, knob::CELL.y),
        );
        let fits = cell.x * shape.units() + MIXER_GAP <= available.x + 1.0 && cell.y * rows <= available.y + 1.0;
        let all_rows = page["rows"].as_array().unwrap_or(&empty);
        let mut draw = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_top(|ui| {
                let mut column = |ui: &mut egui::Ui, mixer: bool, actions: &mut Vec<Action>| {
                    ui.vertical(|ui| {
                        for row in all_rows.iter().filter(|row| (row["mixer"] == true) == mixer) {
                            ui.horizontal(|ui| {
                                for control in row["controls"].as_array().unwrap_or(&empty) {
                                    let mut control = ControlView::from_json(control);
                                    // What was just set, until the unit has answered.
                                    if let Some(asked) = unconfirmed.get(&format!("{block}/{}", control.id)) {
                                        control.raw = *asked as u32;
                                    }
                                    Self::control(ui, block, cell, &control, &mut self.picker, actions);
                                }
                            });
                        }
                    });
                };
                column(ui, false, actions);
                if shape.graph {
                    ui.add_space(cell.x * 0.15);
                    Self::graph(ui, vec2(cell.x * GRAPH_UNITS, cell.y * rows), &page["graph"]);
                }
                if shape.mixer_units > 0.0 {
                    // The output section sits against the right edge.
                    let used = cell.x * shape.units();
                    ui.add_space((available.x - used - MIXER_GAP).max(0.0) + MIXER_GAP / 2.0);
                    let (line, _) = ui.allocate_exact_size(vec2(1.0, cell.y * rows), Sense::hover());
                    ui.painter().rect_filled(line, 0.0_f32, theme::EDGE);
                    ui.add_space(MIXER_GAP / 2.0 - 1.0);
                    column(ui, true, actions);
                }
            });
        };
        if fits {
            draw(ui);
        } else {
            egui::ScrollArea::both().auto_shrink(false).show(ui, draw);
        }
    }

    /// Height at which the current page shows all its controls at full
    /// size, kept to a share of the window so the grid stays usable.
    fn editor_height(&self, ctx: &egui::Context) -> f32 {
        let editor = &self.view.editor;
        let pages = editor["pages"].as_array().map(Vec::as_slice).unwrap_or_default();
        let page = pages.get(self.page.min(pages.len().saturating_sub(1)));
        let rows = page.map_or(1.0, |page| PageShape::of(page).rows());
        let strip = if editor["looper"].is_object() || editor["tone_match"].is_object() { STRIP } else { 0.0 };
        let wanted = EDITOR_CHROME + strip + rows * knob::CELL.y;
        wanted.min(ctx.content_rect().height() * EDITOR_SHARE)
    }

    /// The tuner read-out, shown while the unit's tuner is on.
    fn tuner_window(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let tuner = &self.view.tuner;
        let reading = &tuner["reading"];
        egui::Window::new("Tuner").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 110.0]).show(
            ctx,
            |ui| {
                ui.set_min_width(300.0);
                let offset = reading["offset"].as_i64();
                let in_tune = offset.is_some_and(|o| o.abs() <= IN_TUNE);
                let ink = if in_tune { theme::LAMP_ON } else { theme::TEXT };
                ui.vertical_centered(|ui| {
                    let note = reading["note"].as_str().unwrap_or("–");
                    ui.label(RichText::new(note).size(54.0).strong().color(ink));
                    let string = reading["string"].as_u64();
                    let string = string.map_or_else(|| "play a string".to_owned(), |s| format!("string {s}"));
                    ui.label(RichText::new(string).color(theme::TEXT_DIM));
                });
                let (bar, _) = ui.allocate_exact_size(vec2(300.0, 26.0), Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(Rect::from_center_size(bar.center(), vec2(bar.width(), 4.0)), 2.0_f32, theme::EDGE);
                painter.line_segment([bar.center_top(), bar.center_bottom()], Stroke::new(1.0_f32, theme::TEXT_DIM));
                if let Some(offset) = offset {
                    let x = bar.center().x + (offset as f32 / TUNER_SPAN).clamp(-1.0, 1.0) * bar.width() / 2.0;
                    painter.circle_filled(pos2(x, bar.center().y), 8.0, ink);
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("flat").small().color(theme::TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("sharp").small().color(theme::TEXT_DIM));
                    });
                });
                ui.vertical_centered(|ui| {
                    if ui.button("Close tuner").clicked() {
                        actions.push(("tuner.off", Value::Null));
                    }
                });
            },
        );
    }

    /// Asks before a command from [`CONFIRMED`] goes ahead.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, params)) = self.confirm.clone() else { return };
        let (title, body, go_ahead) = library::confirmation(id, &params, self.view.state.preset.number);
        let mut decision = None;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(
            ctx,
            |ui| {
                let mut lines = body.lines();
                ui.label(lines.next().unwrap_or_default());
                for line in lines {
                    ui.label(RichText::new(line).color(theme::TEXT_DIM));
                }
                ui.label(RichText::new("This cannot be undone.").color(theme::TEXT_DIM));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                    let go = filled_button(go_ahead, theme::DANGER);
                    if ui.add(go).clicked() {
                        decision = Some(true);
                    }
                });
            },
        );
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            decision = Some(false);
        }
        if let Some(go) = decision {
            self.confirm = None;
            if go {
                self.run(id, params);
            }
        }
    }

    /// Draws one frame and runs whatever the user asked for.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        if !std::mem::replace(&mut self.themed, true) {
            theme::apply(ctx);
        }
        self.housekeeping(ctx);
        let mut actions: Vec<Action> = Vec::new();
        if self.confirm.is_none() {
            self.shortcuts(ctx, &mut actions);
        }
        let bar = egui::Frame::new().fill(theme::BACKDROP).inner_margin(egui::Margin::symmetric(10, 4));
        egui::Panel::top("menu").frame(bar).show(ui, |ui| self.menu_bar(ui, &mut actions));
        let header = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 9));
        egui::Panel::top("header").frame(header).show(ui, |ui| self.header(ui, &mut actions));
        egui::Panel::bottom("status").frame(bar).show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.worker.waiting_for().is_some_and(|waited| waited >= SLOW) {
                    ui.spinner();
                    ui.label(RichText::new("Waiting for the unit…").small().color(theme::TEXT));
                }
                ui.label(RichText::new(&self.status).small().color(theme::TEXT_DIM));
            });
        });
        if let Some(block) = self.view.state.selected_block {
            let editor = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 10));
            egui::Panel::bottom("block-editor")
                .frame(editor)
                .resizable(false)
                .exact_size(self.editor_height(ctx))
                .show(ui, |ui| self.block_editor(ui, block, &mut actions));
        }
        let stage = egui::Frame::new().fill(theme::BACKDROP).inner_margin(egui::Margin::same(16));
        egui::CentralPanel::default().frame(stage).show(ui, |ui| {
            if self.view.connected {
                self.gesture = self.grid(ui, &mut actions);
            } else {
                ui.centered_and_justified(|ui| {
                    let hint = "Press Find my Axe-Fx, or choose the MIDI ports your unit is on and press Connect.\n\
                                You can also try the simulator.";
                    let hint = if self.searching { SEARCHING } else { hint };
                    ui.label(RichText::new(hint).size(15.0).color(theme::TEXT_DIM));
                });
            }
        });
        if self.view.state.tuner_on {
            self.tuner_window(ctx, &mut actions);
        }
        if self.view.connected {
            self.modifier_window(ctx, &mut actions);
            self.scene_rename_window(ctx, &mut actions);
            if self.library.presets_open {
                self.preset_manager(ctx, &mut actions);
            }
            if self.library.cabs_open {
                self.cab_manager(ctx, &mut actions);
            }
        }
        // Closes by itself when the block it belongs to is no longer edited.
        self.picker_window(ctx, &mut actions);
        for (id, params) in actions {
            if CONFIRMED.contains(&id) {
                self.confirm = Some((id, params));
            } else if id == "device.connect_auto" {
                self.find_unit();
            } else {
                self.run(id, params);
            }
        }
        self.confirm_dialog(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_shortcut_parses() {
        for command in registry().iter().filter(|c| !c.shortcut.is_empty()) {
            assert!(parse_shortcut(command.shortcut).is_some(), "{} has an unusable shortcut", command.id);
        }
        let save = parse_shortcut("Ctrl+S");
        assert_eq!(save, Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::S)));
        assert_eq!(parse_shortcut("Ctrl+Right").map(|s| s.logical_key), Some(Key::ArrowRight));
    }

    #[test]
    fn every_command_the_shell_names_exists() {
        let source = include_str!("lib.rs");
        for id in CONFIRMED {
            assert!(axegrid_engine::find(id).is_some(), "{id}");
        }
        // Command IDs appear in this file as string literals: two lowercase words joined by a dot.
        for literal in source.split('"').skip(1).step_by(2) {
            let looks_like_id = literal.split_once('.').is_some_and(|(group, verb)| {
                let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_');
                word(group) && word(verb)
            });
            if looks_like_id && literal != "lib.rs" {
                assert!(axegrid_engine::find(literal).is_some(), "the shell names an unknown command {literal:?}");
            }
        }
    }

    #[test]
    fn a_dragged_control_is_keyed_by_what_it_sets() {
        let drive = json!({ "block": 106, "param": 1, "value": 5 });
        assert_eq!(value_key("param.set", &drive).as_deref(), Some("106/1"));
        assert_eq!(value_key("tempo.set", &json!({ "bpm": 120 })).as_deref(), Some(TEMPO_KEY));
        let slope = json!({ "field": "slope", "value": 9 });
        assert_eq!(value_key("modifier.set", &slope), Some(modifier_key("slope")));
        // Anything else is sent as often as it is asked for.
        assert_eq!(value_key("param.set_displayed", &drive), None);
        assert_eq!(value_key("grid.connect", &json!({ "column": 1, "row": 0, "from_row": 0 })), None);
    }

    #[test]
    fn a_name_is_cut_only_when_it_is_wider_than_its_room() {
        assert_eq!(fitted("Plexi Lead", 100.0, 100.0), "Plexi Lead");
        // Ten letters of ten points in room for six: five and an ellipsis.
        assert_eq!(fitted("Plexi Lead", 100.0, 60.0), "Plexi…");
        assert_eq!(fitted("", 0.0, 0.0), "");
        assert_eq!(fitted("Plexi Lead", 100.0, 0.0).chars().count(), 4);
    }

    #[test]
    fn choices_are_cut_to_fit() {
        assert_eq!(shorten("STEREO", 74.0), "STEREO");
        assert_eq!(shorten("PVH 6160 BLOCK", 74.0).chars().count(), 10);
        assert!(shorten("PVH 6160 BLOCK", 74.0).ends_with('…'));
        assert_eq!(shorten("PVH 6160 BLOCK", 200.0), "PVH 6160 BLOCK");
        // However narrow, something readable is left.
        assert_eq!(shorten("PVH 6160 BLOCK", 1.0), "PVH…");
    }

    #[test]
    fn page_shape_counts_rows_and_widths() {
        let knob = json!({ "kind": "knob" });
        let page = json!({ "graph": { "kind": "eq" }, "rows": [
            { "mixer": false, "controls": [knob, knob, knob] },
            { "mixer": false, "controls": [knob, { "kind": "slider" }] },
            { "mixer": true, "controls": [knob, knob] },
        ] });
        let shape = PageShape::of(&page);
        // The row with a fader is as tall as two rows of knobs.
        assert_eq!((shape.param_rows, shape.mixer_rows, shape.graph), (1.0 + FADER_ROWS, 1.0, true));
        assert_eq!((shape.param_units, shape.mixer_units), (3.0, 2.0));
        assert_eq!(shape.rows(), 3.0);
        assert!((shape.units() - (3.0 + GRAPH_UNITS + 0.15 + 2.0)).abs() < 1e-6);
        // An empty page still takes one row and one cell.
        assert_eq!((PageShape::of(&json!({})).rows(), PageShape::of(&json!({})).units()), (1.0, 1.0));
        // A page of faders alone gets their full height, and no more.
        let faders = json!({ "rows": [{ "mixer": false, "controls": [{ "kind": "slider" }, { "kind": "slider" }] }] });
        assert_eq!(PageShape::of(&faders).rows(), FADER_ROWS);
    }

    #[test]
    fn looper_buttons_read_as_lit_or_not() {
        let lit =
            ControlView::from_json(&json!({ "kind": "controller", "label": "Record", "button": "record", "on": true }));
        assert_eq!((lit.name.as_str(), lit.raw), ("record", 1));
        let unknown = ControlView::from_json(&json!({ "kind": "controller", "button": "play", "on": null }));
        assert_eq!(unknown.raw, 0);
        let press = ControlView::from_json(&json!({ "kind": "button", "label": "Match", "sets": [5] }));
        assert_eq!(press.sets, [5]);
    }

    #[test]
    fn control_view_reads_engine_json() {
        let knob = ControlView::from_json(&json!({ "kind": "knob", "param": 1, "name": "DISTORT_DRIVE",
            "label": "Input Drive", "value": 36044, "max": 65534, "text": "5.50", "choices": null }));
        assert_eq!((knob.id, knob.raw, knob.max, knob.label.as_str()), (1, 36044, 65534, "Input Drive"));
        let listed =
            ControlView::from_json(&json!({ "kind": "choice", "param": 0, "value": 1, "choices": ["A", "B"] }));
        assert_eq!(listed.choices.as_deref().map(<[String]>::len), Some(2));
    }

    #[test]
    fn grid_fits_the_space_it_gets() {
        let natural = vec2(1200.0, 220.0);
        assert_eq!(grid_scale(natural, natural), 1.0);
        // An ultrawide window: height is the limit, and growth is capped.
        assert_eq!(grid_scale(vec2(3400.0, 330.0), natural), 1.5);
        assert_eq!(grid_scale(vec2(9000.0, 9000.0), natural), GRID_SCALE.1);
        assert_eq!(grid_scale(vec2(100.0, 100.0), natural), GRID_SCALE.0);
    }

    #[test]
    fn cables_join_an_output_to_the_next_columns_input() {
        // O of (2, 1) then I of (3, 0): cell (3, 0) is fed from row 1.
        assert_eq!(cable_pair(Some((2, 1, true)), (3, 0, false)), Some((3, 0, 1)));
        // The other order gives the same cable.
        assert_eq!(cable_pair(Some((3, 0, false)), (2, 1, true)), Some((3, 0, 1)));
        // Same side, same column, or columns apart: not a cable.
        assert_eq!(cable_pair(Some((2, 1, true)), (3, 0, true)), None);
        assert_eq!(cable_pair(Some((2, 1, true)), (2, 0, false)), None);
        assert_eq!(cable_pair(Some((2, 1, true)), (4, 0, false)), None);
        assert_eq!(cable_pair(None, (3, 0, false)), None);
    }
}
