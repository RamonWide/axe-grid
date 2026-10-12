//! Copying a block's settings: to another block of its type, or between
//! its own X and Y sets. A copy is the raw value of every parameter the
//! device lists for the block; a paste sets the ones that differ.

use axegrid_device::SessionError;
use axegrid_model::defs::RAW_MAX;
use axegrid_model::{blocks, layouts, params};
use axegrid_sysex::{ParamAddress, Request, XyState};
use serde_json::{json, Value};

use crate::command::{connected, opt, Args, CommandError, CommandSpec};
use crate::history_cmds::Edit;
use crate::Engine;

/// `(parameter, raw value)` of every setting of a block.
pub(crate) type Settings = Vec<(u16, u16)>;

/// Parameters that are something happening, not a setting: the looper's
/// transport and the tone match's captures.
const MOMENTARY: &[&str] = &[
    "LOOPER_RECORD",
    "LOOPER_PLAY",
    "LOOPER_ONCE",
    "LOOPER_DUB",
    "LOOPER_UNDO",
    "LOOPER_REVERSE",
    "LOOPER_HALFSPEED",
    "EQMATCH_CAPTUREREF",
    "EQMATCH_CAPTUREUSER",
    "EQMATCH_MATCH",
];

/// What `block.copy` took.
#[derive(Debug, Clone)]
pub(crate) struct BlockClip {
    effect: u8,
    from: u16,
    /// One set, or X and Y in that order.
    sets: Vec<(XyState, Settings)>,
}

fn xy_name(state: XyState) -> &'static str {
    if state == XyState::X {
        "X"
    } else {
        "Y"
    }
}

fn other(state: XyState) -> XyState {
    if state == XyState::X {
        XyState::Y
    } else {
        XyState::X
    }
}

/// Whether a parameter is part of what a copy carries.
fn is_setting(effect: u8, param: u16) -> bool {
    // Bypass belongs to the scene, not to the block's sound.
    let skipped = |name: &str| MOMENTARY.contains(&name) || name.ends_with("_BYPASS");
    param != params::SHARED_BYPASS && !params::name(effect, param).is_some_and(skipped)
}

/// Parameters that decide what the others mean, so they are set first:
/// the block's type, and whatever its pages are chosen by.
fn leading(effect: u8) -> Vec<u16> {
    let names_kind = |name: &str| name.ends_with("_TYPE") || name.ends_with("_MODEL");
    let mut first: Vec<u16> = params::all(effect).filter(|(_, name)| names_kind(name)).map(|(id, _)| id).collect();
    let chooser = layouts::for_effect(effect).and_then(|layout| layout.select).map(|(param, _)| param);
    first.extend(chooser.filter(|param| !first.contains(param)));
    first
}

fn effect_of(block: u16) -> Result<u8, CommandError> {
    blocks::by_id(block)
        .map(|b| b.effect_type)
        .ok_or_else(|| CommandError::NotFound(format!("block {block} is not a block type this build knows")))
}

impl Engine {
    /// The parameter set a block is on, if it is in the grid.
    fn xy_of(&self, block: u16) -> Option<XyState> {
        self.state.preset.states.iter().find(|s| s.block == block).map(|s| s.xy)
    }

    /// Every setting of a block, on the parameter set it is on.
    pub(crate) fn block_settings(&mut self, block: u16) -> Result<Settings, CommandError> {
        let effect = effect_of(block)?;
        let values = self.session()?.dump_block(block)?;
        // A value past the raw range belongs to a parameter the firmware does not define.
        let defined = values.iter().filter(|v| v.value <= u32::from(RAW_MAX) && is_setting(effect, v.address.param));
        Ok(defined.map(|v| (v.address.param, v.value as u16)).collect())
    }

    /// Sets every parameter of `block` that differs from `wanted`, on the
    /// parameter set the block is on. Returns how many were set.
    pub(crate) fn apply_values(&mut self, block: u16, wanted: &[(u16, u16)]) -> Result<usize, CommandError> {
        let effect = effect_of(block)?;
        let want = |param: u16| wanted.iter().find(|(id, _)| *id == param).map(|(_, value)| *value);
        let mut now = self.block_settings(block)?;
        let mut set = 0;
        for param in leading(effect) {
            let have = now.iter().find(|(id, _)| *id == param).map(|(_, value)| *value);
            if let (Some(have), Some(value)) = (have, want(param)) {
                if have != value {
                    self.session()?.set_param(ParamAddress { block, param }, value)?;
                    set += 1;
                    // A new type brings its own defaults: look again.
                    now = self.block_settings(block)?;
                }
            }
        }
        for (param, have) in now {
            match want(param) {
                Some(value) if value != have => {
                    self.session()?.set_param(ParamAddress { block, param }, value)?;
                    set += 1;
                }
                _ => {}
            }
        }
        Ok(set)
    }

    /// Applies settings and returns the edit that takes them back, if anything changed.
    fn paste_set(&mut self, block: u16, wanted: &[(u16, u16)]) -> Result<Option<Edit>, CommandError> {
        let xy = self.xy_of(block);
        let before = self.block_settings(block)?;
        self.apply_values(block, wanted)?;
        let after = self.block_settings(block)?;
        Ok((before != after).then_some(Edit::Block { block, xy, before, after }))
    }

    /// Switches a block's parameter set without recording it. `false` when
    /// the block has only one.
    fn switch_set(&mut self, block: u16, to: XyState) -> Result<bool, CommandError> {
        match self.session()?.request(&Request::BlockXy(block, Some(to))) {
            Ok(_) => Ok(true),
            Err(SessionError::Rejected { .. }) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads the block again wherever the editor shows it.
    fn after_block_change(&mut self, block: u16) -> Result<(), CommandError> {
        self.refresh_preset()?;
        if self.state.selected_block == Some(block) {
            self.refresh_params()?;
        }
        Ok(())
    }

    pub(crate) fn block_clip_json(&self) -> Value {
        match &self.block_clip {
            Some(clip) => json!({ "block": clip.from, "name": blocks::label(clip.from),
                                  "sets": clip.sets.iter().map(|(set, _)| xy_name(*set)).collect::<Vec<_>>() }),
            None => Value::Null,
        }
    }
}

fn has_clip(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.block_clip.as_ref().map(|_| ()).ok_or("no block has been copied")
}

fn in_preset(engine: &Engine, block: u16) -> Result<(), CommandError> {
    if blocks::is_fixed(block) || engine.state.preset.grid.blocks().contains(&block) {
        return Ok(());
    }
    Err(CommandError::NotFound(format!("block {block} ({}) is not in the current preset", blocks::label(block))))
}

fn copy(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    in_preset(engine, block)?;
    let effect = effect_of(block)?;
    let both = match args.opt_str("sets")? {
        None | Some("current") => false,
        Some("both") => true,
        Some(other) => {
            return Err(CommandError::BadParam {
                name: "sets",
                problem: format!("{other:?} is neither \"current\" nor \"both\""),
            })
        }
    };
    let on = engine.xy_of(block).unwrap_or(XyState::X);
    let mut sets = vec![(on, engine.block_settings(block)?)];
    if both && engine.xy_of(block).is_some() && engine.switch_set(block, other(on))? {
        // The block sounds like its other set for the moment this takes.
        let there = engine.block_settings(block);
        engine.switch_set(block, on)?;
        sets.push((other(on), there?));
        sets.sort_by_key(|(set, _)| *set == XyState::Y);
    }
    let count = sets.first().map_or(0, |(_, settings)| settings.len());
    engine.block_clip = Some(BlockClip { effect, from: block, sets });
    let mut result = engine.block_clip_json();
    result["params"] = json!(count);
    Ok(result)
}

fn paste(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    in_preset(engine, block)?;
    let clip = engine
        .block_clip
        .clone()
        .ok_or(CommandError::Disabled { id: "block.paste", reason: "no block has been copied" })?;
    if effect_of(block)? != clip.effect {
        return Err(CommandError::BadParam {
            name: "block",
            problem: format!(
                "{} is not the kind of block that was copied ({})",
                blocks::label(block),
                blocks::label(clip.from)
            ),
        });
    }
    let mut edits = Vec::new();
    let outcome = (|| -> Result<(), CommandError> {
        match (clip.sets.as_slice(), engine.xy_of(block)) {
            // Both sets, into a block that has both: each to its own.
            ([_, _], Some(on)) => {
                for (set, settings) in &clip.sets {
                    if engine.xy_of(block) != Some(*set) {
                        engine.switch_set(block, *set)?;
                        engine.refresh_preset()?;
                    }
                    edits.extend(engine.paste_set(block, settings)?);
                }
                engine.switch_set(block, on)?;
            }
            (sets, _) => {
                if let Some((_, settings)) = sets.first() {
                    edits.extend(engine.paste_set(block, settings)?);
                }
            }
        }
        Ok(())
    })();
    // What went through before a failure can still be taken back.
    let changed = edits.len();
    engine.record(format!("Paste into {}", blocks::label(block)), Edit::Group(edits));
    engine.after_block_change(block)?;
    outcome?;
    Ok(json!({ "block": block, "name": blocks::label(block), "from": clip.from, "sets_changed": changed }))
}

/// Copies one parameter set of a block over its other one.
fn across(engine: &mut Engine, args: &Args<'_>, from: XyState) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    in_preset(engine, block)?;
    let on = engine
        .xy_of(block)
        .ok_or_else(|| CommandError::NotFound(format!("{} has no X and Y sets", blocks::label(block))))?;
    if on != from {
        engine.switch_set(block, from)?;
    }
    let settings = engine.block_settings(block);
    let moved = engine.switch_set(block, other(from));
    let edit = match (settings, moved) {
        (Ok(settings), Ok(true)) => {
            engine.refresh_preset()?;
            engine.paste_set(block, &settings)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (_, Ok(false)) => Err(CommandError::NotFound(format!("{} has no X and Y sets", blocks::label(block)))),
    };
    // Back on the set the block was on, whatever happened.
    let back = engine.switch_set(block, on);
    let label = format!("Copy {} {} to {}", blocks::label(block), xy_name(from), xy_name(other(from)));
    let changed = match &edit {
        Ok(Some(_)) => 1,
        _ => 0,
    };
    if let Ok(Some(edit)) = &edit {
        engine.record(label, Edit::Group(vec![edit.clone()]));
    }
    engine.after_block_change(block)?;
    edit?;
    back?;
    Ok(json!({ "block": block, "from": xy_name(from), "to": xy_name(other(from)), "sets_changed": changed }))
}

fn x_to_y(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    across(engine, args, XyState::X)
}

fn y_to_x(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    across(engine, args, XyState::Y)
}

fn has_selection(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.state.selected_block.map(|_| ()).ok_or("no block is selected")
}

fn has_selection_and_clip(engine: &Engine) -> Result<(), &'static str> {
    has_selection(engine)?;
    has_clip(engine)
}

const BLOCK: crate::ParamDoc =
    opt("block", "integer|string", "Block ID or name such as \"Amp 1\"; defaults to the selected block");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "block.copy",
        label: "Copy Block",
        menu: "Block/Copy Block",
        shortcut: "Ctrl+C",
        doc: "Remember every setting of a block, to paste into a block of the same kind in this or another \
              preset. Bypass and modifiers are not part of it. Only reads, unless both sets are asked for: \
              then the block is switched to its other X/Y set and back, which can be heard.",
        params: &[
            BLOCK,
            opt("sets", "string", "\"current\" (default): the X/Y set the block is on; \"both\": X and Y"),
        ],
        enabled: has_selection,
        run: copy,
    },
    CommandSpec {
        id: "block.paste",
        label: "Paste Block",
        menu: "Block/Paste Block",
        shortcut: "Ctrl+V",
        doc: "Give a block the settings last copied from a block of the same kind, in the edit buffer. One \
              copied set goes to the X/Y set the block is on; a copy of both replaces both. Only parameters \
              that differ are sent, the block's type first. Undo takes it back.",
        params: &[BLOCK],
        enabled: has_selection_and_clip,
        run: paste,
    },
    CommandSpec {
        id: "block.copy_x_to_y",
        label: "Copy X to Y",
        menu: "Block/Copy X to Y",
        shortcut: "",
        doc: "Make a block's Y set the same as its X set. The block ends on the set it was on.",
        params: &[BLOCK],
        enabled: has_selection,
        run: x_to_y,
    },
    CommandSpec {
        id: "block.copy_y_to_x",
        label: "Copy Y to X",
        menu: "Block/Copy Y to X",
        shortcut: "",
        doc: "Make a block's X set the same as its Y set. The block ends on the set it was on.",
        params: &[BLOCK],
        enabled: has_selection,
        run: y_to_x,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_leaves_out_what_is_not_a_setting() {
        let looper = 38;
        let record = params::id(looper, "LOOPER_RECORD").unwrap();
        let mix = params::id(looper, "LOOPER_MIX").unwrap();
        assert!(!is_setting(looper, record) && is_setting(looper, mix));
        assert!(!is_setting(5, params::SHARED_BYPASS));
        assert!(!is_setting(looper, params::id(looper, "LOOPER_BYPASS").unwrap()));
    }

    #[test]
    fn the_type_goes_first() {
        assert_eq!(leading(5), [0]);
        // The delay has a model and a type, and its pages are chosen by the type: once is enough.
        assert_eq!(leading(8), [0, 1]);
        assert_eq!(leading(2), [params::id(2, "COMP_TYPE").unwrap()]);
        assert!(leading(36).is_empty());
    }
}
