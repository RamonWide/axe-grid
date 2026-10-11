//! The egui shell. It reads [`axegrid_engine::State`] to draw and acts only
//! by running commands: every menu item, button, knob and shortcut below
//! ends in [`Shell::run`]. Menus and shortcuts are generated from the
//! command registry, so registering a command with a menu path is all it
//! takes to surface it.
//!
//! The window is arranged the way hardware-editor users expect: preset and
//! scenes across the top, the routing grid under it, the selected block's
//! controls at the bottom. The look itself lives in [`theme`] and [`knob`].

pub mod knob;
pub mod theme;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axegrid_engine::{registry, Engine, XyState};
use axegrid_model::{blocks, grid};
use egui::epaint::CubicBezierShape;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Rect, RichText,
};
use egui::{Sense, Stroke, StrokeKind, Vec2};
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
const CONFIRMED: &[&str] = &["preset.store"];
/// Width of one character of the small text in a control cell, roughly.
const CHOICE_CHAR_WIDTH: f32 = 7.4;
/// A fader's share of a cell's width.
const FADER_WIDTH: f32 = 0.62;
/// Width of a page's curve, in cells, and the frequencies and gain steps it shows.
const GRAPH_UNITS: f32 = 4.0;
const GRAPH_HZ: (f32, f32) = (20.0, 20_000.0);
const GRAPH_STEP_DB: f32 = 6.0;
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
const TUNER_SPAN: f32 = 63.0;

/// A command the user asked for during this frame.
type Action = (&'static str, Value);

/// One end of a cable being drawn: a cell, and whether it is its output side.
type CableEnd = (usize, usize, bool);

/// What the user is in the middle of on the grid.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct GridGesture {
    /// The cable end clicked first, while a cable is being drawn.
    cable: Option<CableEnd>,
    /// The cell a block is being dragged from.
    dragging: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rail {
    Input,
    Output,
}

/// How much room a page of the block editor needs.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct PageShape {
    param_rows: usize,
    mixer_rows: usize,
    /// Widest row of each section, in cells.
    param_units: f32,
    mixer_units: f32,
    graph: bool,
}

impl PageShape {
    fn of(page: &Value) -> Self {
        let mut shape = Self { graph: page["graph"].is_object(), ..Self::default() };
        for row in page["rows"].as_array().into_iter().flatten() {
            let controls = row["controls"].as_array().into_iter().flatten();
            let units: f32 = controls.map(|c| if c["kind"] == "slider" { FADER_WIDTH } else { 1.0 }).sum();
            if row["mixer"] == true {
                shape.mixer_rows += 1;
                shape.mixer_units = shape.mixer_units.max(units);
            } else {
                shape.param_rows += 1;
                shape.param_units = shape.param_units.max(units);
            }
        }
        shape
    }

    fn rows(&self) -> usize {
        self.param_rows.max(self.mixer_rows).max(if self.graph { 2 } else { 1 })
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
        }
    }
}

#[derive(Default)]
pub struct Shell {
    pub engine: Engine,
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
    confirm_save: bool,
    themed: bool,
    gesture: GridGesture,
    cpu_read_at: Option<Instant>,
    beats_seen: u64,
    beat_at: Option<Instant>,
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
    let (fill, ink) = if on { (lit, theme::ON_ACCENT) } else { (theme::RAISED, theme::TEXT_DIM) };
    ui.add(egui::Button::new(RichText::new(text).color(ink).strong()).fill(fill).min_size(vec2(28.0, 24.0)))
}

impl Shell {
    pub fn new() -> Self {
        let mut shell = Self::default();
        shell.run("device.list_ports", &Value::Null);
        // Definitions make choice lists available; their absence is fine.
        let _ = shell.engine.run("definitions.load_installed", &Value::Null);
        shell.status = "Ready".into();
        shell
    }

    /// Runs a command and shows its outcome in the status bar.
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        match self.engine.run(id, params) {
            Ok(result) => {
                if id == "device.list_ports" {
                    let names =
                        |key: &str| -> Vec<String> { serde_json::from_value(result[key].clone()).unwrap_or_default() };
                    (self.inputs, self.outputs) = (names("inputs"), names("outputs"));
                }
                self.status = format!("{id}: ok");
                Some(result)
            }
            Err(error) => {
                self.status = format!("{error}");
                None
            }
        }
    }

    /// Runs a housekeeping command; only a failure reaches the status bar.
    fn run_quietly(&mut self, id: &str) {
        if let Err(error) = self.engine.run(id, &Value::Null) {
            self.status = format!("{error}");
        }
    }

    /// Keeps tuner, tempo lamp, load meter and front-panel changes current.
    fn housekeeping(&mut self, ctx: &egui::Context) {
        if !self.engine.is_connected() {
            self.gesture = GridGesture::default();
            return;
        }
        self.run_quietly("device.poll");
        let beats = self.engine.state().beats;
        if beats != std::mem::replace(&mut self.beats_seen, beats) {
            self.beat_at = Some(Instant::now());
        }
        // Not while a control is being dragged: the request would stall it.
        let idle = !ctx.input(|input| input.pointer.any_down());
        if idle && self.cpu_read_at.is_none_or(|at| at.elapsed() >= CPU_PERIOD) && self.engine.is_connected() {
            self.cpu_read_at = Some(Instant::now());
            self.run_quietly("device.cpu_load");
        }
        let tuning = self.engine.state().tuner_on;
        ctx.request_repaint_after(Duration::from_millis(if tuning { 40 } else { 120 }));
    }

    fn shortcuts(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if ctx.wants_keyboard_input() {
            return;
        }
        for command in registry().iter().filter(|c| !c.shortcut.is_empty()) {
            let Some(shortcut) = parse_shortcut(command.shortcut) else { continue };
            let enabled = self.engine.disabled_reason(command.id).is_none();
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
        egui::menu::bar(ui, |ui| {
            for (menu, items) in menus {
                ui.menu_button(menu, |ui| {
                    for (item, command) in items {
                        let reason = self.engine.disabled_reason(command.id);
                        let button = egui::Button::new(item).shortcut_text(command.shortcut);
                        let response = ui.add_enabled(reason.is_none(), button);
                        if response
                            .on_hover_text(command.doc)
                            .on_disabled_hover_text(reason.unwrap_or_default())
                            .clicked()
                        {
                            actions.push((command.id, Value::Null));
                            ui.close_menu();
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
        ui.label(RichText::new("MIDI IN").small().color(theme::TEXT_DIM));
        port(ui, "midi-in", &self.inputs, &mut self.input);
        ui.label(RichText::new("OUT").small().color(theme::TEXT_DIM));
        port(ui, "midi-out", &self.outputs, &mut self.output);
        if ui.button("Rescan").clicked() {
            actions.push(("device.list_ports", Value::Null));
        }
        let ready = !self.input.is_empty() && !self.output.is_empty();
        let connect = egui::Button::new(RichText::new("Connect").color(theme::ON_ACCENT).strong()).fill(theme::ACCENT);
        if ui.add_enabled(ready, connect).clicked() {
            actions.push(("device.connect", json!({ "input": self.input, "output": self.output })));
        }
        if ui.button("Simulator").on_hover_text("Try the editor without hardware").clicked() {
            actions.push(("device.connect_simulator", Value::Null));
        }
    }

    fn preset_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let state = self.engine.state();
        if ui.button("◀").on_hover_text("Previous preset").clicked() {
            actions.push(("preset.previous", Value::Null));
        }
        let number = egui::TextEdit::singleline(&mut self.goto)
            .hint_text(format!("{:03}", state.preset.number))
            .desired_width(34.0)
            .font(FontId::monospace(14.0));
        let number = ui.add(number).on_hover_text("Type a preset number and press Enter");
        if number.lost_focus() {
            if let (true, Ok(wanted)) = (ui.input(|i| i.key_pressed(Key::Enter)), self.goto.trim().parse::<u16>()) {
                actions.push(("preset.select", json!({ "number": wanted })));
            }
            self.goto.clear();
        }
        match &mut self.rename {
            Some(name) => {
                let edit = ui.add(egui::TextEdit::singleline(name).desired_width(250.0).char_limit(32));
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
                let name = egui::Button::new(RichText::new(shown).size(15.0).strong()).min_size(vec2(250.0, 24.0));
                if ui.add(name).on_hover_text("Click to rename").clicked() {
                    self.rename = Some(state.preset.name.clone());
                    self.rename_needs_focus = true;
                }
            }
        }
        if ui.button("▶").on_hover_text("Next preset").clicked() {
            actions.push(("preset.next", Value::Null));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Disconnect").clicked() {
                actions.push(("device.disconnect", Value::Null));
            }
            let save = egui::Button::new(RichText::new("Save").color(theme::ON_ACCENT).strong()).fill(theme::ACCENT);
            if ui.add(save).on_hover_text("Write the edit buffer to the device (asks first)").clicked() {
                actions.push(("preset.store", Value::Null));
            }
        });
    }

    /// Second row of the header: scenes, tuner, tempo and the load meter.
    fn performance_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let state = self.engine.state();
        ui.label(RichText::new("SCENE").small().color(theme::TEXT_DIM));
        for scene in 0..8u8 {
            if lit_button(ui, &(scene + 1).to_string(), state.preset.scene == scene, theme::ACCENT).clicked() {
                actions.push(("scene.select", json!({ "scene": scene + 1 })));
            }
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("AXE·GRID").size(18.0).strong().color(theme::ACCENT));
            let state = self.engine.state();
            let (lamp, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            let lit = if self.engine.is_connected() { theme::LAMP_ON } else { theme::LAMP_OFF };
            ui.painter().circle_filled(lamp.center(), 4.5, lit);
            let model = state.model.map_or("offline", |m| m.name());
            let firmware = state.firmware.map(|(a, b)| format!("  fw {a}.{b:02}")).unwrap_or_default();
            let tag = if state.simulated { "  simulated" } else { "" };
            ui.label(RichText::new(format!("{model}{firmware}{tag}")).small().color(theme::TEXT_DIM));
            ui.separator();
            if self.engine.is_connected() {
                self.preset_controls(ui, actions);
            } else {
                self.connection_controls(ui, actions);
            }
        });
        if self.engine.is_connected() {
            ui.add_space(4.0);
            ui.horizontal(|ui| self.performance_controls(ui, actions));
        }
    }

    /// The right-click menu of one grid cell.
    fn cell_menu(&self, ui: &mut egui::Ui, column: usize, row: usize, actions: &mut Vec<Action>) {
        let view = &self.engine.state().preset;
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
                ui.close_menu();
            }
            let engaged = live.is_none_or(|s| s.engaged);
            if ui.button(if engaged { "Bypass" } else { "Engage" }).clicked() {
                actions.push((if engaged { "block.bypass" } else { "block.engage" }, json!({ "block": id })));
                ui.close_menu();
            }
            let on_x = live.is_none_or(|s| s.xy == XyState::X);
            if ui.button(if on_x { "Switch to Y" } else { "Switch to X" }).clicked() {
                actions.push(("block.set_xy", json!({ "block": id, "state": if on_x { "Y" } else { "X" } })));
                ui.close_menu();
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
                            ui.close_menu();
                        }
                    }
                });
            }
            if ui.button("Shunt").on_hover_text("A plain pass-through").clicked() {
                actions.push(("grid.place_shunt", cell.clone()));
                ui.close_menu();
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
                    ui.close_menu();
                }
            }
            ui.separator();
        }
        if ui.button(RichText::new("Remove").color(theme::DANGER)).clicked() {
            actions.push(("grid.clear_cell", cell.clone()));
            ui.close_menu();
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
        let selected = self.engine.state().selected_block == Some(block);
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
            painter.text(at, Align2::CENTER_CENTER, letter, font.clone(), ink);
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
        let state = self.engine.state();
        let view = &state.preset;
        let natural = vec2(
            2.0 * (RAIL + TILE_GAP.x) + grid::COLUMNS as f32 * (TILE.x + TILE_GAP.x) - TILE_GAP.x,
            grid::ROWS as f32 * (TILE.y + TILE_GAP.y) - TILE_GAP.y,
        );
        let available = ui.available_size();
        let scale = grid_scale(available, natural);
        let (outer, backdrop) = ui.allocate_exact_size(available.max(natural * scale), Sense::click());
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
        let GridGesture { cable: mut pending, mut dragging } = self.gesture;
        if backdrop.clicked() || backdrop.secondary_clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
            pending = None;
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
        let cell_under = |at: Pos2| {
            let cells = (0..grid::COLUMNS).flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)));
            cells.into_iter().find(|&(column, row)| tile(column, row).contains(at))
        };
        for column in 0..grid::COLUMNS {
            for row in 0..grid::ROWS {
                let rect = tile(column, row);
                let id = view.grid.cell(column, row).block;
                let sense = if id == blocks::EMPTY { Sense::click() } else { Sense::click_and_drag() };
                let response = ui.interact(rect, ui.id().with(("cell", column, row)), sense);
                let block = blocks::by_id(id);
                let lifted = dragging == Some((column, row));
                // A cell a dragged block may be dropped on.
                let landing = dragging.is_some() && id == blocks::EMPTY && pointer.is_some_and(|p| rect.contains(p));
                if id == blocks::EMPTY {
                    let edge = if landing {
                        theme::ACCENT
                    } else if response.hovered() {
                        theme::TEXT_DIM
                    } else {
                        theme::RAISED
                    };
                    let width = if landing { 2.0_f32 } else { 1.0 };
                    painter.rect_stroke(rect, theme::RADIUS, Stroke::new(width, edge), StrokeKind::Inside);
                } else if let Some(block) = block {
                    let live = view.states.iter().find(|s| s.block == id);
                    let engaged = live.is_none_or(|s| s.engaged) && !lifted;
                    let color = Category::of(block.effect_type).color();
                    painter.rect_filled(rect, theme::RADIUS, if engaged { color } else { theme::muted(color) });
                    let ink = if engaged { Color32::WHITE } else { theme::TEXT_DIM };
                    // Shrink the lettering until the longest word fits, so that
                    // a name only ever breaks between words.
                    let room = tile_size.x - 8.0 * scale;
                    let longest = block.name.split(' ').max_by_key(|word| word.len()).unwrap_or_default();
                    let fits = |size: &f32| {
                        let font = FontId::proportional(size * scale);
                        ui.fonts(|fonts| fonts.layout_no_wrap(longest.to_owned(), font, ink)).size().x <= room
                    };
                    let size = [11.0_f32, 10.0, 9.0].into_iter().find(fits).unwrap_or(8.5);
                    let font = FontId::proportional(size * scale);
                    let mut job = egui::text::LayoutJob::simple(block.name.clone(), font, ink, room);
                    job.halign = Align::Center;
                    let galley = ui.fonts(|fonts| fonts.layout_job(job));
                    let top = rect.center().y - galley.size().y / 2.0;
                    painter.galley(pos2(rect.center().x, top), galley, ink);
                    if live.is_some_and(|s| s.xy == XyState::Y) {
                        let corner = rect.right_top() + vec2(-8.0, 8.0) * scale;
                        painter.text(
                            corner,
                            Align2::CENTER_CENTER,
                            "Y",
                            FontId::proportional(9.0 * scale),
                            theme::ACCENT,
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
                if response.clicked() && block.is_some() && pending.is_none() {
                    actions.push(("block.select", json!({ "block": id })));
                }
                if response.drag_started() {
                    (dragging, pending) = (Some((column, row)), None);
                }
                if response.drag_stopped() {
                    let target = pointer.and_then(cell_under).filter(|&cell| cell != (column, row));
                    if let Some((to_column, to_row)) =
                        target.filter(|&(c, r)| view.grid.cell(c, r).block == blocks::EMPTY)
                    {
                        let cells = json!({ "column": column, "row": row, "to_column": to_column, "to_row": to_row });
                        actions.push(("grid.move_block", cells));
                    }
                    dragging = None;
                }
                let hint = match id {
                    blocks::EMPTY => "Right-click to insert".to_owned(),
                    id => format!("{}: click to edit, drag to move", blocks::label(id)),
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
                    let ink = if lit { theme::ON_ACCENT } else { theme::ACCENT };
                    painter.text(centre, Align2::CENTER_CENTER, letter, FontId::proportional(10.0 * scale), ink);
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
        // The block being moved follows the pointer.
        if let (Some((column, row)), Some(pointer)) = (dragging, pointer) {
            let ghost = Rect::from_center_size(pointer, tile_size);
            let name = blocks::label(view.grid.cell(column, row).block);
            let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.id().with("ghost")));
            top.rect_filled(ghost, theme::RADIUS, theme::RAISED_HOT);
            top.rect_stroke(ghost, theme::RADIUS, Stroke::new(2.0_f32, theme::ACCENT), StrokeKind::Outside);
            top.text(ghost.center(), Align2::CENTER_CENTER, name, FontId::proportional(11.0 * scale), theme::TEXT);
        }
        GridGesture { cable: pending, dragging }
    }

    /// One control cell of the block editor.
    fn control(ui: &mut egui::Ui, block: u16, cell: Vec2, control: &ControlView, actions: &mut Vec<Action>) {
        let set = |value: u32| ("param.set", json!({ "block": block, "param": control.id, "value": value }));
        match control.kind.as_str() {
            "spacer" => {
                ui.allocate_exact_size(cell, Sense::hover());
            }
            "knob" => {
                if let Some(value) = knob::knob(ui, cell, &control.label, &control.text, control.raw, control.max) {
                    actions.push(set(value));
                }
            }
            "slider" => {
                let size = vec2(cell.x * FADER_WIDTH, cell.y);
                if let Some(value) = knob::fader(ui, size, &control.label, &control.text, control.raw, control.max) {
                    actions.push(set(value));
                }
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
                let turned = if response.contains_pointer() { ui.input(|i| i.raw_scroll_delta.y) } else { 0.0 };
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

    /// Draws an equaliser curve: gain against frequency on a logarithmic axis.
    fn eq_graph(ui: &mut egui::Ui, size: Vec2, graph: &Value) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let plot = rect.shrink2(vec2(8.0, 10.0));
        let pairs = |key: &str| -> Vec<(f32, f32)> {
            let list = graph[key].as_array().map(Vec::as_slice).unwrap_or_default();
            list.iter().filter_map(|p| Some((p[0].as_f64()? as f32, p[1].as_f64()? as f32))).collect()
        };
        let (points, bands) = (pairs("points"), pairs("bands"));
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
        let line: Vec<Pos2> = points.iter().map(|&(hz, db)| pos2(x(hz), y(db))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, theme::ACCENT)));
        for (index, &(hz, db)) in bands.iter().enumerate() {
            let at = pos2(x(hz), y(db));
            painter.circle_filled(at, 7.0, theme::PANEL);
            painter.circle_stroke(at, 7.0, Stroke::new(1.5_f32, theme::ACCENT));
            painter.text(at, Align2::CENTER_CENTER, (index + 1).to_string(), small.clone(), theme::TEXT);
        }
    }

    fn block_editor(&mut self, ui: &mut egui::Ui, block: u16, actions: &mut Vec<Action>) {
        if self.page_block != Some(block) {
            (self.page_block, self.page) = (Some(block), 0);
        }
        let state = self.engine.state();
        let live = state.preset.states.iter().find(|s| s.block == block);
        let editor = self.engine.editor();
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
                egui::ComboBox::from_id_salt("block-model").selected_text(current).width(240.0).show_ui(ui, |ui| {
                    for (index, choice) in choices.iter().enumerate() {
                        if ui.selectable_label(index == param.raw as usize, choice).clicked() {
                            actions.push(("param.set", json!({ "block": block, "param": param.id, "value": index })));
                        }
                    }
                });
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

        self.page = self.page.min(pages.len().saturating_sub(1));
        let Some(page) = pages.get(self.page) else {
            ui.label(RichText::new("This block has no controls on this firmware.").color(theme::TEXT_DIM));
            return;
        };
        let shape = PageShape::of(page);
        let available = ui.available_size();
        let rows = shape.rows() as f32;
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
                let column = |ui: &mut egui::Ui, mixer: bool, actions: &mut Vec<Action>| {
                    ui.vertical(|ui| {
                        for row in all_rows.iter().filter(|row| (row["mixer"] == true) == mixer) {
                            ui.horizontal(|ui| {
                                for control in row["controls"].as_array().unwrap_or(&empty) {
                                    Self::control(ui, block, cell, &ControlView::from_json(control), actions);
                                }
                            });
                        }
                    });
                };
                column(ui, false, actions);
                if shape.graph {
                    ui.add_space(cell.x * 0.15);
                    Self::eq_graph(ui, vec2(cell.x * GRAPH_UNITS, cell.y * rows), &page["graph"]);
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
        let editor = self.engine.editor();
        let pages = editor["pages"].as_array().map(Vec::as_slice).unwrap_or_default();
        let rows = pages.get(self.page.min(pages.len().saturating_sub(1))).map_or(1, |page| PageShape::of(page).rows());
        let wanted = EDITOR_CHROME + rows as f32 * knob::CELL.y;
        wanted.min(ctx.screen_rect().height() * EDITOR_SHARE)
    }

    /// The tuner read-out, shown while the unit's tuner is on.
    fn tuner_window(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let tuner = self.engine.inspect()["tuner"].clone();
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
    fn save_dialog(&mut self, ctx: &egui::Context) {
        let number = self.engine.state().preset.number;
        let mut decision = None;
        egui::Window::new("Save preset")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Overwrite preset {number:03} on the device with what you hear now?"));
                ui.label(
                    RichText::new("The preset stored in that slot is replaced. This cannot be undone.")
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                    let save = egui::Button::new(RichText::new("Overwrite").color(Color32::WHITE).strong())
                        .fill(theme::DANGER);
                    if ui.add(save).clicked() {
                        decision = Some(true);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            decision = Some(false);
        }
        if let Some(go) = decision {
            self.confirm_save = false;
            if go {
                self.run("preset.store", &Value::Null);
            }
        }
    }

    /// Draws one frame and runs whatever the user asked for.
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !std::mem::replace(&mut self.themed, true) {
            theme::apply(ctx);
        }
        self.housekeeping(ctx);
        let mut actions: Vec<Action> = Vec::new();
        if !self.confirm_save {
            self.shortcuts(ctx, &mut actions);
        }
        let bar = egui::Frame::new().fill(theme::BACKDROP).inner_margin(egui::Margin::symmetric(10, 4));
        egui::TopBottomPanel::top("menu").frame(bar).show(ctx, |ui| self.menu_bar(ui, &mut actions));
        let header = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 9));
        egui::TopBottomPanel::top("header").frame(header).show(ctx, |ui| self.header(ui, &mut actions));
        egui::TopBottomPanel::bottom("status")
            .frame(bar)
            .show(ctx, |ui| ui.label(RichText::new(&self.status).small().color(theme::TEXT_DIM)));
        if let Some(block) = self.engine.state().selected_block {
            let editor = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 10));
            egui::TopBottomPanel::bottom("block-editor")
                .frame(editor)
                .resizable(false)
                .exact_height(self.editor_height(ctx))
                .show(ctx, |ui| self.block_editor(ui, block, &mut actions));
        }
        let stage = egui::Frame::new().fill(theme::BACKDROP).inner_margin(egui::Margin::same(16));
        egui::CentralPanel::default().frame(stage).show(ctx, |ui| {
            if self.engine.is_connected() {
                self.gesture = self.grid(ui, &mut actions);
            } else {
                ui.centered_and_justified(|ui| {
                    let hint = "Choose the MIDI ports your unit is on and press Connect,\nor try the simulator.";
                    ui.label(RichText::new(hint).size(15.0).color(theme::TEXT_DIM));
                });
            }
        });
        if self.engine.state().tuner_on {
            self.tuner_window(ctx, &mut actions);
        }
        for (id, params) in actions {
            if CONFIRMED.contains(&id) {
                self.confirm_save = true;
            } else {
                self.run(id, &params);
            }
        }
        if self.confirm_save {
            self.save_dialog(ctx);
        }
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
        assert_eq!((shape.param_rows, shape.mixer_rows, shape.graph), (2, 1, true));
        assert_eq!((shape.param_units, shape.mixer_units), (3.0, 2.0));
        assert_eq!(shape.rows(), 2);
        assert!((shape.units() - (3.0 + GRAPH_UNITS + 0.15 + 2.0)).abs() < 1e-6);
        // An empty page still takes one row and one cell.
        assert_eq!((PageShape::of(&json!({})).rows(), PageShape::of(&json!({})).units()), (1, 1.0));
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
