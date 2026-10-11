//! Undo and redo. The device holds the truth, so an edit is remembered as
//! what it changed there, and undoing it sends the old value back. The
//! history belongs to the edit buffer: it is dropped when another preset is
//! loaded or the device goes away.

use std::time::{Duration, Instant};

use axegrid_model::defs::RAW_MAX;
use axegrid_model::{blocks, grid, params, Grid};
use axegrid_sysex::{ParamAddress, ParamValue, Request, XyState};
use serde_json::{json, Value};

use crate::command::{connected, Args, CommandError, CommandSpec};
use crate::Engine;

/// Edits kept; the oldest is dropped when one more arrives.
const MAX_EDITS: usize = 200;
/// Changes to one parameter that follow each other this closely are one
/// edit: a knob being turned, not a series of settings.
const MERGE_WINDOW: Duration = Duration::from_millis(600);

/// What an edit changed on the device.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Edit {
    Param {
        address: ParamAddress,
        before: u16,
        after: u16,
        /// The parameter set the block was on, where it has two.
        xy: Option<XyState>,
    },
    Xy {
        block: u16,
        before: XyState,
        after: XyState,
    },
    Grid {
        before: Box<Grid>,
        after: Box<Grid>,
    },
    Rename {
        before: String,
        after: String,
    },
}

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    /// The scene the edit was made in: bypass and X/Y belong to a scene.
    scene: u8,
    at: Instant,
    edit: Edit,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    done: Vec<Entry>,
    undone: Vec<Entry>,
}

impl History {
    pub(crate) fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    pub(crate) fn next_undo(&self) -> Option<&str> {
        self.done.last().map(|entry| entry.label.as_str())
    }

    pub(crate) fn next_redo(&self) -> Option<&str> {
        self.undone.last().map(|entry| entry.label.as_str())
    }

    fn push(&mut self, entry: Entry) {
        // A new edit starts a new future.
        self.undone.clear();
        if let (Some(last), Edit::Param { address, after, xy, .. }) = (self.done.last_mut(), &entry.edit) {
            let recent = entry.at.duration_since(last.at) < MERGE_WINDOW && last.scene == entry.scene;
            if let (true, Edit::Param { address: same, after: last_after, xy: same_xy, before }) =
                (recent, &mut last.edit)
            {
                if same == address && same_xy == xy {
                    *last_after = *after;
                    last.at = entry.at;
                    if before == last_after {
                        // Turned back to where it started: nothing to undo.
                        self.done.pop();
                    }
                    return;
                }
            }
        }
        self.done.push(entry);
        if self.done.len() > MAX_EDITS {
            self.done.remove(0);
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        let labels = |entries: &[Entry]| entries.iter().rev().map(|e| e.label.clone()).collect::<Vec<_>>();
        json!({ "undo": labels(&self.done), "redo": labels(&self.undone) })
    }
}

/// What a parameter edit is called in the Edit menu.
fn param_label(address: ParamAddress, after: u16) -> String {
    let block = blocks::label(address.block);
    if address.param == params::SHARED_BYPASS {
        return format!("{} {block}", if after == 0 { "Engage" } else { "Bypass" });
    }
    let name = blocks::by_id(address.block).map(|b| params::label(b.effect_type, address.param)).unwrap_or_default();
    format!("{block} {name}").trim_end().to_owned()
}

impl Engine {
    fn record(&mut self, label: String, edit: Edit) {
        let entry = Entry { label, scene: self.state.preset.scene, at: Instant::now(), edit };
        self.history.push(entry);
    }

    /// The raw value a parameter has before it is edited: what the editor
    /// already shows, or else what the device says.
    pub(crate) fn value_before(&mut self, address: ParamAddress) -> Result<u32, CommandError> {
        match self.state.params.iter().find(|p| p.address == address) {
            Some(shown) => Ok(shown.value),
            None => Ok(self.session()?.get_param(address)?.value),
        }
    }

    /// Remembers that a parameter went from `before` to what the device applied.
    pub(crate) fn record_param(&mut self, before: u32, applied: &ParamValue) {
        let limit = |raw: u32| raw.min(u32::from(RAW_MAX)) as u16;
        let (before, after) = (limit(before), limit(applied.value));
        if before == after {
            return;
        }
        let address = applied.address;
        let xy = self.state.preset.states.iter().find(|s| s.block == address.block).map(|s| s.xy);
        self.record(param_label(address, after), Edit::Param { address, before, after, xy });
    }

    pub(crate) fn record_xy(&mut self, block: u16, before: XyState, after: XyState) {
        if before != after {
            let label = format!("{} to {}", blocks::label(block), if after == XyState::X { "X" } else { "Y" });
            self.record(label, Edit::Xy { block, before, after });
        }
    }

    pub(crate) fn record_rename(&mut self, before: String) {
        let after = self.state.preset.name.clone();
        if before != after {
            self.record("Rename Preset".into(), Edit::Rename { before, after });
        }
    }

    /// Runs a command body that edits the grid and remembers what it changed.
    pub(crate) fn recording_grid(
        &mut self,
        label: impl FnOnce() -> String,
        run: impl FnOnce(&mut Engine) -> Result<Value, CommandError>,
    ) -> Result<Value, CommandError> {
        let before = self.state.preset.grid.clone();
        let result = run(self);
        if matches!(result, Err(CommandError::Device(axegrid_device::SessionError::Rejected { .. }))) {
            // Some of the edit may have gone through before the refusal.
            let _ = self.refresh_preset();
        }
        if self.state.preset.grid != before {
            let after = self.state.preset.grid.clone();
            self.record(label(), Edit::Grid { before: Box::new(before), after: Box::new(after) });
        }
        result
    }

    /// Makes the scene, and if need be the block's X/Y state, what they
    /// were when an edit was made, so that undoing it changes what it changed.
    fn return_to(
        &mut self,
        scene: u8,
        block: u16,
        xy: Option<XyState>,
        scene_matters: bool,
    ) -> Result<(), CommandError> {
        let on = |engine: &Engine| engine.state.preset.states.iter().find(|s| s.block == block).map(|s| s.xy);
        let other_set = xy.is_some() && on(self).is_some() && on(self) != xy;
        if (scene_matters || other_set) && self.state.preset.scene != scene {
            self.go_to_scene(scene)?;
        }
        if let Some(wanted) = xy.filter(|_| on(self).is_some() && on(self) != xy) {
            self.session()?.request(&Request::BlockXy(block, Some(wanted)))?;
            self.refresh_preset()?;
        }
        Ok(())
    }

    /// Brings the device's grid to `target` by placing blocks and drawing
    /// cables. No shunt ID is ever in two cells at once.
    fn rebuild_grid(&mut self, target: &Grid) -> Result<(), CommandError> {
        let cells = || (0..grid::COLUMNS).flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)));
        let at = |column: usize, row: usize| column * grid::ROWS + row;
        let place = |engine: &mut Engine, block: u16, column: usize, row: usize| {
            let cell = at(column, row) as u8;
            engine.session()?.request(&Request::PlaceBlock { block, cell }).map(|_| ()).map_err(CommandError::from)
        };
        let mut now: Vec<u16> = cells().map(|(column, row)| self.state.preset.grid.cell(column, row).block).collect();
        let wanted: Vec<u16> = cells().map(|(column, row)| target.cell(column, row).block).collect();
        // Lift what is in the way and has no place in the target. A block
        // that only moves is left alone: placing it again moves it.
        for (column, row) in cells() {
            let (have, want) = (now[at(column, row)], wanted[at(column, row)]);
            let moves = !blocks::is_shunt(have) && wanted.contains(&have);
            if have != want && have != blocks::EMPTY && !moves {
                place(self, blocks::EMPTY, column, row)?;
                now[at(column, row)] = blocks::EMPTY;
            }
        }
        for (column, row) in cells() {
            let want = wanted[at(column, row)];
            if want == blocks::EMPTY || now[at(column, row)] == want {
                continue;
            }
            place(self, want, column, row)?;
            now.iter_mut().filter(|block| **block == want).for_each(|block| *block = blocks::EMPTY);
            now[at(column, row)] = want;
        }
        // The device drops cables as cells empty, so read what is left of
        // them before drawing the rest.
        self.refresh_preset()?;
        for (column, row) in cells().filter(|&(column, _)| column > 0) {
            for from in 0..grid::ROWS {
                let connect = target.is_fed_by(column, row, from);
                if self.state.preset.grid.is_fed_by(column, row, from) != connect {
                    let cable = Request::Cable { from: at(column - 1, from) as u8, to: at(column, row) as u8, connect };
                    self.session()?.request(&cable)?;
                }
            }
        }
        self.refresh_preset()
    }

    /// Sends the device back to before an edit (`undo`) or forward to after it.
    fn restore(&mut self, entry: &Entry, undo: bool) -> Result<(), CommandError> {
        match &entry.edit {
            Edit::Param { address, before, after, xy } => {
                let bypass = address.param == params::SHARED_BYPASS;
                self.return_to(entry.scene, address.block, *xy, bypass)?;
                let value = self.session()?.set_param(*address, if undo { *before } else { *after })?;
                self.remember(&value);
                if bypass {
                    self.refresh_preset()?;
                }
                if crate::live_cmds::tempo_address().is_ok_and(|tempo| tempo == *address) {
                    self.state.tempo = u16::try_from(value.value).ok();
                }
            }
            Edit::Xy { block, before, after } => {
                self.return_to(entry.scene, *block, None, true)?;
                let state = if undo { *before } else { *after };
                self.session()?.request(&Request::BlockXy(*block, Some(state)))?;
                self.refresh_preset()?;
            }
            Edit::Grid { before, after } => self.rebuild_grid(if undo { before } else { after })?,
            Edit::Rename { before, after } => {
                let name = if undo { before } else { after };
                self.session()?.request(&Request::SetPresetName(name.clone()))?;
                self.refresh_preset()?;
            }
        }
        Ok(())
    }

    fn step(&mut self, undo: bool) -> Result<Value, CommandError> {
        let taken = if undo { self.history.done.pop() } else { self.history.undone.pop() };
        let Some(entry) = taken else { return Ok(Value::Null) };
        let outcome = self.restore(&entry, undo);
        let label = entry.label.clone();
        // A step that failed stays where it was, to be tried again.
        if outcome.is_err() == undo {
            self.history.done.push(entry);
        } else {
            self.history.undone.push(entry);
        }
        outcome?;
        let mut result = self.history_summary();
        result[if undo { "undone" } else { "redone" }] = json!(label);
        Ok(result)
    }

    pub(crate) fn history_summary(&self) -> Value {
        json!({ "can_undo": self.history.done.len(), "can_redo": self.history.undone.len(),
                "next_undo": self.history.next_undo(), "next_redo": self.history.next_redo() })
    }
}

fn can_undo(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.history.next_undo().map(|_| ()).ok_or("nothing to undo")
}

fn can_redo(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.history.next_redo().map(|_| ()).ok_or("nothing to redo")
}

fn undo(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.step(true)
}

fn redo(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.step(false)
}

fn list(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    Ok(engine.history.to_json())
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.undo",
        label: "Undo",
        menu: "Edit/Undo",
        shortcut: "Ctrl+Z",
        doc: "Take back the last edit to a parameter, bypass, X/Y state, the grid or the preset name, by \
              sending the device what was there before. An edit made in another scene or on a block's \
              other X/Y set switches back to it first. Loading a preset empties the history.",
        params: &[],
        enabled: can_undo,
        run: undo,
    },
    CommandSpec {
        id: "edit.redo",
        label: "Redo",
        menu: "Edit/Redo",
        shortcut: "Ctrl+Y",
        doc: "Make the edit that was last undone again.",
        params: &[],
        enabled: can_redo,
        run: redo,
    },
    CommandSpec {
        id: "edit.history",
        label: "List Edit History",
        menu: "",
        shortcut: "",
        doc: "The edits that can be undone and redone, the next one first.",
        params: &[],
        enabled: connected,
        run: list,
    },
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn param(param: u16, before: u16, after: u16, at: Instant) -> Entry {
        let address = ParamAddress { block: 106, param };
        Entry { label: format!("p{param}"), scene: 0, at, edit: Edit::Param { address, before, after, xy: None } }
    }

    #[test]
    fn a_turn_of_a_knob_is_one_edit() {
        let start = Instant::now();
        let mut history = History::default();
        history.push(param(1, 10, 20, start));
        history.push(param(1, 20, 30, start + Duration::from_millis(100)));
        history.push(param(1, 30, 40, start + Duration::from_millis(400)));
        assert_eq!(history.done.len(), 1);
        assert!(matches!(history.done[0].edit, Edit::Param { before: 10, after: 40, .. }));
        // Another parameter, or the same one after a pause, is an edit of its own.
        history.push(param(2, 5, 6, start + Duration::from_millis(450)));
        history.push(param(2, 6, 7, start + Duration::from_secs(3)));
        assert_eq!(history.done.len(), 3);
        // Back where it started within one turn: nothing happened.
        history.push(param(2, 7, 6, start + Duration::from_millis(3100)));
        assert_eq!(history.done.len(), 2);
    }

    #[test]
    fn a_new_edit_drops_what_was_undone_and_the_oldest_falls_off() {
        let start = Instant::now();
        let mut history = History::default();
        history.undone.push(param(9, 1, 2, start));
        history.push(param(1, 1, 2, start));
        assert!(history.next_redo().is_none());
        for n in 0..MAX_EDITS as u16 + 5 {
            history.push(param(100 + n, 0, 1, start));
        }
        assert_eq!(history.done.len(), MAX_EDITS);
        assert_eq!(history.next_undo(), Some("p304"));
        assert_eq!(history.to_json()["undo"][0], "p304");
    }

    /// An edit whose block has since been switched to its other parameter
    /// set, as the unit's front panel can do behind the editor's back.
    #[test]
    fn undoing_returns_to_the_parameter_set_and_scene_of_the_edit() {
        let mut engine = Engine::new();
        engine.run("device.connect_simulator", &Value::Null).unwrap();
        engine.run("scene.select", &json!({ "scene": 4 })).unwrap();
        let address = ParamAddress { block: 106, param: 1 };
        let edit = Edit::Param { address, before: 111, after: 222, xy: Some(XyState::Y) };
        engine.history.done.push(Entry { label: "Amp 1 Drive".into(), scene: 1, at: Instant::now(), edit });

        assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Amp 1 Drive");
        let amp = engine.state.preset.states.iter().find(|s| s.block == 106).unwrap();
        assert_eq!((engine.state.preset.scene, amp.xy), (1, XyState::Y));
        assert_eq!(engine.session().unwrap().get_param(address).unwrap().value, 111);
        // A block without two sets, such as the output mixer, needs no switching.
        let output = ParamAddress { block: blocks::OUTPUT, param: 0 };
        let edit = Edit::Param { address: output, before: 5, after: 6, xy: None };
        engine.history.done.push(Entry { label: "Output".into(), scene: 6, at: Instant::now(), edit });
        engine.run("edit.undo", &Value::Null).unwrap();
        assert_eq!(engine.state.preset.scene, 1);
        assert_eq!(engine.run("edit.redo", &Value::Null).unwrap()["can_undo"], 1);
        assert_eq!(engine.session().unwrap().get_param(output).unwrap().value, 6);
    }

    #[test]
    fn edits_are_named_for_the_menu() {
        assert_eq!(param_label(ParamAddress { block: 106, param: 1 }, 5), "Amp 1 Drive");
        assert_eq!(param_label(ParamAddress { block: 106, param: params::SHARED_BYPASS }, 1), "Bypass Amp 1");
        assert_eq!(param_label(ParamAddress { block: 106, param: params::SHARED_BYPASS }, 0), "Engage Amp 1");
    }
}
