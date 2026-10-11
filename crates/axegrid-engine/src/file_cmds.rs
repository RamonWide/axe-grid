//! Definitions, catalogs, files and introspection. None of these need a
//! device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axegrid_model::{blocks, params, Definitions};
use axegrid_sysex::{split_messages, Dump, Frame};
use serde_json::{json, Value};

use crate::command::{always, opt, req, Args, CommandError, CommandSpec};
use crate::Engine;

/// Refuse to slurp files that cannot be what the command expects.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, CommandError> {
    let describe = |e: std::io::Error| CommandError::Io(format!("{}: {e}", path.display()));
    let size = std::fs::metadata(path).map_err(describe)?.len();
    if size > MAX_FILE_BYTES {
        return Err(CommandError::Io(format!("{} is larger than 64 MiB", path.display())));
    }
    std::fs::read(path).map_err(describe)
}

fn load_definitions(engine: &mut Engine, path: &Path) -> Result<Value, CommandError> {
    let defs =
        Definitions::parse(&read_file(path)?).map_err(|e| CommandError::Io(format!("{}: {e}", path.display())))?;
    let summary = json!({
        "path": path.display().to_string(),
        "effects": defs.effects.len(),
        "parameters": defs.effects.iter().map(|e| e.params.len()).sum::<usize>(),
    });
    engine.defs = Some(defs);
    Ok(summary)
}

fn defs_load(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    load_definitions(engine, Path::new(args.str("path")?))
}

/// Where the original editor keeps its per-model definition caches.
fn installed_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support")))?;
    Some(base.join("Fractal Audio").join("Axe-Edit"))
}

fn defs_load_installed(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let missing = || CommandError::NotFound("no Axe-Edit definition cache found on this computer".into());
    let dir = installed_cache_dir().ok_or_else(missing)?;
    // Prefer the cache of the connected model; otherwise take any.
    let wanted = engine.state.model.map(|m| format!("effectDefinitions_{:02}.cache", m.byte()));
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|_| missing())?
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cache"))
        .collect();
    found.sort();
    let pick = found
        .iter()
        .find(|p| wanted.as_deref().is_some_and(|w| p.file_name().is_some_and(|n| n == w)))
        .or(found.first())
        .ok_or_else(missing)?;
    load_definitions(engine, pick)
}

fn defs_describe(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let info = blocks::by_id(block).ok_or_else(|| CommandError::NotFound(format!("unknown block {block}")))?;
    let effect = engine
        .defs
        .as_ref()
        .and_then(|d| d.effect(info.effect_type.into()))
        .ok_or_else(|| CommandError::NotFound("no definitions loaded; run definitions.load_installed".into()))?;
    let items: Vec<Value> = effect
        .params
        .iter()
        .map(|p| {
            json!({
                "param": p.id, "name": params::name(info.effect_type, p.id),
                "min": p.min, "max": p.max, "scale": p.scale, "step": p.step,
                "choices": if p.is_enum() { Some(&p.strings) } else { None },
            })
        })
        .collect();
    Ok(json!({ "block": block, "effect_type": info.effect_type, "params": items }))
}

fn catalog_blocks(_: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    Ok(blocks::all().iter().map(|b| json!({ "block": b.id, "name": b.name, "effect_type": b.effect_type })).collect())
}

fn catalog_params(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let info = blocks::by_id(block).ok_or_else(|| CommandError::NotFound(format!("unknown block {block}")))?;
    Ok(params::all(info.effect_type)
        .map(|(id, name)| json!({ "param": id, "name": name, "label": params::label(info.effect_type, id) }))
        .collect())
}

fn sysex_inspect(_: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let path = Path::new(args.str("path")?);
    let data = read_file(path)?;
    let mut by_kind: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let (mut total, mut bad) = (0usize, 0usize);
    for message in split_messages(&data) {
        total += 1;
        match Frame::parse(message) {
            Ok(frame) => *by_kind.entry((frame.model, frame.function)).or_default() += 1,
            Err(_) => bad += 1,
        }
    }
    let kind = match by_kind.keys().map(|k| k.1).collect::<Vec<_>>().as_slice() {
        [0x77, 0x78, 0x79] => "preset",
        [0x7A, 0x7B, 0x7C] => "cabinet IR",
        [] => "not a Fractal SysEx file",
        _ => "other",
    };
    let messages: Vec<Value> = by_kind
        .iter()
        .map(|((model, function), count)| {
            json!({ "model": format!("{model:#04x}"), "function": format!("{function:#04x}"), "count": count })
        })
        .collect();
    // A complete dump also tells its name and whether its checksum holds.
    let dump = Dump::parse(&data);
    let name = dump.as_ref().ok().map(Dump::name);
    let problem = dump.err().filter(|_| kind != "other").map(|e| e.to_string());
    Ok(json!({ "bytes": data.len(), "messages": total, "invalid": bad, "kind": kind, "name": name,
               "problem": problem, "by_function": messages }))
}

fn inspect(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    Ok(engine.inspect())
}

fn commands(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    Ok(crate::command_list(engine))
}

const BLOCK: crate::ParamDoc = opt("block", "integer|string", "Block ID or name; defaults to the selected block");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "definitions.load",
        label: "Load Definitions File",
        menu: "",
        shortcut: "",
        doc: "Load parameter ranges and choice lists from an effectDefinitions_NN.cache file.",
        params: &[req("path", "string", "Path of the cache file")],
        enabled: always,
        run: defs_load,
    },
    CommandSpec {
        id: "definitions.load_installed",
        label: "Load Installed Definitions",
        menu: "File/Load Installed Definitions",
        shortcut: "",
        doc: "Load the definition cache an Axe-Edit installation left on this computer.",
        params: &[],
        enabled: always,
        run: defs_load_installed,
    },
    CommandSpec {
        id: "definitions.describe",
        label: "Describe Block Parameters",
        menu: "",
        shortcut: "",
        doc: "Ranges, steps and choice lists of every parameter of a block type.",
        params: &[BLOCK],
        enabled: always,
        run: defs_describe,
    },
    CommandSpec {
        id: "catalog.blocks",
        label: "List Block Types",
        menu: "",
        shortcut: "",
        doc: "Every block the device family offers, with its ID.",
        params: &[],
        enabled: always,
        run: catalog_blocks,
    },
    CommandSpec {
        id: "catalog.params",
        label: "List Block Parameters",
        menu: "",
        shortcut: "",
        doc: "Parameter IDs and symbolic names of a block type.",
        params: &[BLOCK],
        enabled: always,
        run: catalog_params,
    },
    CommandSpec {
        id: "sysex.inspect_file",
        label: "Inspect SysEx File",
        menu: "",
        shortcut: "",
        doc: "Check a .syx file: message count, checksums, whether it is a preset or a cabinet IR, and the name stored in it.",
        params: &[req("path", "string", "Path of the .syx file")],
        enabled: always,
        run: sysex_inspect,
    },
    CommandSpec {
        id: "app.inspect",
        label: "Inspect State",
        menu: "",
        shortcut: "",
        doc: "The whole editor state as JSON: connection, preset, grid, block states, selected block.",
        params: &[],
        enabled: always,
        run: inspect,
    },
    CommandSpec {
        id: "app.commands",
        label: "List Commands",
        menu: "",
        shortcut: "",
        doc: "Every command with its parameters and whether it can run right now.",
        params: &[],
        enabled: always,
        run: commands,
    },
];
