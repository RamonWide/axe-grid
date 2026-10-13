//! The preset and cabinet managers: two windows that list what the unit
//! stores and move presets and cabinets between the unit and files. Like
//! the rest of the shell they act only by running commands.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use axegrid_engine::{CommandError, NameScan};
use axegrid_model::{blocks, library};
use egui::{vec2, Align, Key, Layout, RichText};
use serde_json::{json, Value};

use crate::{theme, Action, Shell};

/// Names asked for at a time, so that an edit made meanwhile does not wait long.
const NAMES_PER_TURN: u16 = 8;
/// How long the reading of names rests after the unit failed to answer.
const SCAN_RETRY: Duration = Duration::from_secs(5);
const ROW_HEIGHT: f32 = 20.0;
const LIST_SIZE: egui::Vec2 = vec2(430.0, 380.0);
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

/// Whether `name` or the slot number matches what was typed in a filter box.
fn matches(filter: &str, number: u16, name: &str) -> bool {
    let filter = filter.trim().to_ascii_lowercase();
    filter.is_empty() || name.to_ascii_lowercase().contains(&filter) || number.to_string().contains(&filter)
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
        egui::Window::new("Presets").open(&mut open).resizable(false).default_pos([60.0, 120.0]).show(ctx, |ui| {
            let state = &view.state;
            let slots = state.model.map_or(0, library::preset_count);
            let current = state.preset.number;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find").small().color(theme::TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut self.library.preset_filter).desired_width(180.0));
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
            ui.add_space(4.0);
            let shown: Vec<(u16, &str)> = state
                .preset_names
                .iter()
                .filter(|(number, name)| matches(&self.library.preset_filter, **number, name))
                .map(|(number, name)| (*number, name.as_str()))
                .collect();
            let mut load = None;
            egui::ScrollArea::vertical().max_height(LIST_SIZE.y).min_scrolled_height(LIST_SIZE.y).show_rows(
                ui,
                ROW_HEIGHT,
                shown.len(),
                |ui, rows| {
                    ui.set_min_width(LIST_SIZE.x);
                    for &(number, name) in shown.get(rows).unwrap_or_default() {
                        let empty = name.is_empty() || name == EMPTY_PRESET;
                        let shown_name = if empty { "(empty)" } else { name };
                        let mut text = RichText::new(format!("{number:03}   {shown_name}")).monospace();
                        let picked = self.library.preset_pick == Some(number);
                        if number == current {
                            text = text.strong();
                        }
                        // A picked row is filled with the accent, so its text takes the colour for that.
                        if picked {
                            text = text.color(theme::text_on(theme::ACCENT));
                        } else if number == current {
                            text = text.color(theme::ACCENT);
                        } else if empty {
                            text = text.color(theme::TEXT_DIM);
                        }
                        let row = ui.selectable_label(picked, text);
                        if row.clicked() {
                            self.library.preset_pick = Some(number);
                        }
                        if row.double_clicked() {
                            load = Some(number);
                        }
                    }
                },
            );
            ui.separator();
            let pick = self.library.preset_pick;
            let pick_name = pick.and_then(|number| state.preset_names.get(&number)).cloned().unwrap_or_default();
            ui.horizontal(|ui| {
                let label =
                    pick.map_or_else(|| "Pick a preset above".to_owned(), |number| format!("Preset {number:03}"));
                ui.label(RichText::new(label).strong());
                ui.add_enabled_ui(pick.is_some(), |ui| {
                    if ui.button("Load").on_hover_text("Select this preset on the unit (or double-click it)").clicked()
                    {
                        load = pick;
                    }
                    let export = ui.button("Export…").on_hover_text("Save the stored preset as a file");
                    if let (true, Some(number)) = (export.clicked(), pick) {
                        if let Some(path) = file_dialog().set_file_name(suggested_file(number, &pick_name)).save_file()
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
            ui.horizontal(|ui| {
                ui.add_enabled_ui(pick.is_some(), |ui| {
                    ui.label("Copy to slot");
                    let field =
                        egui::TextEdit::singleline(&mut self.library.copy_to).desired_width(40.0).hint_text("000");
                    let entered = ui.add(field).lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    let target =
                        self.library.copy_to.trim().parse::<u16>().ok().filter(|&to| to < slots && Some(to) != pick);
                    let copy = ui.add_enabled(target.is_some(), egui::Button::new("Copy"));
                    let hint = "Copy the picked preset over the slot typed here (asks first)";
                    if let (true, Some(from), Some(to)) = (copy.on_hover_text(hint).clicked() || entered, pick, target)
                    {
                        actions.push(("preset.copy", json!({ "from": from, "to": to })));
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
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
            if let Some(number) = load {
                actions.push(("preset.select", json!({ "number": number })));
            }
        });
        self.library.presets_open = open;
    }

    pub(crate) fn cab_manager(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let mut open = self.library.cabs_open;
        let view = self.view.clone();
        egui::Window::new("User cabs").open(&mut open).resizable(false).default_pos([520.0, 120.0]).show(ctx, |ui| {
            let state = &view.state;
            let Some(slots) = state.model.and_then(library::cab_slots) else {
                ui.label("The cabinet memory of this model is not known to Axe-Grid yet.");
                ui.label(RichText::new("A hardware report from an owner would add it.").color(theme::TEXT_DIM));
                return;
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find").small().color(theme::TEXT_DIM));
                ui.add(egui::TextEdit::singleline(&mut self.library.cab_filter).desired_width(160.0));
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
            ui.add_space(4.0);
            let hide_empty = self.library.hide_empty;
            let shown: Vec<(u16, &str)> = state
                .cab_names
                .iter()
                .filter(|(_, name)| !(hide_empty && name.as_str() == EMPTY_CAB))
                .filter(|(cab, name)| matches(&self.library.cab_filter, **cab, name))
                .map(|(cab, name)| (*cab, name.as_str()))
                .collect();
            egui::ScrollArea::vertical().max_height(LIST_SIZE.y).min_scrolled_height(LIST_SIZE.y).show_rows(
                ui,
                ROW_HEIGHT,
                shown.len(),
                |ui, rows| {
                    ui.set_min_width(LIST_SIZE.x);
                    for &(cab, name) in shown.get(rows).unwrap_or_default() {
                        let empty = name == EMPTY_CAB;
                        let mut text =
                            RichText::new(format!("{cab:4}   {}", if empty { "(empty)" } else { name })).monospace();
                        let picked = self.library.cab_pick == Some(cab);
                        // A picked row is filled with the accent, so its text takes the colour for that.
                        if picked {
                            text = text.color(theme::text_on(theme::ACCENT));
                        } else if empty {
                            text = text.color(theme::TEXT_DIM);
                        }
                        if ui.selectable_label(picked, text).clicked() {
                            self.library.cab_pick = Some(cab);
                        }
                    }
                },
            );
            ui.separator();
            let pick = self.library.cab_pick;
            let pick_name = pick.and_then(|cab| state.cab_names.get(&cab)).cloned().unwrap_or_default();
            // The cab block that can audition the picked cab, if the preset has one.
            let placed = state.preset.grid.blocks();
            let cab_block =
                placed.into_iter().find(|&id| blocks::by_id(id).is_some_and(|b| b.effect_type == CAB_EFFECT));
            ui.horizontal(|ui| {
                let label = pick.map_or_else(|| "Pick a cab above".to_owned(), |cab| format!("User cab {cab}"));
                ui.label(RichText::new(label).strong());
                ui.add_enabled_ui(pick.is_some(), |ui| {
                    let filled = pick_name != EMPTY_CAB;
                    let export = ui.add_enabled(filled, egui::Button::new("Export…"));
                    if let (true, Some(cab)) = (export.on_hover_text("Save this cab as a file").clicked(), pick) {
                        if let Some(path) = file_dialog().set_file_name(suggested_file(cab, &pick_name)).save_file() {
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
                    let audition = ui.add_enabled(cab_block.is_some() && filled, egui::Button::new("Use in cab block"));
                    let hint = match cab_block {
                        Some(_) => format!("Choose this cab in {name} of the current preset, to hear it"),
                        None => "The current preset has no cab block".to_owned(),
                    };
                    let chosen = audition.on_hover_text(&hint).on_disabled_hover_text(hint).clicked();
                    if let (true, Some(cab), Some(block)) = (chosen, pick, cab_block) {
                        if let Some(index) = slots.list_index(cab) {
                            actions.push(("param.set", json!({ "block": block, "param": CAB_CHOICE, "value": index })));
                        }
                    }
                });
            });
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let note = "Importing writes to the unit's memory. It follows the documented message layout but has \
                            not yet been confirmed on a unit: keep a backup of the slot you overwrite.";
                ui.label(RichText::new(note).small().color(theme::TEXT_DIM));
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
