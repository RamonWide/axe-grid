//! The preset and cabinet managers: two windows that list what the unit
//! stores and move presets and cabinets between the unit and files. Like
//! the rest of the shell they act only by running commands.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use axegrid_engine::{CommandError, NameScan};
use axegrid_model::{blocks, library};
use egui::{vec2, Align, Id, Key, Layout, Modifiers, RichText, Sense};
use serde_json::{json, Value};

use crate::picker::{self, EntryLook, Step};
use crate::{theme, Action, Shell};

/// Names asked for at a time, so that an edit made meanwhile does not wait long.
const NAMES_PER_TURN: u16 = 8;
/// How long the reading of names rests after the unit failed to answer.
const SCAN_RETRY: Duration = Duration::from_secs(5);
/// The size a manager opens at, and the smallest it is made: its header and
/// its buttons fit that width, and a few rows of the list that height.
const PRESETS_SIZE: egui::Vec2 = vec2(820.0, 560.0);
const PRESETS_MIN: egui::Vec2 = vec2(500.0, 320.0);
const CABS_SIZE: egui::Vec2 = vec2(820.0, 560.0);
const CABS_MIN: egui::Vec2 = vec2(600.0, 340.0);
/// Room left around a manager that opens in a small app window.
const WINDOW_MARGIN: egui::Vec2 = vec2(40.0, 80.0);
/// Frames after opening during which the list is scrolled to its pick;
/// the window has its final size only on the second one.
const REVEAL_FRAMES: u8 = 2;
/// Digits a preset number is written with.
const PRESET_DIGITS: usize = 3;
/// Extension of preset and cabinet files.
const EXTENSION: &str = "syx";
/// What the unit calls a user-cab slot that holds nothing.
const EMPTY_CAB: &str = "EMPTY";
/// What it calls a preset slot that holds nothing.
const EMPTY_PRESET: &str = "<EMPTY>";
/// Effect type of the cabinet block, and the parameter that picks its cab.
const CAB_EFFECT: u8 = 6;
const CAB_CHOICE: &str = "CABINET_TYPEL";

/// A run of presets being written to a folder, a few per frame.
struct Backup {
    folder: PathBuf,
    next: u16,
    written: u64,
}

#[derive(Default)]
pub(crate) struct LibraryView {
    pub presets_open: bool,
    pub cabs_open: bool,
    preset_filter: String,
    cab_filter: String,
    preset_pick: Option<u16>,
    cab_pick: Option<u16>,
    /// When the reading of names may go on, after the unit failed to answer.
    scan_retry_at: Option<Instant>,
    hide_empty: bool,
    /// Text of the "copy to slot" box.
    copy_to: String,
    backup: Option<Backup>,
    /// Something was asked of the unit and has not come back yet.
    waiting: bool,
    /// Whose turn it is when names are being read and presets backed up at once.
    turn: usize,
    /// The frame each window was last drawn in, which tells when it has just been opened.
    presets_drawn: Option<u64>,
    cabs_drawn: Option<u64>,
    /// Frames left in which a list is still to be scrolled to its pick.
    preset_reveal: u8,
    cab_reveal: u8,
}

impl LibraryView {
    /// Forgets what was read; the names belong to the unit that was connected.
    pub fn reset(&mut self) {
        let (presets_open, cabs_open, hide_empty) = (self.presets_open, self.cabs_open, self.hide_empty);
        *self = Self { presets_open, cabs_open, hide_empty, ..Self::default() };
    }

    /// Whether names are being read or presets written in the background.
    /// The names are read by the engine (`library.scan`), which knows how
    /// far it has come: `scan`.
    pub fn busy(&self, scan: &NameScan) -> bool {
        self.backup.is_some() || (self.scan_wanted(scan) && self.scan_retry_at.is_none())
    }

    /// Whether there are names still to be read from the unit that is connected.
    fn scan_wanted(&self, scan: &NameScan) -> bool {
        scan.total() > 0 && !scan.verified()
    }
}

/// What a manager says beside its filter while its names are not certain
/// yet: `checked` of `slots` have been read from the unit.
fn scan_note(from_cache: bool, checked: u16, slots: u16) -> String {
    if from_cache {
        // The list is complete already, from last time; this only confirms it.
        format!("saved names, checking {checked} of {slots}")
    } else {
        format!("reading names {checked} of {slots}")
    }
}

/// Whether `name` or the slot number matches what was typed in a search box:
/// the rule of the browser for amp models and cabs.
fn matches(filter: &str, number: u16, name: &str) -> bool {
    picker::matches(filter, Some(u32::from(number)), name)
}

/// One slot as a manager lists it: its number, what is shown for its name,
/// and whether that stands for a slot that holds nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot<'a> {
    number: u16,
    name: &'a str,
    empty: bool,
}

impl<'a> Slot<'a> {
    fn preset(number: u16, name: &'a str) -> Self {
        let empty = name.is_empty() || name == EMPTY_PRESET;
        Self { number, name: if empty { "(empty)" } else { name }, empty }
    }

    fn cab(number: u16, name: &'a str) -> Self {
        let empty = name == EMPTY_CAB;
        Self { number, name: if empty { "(empty)" } else { name }, empty }
    }

    fn found(&self, filter: &str) -> bool {
        matches(filter, self.number, self.name)
    }
}

/// What the keyboard asked of a list: steps of the pick, and Enter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Keys {
    steps: Vec<Step>,
    enter: bool,
}

/// The keys a list acts on. They are its own only while its search box has
/// the keyboard: a manager is one window among others, and the arrows and
/// Enter mean something elsewhere too.
fn list_keys(ctx: &egui::Context, field: Id) -> Keys {
    if !ctx.memory(|memory| memory.has_focus(field)) {
        return Keys::default();
    }
    ctx.input_mut(|input| Keys {
        steps: picker::take_arrows(input),
        enter: input.consume_key(Modifiers::NONE, Key::Enter),
    })
}

/// What the keys make of the pick in a list laid out in columns of `rows`:
/// the pick after them, and the slot Enter asked for. Enter acts on the pick;
/// when that is not among the slots listed it picks the first one instead.
fn keyed(slots: &[Slot<'_>], pick: Option<u16>, rows: usize, keys: &Keys) -> (Option<u16>, Option<u16>) {
    let position_of = |pick: Option<u16>| pick.and_then(|number| slots.iter().position(|slot| slot.number == number));
    let mut pick = pick;
    for &arrow in &keys.steps {
        let next = picker::step(position_of(pick), slots.len(), rows, arrow);
        pick = next.and_then(|at| slots.get(at)).map(|slot| slot.number).or(pick);
    }
    if !keys.enter {
        return (pick, None);
    }
    match position_of(pick) {
        Some(_) => (pick, pick),
        None => (slots.first().map(|slot| slot.number).or(pick), None),
    }
}

/// Whether a window drawn in frame `now` and last drawn in `last` has just
/// been opened: it was not there in the frame before.
fn just_opened(last: Option<u64>, now: u64) -> bool {
    !last.is_some_and(|last| now.saturating_sub(last) <= 1)
}

/// The size a manager opens at: the one `wanted`, or what the app window has
/// room for, and never less than the `least` its buttons need.
fn opening_size(room: egui::Vec2, wanted: egui::Vec2, least: egui::Vec2) -> egui::Vec2 {
    wanted.min(room - WINDOW_MARGIN).max(least)
}

/// A row of buttons in the part of a manager that is laid out from the bottom
/// up. The row is given the height of a button at once: a row that starts
/// lower and grows runs out under the window, which then grows every frame.
fn button_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    let text = ui.text_style_height(&egui::TextStyle::Button);
    // A text box in the row is a little taller than its text as well.
    let field = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let height = (text + 2.0 * ui.spacing().button_padding.y).max(field).max(ui.spacing().interact_size.y).ceil();
    let size = vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), add)
}

/// A list of slots as a manager shows it.
struct SlotList<'a> {
    salt: &'static str,
    slots: &'a [Slot<'a>],
    /// Digits the numbers are padded to with zeros; none when 0.
    digits: usize,
    pick: Option<u16>,
    /// The slot the unit is on, if the list has such a thing.
    current: Option<u16>,
    /// The window has just been opened: bring the pick, or else the current slot, into view.
    reveal: bool,
    /// What to say when there is no slot to list.
    nothing: &'static str,
}

/// Draws the slots in columns that fill what is left of the window, like the
/// browser for amp models and cabs, and returns the pick after this frame and
/// the slot that was double-clicked or entered. Only the columns in view are drawn.
fn slot_list(ui: &mut egui::Ui, list: &SlotList<'_>, keys: &Keys) -> (Option<u16>, Option<u16>) {
    let slots = list.slots;
    let space = ui.available_size();
    let grid = picker::columns(slots.len(), space.x, space.y);
    let (mut pick, mut activated) = keyed(slots, list.pick, grid.rows, keys);
    if slots.is_empty() {
        ui.label(RichText::new(list.nothing).color(theme::TEXT_DIM));
        return (pick, activated);
    }
    // A step is followed just far enough to be seen; on opening the slot is centred.
    let reveal = if keys.steps.is_empty() {
        list.reveal.then_some((pick.or(list.current), Some(Align::Center)))
    } else {
        Some((pick, None))
    };
    // The wheel moves along the columns, the only way there is to go.
    ui.style_mut().always_scroll_the_only_direction = true;
    let area = egui::ScrollArea::horizontal().id_salt(list.salt).auto_shrink(false);
    area.show_viewport(ui, |ui, viewport| {
        let (area, _) = ui.allocate_exact_size(grid.extent(), Sense::hover());
        let place = |position: usize| grid.place(position).translate(area.min.to_vec2());
        for position in grid.visible(viewport.min.x, viewport.max.x, slots.len()) {
            let Some(slot) = slots.get(position) else { continue };
            let number = format!("{:0digits$}", slot.number, digits = list.digits);
            let look = EntryLook {
                number: Some(&number),
                name: slot.name,
                // The picked slot is filled; the one the unit is on is written in the accent.
                filled: pick == Some(slot.number),
                lit: false,
                marked: list.current == Some(slot.number),
                faint: slot.empty,
            };
            let response = picker::entry(ui, place(position), ui.id().with(("slot", slot.number)), &look);
            if response.clicked() {
                pick = Some(slot.number);
            }
            if response.double_clicked() {
                activated = Some(slot.number);
            }
        }
        if let Some((Some(target), align)) = reveal {
            if let Some(position) = slots.iter().position(|slot| slot.number == target) {
                ui.scroll_to_rect(place(position), align);
            }
        }
    });
    (pick, activated)
}

/// A file name offered when saving: the slot number and what is safe of the name.
fn suggested_file(number: u16, name: &str) -> String {
    let kept: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || " -_+()".contains(c) { c } else { '_' }).collect();
    format!("{number:03} {}", kept.trim()).trim().to_owned() + "." + EXTENSION
}

fn file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("SysEx files", &[EXTENSION])
}

impl Shell {
    /// Asks the unit for a few more names, or for the next preset of a
    /// backup, once the last answer is in.
    pub(crate) fn library_work(&mut self) {
        if self.library.waiting || self.view.state.model.is_none() {
            return;
        }
        let mut wanted: Vec<Action> = Vec::new();
        if self.library.scan_retry_at.is_some_and(|at| Instant::now() >= at) {
            self.library.scan_retry_at = None;
        }
        // Whether a window is open or not: the names are checked once after
        // connecting, so that they are right by the time they are looked at.
        if self.library.scan_wanted(&self.view.state.name_scan) && self.library.scan_retry_at.is_none() {
            // The list that is being looked at goes first.
            let first = if self.library.cabs_open && !self.library.presets_open { "cabs" } else { "presets" };
            wanted.push(("library.scan", json!({ "budget": NAMES_PER_TURN, "first": first })));
        }
        if let Some(backup) = &self.library.backup {
            let params = json!({ "folder": backup.folder.display().to_string(), "first": backup.next, "count": 1 });
            wanted.push(("preset.export_range", params));
        }
        if wanted.is_empty() {
            return;
        }
        self.library.turn = self.library.turn.wrapping_add(1);
        let (id, params) = wanted.swap_remove(self.library.turn % wanted.len());
        self.library.waiting = true;
        self.submit(id, params, true);
    }

    /// Takes in the unit's answer to what [`Self::library_work`] asked.
    pub(crate) fn library_done(&mut self, id: &str, params: &Value, outcome: &Result<Value, CommandError>) {
        self.library.waiting = false;
        let Some(model) = self.view.state.model else { return };
        let asked = |key: &str| params[key].as_u64().unwrap_or_default() as u16;
        let first = asked("first");
        let slots = library::preset_count(model);
        match id {
            // The engine keeps the place; a unit that did not answer is left alone for a while.
            "library.scan" if outcome.is_err() => self.library.scan_retry_at = Some(Instant::now() + SCAN_RETRY),
            "preset.export_range" => {
                let Some(backup) = self.library.backup.as_mut().filter(|backup| backup.next == first) else { return };
                match outcome {
                    Ok(result) => {
                        backup.written += result["written"].as_u64().unwrap_or(0);
                        backup.next += 1;
                        self.status = format!("Backing up presets: {} of {slots}", backup.next);
                        if backup.next >= slots {
                            self.status =
                                format!("Backed up {} presets to {}", backup.written, backup.folder.display());
                            self.library.backup = None;
                        }
                    }
                    Err(error) => {
                        self.status = format!("Backup stopped at preset {}: {error}", backup.next);
                        self.library.backup = None;
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn preset_manager(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut open = self.library.presets_open;
        let view = self.view.clone();
        let field = Id::new("preset-filter");
        let keys = list_keys(ctx, field);
        let frame = ctx.cumulative_frame_nr();
        if just_opened(self.library.presets_drawn.replace(frame), frame) {
            self.library.preset_reveal = REVEAL_FRAMES;
        }
        let reveal = self.library.preset_reveal > 0;
        self.library.preset_reveal = self.library.preset_reveal.saturating_sub(1);
        let window = egui::Window::new("Presets")
            .open(&mut open)
            .resizable(true)
            .min_size(PRESETS_MIN)
            .default_size(opening_size(ctx.content_rect().size(), PRESETS_SIZE, PRESETS_MIN))
            .default_pos([60.0, 120.0]);
        window.show(ctx, |ui| {
            let state = &view.state;
            let slots = state.model.map_or(0, library::preset_count);
            let current = state.preset.number;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find").small().color(theme::TEXT_DIM));
                let find = egui::TextEdit::singleline(&mut self.library.preset_filter).id(field);
                ui.add(find.desired_width(180.0).hint_text("name or number"));
                let scan = &state.name_scan;
                if !scan.presets_done() {
                    ui.spinner();
                    let note = scan_note(scan.from_cache, scan.presets_checked, scan.preset_slots);
                    ui.label(RichText::new(note).small().color(theme::TEXT_DIM))
                        .on_hover_text("The names are read from the unit in the background, between your edits.");
                } else if ui.small_button("Read again").on_hover_text("Read every name from the unit again").clicked() {
                    actions.push(("library.rescan", json!({ "budget": 0, "first": "presets" })));
                }
            });
            ui.separator();
            let listed = state.preset_names.iter().map(|(number, name)| Slot::preset(*number, name));
            let shown: Vec<Slot<'_>> = listed.filter(|slot| slot.found(&self.library.preset_filter)).collect();
            let mut load = None;
            let pick = self.library.preset_pick;
            let pick_name = pick.and_then(|number| state.preset_names.get(&number)).cloned().unwrap_or_default();
            // The buttons are laid out from the bottom of the window up, last row
            // first, so that the list takes all the room between them and the header.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                button_row(ui, |ui| {
                    let import = ui.button("Import file…");
                    let hint = "Load a preset file into the edit buffer. Nothing is stored until you save.";
                    if import.on_hover_text(hint).clicked() {
                        if let Some(path) = file_dialog().pick_file() {
                            actions.push(("preset.import", json!({ "path": path.display().to_string() })));
                        }
                    }
                    match &self.library.backup {
                        Some(backup) => {
                            ui.spinner();
                            ui.label(format!("backing up {} of {slots}", backup.next));
                            if ui.button("Stop").clicked() {
                                self.library.backup = None;
                            }
                        }
                        None => {
                            let all = ui.button("Back up all…");
                            let hint = "Save every stored preset into a folder, one file each. Only reads the unit.";
                            if all.on_hover_text(hint).clicked() {
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    self.library.backup = Some(Backup { folder, next: 0, written: 0 });
                                }
                            }
                        }
                    }
                });
                ui.separator();
                button_row(ui, |ui| {
                    ui.add_enabled_ui(pick.is_some(), |ui| {
                        ui.label("Copy to slot");
                        let field =
                            egui::TextEdit::singleline(&mut self.library.copy_to).desired_width(40.0).hint_text("000");
                        let entered = ui.add(field).lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let target = self
                            .library
                            .copy_to
                            .trim()
                            .parse::<u16>()
                            .ok()
                            .filter(|&to| to < slots && Some(to) != pick);
                        let copy = ui.add_enabled(target.is_some(), egui::Button::new("Copy"));
                        let hint = "Copy the picked preset over the slot typed here (asks first)";
                        if let (true, Some(from), Some(to)) =
                            (copy.on_hover_text(hint).clicked() || entered, pick, target)
                        {
                            actions.push(("preset.copy", json!({ "from": from, "to": to })));
                        }
                    });
                });
                button_row(ui, |ui| {
                    let label =
                        pick.map_or_else(|| "Pick a preset above".to_owned(), |number| format!("Preset {number:03}"));
                    ui.label(RichText::new(label).strong());
                    ui.add_enabled_ui(pick.is_some(), |ui| {
                        if ui
                            .button("Load")
                            .on_hover_text("Select this preset on the unit (or double-click it)")
                            .clicked()
                        {
                            load = pick;
                        }
                        let export = ui.button("Export…").on_hover_text("Save the stored preset as a file");
                        if let (true, Some(number)) = (export.clicked(), pick) {
                            if let Some(path) =
                                file_dialog().set_file_name(suggested_file(number, &pick_name)).save_file()
                            {
                                actions.push((
                                    "preset.export",
                                    json!({ "number": number, "path": path.display().to_string() }),
                                ));
                            }
                        }
                        let here = ui.button("Save current here");
                        let hint = "Store what you hear now in this slot, replacing what it holds (asks first)";
                        if let (true, Some(number)) = (here.on_hover_text(hint).clicked(), pick) {
                            actions.push(("preset.store", json!({ "number": number })));
                        }
                    });
                });
                ui.separator();
                let list = SlotList {
                    salt: "preset-list",
                    slots: &shown,
                    digits: PRESET_DIGITS,
                    pick,
                    current: Some(current),
                    reveal,
                    nothing: if state.preset_names.is_empty() { "Nothing to show yet." } else { "Nothing matches." },
                };
                let (picked, activated) =
                    ui.with_layout(Layout::top_down(Align::Min), |ui| slot_list(ui, &list, &keys)).inner;
                self.library.preset_pick = picked;
                // A double click, or Enter in the search box, loads like the button.
                load = load.or(activated);
            });
            if let Some(number) = load {
                actions.push(("preset.select", json!({ "number": number })));
            }
        });
        self.library.presets_open = open;
    }

    pub(crate) fn cab_manager(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut open = self.library.cabs_open;
        let view = self.view.clone();
        let field = Id::new("cab-filter");
        let keys = list_keys(ctx, field);
        let frame = ctx.cumulative_frame_nr();
        if just_opened(self.library.cabs_drawn.replace(frame), frame) {
            self.library.cab_reveal = REVEAL_FRAMES;
        }
        let reveal = self.library.cab_reveal > 0;
        self.library.cab_reveal = self.library.cab_reveal.saturating_sub(1);
        let window = egui::Window::new("User cabs")
            .open(&mut open)
            .resizable(true)
            .min_size(CABS_MIN)
            .default_size(opening_size(ctx.content_rect().size(), CABS_SIZE, CABS_MIN))
            .default_pos([520.0, 120.0]);
        window.show(ctx, |ui| {
            let state = &view.state;
            let Some(slots) = state.model.and_then(library::cab_slots) else {
                ui.label("The cabinet memory of this model is not known to Axe-Grid yet.");
                ui.label(RichText::new("A hardware report from an owner would add it.").color(theme::TEXT_DIM));
                return;
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find").small().color(theme::TEXT_DIM));
                let find = egui::TextEdit::singleline(&mut self.library.cab_filter).id(field);
                ui.add(find.desired_width(160.0).hint_text("name or number"));
                ui.checkbox(&mut self.library.hide_empty, "Hide empty");
                let scan = &state.name_scan;
                if !scan.cabs_done() {
                    ui.spinner();
                    // The factory cabs come first in the unit's list and are checked with the rest.
                    let note = scan_note(scan.from_cache, scan.cabs_checked, scan.cab_slots);
                    ui.label(RichText::new(note).small().color(theme::TEXT_DIM))
                        .on_hover_text("The names are read from the unit in the background, between your edits.");
                } else if ui.small_button("Read again").on_hover_text("Read every name from the unit again").clicked() {
                    actions.push(("library.rescan", json!({ "budget": 0, "first": "cabs" })));
                }
            });
            ui.separator();
            let hide_empty = self.library.hide_empty;
            let listed = state.cab_names.iter().map(|(cab, name)| Slot::cab(*cab, name));
            let shown: Vec<Slot<'_>> = listed
                .filter(|slot| !(hide_empty && slot.empty))
                .filter(|slot| slot.found(&self.library.cab_filter))
                .collect();
            let pick = self.library.cab_pick;
            let pick_name = pick.and_then(|cab| state.cab_names.get(&cab)).cloned().unwrap_or_default();
            // The cab block that can audition the picked cab, if the preset has one.
            let placed = state.preset.grid.blocks();
            let cab_block =
                placed.into_iter().find(|&id| blocks::by_id(id).is_some_and(|b| b.effect_type == CAB_EFFECT));
            // Laid out from the bottom of the window up, the note first, so that
            // the list takes all the room between the buttons and the header.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                let note = "Importing writes to the unit's memory. It follows the documented message layout but has \
                            not yet been confirmed on a unit: keep a backup of the slot you overwrite.";
                ui.label(RichText::new(note).small().color(theme::TEXT_DIM));
                button_row(ui, |ui| {
                    let label = pick.map_or_else(|| "Pick a cab above".to_owned(), |cab| format!("User cab {cab}"));
                    ui.label(RichText::new(label).strong());
                    ui.add_enabled_ui(pick.is_some(), |ui| {
                        let filled = pick_name != EMPTY_CAB;
                        let export = ui.add_enabled(filled, egui::Button::new("Export…"));
                        if let (true, Some(cab)) = (export.on_hover_text("Save this cab as a file").clicked(), pick) {
                            if let Some(path) = file_dialog().set_file_name(suggested_file(cab, &pick_name)).save_file()
                            {
                                actions.push(("cab.export", json!({ "cab": cab, "path": path.display().to_string() })));
                            }
                        }
                        let import = ui.button("Import file here…");
                        let hint = "Write a cab file into this slot on the unit, replacing what it holds (asks first)";
                        if let (true, Some(cab)) = (import.on_hover_text(hint).clicked(), pick) {
                            if let Some(path) = file_dialog().pick_file() {
                                actions.push(("cab.import", json!({ "cab": cab, "path": path.display().to_string() })));
                            }
                        }
                        let name = cab_block.map(blocks::label).unwrap_or_default();
                        let audition =
                            ui.add_enabled(cab_block.is_some() && filled, egui::Button::new("Use in cab block"));
                        let hint = match cab_block {
                            Some(_) => format!("Choose this cab in {name} of the current preset, to hear it"),
                            None => "The current preset has no cab block".to_owned(),
                        };
                        let chosen = audition.on_hover_text(&hint).on_disabled_hover_text(hint).clicked();
                        if let (true, Some(cab), Some(block)) = (chosen, pick, cab_block) {
                            if let Some(index) = slots.list_index(cab) {
                                actions.push((
                                    "param.set",
                                    json!({ "block": block, "param": CAB_CHOICE, "value": index }),
                                ));
                            }
                        }
                    });
                });
                ui.separator();
                let list = SlotList {
                    salt: "cab-list",
                    slots: &shown,
                    digits: 0,
                    pick,
                    current: None,
                    reveal,
                    nothing: if state.cab_names.is_empty() { "Nothing to show yet." } else { "Nothing matches." },
                };
                // A double click does what a click does here: nothing is loaded by it.
                let (picked, _) = ui.with_layout(Layout::top_down(Align::Min), |ui| slot_list(ui, &list, &keys)).inner;
                self.library.cab_pick = picked;
            });
        });
        self.library.cabs_open = open;
    }
}

/// What the confirmation dialog says before a command that overwrites.
pub(crate) fn confirmation(id: &str, params: &Value, current: u16) -> (String, String, &'static str) {
    let number = |key: &str| params[key].as_u64();
    match id {
        "preset.copy" => (
            "Copy preset".into(),
            format!(
                "Copy preset {:03} over preset {:03}?\nThe preset stored in slot {1:03} is replaced, and unsaved \
                 edits are lost.",
                number("from").unwrap_or_default(),
                number("to").unwrap_or_default()
            ),
            "Overwrite",
        ),
        "cab.import" => (
            "Import cab".into(),
            format!(
                "Write this file into user cab {} on the device?\nThe cab stored in that slot is replaced.",
                number("cab").unwrap_or_default()
            ),
            "Overwrite",
        ),
        _ => (
            "Save preset".into(),
            format!(
                "Overwrite preset {:03} on the device with what you hear now?\nThe preset stored in that slot is \
                 replaced.",
                number("number").unwrap_or(u64::from(current))
            ),
            "Overwrite",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_names_and_numbers() {
        assert!(matches("", 5, "Clean"));
        assert!(matches(" cle ", 5, "Clean Chorus"));
        assert!(matches("41", 41, "Lead"));
        assert!(!matches("crunch", 41, "Lead"));
    }

    #[test]
    fn the_search_is_the_one_of_the_browser() {
        // Every word typed has to be found, in the name or the number, whatever the case.
        assert!(matches("chorus CLEAN", 5, "Clean Chorus"));
        assert!(!matches("clean lead", 5, "Clean Chorus"));
        assert!(matches("lead 41", 41, "Lead"));
        // An empty slot is found as it is shown.
        assert!(Slot::preset(3, EMPTY_PRESET).found("empty") && Slot::cab(3, EMPTY_CAB).found("EMPTY"));
        assert!(!Slot::preset(3, "Lead").found("empty"));
    }

    #[test]
    fn empty_slots_are_shown_as_such() {
        assert_eq!(Slot::preset(7, "Lead"), Slot { number: 7, name: "Lead", empty: false });
        for nothing in ["", EMPTY_PRESET] {
            assert_eq!(Slot::preset(7, nothing), Slot { number: 7, name: "(empty)", empty: true });
        }
        assert_eq!(Slot::cab(9, EMPTY_CAB), Slot { number: 9, name: "(empty)", empty: true });
        // Only the unit's own word marks a cab slot as empty.
        assert_eq!(Slot::cab(9, ""), Slot { number: 9, name: "", empty: false });
        assert!(!Slot::cab(9, EMPTY_PRESET).empty);
    }

    /// Slots numbered 10, 20, 30 and so on, as a filtered list would hold them.
    fn tens(count: u16) -> Vec<Slot<'static>> {
        (1..=count).map(|at| Slot::preset(at * 10, "Name")).collect()
    }

    #[test]
    fn arrows_move_the_pick_through_the_slots_listed() {
        // Ten slots in columns of four: 10-40, 50-80, 90-100.
        let slots = tens(10);
        let go = |pick, steps: &[Step]| keyed(&slots, pick, 4, &Keys { steps: steps.to_vec(), enter: false });
        assert_eq!(go(Some(60), &[Step::Down]), (Some(70), None));
        assert_eq!(go(Some(60), &[Step::Left]), (Some(20), None));
        assert_eq!(go(Some(60), &[Step::Right]), (Some(100), None));
        assert_eq!(go(Some(40), &[Step::Down, Step::Down]), (Some(60), None));
        // With no pick, or one that is filtered out, an arrow starts at the top.
        assert_eq!(go(None, &[Step::Down]), (Some(10), None));
        assert_eq!(go(Some(55), &[Step::Up]), (Some(10), None));
        // No key, no change; and an empty list keeps the pick it cannot show.
        assert_eq!(go(Some(55), &[]), (Some(55), None));
        let down = Keys { steps: vec![Step::Down], enter: true };
        assert_eq!(keyed(&[], Some(55), 4, &down), (Some(55), None));
        assert_eq!(keyed(&[], None, 0, &down), (None, None));
    }

    #[test]
    fn enter_asks_for_the_pick_or_picks_the_first_slot_found() {
        let slots = tens(10);
        let enter = |pick, steps: &[Step]| keyed(&slots, pick, 4, &Keys { steps: steps.to_vec(), enter: true });
        assert_eq!(enter(Some(60), &[]), (Some(60), Some(60)));
        // The arrows of the same frame come first.
        assert_eq!(enter(Some(60), &[Step::Up]), (Some(50), Some(50)));
        // Nothing is asked for on the strength of a pick that is not on show.
        assert_eq!(enter(None, &[]), (Some(10), None));
        assert_eq!(enter(Some(55), &[]), (Some(10), None));
    }

    #[test]
    fn a_window_is_new_when_it_was_not_drawn_the_frame_before() {
        assert!(just_opened(None, 0) && just_opened(None, 900));
        assert!(!just_opened(Some(41), 42));
        // Twice in one frame is still the same showing.
        assert!(!just_opened(Some(42), 42));
        assert!(just_opened(Some(40), 42));
        assert!(!just_opened(Some(42), 3));
    }

    #[test]
    fn a_manager_opens_no_larger_than_the_app_window_and_no_smaller_than_its_buttons() {
        let roomy = vec2(1920.0, 1080.0);
        assert_eq!(opening_size(roomy, PRESETS_SIZE, PRESETS_MIN), PRESETS_SIZE);
        let narrow = opening_size(vec2(700.0, 1080.0), CABS_SIZE, CABS_MIN);
        assert_eq!(narrow, vec2(700.0 - WINDOW_MARGIN.x, CABS_SIZE.y));
        for room in [vec2(0.0, 0.0), vec2(200.0, 100.0), vec2(-1.0e9, f32::NEG_INFINITY)] {
            assert_eq!(opening_size(room, PRESETS_SIZE, PRESETS_MIN), PRESETS_MIN);
        }
        // A size that is no size is not taken for one.
        assert_eq!(opening_size(vec2(f32::NAN, f32::NAN), PRESETS_SIZE, PRESETS_MIN), PRESETS_SIZE);
        for (size, least) in [(PRESETS_SIZE, PRESETS_MIN), (CABS_SIZE, CABS_MIN)] {
            assert!(size.x >= least.x && size.y >= least.y);
        }
    }

    #[test]
    fn only_the_columns_in_view_are_drawn() {
        // 1024 user cabs in columns of 20 rows, each 200 wide: 52 columns.
        let grid = picker::Columns { rows: 20, columns: 52, column_width: 200.0 };
        assert_eq!(grid.visible(0.0, 800.0, 1024), 0..80);
        // Scrolled half a column along, a part of a fifth column shows.
        assert_eq!(grid.visible(100.0, 900.0, 1024), 0..100);
        assert_eq!(grid.visible(400.0, 1200.0, 1024), 40..120);
        // The last column is short, and nothing lies past it.
        assert_eq!(grid.visible(10_000.0, 10_800.0, 1024), 1000..1024);
        assert!(grid.visible(20_000.0, 20_800.0, 1024).is_empty());
        for (left, right) in [(f32::NAN, f32::NAN), (-500.0, -100.0), (800.0, 0.0), (0.0, f32::INFINITY)] {
            let range = grid.visible(left, right, 1024);
            assert!(range.start <= range.end && range.end <= 1024, "{left} {right}: {range:?}");
        }
        assert!(grid.visible(0.0, 800.0, 0).is_empty());
        // Every entry in view has its cell inside what the columns take.
        let extent = grid.extent();
        assert_eq!(extent.x, 52.0 * 200.0);
        for position in grid.visible(0.0, extent.x, 1024) {
            let cell = grid.place(position);
            assert!(cell.max.x <= extent.x && cell.max.y <= extent.y, "{position}: {cell:?}");
        }
    }

    #[test]
    fn a_picked_slot_is_filled_and_its_text_reads_on_the_fill() {
        let look = |filled, marked, faint| EntryLook { name: "Lead", filled, marked, faint, ..EntryLook::default() };
        let on_accent = theme::text_on(theme::ACCENT);
        // Picked: the accent fill, whatever else the slot is.
        for (marked, faint) in [(false, false), (true, false), (false, true), (true, true)] {
            for hovered in [false, true] {
                let colors = picker::entry_colors(&look(true, marked, faint), hovered);
                assert_eq!(colors, (theme::ACCENT, on_accent, on_accent));
            }
        }
        // The slot the unit is on is written in the accent, an empty one dimmed.
        let (fill, number, name) = picker::entry_colors(&look(false, true, false), false);
        assert_eq!((fill, number, name), (egui::Color32::TRANSPARENT, theme::ACCENT, theme::ACCENT));
        assert_eq!(picker::entry_colors(&look(false, false, true), false).2, theme::TEXT_DIM);
        assert_eq!(picker::entry_colors(&look(false, false, false), false).2, theme::TEXT);
        // Under the pointer or the keyboard highlight a slot is raised.
        assert_eq!(picker::entry_colors(&look(false, false, false), true).0, theme::RAISED_HOT);
        let lit = EntryLook { lit: true, ..look(false, false, false) };
        assert_eq!(picker::entry_colors(&lit, false).0, theme::RAISED_HOT);
    }

    /// Draws a window built like a manager for a few frames, without a
    /// screen, and returns where its header, its list and its buttons came
    /// to lie in the last one, with the pick the list was left with.
    fn laid_out(slots: &[Slot<'_>], pick: Option<u16>) -> (egui::Rect, egui::Rect, egui::Rect, Option<u16>) {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0));
        let mut seen = None;
        for frame in 0..4 {
            let input = egui::RawInput { screen_rect: Some(screen), ..egui::RawInput::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let window = egui::Window::new("Manager").resizable(true).min_size(PRESETS_MIN);
                window.default_size(PRESETS_SIZE).show(ui.ctx(), |ui| {
                    let header = ui.horizontal(|ui| ui.label("Find")).response.rect;
                    ui.separator();
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        let lower = button_row(ui, |ui| ui.button("Import file…")).response.rect;
                        ui.separator();
                        let upper = button_row(ui, |ui| ui.button("Load")).response.rect;
                        ui.separator();
                        let list = SlotList {
                            salt: "list",
                            slots,
                            digits: PRESET_DIGITS,
                            pick,
                            current: Some(20),
                            reveal: frame < 2,
                            nothing: "Nothing matches.",
                        };
                        let drawn = ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            slot_list(ui, &list, &Keys { steps: vec![Step::Down], enter: false })
                        });
                        seen = Some((header, drawn.response.rect, upper.union(lower), drawn.inner.0));
                    });
                });
            });
            output.textures_delta.clear();
        }
        seen.unwrap_or((egui::Rect::NOTHING, egui::Rect::NOTHING, egui::Rect::NOTHING, None))
    }

    #[test]
    fn the_list_fills_the_room_between_the_header_and_the_buttons() {
        for count in [0, 5, 768, 1024] {
            let slots = tens(count);
            let (header, list, buttons, pick) = laid_out(&slots, Some(30));
            // Nothing overlaps, and the buttons stay at the bottom of the window.
            assert!(header.bottom() <= list.top(), "{count}: {header:?} over {list:?}");
            assert!(list.bottom() <= buttons.top(), "{count}: {list:?} over {buttons:?}");
            // The window keeps the size it opened at: the list takes what is left, no more.
            assert!(buttons.bottom() - header.top() <= PRESETS_SIZE.y, "{count}: {header:?} to {buttons:?}");
            if count > 0 {
                assert!(
                    list.height() > PRESETS_SIZE.y / 2.0 && list.width() > PRESETS_SIZE.x / 2.0,
                    "{count}: {list:?}"
                );
            }
            // The arrow key of each frame was acted on.
            assert_eq!(pick, if count == 0 { Some(30) } else { Some(40) });
        }
    }

    /// The window of a manager, drawn with the app's own style for a number
    /// of frames without a screen, with a row as tall as the real ones.
    fn window_rects(frames: usize) -> Vec<egui::Rect> {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let slots = tens(768);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0));
        let mut rects = Vec::new();
        for _ in 0..frames {
            let input = egui::RawInput { screen_rect: Some(screen), ..egui::RawInput::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let window = egui::Window::new("Manager").resizable(true).min_size(PRESETS_MIN);
                let shown = window.default_size(PRESETS_SIZE).show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| ui.label("Find"));
                    ui.separator();
                    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                        button_row(ui, |ui| ui.button("Import file…"));
                        ui.separator();
                        let mut text = String::new();
                        button_row(ui, |ui| {
                            ui.label("Copy to slot");
                            ui.add(egui::TextEdit::singleline(&mut text).desired_width(40.0));
                            ui.button("Copy")
                        });
                        ui.separator();
                        let list = SlotList {
                            salt: "list",
                            slots: &slots,
                            digits: PRESET_DIGITS,
                            pick: None,
                            current: Some(20),
                            reveal: false,
                            nothing: "Nothing matches.",
                        };
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            slot_list(ui, &list, &Keys { steps: vec![], enter: false })
                        });
                    });
                });
                rects.extend(shown.map(|shown| shown.response.rect));
            });
            output.textures_delta.clear();
        }
        rects
    }

    #[test]
    fn a_manager_keeps_the_size_it_is_given() {
        let rects = window_rects(12);
        assert_eq!(rects.len(), 12);
        // Nothing in the window pushes it larger from one frame to the next.
        assert_eq!(rects.first(), rects.last(), "{rects:?}");
    }

    #[test]
    fn suggested_files_are_safe_names() {
        assert_eq!(suggested_file(7, "Clean / Chorus"), "007 Clean _ Chorus.syx");
        assert_eq!(suggested_file(500, ""), "500.syx");
    }

    #[test]
    fn confirmations_name_the_slot_that_is_overwritten() {
        let (title, body, _) = confirmation("preset.copy", &json!({ "from": 3, "to": 12 }), 0);
        assert_eq!(title, "Copy preset");
        assert!(body.contains("003") && body.contains("slot 012"));
        assert!(confirmation("preset.store", &Value::Null, 41).1.contains("041"));
        assert!(confirmation("preset.store", &json!({ "number": 7 }), 41).1.contains("007"));
        assert!(confirmation("cab.import", &json!({ "cab": 9 }), 0).1.contains("user cab 9"));
    }

    #[test]
    fn a_reset_keeps_the_windows_open() {
        let mut view = LibraryView {
            presets_open: true,
            preset_pick: Some(40),
            scan_retry_at: Some(Instant::now()),
            ..LibraryView::default()
        };
        view.reset();
        assert!(view.presets_open && view.preset_pick.is_none() && view.scan_retry_at.is_none());
    }

    #[test]
    fn names_are_read_in_the_background_until_they_are_verified() {
        let mut view = LibraryView::default();
        // No unit, nothing to read.
        assert!(!view.busy(&NameScan::default()));
        let mut scan = NameScan { from_cache: true, preset_slots: 768, cab_slots: 1213, ..NameScan::default() };
        // Even with every window closed, and with names from last time on show.
        assert!(view.busy(&scan));
        // A unit that did not answer is given a rest.
        view.scan_retry_at = Some(Instant::now() + SCAN_RETRY);
        assert!(!view.busy(&scan));
        view.scan_retry_at = None;
        (scan.presets_checked, scan.cabs_checked) = (768, 1213);
        assert!(!view.busy(&scan));
    }

    #[test]
    fn the_note_tells_saved_names_from_a_first_read() {
        assert_eq!(scan_note(true, 120, 768), "saved names, checking 120 of 768");
        assert_eq!(scan_note(false, 16, 768), "reading names 16 of 768");
    }
}
