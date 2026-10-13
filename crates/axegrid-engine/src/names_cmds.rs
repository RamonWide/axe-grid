//! Preset and cab names without the wait: what was read last time is shown
//! the moment a unit connects, and the unit is then read again a little at
//! a time to correct whatever has changed since.
//!
//! The file is `name_cache.rs`. Everything here only reads the unit, with
//! the two requests the name commands already use (`15` for a preset name,
//! `12` for a cab name), and only for slots inside the tables of
//! `axegrid_model::library`: a unit asked about a slot it does not have may
//! stop answering. The device has no message that says "a stored name has
//! changed", so the check is a plain re-read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axegrid_device::SessionError;
use axegrid_model::library;
use serde_json::{json, Value};

use crate::command::{connected, opt, Args, CommandError, CommandSpec};
use crate::name_cache::{self, Limits, Unit};
use crate::Engine;

/// Largest number of requests one command may be limited to.
const MAX_BUDGET: u64 = 100_000;
/// How long names that were read may wait before they are written to the file.
const SAVE_AFTER: Duration = Duration::from_secs(5);
/// Commands that change what the unit stores under a name; the file follows at once.
const WRITERS: [&str; 3] = ["preset.store", "preset.copy", "cab.import"];

/// How far the names on show have been checked against the unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NameScan {
    /// The names were taken from the file on this computer when the unit connected.
    pub from_cache: bool,
    /// Preset slots read from the unit since connecting, counted from slot 0.
    pub presets_checked: u16,
    /// Preset slots there are to read.
    pub preset_slots: u16,
    /// Positions of the cab list read from the unit, counted from position 0.
    pub cabs_checked: u16,
    /// Positions of the cab list there are to read: the factory cabs, then
    /// the user cabs. 0 for a model whose cab memory is not known.
    pub cab_slots: u16,
    /// Names that turned out to differ from the ones that were on show.
    pub changed: u32,
}

impl NameScan {
    pub fn presets_done(&self) -> bool {
        self.presets_checked >= self.preset_slots
    }

    pub fn cabs_done(&self) -> bool {
        self.cabs_checked >= self.cab_slots
    }

    /// Whether every name on show has been read from the unit that is connected.
    pub fn verified(&self) -> bool {
        self.total() > 0 && self.presets_done() && self.cabs_done()
    }

    pub fn checked(&self) -> u32 {
        u32::from(self.presets_checked) + u32::from(self.cabs_checked)
    }

    pub fn total(&self) -> u32 {
        u32::from(self.preset_slots) + u32::from(self.cab_slots)
    }

    /// `"verified"`, `"cached"` (names from the file, still being checked),
    /// `"reading"` (no file; names appear as they are read) or `"none"`
    /// (no unit).
    pub fn phase(&self) -> &'static str {
        match (self.total() > 0, self.verified(), self.from_cache) {
            (false, ..) => "none",
            (_, true, _) => "verified",
            (_, _, true) => "cached",
            _ => "reading",
        }
    }
}

/// The engine's side of the name file.
#[derive(Debug, Default)]
pub(crate) struct Names {
    /// Where the files are kept; nowhere when the engine was given no folder.
    folder: Option<PathBuf>,
    /// The unit whose names are in the state.
    unit: Option<Unit>,
    /// The state holds names the file does not.
    dirty: bool,
    saved_at: Option<Instant>,
    /// Where the connected unit refused a slot, if it did: its lists end
    /// there, and nothing asks about that slot or a later one again.
    refused: [Option<u16>; 2],
}

impl Names {
    pub(crate) fn kept_in(data_dir: &Path) -> Self {
        Self { folder: Some(data_dir.join(name_cache::FOLDER)), ..Self::default() }
    }
}

/// The two lists that are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Presets,
    Cabs,
}

impl Engine {
    /// How many slots of each list this model has, by the tables the name
    /// commands go by.
    fn name_limits(&self) -> Limits {
        let cabs = self.state.model.and_then(library::cab_slots);
        Limits {
            presets: self.state.model.map_or(0, library::preset_count),
            cabs: cabs.map_or(0, |slots| slots.factory.saturating_add(slots.user)),
        }
    }

    /// Takes up the names kept for the unit that has just connected, so
    /// that the lists are there before a single name has been read.
    pub(crate) fn load_name_cache(&mut self) {
        let Some(model) = self.state.model else { return };
        let unit = Unit { model: model.byte(), firmware: self.state.firmware, simulated: self.state.simulated };
        let limits = self.name_limits();
        self.names.unit = Some(unit);
        self.names.dirty = false;
        self.names.saved_at = None;
        self.names.refused = [None; 2];
        self.state.name_scan = NameScan { preset_slots: limits.presets, cab_slots: limits.cabs, ..NameScan::default() };
        let cached = self.names.folder.as_ref().and_then(|folder| name_cache::load(folder, &unit, limits));
        if let Some(cached) = cached {
            self.state.preset_names = cached.presets;
            self.note_cached_cabs(cached.cabs);
            self.state.name_scan.from_cache = true;
        }
    }

    /// Marks the name file as behind the state.
    pub(crate) fn names_changed(&mut self) {
        self.names.dirty = true;
    }

    /// Files the name of a stored preset. Returns whether it is news.
    pub(crate) fn note_preset_name(&mut self, number: u16, name: String) -> bool {
        if self.state.preset_names.get(&number) == Some(&name) {
            return false;
        }
        self.state.preset_names.insert(number, name);
        self.names.dirty = true;
        true
    }

    /// Writes the names to the file if it is behind. A file that cannot be
    /// written is not worth failing a command over: the names are a
    /// convenience and the unit has them all.
    pub(crate) fn save_name_cache(&mut self) {
        if !std::mem::take(&mut self.names.dirty) {
            return;
        }
        self.names.saved_at = Some(Instant::now());
        // Without a unit there are no names; an empty state must not empty the file.
        if self.session.is_none() || self.state.model.is_none() {
            return;
        }
        let (Some(folder), Some(unit)) = (&self.names.folder, &self.names.unit) else { return };
        let limits = self.name_limits();
        let within = |names: &BTreeMap<u16, String>, limit: u16| -> BTreeMap<u16, String> {
            names.range(..limit).map(|(slot, name)| (*slot, name.clone())).collect()
        };
        let presets = within(&self.state.preset_names, limits.presets);
        let cabs = within(self.cab_list(), limits.cabs);
        let _ = name_cache::save(folder, unit, &presets, &cabs);
    }

    /// Runs after every command: keeps the file close behind what the
    /// commands learn, and level with what they write to the unit.
    pub(crate) fn names_after(&mut self, id: &str) {
        if !self.names.dirty {
            return;
        }
        let due = self.names.saved_at.is_none_or(|at| at.elapsed() >= SAVE_AFTER);
        if due || WRITERS.contains(&id) {
            self.save_name_cache();
        }
    }

    fn scan_done(&self, list: List) -> bool {
        match list {
            List::Presets => self.state.name_scan.presets_done(),
            List::Cabs => self.state.name_scan.cabs_done(),
        }
    }

    /// Reads the next name of `list` from the unit and corrects the state.
    fn scan_one(&mut self, list: List, changed: &mut Vec<Value>) -> Result<(), CommandError> {
        let scan = self.state.name_scan;
        match list {
            List::Presets => {
                let number = scan.presets_checked;
                match self.session()?.preset_name_at(number) {
                    Ok(name) => {
                        let was = self.state.preset_names.get(&number).cloned();
                        if self.note_preset_name(number, name.clone()) {
                            if let Some(was) = was {
                                changed.push(json!({ "list": "presets", "slot": number, "was": was, "name": name }));
                            }
                        }
                        self.state.name_scan.presets_checked = number.saturating_add(1);
                    }
                    // The device has fewer slots than this model's table says: the list ends here.
                    Err(SessionError::Rejected { .. }) => {
                        self.names.refused[0] = Some(number);
                        self.state.name_scan.preset_slots = number;
                        if !self.state.preset_names.split_off(&number).is_empty() {
                            self.names.dirty = true;
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            List::Cabs => {
                let at = scan.cabs_checked;
                match self.session()?.cab_name(at) {
                    Ok(name) => {
                        let was = self.cab_list().get(&at).cloned();
                        if was.as_ref() != Some(&name) {
                            if let Some(was) = was {
                                changed.push(json!({ "list": "cabs", "slot": at, "was": was, "name": name }));
                            }
                            self.note_cab(at, name);
                        }
                        self.state.name_scan.cabs_checked = at.saturating_add(1);
                    }
                    // Never ask past a position the device refused.
                    Err(SessionError::Rejected { .. }) => {
                        self.names.refused[1] = Some(at);
                        self.state.name_scan.cab_slots = at;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        Ok(())
    }

    /// Reads up to `budget` names that have not been checked yet.
    fn scan_names(&mut self, mut budget: u64, cabs_first: bool) -> Result<Value, CommandError> {
        let order = if cabs_first { [List::Cabs, List::Presets] } else { [List::Presets, List::Cabs] };
        let before = self.state.name_scan.checked();
        let mut changed = Vec::new();
        let mut outcome = Ok(());
        'lists: for list in order {
            while budget > 0 && !self.scan_done(list) {
                budget -= 1;
                outcome = self.scan_one(list, &mut changed);
                if outcome.is_err() {
                    break 'lists;
                }
            }
        }
        let corrected = self.state.name_scan.changed.saturating_add(changed.len() as u32);
        self.state.name_scan.changed = corrected;
        if self.state.name_scan.verified() {
            // The whole of it has been read: the file is as good as it gets.
            self.save_name_cache();
        }
        outcome?;
        let mut result = self.name_scan_json();
        result["read"] = json!(self.state.name_scan.checked().saturating_sub(before));
        result["corrected"] = json!(changed);
        Ok(result)
    }

    /// Where the name file of the connected unit is, if one is kept.
    fn name_cache_file(&self) -> Option<PathBuf> {
        Some(name_cache::path(self.names.folder.as_ref()?, self.names.unit.as_ref()?))
    }

    /// How far the names have been checked, for `state.inspect`.
    pub(crate) fn name_scan_json(&self) -> Value {
        let scan = &self.state.name_scan;
        json!({
            "phase": scan.phase(),
            "from_cache": scan.from_cache,
            "verified": scan.verified(),
            "checked": scan.checked(),
            "total": scan.total(),
            "changed": scan.changed,
            "presets": { "checked": scan.presets_checked, "slots": scan.preset_slots, "done": scan.presets_done() },
            "cabs": { "checked": scan.cabs_checked, "slots": scan.cab_slots, "done": scan.cabs_done() },
            "cache_file": self.name_cache_file().map(|file| file.display().to_string()),
        })
    }
}

/// The `budget` and `first` parameters of the two commands.
fn scan_args(args: &Args<'_>) -> Result<(u64, bool), CommandError> {
    let budget = args.opt_int("budget", MAX_BUDGET)?.unwrap_or(MAX_BUDGET);
    let cabs_first = match args.opt_str("first")? {
        None | Some("presets") => false,
        Some("cabs") => true,
        Some(other) => {
            return Err(CommandError::BadParam {
                name: "first",
                problem: format!("{other:?} is not one of \"presets\", \"cabs\""),
            })
        }
    };
    Ok((budget, cabs_first))
}

fn scan(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (budget, cabs_first) = scan_args(args)?;
    engine.scan_names(budget, cabs_first)
}

fn rescan(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (budget, cabs_first) = scan_args(args)?;
    let limits = engine.name_limits();
    let [presets_end, cabs_end] = engine.names.refused;
    // The names stay on show while they are read again.
    let scan = &mut engine.state.name_scan;
    (scan.presets_checked, scan.cabs_checked, scan.changed) = (0, 0, 0);
    scan.preset_slots = presets_end.map_or(limits.presets, |end| end.min(limits.presets));
    scan.cab_slots = cabs_end.map_or(limits.cabs, |end| end.min(limits.cabs));
    engine.scan_names(budget, cabs_first)
}

const BUDGET: crate::ParamDoc = opt(
    "budget",
    "integer",
    "Most names to read in this call, one request each; 0 reads none. Without it everything that is left is read",
);
const FIRST: crate::ParamDoc =
    opt("first", "string", "Which list to go on with first: \"presets\" (default) or \"cabs\"");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "library.scan",
        label: "Check Library Names",
        menu: "",
        shortcut: "",
        doc: "Go on checking the names of stored presets and cabs against the device, from where the last call \
              stopped, and correct the ones that differ. The names shown right after connecting are the ones kept \
              on this computer from last time; this is what makes them certain. \"phase\" in the result is \
              \"cached\" or \"reading\" until every slot has been read, then \"verified\"; a call after that reads \
              nothing. Only reads, and only slots the model has.",
        params: &[BUDGET, FIRST],
        enabled: connected,
        run: scan,
    },
    CommandSpec {
        id: "library.rescan",
        label: "Read Library Names Again",
        menu: "",
        shortcut: "",
        doc: "Start the check of preset and cab names over from the first slot, for when presets or cabs were \
              changed on the device itself or by another program. The names stay on show meanwhile. With a budget \
              the rest is left to library.scan. Only reads.",
        params: &[BUDGET, FIRST],
        enabled: connected,
        run: rescan,
    },
];
