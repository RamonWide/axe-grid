//! Modifiers: a controller (an LFO, an envelope, a pedal, ...) attached to
//! a parameter, with the curve that turns the controller's position into
//! the parameter's value. A parameter has a modifier when its source is
//! anything but the first choice ("none").

use std::collections::BTreeMap;

use axegrid_device::SessionError;
use axegrid_model::curves;
use axegrid_model::defs::RAW_MAX;
use axegrid_model::eq::leading_number;
use axegrid_model::{blocks, layouts, params};
use axegrid_sysex::{function, ModifierValue, ParamAddress};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::history_cmds::Edit;
use crate::Engine;

/// The source field: which controller drives the parameter. 0 is none.
const SOURCE: u16 = 0;
const NO_SOURCE: u16 = 0;
/// Points along a modifier's curve.
const CURVE_POINTS: usize = 65;

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// One of a list the device names choice by choice.
    Step,
    /// A continuous value over the raw range.
    Knob,
}

/// The fields of a modifier the editor shows: selector, name for
/// commands, label, kind. Selectors 8 and 9 repeat the block and the
/// parameter and are not shown; there is none past 14.
const FIELDS: &[(u16, &str, &str, Kind)] = &[
    (SOURCE, "source", "Source", Kind::Step),
    (1, "min", "Min", Kind::Knob),
    (2, "max", "Max", Kind::Knob),
    (3, "start", "Start", Kind::Knob),
    (4, "mid", "Mid", Kind::Knob),
    (5, "end", "End", Kind::Knob),
    (6, "slope", "Slope", Kind::Knob),
    (7, "damping", "Damping", Kind::Knob),
    (10, "auto_engage", "Auto Engage", Kind::Step),
    (11, "pc_reset", "PC Reset", Kind::Step),
    (12, "off_value", "Off Value", Kind::Knob),
    (13, "scale", "Scale", Kind::Knob),
    (14, "offset", "Offset", Kind::Knob),
];

/// The modifier the editor has open.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Open {
    address: ParamAddress,
    fields: Vec<ModifierValue>,
}

/// Names the device has given for the choices of a stepped field, by
/// field and value, collected as they are seen.
pub(crate) type Seen = BTreeMap<(u16, u16), String>;

/// The firmware whose source list a unit has shown to run to 27.
const FULL_SOURCES_FROM: u8 = 32;

/// The highest value this build sends for a field. A choice is never
/// stepped past the last one known to exist: a unit asked for a modifier
/// field that does not exist stopped answering, and nothing says a choice
/// that does not exist would be taken any better. A unit on firmware 32.00
/// took sources up to 27 (the last being the second scene controller);
/// for older firmware the 23 that public documentation lists are the limit.
fn highest(selector: u16, firmware: Option<(u8, u8)>) -> u16 {
    match selector {
        SOURCE if firmware.is_some_and(|(major, _)| major >= FULL_SOURCES_FROM) => 27,
        SOURCE => 23,
        10 => 6,
        11 => 1,
        _ => RAW_MAX,
    }
}

/// The parameter a modifier is opened from: the one it is addressed by,
/// except for a bypass mode and an LFO type.
fn owner(address: ParamAddress) -> u16 {
    blocks::by_id(address.block).map_or(address.param, |b| layouts::modifier_owner(b.effect_type, address.param))
}

/// A parameter as the history names it, such as "Amp 1 Drive".
fn param_label(address: ParamAddress) -> String {
    let address = ParamAddress { block: address.block, param: owner(address) };
    let name = blocks::by_id(address.block).map(|b| params::label(b.effect_type, address.param));
    format!("{} {}", blocks::label(address.block), name.unwrap_or_default()).trim_end().to_owned()
}

fn field_by_key(key: &str) -> Option<u16> {
    FIELDS.iter().find(|field| field.1 == key).map(|field| field.0)
}

impl Open {
    fn field(&self, selector: u16) -> Option<&ModifierValue> {
        self.fields.iter().find(|field| field.field == selector)
    }

    fn attached(&self) -> bool {
        self.field(SOURCE).is_some_and(|source| source.value != u32::from(NO_SOURCE))
    }

    /// The curve the fields describe, drawn from what the device says they are.
    fn curve(&self) -> Option<Value> {
        let share = |selector: u16| self.field(selector).map(|field| f64::from(field.value) / f64::from(RAW_MAX));
        let shown = |selector: u16| self.field(selector).and_then(|field| leading_number(&field.text));
        let shape = curves::Modifier {
            start: share(3)?,
            mid: share(4)?,
            end: share(5)?,
            slope: share(6)?,
            // Shown as a factor and as a percentage either side of nothing.
            scale: shown(13).unwrap_or(1.0),
            offset: shown(14).unwrap_or(0.0) / 100.0,
        };
        let points: Vec<Value> = shape
            .curve(CURVE_POINTS)
            .into_iter()
            .map(|(x, y)| json!([(x * 1000.0).round() / 1000.0, (y * 1000.0).round() / 1000.0]))
            .collect();
        let text = |selector: u16| self.field(selector).map(|field| field.text.clone());
        Some(json!({ "kind": "modifier", "points": points, "min": text(1), "max": text(2) }))
    }
}

impl Engine {
    fn note_modifier(&mut self, value: &ModifierValue) {
        let stepped = FIELDS.iter().any(|field| field.0 == value.field && field.3 == Kind::Step);
        if let (true, Ok(raw)) = (stepped, u16::try_from(value.value)) {
            if !value.text.is_empty() {
                self.modifier_seen.insert((value.field, raw), value.text.clone());
            }
        }
        if let Some(open) = self.modifier.as_mut().filter(|open| open.address == value.address) {
            match open.fields.iter_mut().find(|field| field.field == value.field) {
                Some(slot) => *slot = value.clone(),
                None => open.fields.push(value.clone()),
            }
        }
    }

    /// Reads every field the editor shows of the modifier on a parameter.
    fn modifier_fields(&mut self, address: ParamAddress) -> Result<Vec<ModifierValue>, CommandError> {
        let mut fields = Vec::with_capacity(FIELDS.len());
        for (selector, ..) in FIELDS {
            fields.push(self.session()?.modifier(address, *selector)?);
        }
        Ok(fields)
    }

    /// Reads every field of the modifier on a parameter and opens it.
    fn read_modifier(&mut self, address: ParamAddress) -> Result<(), CommandError> {
        let fields = self.modifier_fields(address)?;
        self.modifier = Some(Open { address, fields: fields.clone() });
        fields.iter().for_each(|field| self.note_modifier(field));
        Ok(())
    }

    /// Takes the modifier off a parameter and returns its source field as
    /// the device then reports it. The device puts the other fields back to
    /// their defaults as the source goes, so they are remembered with it and
    /// the removal is one step to undo.
    fn remove_modifier(&mut self, address: ParamAddress) -> Result<ModifierValue, CommandError> {
        let before = self.modifier_fields(address)?;
        let raw = |fields: &[ModifierValue], selector: u16| {
            fields.iter().find(|field| field.field == selector).and_then(|field| u16::try_from(field.value).ok())
        };
        if raw(&before, SOURCE) != Some(NO_SOURCE) {
            self.session()?.set_modifier(address, SOURCE, NO_SOURCE)?;
        }
        let after = self.modifier_fields(address)?;
        after.iter().for_each(|field| self.note_modifier(field));
        // Undone last to first: the source goes back on before the fields
        // its removal reset.
        let others = FIELDS.iter().map(|field| field.0).filter(|&selector| selector != SOURCE);
        let edits = others
            .chain([SOURCE])
            .filter_map(|field| match (raw(&before, field), raw(&after, field)) {
                (Some(before), Some(after)) if before != after => {
                    Some(Edit::Modifier { address, field, before, after })
                }
                _ => None,
            })
            .collect();
        self.record(format!("Remove modifier of {}", param_label(address)), Edit::Group(edits));
        let unread = SessionError::UnexpectedReply { function: function::MODIFIER };
        after.into_iter().find(|field| field.field == SOURCE).ok_or(CommandError::Device(unread))
    }

    /// Sets one field and remembers the edit.
    fn set_modifier_field(
        &mut self,
        address: ParamAddress,
        selector: u16,
        raw: u16,
    ) -> Result<ModifierValue, CommandError> {
        let known = self.modifier.as_ref().filter(|open| open.address == address).and_then(|open| open.field(selector));
        let before = match known {
            Some(field) => field.value,
            None => self.session()?.modifier(address, selector)?.value,
        };
        let highest = highest(selector, self.state.firmware);
        if raw > highest {
            return Err(CommandError::BadParam {
                name: "value",
                problem: format!("{raw} is past {highest}, the highest this field is sent"),
            });
        }
        // Without a source the device answers with the value asked for and
        // keeps its default, so there is nothing to set.
        if selector != SOURCE {
            let attached = match self.modifier.as_ref().filter(|open| open.address == address) {
                Some(open) => open.attached(),
                None => self.session()?.modifier(address, SOURCE)?.value != u32::from(NO_SOURCE),
            };
            if !attached {
                return Err(CommandError::BadParam {
                    name: "field",
                    problem:
                        "the parameter has no modifier: attach a source first, the device keeps no field without one"
                            .into(),
                });
            }
        }
        let applied = self.session()?.set_modifier(address, selector, raw)?;
        self.note_modifier(&applied);
        if let (Ok(before), Ok(after)) = (u16::try_from(before), u16::try_from(applied.value)) {
            if before != after {
                let label = FIELDS.iter().find(|field| field.0 == selector).map_or("Field", |field| field.2);
                let what = format!("Modifier {label} of {}", param_label(address));
                self.record(what, Edit::Modifier { address, field: selector, before, after });
            }
        }
        Ok(applied)
    }

    /// The open modifier as the window draws it; `null` when none is open.
    pub(crate) fn modifier_json(&self) -> Value {
        let Some(open) = &self.modifier else { return Value::Null };
        let effect = blocks::by_id(open.address.block).map(|b| b.effect_type);
        let fields: Vec<Value> = FIELDS
            .iter()
            .filter_map(|&(selector, key, label, kind)| {
                let field = open.field(selector)?;
                Some(json!({ "field": selector, "key": key, "label": label, "value": field.value, "text": field.text,
                             "kind": if kind == Kind::Step { "step" } else { "knob" }, "max": highest(selector, self.state.firmware) }))
            })
            .collect();
        json!({
            "block": open.address.block,
            "block_name": blocks::label(open.address.block),
            "param": owner(open.address),
            "at": open.address.param,
            "name": effect.and_then(|t| params::name(t, owner(open.address))),
            "label": effect.map(|t| params::label(t, owner(open.address))),
            "attached": open.attached(),
            "fields": fields,
            "graph": open.curve(),
        })
    }

    /// Closes the modifier window when its block is no longer there.
    pub(crate) fn drop_stale_modifier(&mut self) {
        let gone = |open: &Open| {
            let block = open.address.block;
            !blocks::is_fixed(block) && !self.state.preset.grid.blocks().contains(&block)
        };
        if self.modifier.as_ref().is_some_and(gone) {
            self.modifier = None;
        }
    }
}

/// The parameter a command is about: named in its arguments, or the one
/// whose modifier is open.
fn target(engine: &Engine, args: &Args<'_>) -> Result<ParamAddress, CommandError> {
    if !args.has("param") {
        if let Some(open) = &engine.modifier {
            return Ok(open.address);
        }
    }
    let block = engine.block_arg(args)?;
    let param = engine.param_arg(args, block)?;
    // A unit answers for any parameter as if it could have a modifier, so
    // the editor is the one to say which can. A bypass mode and an LFO type
    // keep theirs on a parameter of its own.
    let at = blocks::by_id(block).and_then(|b| layouts::modifier_param(b.effect_type, param));
    let param = at.ok_or_else(|| CommandError::BadParam {
        name: "param",
        problem: format!("{} takes no modifier", param_label(ParamAddress { block, param })),
    })?;
    Ok(ParamAddress { block, param })
}

fn open(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = target(engine, args)?;
    engine.read_modifier(address)?;
    Ok(engine.modifier_json())
}

fn close(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.modifier = None;
    Ok(Value::Null)
}

/// Reads a `source` argument: a number, or a name the device has shown.
fn source_arg(engine: &Engine, args: &Args<'_>) -> Result<u16, CommandError> {
    match args.raw("source") {
        Some(Value::String(name)) => {
            let seen = engine
                .modifier_seen
                .iter()
                .find(|((field, _), text)| *field == SOURCE && text.trim().eq_ignore_ascii_case(name.trim()));
            seen.map(|((_, value), _)| *value).ok_or_else(|| CommandError::BadParam {
                name: "source",
                problem: format!("{name:?} is not a source the device has named yet; give its number"),
            })
        }
        _ => Ok(args.int("source", u64::from(highest(SOURCE, engine.state.firmware)))? as u16),
    }
}

fn attach(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = target(engine, args)?;
    let source = source_arg(engine, args)?;
    if source == NO_SOURCE {
        return Err(CommandError::BadParam { name: "source", problem: "0 is no source; use modifier.remove".into() });
    }
    let applied = engine.set_modifier_field(address, SOURCE, source)?;
    engine.read_modifier(address)?;
    let mut result = engine.modifier_json();
    result["source"] = json!({ "value": applied.value, "text": applied.text });
    Ok(result)
}

fn remove(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = target(engine, args)?;
    engine.remove_modifier(address)?;
    Ok(json!({ "block": address.block, "param": address.param, "attached": false }))
}

fn set(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = target(engine, args)?;
    let key = args.str("field")?;
    let selector = field_by_key(key).ok_or_else(|| CommandError::BadParam {
        name: "field",
        problem: format!("{key:?} is not one of {}", FIELDS.iter().map(|f| f.1).collect::<Vec<_>>().join(", ")),
    })?;
    let raw = args.int("value", u64::from(RAW_MAX))? as u16;
    // Stepping the source down to none is a removal, with all it resets.
    let applied = if (selector, raw) == (SOURCE, NO_SOURCE) {
        engine.remove_modifier(address)?
    } else {
        engine.set_modifier_field(address, selector, raw)?
    };
    Ok(
        json!({ "block": address.block, "param": address.param, "field": key, "value": applied.value, "text": applied.text }),
    )
}

fn sources(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let seen = engine.modifier_seen.iter().filter(|((field, _), _)| *field == SOURCE);
    Ok(seen.map(|((_, value), text)| json!({ "source": value, "name": text })).collect())
}

fn is_open(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.modifier.as_ref().map(|_| ()).ok_or("no modifier is open")
}

const BLOCK: crate::ParamDoc =
    opt("block", "integer|string", "Block ID or name such as \"Wah 1\"; defaults to the selected block");
const PARAM: crate::ParamDoc = opt(
    "param",
    "integer|string",
    "Parameter ID or symbolic name such as \"WAH_CONTROL\"; defaults to the parameter whose modifier is open",
);

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "modifier.open",
        label: "Open Modifier",
        menu: "",
        shortcut: "",
        doc: "Read the modifier of a parameter: its source, range, curve points and the rest, each with \
              the device's own text, and a sketch of the curve. Only reads. Refused for a \
              parameter that takes no modifier, and by the device once the preset holds 24 modifiers \
              and this parameter has none.",
        params: &[BLOCK, PARAM],
        enabled: connected,
        run: open,
    },
    CommandSpec {
        id: "modifier.close",
        label: "Close Modifier",
        menu: "",
        shortcut: "",
        doc: "Close the modifier window.",
        params: &[],
        enabled: is_open,
        run: close,
    },
    CommandSpec {
        id: "modifier.attach",
        label: "Attach Modifier",
        menu: "",
        shortcut: "",
        doc: "Attach a controller to a parameter in the edit buffer by setting the modifier's source. The \
              device names the sources; 0 is none, and the numbering is the device's own. Sources 1 to \
              23 are confirmed on a unit; none past 23 is sent.",
        params: &[
            BLOCK,
            PARAM,
            req("source", "integer|string", "Source number from 1, or a name listed by modifier.sources"),
        ],
        enabled: connected,
        run: attach,
    },
    CommandSpec {
        id: "modifier.remove",
        label: "Remove Modifier",
        menu: "",
        shortcut: "",
        doc: "Take the modifier off a parameter in the edit buffer by setting its source to none. The \
              device puts the modifier's other fields back to their defaults; undoing the removal \
              brings them back with the source.",
        params: &[BLOCK, PARAM],
        enabled: connected,
        run: remove,
    },
    CommandSpec {
        id: "modifier.set",
        label: "Set Modifier Field",
        menu: "",
        shortcut: "",
        doc: "Set one field of a modifier to a raw value and return what the device applied, with its \
              text. Setting the source to 0 removes the modifier, as modifier.remove does. The other \
              fields need a source first: the device keeps none of them without one.",
        params: &[
            BLOCK,
            PARAM,
            req(
                "field",
                "string",
                "source, min, max, start, mid, end, slope, damping, auto_engage, pc_reset, off_value, scale or offset",
            ),
            req("value", "integer", "Raw value, 0 to 65534; for source, auto_engage and pc_reset, the choice number"),
        ],
        enabled: connected,
        run: set,
    },
    CommandSpec {
        id: "modifier.sources",
        label: "List Modifier Sources",
        menu: "",
        shortcut: "",
        doc: "The modifier sources the device has named so far in this session, by number. The device \
              offers no list; a name is learned when a modifier that uses the source is read or set.",
        params: &[],
        enabled: connected,
        run: sources,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use axegrid_sysex::MODIFIER_FIELDS;

    #[test]
    fn no_field_is_past_what_the_device_has() {
        assert!(FIELDS.iter().all(|field| field.0 < MODIFIER_FIELDS));
        assert_eq!(field_by_key("offset"), Some(14));
        assert_eq!(field_by_key("end_marker"), None);
        assert_eq!((highest(SOURCE, None), highest(11, None), highest(4, None)), (23, 1, RAW_MAX));
        assert_eq!((highest(SOURCE, Some((32, 0))), highest(SOURCE, Some((31, 3)))), (27, 23));
    }

    #[test]
    fn the_curve_follows_the_fields() {
        let address = ParamAddress { block: 106, param: 1 };
        let field = |field: u16, value: u32, text: &str| ModifierValue { address, field, value, text: text.into() };
        let fields = vec![
            field(0, 3, "PEDAL"),
            field(1, 0, "0.00"),
            field(2, 65534, "10.00"),
            field(3, 0, "0.0 %"),
            field(4, 32767, "50.0 %"),
            field(5, 65534, "100.0 %"),
            field(6, 32767, "50.0 %"),
            field(13, 32767, "1.000"),
            field(14, 32767, "0.0 %"),
        ];
        let open = Open { address, fields };
        assert!(open.attached());
        let graph = open.curve().unwrap();
        assert_eq!(graph["points"][0], json!([0.0, 0.0]));
        assert_eq!(graph["points"][CURVE_POINTS - 1], json!([1.0, 1.0]));
        assert_eq!((graph["min"].as_str(), graph["max"].as_str()), (Some("0.00"), Some("10.00")));
        // Without its curve points there is nothing to draw.
        assert!(Open { address, fields: vec![field(0, 0, "NONE")] }.curve().is_none());
    }
}
