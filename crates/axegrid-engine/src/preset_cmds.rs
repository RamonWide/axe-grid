//! Presets and scenes.

use axegrid_sysex::{Reply, Request};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::Engine;

/// Largest value a two-group preset number can carry.
const MAX_PRESET: u64 = 0x3FFF;
const SCENES: u8 = 8;

fn summary(engine: &Engine) -> Value {
    engine.inspect()["preset"].clone()
}

fn select_number(engine: &mut Engine, number: u16) -> Result<Value, CommandError> {
    engine.session()?.request(&Request::SelectPreset(number))?;
    engine.refresh_preset()?;
    Ok(summary(engine))
}

fn select(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    select_number(engine, args.int("number", MAX_PRESET)? as u16)
}

fn next(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    select_number(engine, engine.state.preset.number.saturating_add(1))
}

fn previous(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    select_number(engine, engine.state.preset.number.saturating_sub(1))
}

fn rename(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let name = args.str("name")?;
    if name.is_empty() || name.chars().count() > axegrid_sysex::message::PRESET_NAME_LEN {
        return Err(CommandError::BadParam { name: "name", problem: "must be 1 to 32 characters".into() });
    }
    engine.session()?.request(&Request::SetPresetName(name.to_owned()))?;
    engine.refresh_preset()?;
    Ok(summary(engine))
}

fn store(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let number = match args.opt_int("number", MAX_PRESET)? {
        Some(number) => number as u16,
        None => engine.state.preset.number,
    };
    engine.session()?.request(&Request::StorePreset(number))?;
    let name = engine.state.preset.name.clone();
    engine.state.preset_names.insert(number, name);
    Ok(json!({ "stored_to": number }))
}

fn set_scene(engine: &mut Engine, index: u8) -> Result<Value, CommandError> {
    if let Reply::Scene(scene) = engine.session()?.request(&Request::Scene(Some(index)))? {
        engine.state.preset.scene = scene;
    }
    // Scenes carry their own bypass and X/Y states.
    engine.refresh_preset()?;
    Ok(summary(engine))
}

fn scene_select(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let scene = args.int("scene", u64::from(SCENES))?;
    if scene == 0 {
        return Err(CommandError::BadParam { name: "scene", problem: "scenes are numbered from 1".into() });
    }
    set_scene(engine, scene as u8 - 1)
}

fn scene_next(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    set_scene(engine, (engine.state.preset.scene + 1) % SCENES)
}

fn scene_previous(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    set_scene(engine, (engine.state.preset.scene + SCENES - 1) % SCENES)
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "preset.select",
        label: "Select Preset",
        menu: "",
        shortcut: "",
        doc: "Load a preset on the device and read it back.",
        params: &[req("number", "integer", "Zero-based preset number")],
        enabled: connected,
        run: select,
    },
    CommandSpec {
        id: "preset.next",
        label: "Next Preset",
        menu: "Preset/Next Preset",
        shortcut: "Ctrl+Right",
        doc: "Load the preset after the current one.",
        params: &[],
        enabled: connected,
        run: next,
    },
    CommandSpec {
        id: "preset.previous",
        label: "Previous Preset",
        menu: "Preset/Previous Preset",
        shortcut: "Ctrl+Left",
        doc: "Load the preset before the current one.",
        params: &[],
        enabled: connected,
        run: previous,
    },
    CommandSpec {
        id: "preset.rename",
        label: "Rename Preset",
        menu: "",
        shortcut: "",
        doc: "Change the name of the preset in the edit buffer. Not saved until preset.store.",
        params: &[req("name", "string", "New name, 1 to 32 printable ASCII characters")],
        enabled: connected,
        run: rename,
    },
    CommandSpec {
        id: "preset.store",
        label: "Save Preset",
        menu: "Preset/Save Preset",
        shortcut: "Ctrl+S",
        doc: "Write the edit buffer to a preset slot on the device, overwriting what is there.",
        params: &[opt("number", "integer", "Destination slot; defaults to the current preset")],
        enabled: connected,
        run: store,
    },
    CommandSpec {
        id: "scene.select",
        label: "Select Scene",
        menu: "",
        shortcut: "",
        doc: "Switch to one of the preset's eight scenes.",
        params: &[req("scene", "integer", "Scene number, 1 to 8")],
        enabled: connected,
        run: scene_select,
    },
    CommandSpec {
        id: "scene.next",
        label: "Next Scene",
        menu: "Preset/Next Scene",
        shortcut: "Ctrl+Up",
        doc: "Switch to the next scene, wrapping from 8 to 1.",
        params: &[],
        enabled: connected,
        run: scene_next,
    },
    CommandSpec {
        id: "scene.previous",
        label: "Previous Scene",
        menu: "Preset/Previous Scene",
        shortcut: "Ctrl+Down",
        doc: "Switch to the previous scene, wrapping from 1 to 8.",
        params: &[],
        enabled: connected,
        run: scene_previous,
    },
];
