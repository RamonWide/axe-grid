//! Scene tools: names, and copying one scene onto another.
//!
//! The device has no scene names. The ones given here are kept in a file
//! on this computer, by model and preset number, and shown only while the
//! preset in that slot still has the name it had when they were given.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axegrid_model::{blocks, params};
use axegrid_sysex::{ParamAddress, Request, XyState};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::history_cmds::Edit;
use crate::Engine;

pub(crate) const SCENES: usize = 8;
/// Longest scene name, in characters.
const NAME_LEN: usize = 24;
const NAMES_FILE: &str = "scene-names.json";
/// The two scene controllers, whose values belong to a scene.
const SCENE_CONTROLLERS: [u8; 2] = [1, 2];

/// The names given to the scenes of one preset.
#[derive(Debug, Clone, Default, PartialEq)]
struct Named {
    preset: String,
    scenes: Vec<String>,
}

/// Scene names of every preset that has any.
#[derive(Debug, Default)]
pub(crate) struct SceneNames {
    /// Where they are kept; nowhere when the engine was given no folder.
    file: Option<PathBuf>,
    loaded: bool,
    presets: BTreeMap<String, Named>,
}

fn key(model: u8, number: u16) -> String {
    format!("{model:02X}/{number}")
}

impl SceneNames {
    pub(crate) fn kept_in(folder: &Path) -> Self {
        Self { file: Some(folder.join(NAMES_FILE)), ..Self::default() }
    }

    /// Reads the file once. A file that cannot be read counts as empty.
    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        let Some(text) = self.file.as_ref().and_then(|file| std::fs::read_to_string(file).ok()) else { return };
        let Ok(Value::Object(doc)) = serde_json::from_str::<Value>(&text) else { return };
        for (key, entry) in doc {
            let scenes = entry["scenes"].as_array().into_iter().flatten();
            let scenes: Vec<String> =
                scenes.take(SCENES).map(|name| name.as_str().unwrap_or_default().to_owned()).collect();
            let preset = entry["preset"].as_str().unwrap_or_default().to_owned();
            self.presets.insert(key, Named { preset, scenes });
        }
    }

    fn save(&self) -> Result<(), CommandError> {
        let Some(file) = &self.file else { return Ok(()) };
        let doc: serde_json::Map<String, Value> = self
            .presets
            .iter()
            .map(|(key, named)| (key.clone(), json!({ "preset": named.preset, "scenes": named.scenes })))
            .collect();
        let describe = |e: std::io::Error| CommandError::Io(format!("{}: {e}", file.display()));
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(describe)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| CommandError::Io(e.to_string()))?;
        // Written beside the file and moved over it, so a crash leaves one of the two whole.
        let draft = file.with_extension("json.new");
        std::fs::write(&draft, text).map_err(describe)?;
        std::fs::rename(&draft, file).map_err(describe)
    }

    /// The eight names of a preset; all empty when it has none, or when
    /// another preset has taken its slot since.
    fn of(&mut self, model: u8, number: u16, preset: &str) -> Vec<String> {
        self.load();
        let named = self.presets.get(&key(model, number)).filter(|named| named.preset == preset);
        let mut names = named.map(|named| named.scenes.clone()).unwrap_or_default();
        names.resize(SCENES, String::new());
        names
    }

    fn set(&mut self, model: u8, number: u16, preset: &str, names: Vec<String>) -> Result<(), CommandError> {
        self.load();
        if names.iter().all(String::is_empty) {
            self.presets.remove(&key(model, number));
        } else {
            self.presets.insert(key(model, number), Named { preset: preset.to_owned(), scenes: names });
        }
        self.save()
    }
}

/// What `scene.copy` took: everything that belongs to a scene.
#[derive(Debug, Clone)]
pub(crate) struct SceneClip {
    from: u8,
    /// `(block, engaged, parameter set)` of every block in the grid.
    states: Vec<(u16, bool, XyState)>,
    /// The output's main level for the scene, raw.
    main: Option<u16>,
    /// The values of the two scene controllers, raw.
    controllers: [Option<u16>; 2],
}

/// The parameters that hold a scene's own values, for scene `index` (0 to 7).
fn main_level(index: u8) -> Option<ParamAddress> {
    let effect = blocks::by_id(blocks::OUTPUT)?.effect_type;
    let param = params::id(effect, &format!("OUTPUT_MAIN{}", index + 1))?;
    Some(ParamAddress { block: blocks::OUTPUT, param })
}

fn scene_controller(controller: u8, index: u8) -> Option<ParamAddress> {
    let effect = blocks::by_id(blocks::CONTROLLERS)?.effect_type;
    let param = params::id(effect, &format!("CONTROLLERS_SCENE{controller}_VAL{}", index + 1))?;
    Some(ParamAddress { block: blocks::CONTROLLERS, param })
}

impl Engine {
    /// The names of the current preset's scenes, from this computer.
    pub(crate) fn scene_names_now(&mut self) -> Vec<String> {
        let Some(model) = self.state.model else { return vec![String::new(); SCENES] };
        let (number, name) = (self.state.preset.number, self.state.preset.name.clone());
        self.scene_names.of(model.byte(), number, &name)
    }

    /// Keeps the scene names with a preset that was renamed, or saved to
    /// `number` under its current name.
    pub(crate) fn carry_scene_names(&mut self, names: Vec<String>, number: u16) {
        let Some(model) = self.state.model else { return };
        let preset = self.state.preset.name.clone();
        // The names are a convenience; failing to write them must not fail a save.
        let _ = self.scene_names.set(model.byte(), number, &preset, names);
        self.state.scene_names = self.scene_names_now();
    }

    fn raw_of(&mut self, address: Option<ParamAddress>) -> Option<u16> {
        let value = self.session().ok()?.get_param(address?).ok()?.value;
        u16::try_from(value).ok()
    }

    /// Sets a parameter that belongs to a scene and returns the edit, if it changed.
    fn paste_value(
        &mut self,
        address: Option<ParamAddress>,
        wanted: Option<u16>,
    ) -> Result<Option<Edit>, CommandError> {
        let (Some(address), Some(after)) = (address, wanted) else { return Ok(None) };
        let before = self.session()?.get_param(address)?.value;
        let Ok(before) = u16::try_from(before) else { return Ok(None) };
        if before == after {
            return Ok(None);
        }
        let applied = self.session()?.set_param(address, after)?;
        self.remember(&applied);
        Ok(Some(Edit::Param { address, before, after, xy: None }))
    }

    pub(crate) fn scenes_json(&self) -> Value {
        let names = &self.state.scene_names;
        (0..SCENES)
            .map(|index| {
                json!({ "scene": index + 1, "name": names.get(index).cloned().unwrap_or_default(),
                        "current": usize::from(self.state.preset.scene) == index })
            })
            .collect()
    }
}

/// Reads a scene number, 1 to 8, as an index; the current scene without one.
fn scene_arg(engine: &Engine, args: &Args<'_>) -> Result<u8, CommandError> {
    match args.opt_int("scene", SCENES as u64)? {
        Some(0) => Err(CommandError::BadParam { name: "scene", problem: "scenes are numbered from 1".into() }),
        Some(scene) => Ok(scene as u8 - 1),
        None => Ok(engine.state.preset.scene),
    }
}

fn rename(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let index = scene_arg(engine, args)?;
    let name = args.str("name")?.trim().to_owned();
    if name.chars().count() > NAME_LEN || name.chars().any(char::is_control) {
        return Err(CommandError::BadParam {
            name: "name",
            problem: format!("must be at most {NAME_LEN} printable characters"),
        });
    }
    let model =
        engine.state.model.ok_or(CommandError::Disabled { id: "scene.rename", reason: "no device connected" })?;
    let mut names = engine.scene_names_now();
    if let Some(slot) = names.get_mut(usize::from(index)) {
        *slot = name;
    }
    let (number, preset) = (engine.state.preset.number, engine.state.preset.name.clone());
    engine.scene_names.set(model.byte(), number, &preset, names)?;
    engine.state.scene_names = engine.scene_names_now();
    Ok(engine.scenes_json())
}

fn names(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.state.scene_names = engine.scene_names_now();
    Ok(engine.scenes_json())
}

fn copy(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.refresh_preset()?;
    let from = engine.state.preset.scene;
    let states = engine.state.preset.states.iter().map(|s| (s.block, s.engaged, s.xy)).collect::<Vec<_>>();
    let main = engine.raw_of(main_level(from));
    let controllers = SCENE_CONTROLLERS.map(|controller| engine.raw_of(scene_controller(controller, from)));
    let blocks = states.len();
    engine.scene_clip = Some(SceneClip { from, states, main, controllers });
    Ok(json!({ "scene": from + 1, "blocks": blocks }))
}

fn paste(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let to = scene_arg(engine, args)?;
    let clip = engine
        .scene_clip
        .clone()
        .ok_or(CommandError::Disabled { id: "scene.paste", reason: "no scene has been copied" })?;
    if engine.state.preset.scene != to {
        engine.go_to_scene(to)?;
    }
    let mut edits = Vec::new();
    let outcome = (|| -> Result<(), CommandError> {
        let here = engine.state.preset.states.clone();
        for state in here {
            let Some(&(_, engaged, xy)) = clip.states.iter().find(|(block, ..)| *block == state.block) else {
                continue;
            };
            if state.xy != xy {
                engine.session()?.request(&Request::BlockXy(state.block, Some(xy)))?;
                edits.push(Edit::Xy { block: state.block, before: state.xy, after: xy });
            }
            if state.engaged != engaged {
                let address = ParamAddress { block: state.block, param: params::SHARED_BYPASS };
                engine.session()?.set_param(address, u16::from(!engaged))?;
                let (before, after) = (u16::from(!state.engaged), u16::from(!engaged));
                edits.push(Edit::Param { address, before, after, xy: None });
            }
        }
        edits.extend(engine.paste_value(main_level(to), clip.main)?);
        for (controller, value) in SCENE_CONTROLLERS.into_iter().zip(clip.controllers) {
            edits.extend(engine.paste_value(scene_controller(controller, to), value)?);
        }
        Ok(())
    })();
    let changed = edits.len();
    engine.refresh_preset()?;
    engine.record(format!("Paste Scene {} into {}", clip.from + 1, to + 1), Edit::Group(edits));
    outcome?;
    Ok(json!({ "from": clip.from + 1, "scene": to + 1, "changes": changed }))
}

fn has_clip(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.scene_clip.as_ref().map(|_| ()).ok_or("no scene has been copied")
}

const SCENE: crate::ParamDoc = opt("scene", "integer", "Scene number, 1 to 8; defaults to the current scene");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "scene.rename",
        label: "Name Scene",
        menu: "",
        shortcut: "",
        doc: "Give a scene of the current preset a name. The device has no scene names: they are kept on \
              this computer, by preset slot, and shown while the preset in that slot keeps its name.",
        params: &[SCENE, req("name", "string", "Up to 24 characters; empty takes the name away")],
        enabled: connected,
        run: rename,
    },
    CommandSpec {
        id: "scene.names",
        label: "List Scene Names",
        menu: "",
        shortcut: "",
        doc: "The eight scenes of the current preset with the names given to them on this computer.",
        params: &[],
        enabled: connected,
        run: names,
    },
    CommandSpec {
        id: "scene.copy",
        label: "Copy Scene",
        menu: "Scene/Copy Scene",
        shortcut: "Ctrl+Shift+C",
        doc: "Remember what makes up the current scene: which blocks are bypassed, which X/Y set each is \
              on, the output's main level for the scene and the two scene controllers. Only reads.",
        params: &[],
        enabled: connected,
        run: copy,
    },
    CommandSpec {
        id: "scene.paste",
        label: "Paste Scene",
        menu: "Scene/Paste Scene",
        shortcut: "Ctrl+Shift+V",
        doc: "Make a scene the same as the one last copied, in the edit buffer: bypass and X/Y of every \
              block both have, the main level and the scene controllers. Switches to that scene. Undo \
              takes it back.",
        params: &[SCENE],
        enabled: has_clip,
        run: paste,
    },
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn names_belong_to_the_preset_that_had_them() {
        let folder = std::env::temp_dir().join(format!("axegrid-scenes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut names = SceneNames::kept_in(&folder);
        assert!(names.of(6, 41, "Lead").iter().all(String::is_empty));
        let mut given = vec![String::new(); SCENES];
        given[2] = "Solo".into();
        names.set(6, 41, "Lead", given).unwrap();

        // Another engine, later: read back from the file.
        let mut again = SceneNames::kept_in(&folder);
        assert_eq!(again.of(6, 41, "Lead")[2], "Solo");
        // Another preset in that slot, another slot, another model: no names.
        assert!(again.of(6, 41, "Rhythm").iter().all(String::is_empty));
        assert!(again.of(6, 42, "Lead").iter().all(String::is_empty));
        assert!(again.of(7, 41, "Lead").iter().all(String::is_empty));
        // Taking the last name away removes the entry.
        again.set(6, 41, "Lead", vec![String::new(); SCENES]).unwrap();
        assert!(!std::fs::read_to_string(folder.join(NAMES_FILE)).unwrap().contains("Lead"));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_damaged_names_file_reads_as_empty() {
        let folder = std::env::temp_dir().join(format!("axegrid-scenes-bad-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(NAMES_FILE), "{ not json").unwrap();
        let mut names = SceneNames::kept_in(&folder);
        assert_eq!(names.of(6, 0, "x").len(), SCENES);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn scene_values_have_a_parameter_each() {
        assert_eq!(main_level(0).map(|a| (a.block, a.param)), Some((blocks::OUTPUT, 8)));
        assert_eq!(main_level(7).map(|a| a.param), Some(15));
        assert_eq!(scene_controller(2, 7).map(|a| (a.block, a.param)), Some((blocks::CONTROLLERS, 93)));
        assert_eq!(main_level(8), None);
    }
}
