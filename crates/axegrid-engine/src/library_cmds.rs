//! The device's library: the names of all preset slots, presets and
//! cabinets as files, and copies between slots.

use std::path::{Path, PathBuf};

use axegrid_device::SessionError;
use axegrid_model::library::{self, CabSlots};
use axegrid_sysex::{Dump, DumpKind, Request};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::file_cmds::read_file;
use crate::Engine;

/// Largest value a two-group number can carry.
const MAX_NUMBER: u64 = 0x3FFF;
/// Names read by one command unless it is told otherwise. The window asks
/// for a slice at a time so that it stays responsive.
const ALL: u64 = MAX_NUMBER + 1;
/// What an unused user-cab slot is called by the device.
pub const EMPTY_CAB: &str = "EMPTY";

fn io_error(path: &Path, error: impl std::fmt::Display) -> CommandError {
    CommandError::Io(format!("{}: {error}", path.display()))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    std::fs::write(path, bytes).map_err(|e| io_error(path, e))
}

/// Reads a `.syx` file that must hold a dump of `kind`.
fn load_dump(path: &Path, kind: DumpKind) -> Result<Dump, CommandError> {
    let dump = Dump::parse(&read_file(path)?).map_err(|e| io_error(path, e))?;
    if dump.kind != kind {
        return Err(io_error(path, format!("holds a {}, not a {}", dump.kind.noun(), kind.noun())));
    }
    Ok(dump)
}

/// A file name that is safe on every system: the slot number, then what is
/// left of the name.
fn file_name(number: u16, name: &str) -> String {
    let kept: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || " -_+()".contains(c) { c } else { '_' }).collect();
    format!("{number:03} {}.syx", kept.trim()).replace(" .syx", ".syx")
}

impl Engine {
    fn preset_slots(&self) -> u16 {
        self.state.model.map_or(0, library::preset_count)
    }

    fn cab_slots(&self) -> Result<CabSlots, CommandError> {
        self.state.model.and_then(library::cab_slots).ok_or_else(|| {
            CommandError::NotFound("the cabinet memory of this model is not known to Axe-Grid yet".into())
        })
    }

    /// A `cab` argument: a user cab numbered from 1.
    fn cab_arg(&self, args: &Args<'_>) -> Result<u16, CommandError> {
        let slots = self.cab_slots()?;
        let cab = args.int("cab", u64::from(slots.user))? as u16;
        if cab == 0 {
            return Err(CommandError::BadParam { name: "cab", problem: "user cabs are numbered from 1".into() });
        }
        Ok(cab)
    }
}

/// The slice `first..first + count` of `total` slots that a command should read.
fn slice(args: &Args<'_>, total: u16) -> Result<std::ops::Range<u16>, CommandError> {
    let first = args.opt_int("first", MAX_NUMBER)?.unwrap_or(0).min(u64::from(total));
    let count = args.opt_int("count", ALL)?.unwrap_or(ALL);
    Ok(first as u16..(first + count).min(u64::from(total)) as u16)
}

fn preset_names(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let total = engine.preset_slots();
    let wanted = slice(args, total)?;
    let mut read = Vec::new();
    for number in wanted {
        match engine.session()?.preset_name_at(number) {
            Ok(name) => {
                read.push(json!({ "number": number, "name": name }));
                engine.state.preset_names.insert(number, name);
            }
            // The device has fewer slots than this model's table says.
            Err(SessionError::Rejected { .. }) => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(json!({ "names": read, "known": engine.state.preset_names.len(), "slots": total }))
}

fn preset_export(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let path = PathBuf::from(args.str("path")?);
    let number = match args.opt_int("number", MAX_NUMBER)? {
        Some(number) => number as u16,
        None => engine.state.preset.number,
    };
    let dump = engine.session()?.read_dump(&Request::PresetDump(number))?;
    let bytes = dump.to_bytes();
    write_file(&path, &bytes)?;
    Ok(json!({ "number": number, "name": dump.name(), "path": path.display().to_string(), "bytes": bytes.len() }))
}

fn preset_export_range(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let folder = PathBuf::from(args.str("folder")?);
    let wanted = slice(args, engine.preset_slots())?;
    let skip_empty = args.opt_bool("skip_empty")?.unwrap_or(true);
    std::fs::create_dir_all(&folder).map_err(|e| io_error(&folder, e))?;
    let mut files = Vec::new();
    for number in wanted {
        let dump = match engine.session()?.read_dump(&Request::PresetDump(number)) {
            Ok(dump) => dump,
            Err(SessionError::Rejected { .. }) => break,
            Err(error) => return Err(error.into()),
        };
        let name = dump.name();
        if skip_empty && name.is_empty() {
            continue;
        }
        let path = folder.join(file_name(number, &name));
        write_file(&path, &dump.to_bytes())?;
        files.push(json!({ "number": number, "name": name, "path": path.display().to_string() }));
    }
    Ok(json!({ "folder": folder.display().to_string(), "written": files.len(), "files": files }))
}

fn preset_import(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let dump = load_dump(Path::new(args.str("path")?), DumpKind::Preset)?;
    let model = engine.session()?.model();
    engine.session()?.send_dump(&dump.frames(model, 0))?;
    engine.refresh_preset()?;
    Ok(json!({ "name": dump.name(), "preset": engine.inspect()["preset"] }))
}

fn preset_copy(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let last = u64::from(engine.preset_slots().saturating_sub(1));
    let (from, to) = (args.int("from", last)? as u16, args.int("to", last)? as u16);
    if from == to {
        return Err(CommandError::BadParam { name: "to", problem: "is the slot the copy comes from".into() });
    }
    // Loading the source fills the edit buffer; storing writes it elsewhere.
    engine.session()?.request(&Request::SelectPreset(from))?;
    engine.session()?.request(&Request::StorePreset(to))?;
    engine.session()?.request(&Request::SelectPreset(to))?;
    engine.refresh_preset()?;
    let name = engine.state.preset.name.clone();
    engine.state.preset_names.insert(to, name.clone());
    Ok(json!({ "from": from, "to": to, "name": name }))
}

fn cab_names(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let slots = engine.cab_slots()?;
    // Slots are numbered from 1; `first` is the first slot wanted.
    let first = args.opt_int("first", u64::from(slots.user))?.unwrap_or(1).max(1) as u16;
    let count = args.opt_int("count", ALL)?.unwrap_or(ALL);
    let end = (u64::from(first) + count).min(u64::from(slots.user) + 1) as u16;
    let mut read = Vec::new();
    for cab in first..end {
        let Some(index) = slots.list_index(cab) else { break };
        let name = engine.session()?.cab_name(index)?;
        read.push(json!({ "cab": cab, "name": name, "empty": name == EMPTY_CAB }));
        engine.state.cab_names.insert(cab, name);
    }
    Ok(json!({ "names": read, "known": engine.state.cab_names.len(), "slots": slots.user }))
}

fn cab_export(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let cab = engine.cab_arg(args)?;
    let path = PathBuf::from(args.str("path")?);
    let dump = engine.session()?.read_dump(&Request::CabDump(cab - 1))?;
    let bytes = dump.to_bytes();
    write_file(&path, &bytes)?;
    Ok(json!({ "cab": cab, "name": dump.name(), "path": path.display().to_string(), "bytes": bytes.len() }))
}

fn cab_import(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let cab = engine.cab_arg(args)?;
    let dump = load_dump(Path::new(args.str("path")?), DumpKind::Cab)?;
    let model = engine.session()?.model();
    engine.session()?.send_dump(&dump.frames(model, cab - 1))?;
    // Read the name back, so the list shows what the device now holds.
    let index = engine.cab_slots()?.list_index(cab);
    let stored = index.map(|index| engine.session()?.cab_name(index).map_err(CommandError::from)).transpose()?;
    if let Some(name) = &stored {
        engine.state.cab_names.insert(cab, name.clone());
    }
    Ok(json!({ "cab": cab, "name": dump.name(), "stored_as": stored }))
}

const FIRST: crate::ParamDoc = opt("first", "integer", "First slot to read; defaults to the first one");
const COUNT: crate::ParamDoc = opt("count", "integer", "How many slots to read; defaults to all that remain");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "preset.names",
        label: "Read Preset Names",
        menu: "",
        shortcut: "",
        doc: "Read the names of stored presets without loading them. Only reads.",
        params: &[FIRST, COUNT],
        enabled: connected,
        run: preset_names,
    },
    CommandSpec {
        id: "preset.export",
        label: "Export Preset",
        menu: "",
        shortcut: "",
        doc: "Save a stored preset as a .syx file. Exports what is stored in the slot, not unsaved edits.",
        params: &[
            req("path", "string", "File to write"),
            opt("number", "integer", "Zero-based preset slot; defaults to the current preset"),
        ],
        enabled: connected,
        run: preset_export,
    },
    CommandSpec {
        id: "preset.export_range",
        label: "Back Up Presets",
        menu: "",
        shortcut: "",
        doc: "Save a run of stored presets into a folder, one .syx file each, named by slot and preset name.",
        params: &[
            req("folder", "string", "Folder to write into; created if missing"),
            FIRST,
            COUNT,
            opt("skip_empty", "boolean", "Leave out unnamed (empty) slots; default true"),
        ],
        enabled: connected,
        run: preset_export_range,
    },
    CommandSpec {
        id: "preset.import",
        label: "Import Preset",
        menu: "",
        shortcut: "",
        doc: "Load a preset .syx file into the edit buffer, replacing unsaved edits. Nothing is stored until preset.store.",
        params: &[req("path", "string", "Preset file to load")],
        enabled: connected,
        run: preset_import,
    },
    CommandSpec {
        id: "preset.copy",
        label: "Copy Preset",
        menu: "",
        shortcut: "",
        doc: "Copy a stored preset to another slot on the device, overwriting what is there, and select the copy. Unsaved edits are lost.",
        params: &[
            req("from", "integer", "Zero-based slot to copy"),
            req("to", "integer", "Zero-based slot to overwrite"),
        ],
        enabled: connected,
        run: preset_copy,
    },
    CommandSpec {
        id: "cab.names",
        label: "Read User Cab Names",
        menu: "",
        shortcut: "",
        doc: "Read the names of the user cabinet slots. Only reads. Models whose cabinet memory is not known are refused.",
        params: &[
            opt("first", "integer", "First user cab to read, counted from 1; defaults to 1"),
            COUNT,
        ],
        enabled: connected,
        run: cab_names,
    },
    CommandSpec {
        id: "cab.export",
        label: "Export User Cab",
        menu: "",
        shortcut: "",
        doc: "Save a user cabinet (impulse response) as a .syx file.",
        params: &[req("cab", "integer", "User cab number, counted from 1"), req("path", "string", "File to write")],
        enabled: connected,
        run: cab_export,
    },
    CommandSpec {
        id: "cab.import",
        label: "Import User Cab",
        menu: "",
        shortcut: "",
        doc: "Write a cabinet .syx file into a user cab slot on the device, overwriting what is there. The slot \
              addressing is taken from the message layout and public documentation and has not been confirmed on \
              hardware.",
        params: &[
            req("cab", "integer", "User cab number to overwrite, counted from 1"),
            req("path", "string", "Cabinet file to send"),
        ],
        enabled: connected,
        run: cab_import,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_survive_any_preset_name() {
        assert_eq!(file_name(7, "Clean / Chorus?"), "007 Clean _ Chorus_.syx");
        assert_eq!(file_name(500, ""), "500.syx");
        assert_eq!(file_name(12, "  Lead (Delay) "), "012 Lead (Delay).syx");
    }
}
