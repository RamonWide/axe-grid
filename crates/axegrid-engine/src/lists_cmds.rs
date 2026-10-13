//! Choice lists read from the device: the names behind every list control,
//! such as amp models, effect types and cabs. The device is asked once per
//! parameter of a block type and the answer is kept until it disconnects.

use std::collections::BTreeMap;

use axegrid_device::SessionError;
use axegrid_model::{blocks, defs, layouts};
use axegrid_sysex::{ParamAddress, ParamValue};
use serde_json::{json, Value};

use crate::command::{connected, opt, Args, CommandError, CommandSpec};
use crate::Engine;

/// Largest number of requests one command may be limited to.
const MAX_BUDGET: u64 = 100_000;

/// What the device said about the choices of one parameter.
#[derive(Debug, Clone, PartialEq)]
enum Choices {
    /// Its own list, in the order of its raw values.
    Strings(Vec<String>),
    /// Its value is a position in the device's cab list, up to `highest`.
    CabList { highest: u16 },
    /// It has no list: the device refused, or it is not a list after all.
    None,
}

/// Everything read so far.
#[derive(Debug, Default)]
pub(crate) struct Lists {
    /// By effect type and parameter: two instances of a block share a list.
    by_param: BTreeMap<(u8, u16), Choices>,
    /// Names in the device's cab list, by position.
    cabs: BTreeMap<u16, String>,
    /// The cab list as choices, once every name in it has been read.
    cab_choices: Option<Vec<String>>,
}

/// Counts requests against the limit a command was given.
struct Budget(u64);

impl Budget {
    fn spend(&mut self) -> bool {
        let left = self.0 > 0;
        self.0 = self.0.saturating_sub(1);
        left
    }
}

impl Lists {
    /// Puts the cab list together when all of it is there.
    fn rebuild_cab_choices(&mut self) {
        let highest = self.by_param.values().filter_map(|choices| match choices {
            Choices::CabList { highest } => Some(*highest),
            _ => None,
        });
        self.cab_choices =
            highest.max().and_then(|highest| (0..=highest).map(|at| self.cabs.get(&at).cloned()).collect());
    }

    /// Whether the list of a parameter still has to be read, given what the
    /// device reported for it.
    fn is_missing(&self, effect: u8, value: &ParamValue) -> bool {
        match self.by_param.get(&(effect, value.address.param)) {
            Some(Choices::CabList { .. }) => self.cab_choices.is_none(),
            Some(_) => false,
            None => value.extra[1] > 0 || layouts::is_stepped(effect, value.address.param),
        }
    }
}

impl Engine {
    /// The choices of a parameter, if they have been read.
    pub(crate) fn choices(&self, effect: u8, param: u16) -> Option<&Vec<String>> {
        match self.lists.by_param.get(&(effect, param))? {
            Choices::Strings(list) => Some(list),
            Choices::CabList { .. } => self.lists.cab_choices.as_ref(),
            Choices::None => None,
        }
    }

    /// Lists of the selected block that have not been read yet.
    pub(crate) fn lists_missing(&self) -> usize {
        let effect = self.state.selected_block.and_then(blocks::by_id).map(|b| b.effect_type);
        effect.map_or(0, |effect| self.state.params.iter().filter(|v| self.lists.is_missing(effect, v)).count())
    }

    pub(crate) fn lists_known(&self) -> usize {
        self.lists.by_param.values().filter(|choices| **choices != Choices::None).count()
    }

    /// For a parameter whose value is a position in the device's cab list:
    /// how long the list is, how much of it has been read, and where the
    /// user cabs sit in it on a model whose cabinet memory is known. A
    /// front end needs it to offer the cabs before every name is in.
    pub(crate) fn cab_list_json(&self, effect: u8, param: u16) -> Option<Value> {
        let Some(Choices::CabList { highest }) = self.lists.by_param.get(&(effect, param)) else { return None };
        let slots = self.state.model.and_then(axegrid_model::library::cab_slots);
        Some(json!({
            "total": u32::from(*highest) + 1,
            "known": self.lists.cabs.range(..=*highest).count(),
            "factory": slots.map(|slots| slots.factory),
            "user": slots.map(|slots| slots.user),
        }))
    }

    pub(crate) fn forget_lists(&mut self) {
        self.lists = Lists::default();
    }

    /// Files the name at a position of the cab list, also as the name of
    /// the user cab that sits there.
    pub(crate) fn note_cab(&mut self, index: u16, name: String) {
        let slots = self.state.model.and_then(axegrid_model::library::cab_slots);
        let user = slots.filter(|slots| (slots.factory..slots.factory + slots.user).contains(&index));
        if let Some(slots) = user {
            self.state.cab_names.insert(index - slots.factory + 1, name.clone());
        }
        if self.lists.cabs.get(&index) != Some(&name) && index < self.state.name_scan.cab_slots {
            // The name file on this computer is behind now.
            self.names_changed();
        }
        self.lists.cabs.insert(index, name);
        self.lists.rebuild_cab_choices();
    }

    /// Names in the device's cab list read or remembered so far, by position.
    pub(crate) fn cab_list(&self) -> &BTreeMap<u16, String> {
        &self.lists.cabs
    }

    /// Takes up cab names remembered from an earlier session, by position.
    pub(crate) fn note_cached_cabs(&mut self, names: BTreeMap<u16, String>) {
        let slots = self.state.model.and_then(axegrid_model::library::cab_slots);
        for (index, name) in names {
            let user = slots.filter(|slots| (slots.factory..slots.factory + slots.user).contains(&index));
            if let Some(slots) = user {
                self.state.cab_names.insert(index - slots.factory + 1, name.clone());
            }
            self.lists.cabs.insert(index, name);
        }
        self.lists.rebuild_cab_choices();
    }

    /// Reads what is still missing of one parameter's list. Returns whether
    /// the list is complete now; it is not when the budget ran out.
    fn read_list(&mut self, effect: u8, value: &ParamValue, budget: &mut Budget) -> Result<bool, CommandError> {
        let key = (effect, value.address.param);
        if !self.lists.by_param.contains_key(&key) {
            if !budget.spend() {
                return Ok(false);
            }
            // A parameter that reports a highest choice has a list of its own.
            // One that only looks like a list may be a position in the cab list.
            let choices = if value.extra[1] > 0 {
                self.session()?.param_strings(value.address).map(Choices::Strings)
            } else {
                self.session()?.param_info(value.address).map(|info| {
                    let highest = info.max.clamp(0.0, f32::from(u16::MAX)) as u16;
                    if info.type_word == defs::TYPE_CAB_LIST && highest > 0 {
                        Choices::CabList { highest }
                    } else {
                        Choices::None
                    }
                })
            };
            let choices = match choices {
                Ok(choices) => choices,
                // The device knows best what is a list.
                Err(SessionError::Rejected { .. }) => Choices::None,
                Err(error) => return Err(error.into()),
            };
            self.lists.by_param.insert(key, choices);
            self.lists.rebuild_cab_choices();
        }
        let Some(Choices::CabList { highest }) = self.lists.by_param.get(&key).cloned() else { return Ok(true) };
        for index in 0..=highest {
            if self.lists.cabs.contains_key(&index) {
                continue;
            }
            if !budget.spend() {
                return Ok(false);
            }
            match self.session()?.cab_name(index) {
                Ok(name) => self.note_cab(index, name),
                // The cab list is shorter than the parameter's range says.
                Err(SessionError::Rejected { .. }) => {
                    self.lists.by_param.insert(key, Choices::None);
                    return Ok(true);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }

    /// Makes sure the list of one parameter has been read, for commands
    /// that need it at once.
    pub(crate) fn ensure_list(&mut self, address: ParamAddress) -> Result<(), CommandError> {
        let Some(effect) = blocks::by_id(address.block).map(|b| b.effect_type) else { return Ok(()) };
        let cached = self.state.params.iter().find(|p| p.address == address).cloned();
        let value = match cached {
            Some(value) => value,
            None => self.session()?.get_param(address)?,
        };
        if self.lists.is_missing(effect, &value) {
            self.read_list(effect, &value, &mut Budget(MAX_BUDGET))?;
        }
        Ok(())
    }
}

fn read_lists(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let effect = blocks::by_id(block)
        .map(|b| b.effect_type)
        .ok_or_else(|| CommandError::NotFound(format!("block {block} is not a block type this build knows")))?;
    let mut budget = Budget(args.opt_int("budget", MAX_BUDGET)?.unwrap_or(MAX_BUDGET));
    let allowed = budget.0;
    let values = if engine.state.selected_block == Some(block) {
        engine.state.params.clone()
    } else {
        engine.session()?.dump_block(block)?
    };
    let mut missing = 0;
    for value in &values {
        // Checked one at a time: reading the cab list for one parameter completes it for the other.
        if engine.lists.is_missing(effect, value) && !engine.read_list(effect, value, &mut budget)? {
            missing += 1;
        }
    }
    let lists: Vec<Value> = values
        .iter()
        .filter_map(|value| {
            let choices = engine.choices(effect, value.address.param)?;
            Some(json!({ "param": value.address.param,
                         "name": axegrid_model::params::name(effect, value.address.param),
                         "choices": choices.len() }))
        })
        .collect();
    Ok(json!({ "block": block, "requests": allowed - budget.0, "missing": missing, "lists": lists }))
}

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "block.read_lists",
    label: "Read Choice Lists",
    menu: "",
    shortcut: "",
    doc: "Read from the device the choices of every list of a block: amp models, effect types, cabs and \
          the like. Each list is read once and kept until the device disconnects. Only reads. The cab \
          list is one request per cab, so over a 5-pin MIDI cable it takes a while.",
    params: &[
        opt("block", "integer|string", "Block ID or name such as \"Amp 1\"; defaults to the selected block"),
        opt(
            "budget",
            "integer",
            "Most requests to send in this call; \"missing\" in the result counts the lists still to \
             read. Without it everything is read",
        ),
    ],
    enabled: connected,
    run: read_lists,
}];
