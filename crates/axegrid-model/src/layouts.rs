//! Which controls a block's editor shows, on which page and row, and what
//! they are called. The data is `data/layouts.json`; see `ATTRIBUTION.md`
//! for where it comes from.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Knob,
    /// One of a list of named values.
    Choice,
    /// On or off.
    Toggle,
    /// A vertical fader, as on a graphic equaliser.
    Slider,
    /// Shown, not edited.
    Readout,
    /// An empty column that keeps the controls after it aligned.
    Spacer,
    /// A push button that sets every parameter in [`Control::sets`] to 1.
    Button,
    /// A button that sends MIDI controller [`Control::controller`].
    Controller,
    /// Reads the block again from the device.
    Refresh,
}

/// A condition on another parameter of the same block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// The parameter's raw value is one of these.
    Values { param: u16, values: Vec<u32> },
    /// The parameter's display text starts with this, or with one of
    /// these when it is a comma-separated list, ignoring case and punctuation.
    Text { param: u16, text: String },
}

/// A firmware version as `(major, minor)`.
pub type Firmware = (u8, u8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub label: String,
    pub param: u16,
    pub kind: Kind,
    /// Parameters a [`Kind::Button`] sets.
    pub sets: Vec<u16>,
    /// Controller number of a [`Kind::Controller`].
    pub controller: Option<u8>,
    /// The controller button fires once instead of latching.
    pub once: bool,
    pub when: Option<When>,
    /// Present from this firmware on.
    pub since: Option<Firmware>,
    /// Gone from this firmware on.
    pub until: Option<Firmware>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Part of the block's output section (level, balance, bypass mode).
    pub mixer: bool,
    pub controls: Vec<Control>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub rows: Vec<Row>,
    pub since: Option<Firmware>,
    /// Shown only while the block's type text contains this.
    pub type_contains: Option<String>,
    /// A curve drawn beside the controls, by kind: `"eq"`, `"speaker"`,
    /// `"delay_eq"`, `"reverb_eq"`, `"filter"`, `"tone"`, `"adsr1"`, `"adsr2"`.
    pub graph: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub value: u32,
    pub name: String,
    pub pages: Vec<Page>,
}

/// How the current value of [`Layout::select`] picks a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Value,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The parameter whose value chooses between variants, when there are several.
    pub select: Option<(u16, By)>,
    pub variants: Vec<Variant>,
}

/// Parameter of every block that names its type or model.
const TYPE_PARAM: u16 = 0;

/// Compares display texts the way the conditions need: letters and digits only.
fn plain(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

fn firmware(value: &Value) -> Option<Firmware> {
    Some((value.get(0)?.as_u64()? as u8, value.get(1)?.as_u64()? as u8))
}

fn control(value: &Value) -> Option<Control> {
    let kind = match value["k"].as_str()? {
        "knob" => Kind::Knob,
        "choice" => Kind::Choice,
        "toggle" => Kind::Toggle,
        "slider" => Kind::Slider,
        "readout" => Kind::Readout,
        "spacer" => Kind::Spacer,
        "button" => Kind::Button,
        "controller" => Kind::Controller,
        "refresh" => Kind::Refresh,
        _ => return None,
    };
    let gate = &value["w"];
    let when = gate["p"].as_u64().and_then(|param| {
        let param = param as u16;
        if let Some(values) = gate["v"].as_array() {
            let values = values.iter().filter_map(Value::as_u64).map(|v| v as u32).collect();
            return Some(When::Values { param, values });
        }
        Some(When::Text { param, text: gate["t"].as_str()?.to_owned() })
    });
    Some(Control {
        label: value["l"].as_str().unwrap_or_default().replace('\n', " "),
        param: value["p"].as_u64().unwrap_or_default() as u16,
        kind,
        sets: list(&value["set"]).filter_map(Value::as_u64).map(|id| id as u16).collect(),
        controller: value["cc"].as_u64().map(|number| number as u8),
        once: value["once"] == true,
        when,
        since: firmware(&value["fw"]),
        until: firmware(&value["fwlt"]),
    })
}

fn list(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn layout(value: &Value) -> Layout {
    let select = value["select"]["p"].as_u64().map(|param| {
        let by = if value["select"]["by"] == "text" { By::Text } else { By::Value };
        (param as u16, by)
    });
    let page = |page: &Value| Page {
        name: page["name"].as_str().unwrap_or_default().to_owned(),
        rows: list(&page["rows"])
            .map(|row| Row { mixer: row["m"] == true, controls: list(&row["c"]).filter_map(control).collect() })
            .collect(),
        since: firmware(&page["fw"]),
        type_contains: page["type_contains"].as_str().map(str::to_owned),
        graph: page["graph"].as_str().map(str::to_owned),
    };
    let variants = list(&value["variants"])
        .map(|variant| Variant {
            value: variant["value"].as_u64().unwrap_or_default() as u32,
            name: variant["name"].as_str().unwrap_or_default().to_owned(),
            pages: list(&variant["pages"]).map(page).collect(),
        })
        .collect();
    Layout { select, variants }
}

#[allow(clippy::expect_used)] // embedded data, checked by tests
fn table() -> &'static BTreeMap<u8, Layout> {
    static TABLE: OnceLock<BTreeMap<u8, Layout>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let doc: Value =
            serde_json::from_str(include_str!("../data/layouts.json")).expect("embedded layouts.json is valid");
        let effects = doc["effects"].as_object().expect("effects object");
        effects.iter().filter_map(|(key, value)| Some((key.parse().ok()?, layout(value)))).collect()
    })
}

/// The editor layout of an effect type, if one is known.
pub fn for_effect(effect_type: u8) -> Option<&'static Layout> {
    table().get(&effect_type).filter(|layout| !layout.variants.is_empty())
}

/// Raw value and display text of each parameter of a block, by ID.
pub type Values = BTreeMap<u16, (u32, String)>;

/// Whether the parameter counts in steps (a list, a switch, or something
/// other controls depend on) instead of sweeping a continuous range.
pub fn is_stepped(effect_type: u8, param: u16) -> bool {
    let Some(layout) = for_effect(effect_type) else { return false };
    if layout.select.is_some_and(|(selector, _)| selector == param) {
        return true;
    }
    let mut controls = layout.variants.iter().flat_map(|v| &v.pages).flat_map(|p| &p.rows).flat_map(|r| &r.controls);
    controls.any(|control| {
        let gates = matches!(&control.when, Some(When::Values { param: gate, .. } | When::Text { param: gate, .. }) if *gate == param);
        gates || (control.param == param && matches!(control.kind, Kind::Choice | Kind::Toggle))
    })
}

impl Layout {
    /// The variant in force for the block's current values.
    pub fn variant(&self, values: &Values) -> Option<&Variant> {
        let first = self.variants.first();
        let Some((param, by)) = self.select else { return first };
        let Some((raw, text)) = values.get(&param) else { return first };
        let found = match by {
            By::Value => self.variants.iter().find(|v| v.value == *raw),
            By::Text => self.variants.iter().find(|v| plain(&v.name) == plain(text)),
        };
        found.or(first)
    }
}

fn at_least(firmware: Firmware, wanted: Firmware) -> bool {
    firmware >= wanted
}

impl Page {
    pub fn visible(&self, firmware: Option<Firmware>, values: &Values) -> bool {
        let new_enough = match (firmware, self.since) {
            (Some(have), Some(wanted)) => at_least(have, wanted),
            _ => true,
        };
        let right_type = match (&self.type_contains, values.get(&TYPE_PARAM)) {
            (Some(part), Some((_, text))) => plain(text).contains(&plain(part)),
            _ => true,
        };
        new_enough && right_type
    }
}

impl Control {
    /// Whether the control applies to this firmware and these values.
    /// Unknown firmware or a missing controlling parameter counts as a yes.
    pub fn visible(&self, firmware: Option<Firmware>, values: &Values) -> bool {
        if let Some(have) = firmware {
            if self.since.is_some_and(|wanted| !at_least(have, wanted)) {
                return false;
            }
            if self.until.is_some_and(|gone| at_least(have, gone)) {
                return false;
            }
        }
        match &self.when {
            None => true,
            Some(When::Values { param, values: wanted }) => {
                values.get(param).is_none_or(|(raw, _)| wanted.contains(raw))
            }
            // The text lists alternatives, each the start of a type name:
            // "10 Band" stands for every ten-band type.
            Some(When::Text { param, text }) => values.get(param).is_none_or(|(_, have)| {
                let have = plain(have);
                text.split(',').map(plain).any(|wanted| !wanted.is_empty() && have.starts_with(&wanted))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{blocks, params};

    fn values(pairs: &[(u16, u32, &str)]) -> BTreeMap<u16, (u32, String)> {
        pairs.iter().map(|&(param, raw, text)| (param, (raw, text.to_owned()))).collect()
    }

    #[test]
    fn every_placeable_block_has_pages() {
        for block in blocks::all() {
            let layout = for_effect(block.effect_type).unwrap_or_else(|| panic!("{} has no layout", block.name));
            assert!(layout.variants.iter().all(|v| !v.pages.is_empty()), "{}", block.name);
        }
    }

    #[test]
    fn every_control_names_a_known_parameter() {
        for (&effect, layout) in table() {
            let controls =
                layout.variants.iter().flat_map(|v| &v.pages).flat_map(|p| &p.rows).flat_map(|r| &r.controls);
            for control in controls {
                let edited: Vec<u16> = match control.kind {
                    Kind::Spacer | Kind::Refresh => vec![],
                    Kind::Controller => {
                        assert!(control.controller.is_some(), "effect {effect}: {}", control.label);
                        vec![]
                    }
                    Kind::Button => {
                        assert!(!control.sets.is_empty(), "effect {effect}: {}", control.label);
                        control.sets.clone()
                    }
                    _ => vec![control.param],
                };
                for param in edited {
                    assert!(params::name(effect, param).is_some(), "effect {effect} param {param}");
                }
                assert!(!control.label.contains('\n'));
            }
        }
    }

    #[test]
    fn amp_starts_with_its_basic_page() {
        let amp = for_effect(5).unwrap();
        let basic = &amp.variants[0].pages[0];
        assert_eq!(basic.name, "Basic");
        let drive = &basic.rows[0].controls[0];
        assert_eq!(
            (drive.label.as_str(), params::name(5, drive.param), drive.kind),
            ("Input Drive", Some("DISTORT_DRIVE"), Kind::Knob)
        );
    }

    #[test]
    fn variant_follows_the_selecting_parameter() {
        let delay = for_effect(8).unwrap();
        let (param, by) = delay.select.unwrap();
        assert_eq!((params::name(8, param), by), (Some("DELAY_TYPE"), By::Value));
        assert_eq!(delay.variant(&values(&[(param, 2, "PING-PONG")])).unwrap().name, "Ping-Pong");
        // Unknown value or no value at all: the first variant.
        assert_eq!(delay.variant(&values(&[(param, 999, "?")])).unwrap().value, 0);
        assert_eq!(delay.variant(&values(&[])).unwrap().value, 0);

        let phaser = for_effect(13).unwrap();
        let (param, by) = phaser.select.unwrap();
        assert_eq!(by, By::Text);
        assert_eq!(phaser.variant(&values(&[(param, 7, "BARBERPOLE")])).unwrap().name, "BarberPole");
        assert_eq!(phaser.variant(&values(&[(param, 1, "DIGITAL STEREO")])).unwrap().name, "Phaser");
    }

    #[test]
    fn buttons_and_graphs() {
        let looper = &for_effect(38).unwrap().variants[0].pages[0];
        let transport: Vec<(&str, Option<u8>, bool)> = looper
            .rows
            .iter()
            .flat_map(|r| &r.controls)
            .filter(|c| c.kind == Kind::Controller)
            .map(|c| (c.label.as_str(), c.controller, c.once))
            .collect();
        assert!(transport.contains(&("Record", Some(28), false)));
        assert!(transport.contains(&("Undo", Some(121), true)));
        assert_eq!(transport.len(), 7);

        let tone = for_effect(39).unwrap();
        let buttons: Vec<&Control> = tone.variants[0]
            .pages
            .iter()
            .flat_map(|p| &p.rows)
            .flat_map(|r| &r.controls)
            .filter(|c| c.kind == Kind::Button)
            .collect();
        let both = buttons.iter().find(|c| c.label == "Start Both").unwrap();
        let named: Vec<_> = both.sets.iter().map(|&id| params::name(39, id).unwrap()).collect();
        assert_eq!(named, ["EQMATCH_CAPTUREREF", "EQMATCH_CAPTUREUSER"]);

        assert!(for_effect(4).unwrap().variants[0].pages.iter().all(|p| p.graph.as_deref() == Some("eq")));
        // The amp draws a curve on its speaker page only.
        let amp: Vec<(&str, Option<&str>)> =
            for_effect(5).unwrap().variants[0].pages.iter().map(|p| (p.name.as_str(), p.graph.as_deref())).collect();
        assert!(amp.contains(&("Speaker", Some("speaker"))));
        assert_eq!(amp.iter().filter(|(_, graph)| graph.is_some()).count(), 1);
        assert_eq!(for_effect(18).unwrap().variants[0].pages[0].graph.as_deref(), Some("filter"));
    }

    #[test]
    fn stepped_parameters() {
        let id = |name| params::id(5, name).unwrap();
        assert!(is_stepped(5, id("DISTORT_DRIVETYPE")));
        assert!(is_stepped(5, id("DISTORT_BOOST")));
        assert!(!is_stepped(5, id("DISTORT_DRIVE")));
        assert!(is_stepped(8, params::id(8, "DELAY_TYPE").unwrap()));
        assert!(!is_stepped(200, 0));
    }

    #[test]
    fn conditions_hide_controls() {
        let control = Control {
            label: "x".into(),
            param: 1,
            kind: Kind::Knob,
            sets: vec![],
            controller: None,
            once: false,
            when: Some(When::Values { param: 9, values: vec![0, 4] }),
            since: Some((27, 2)),
            until: None,
        };
        assert!(control.visible(Some((32, 0)), &values(&[(9, 4, "")])));
        assert!(!control.visible(Some((32, 0)), &values(&[(9, 1, "")])));
        assert!(!control.visible(Some((27, 1)), &values(&[(9, 4, "")])));
        assert!(control.visible(None, &values(&[])));

        let banded = Control { when: Some(When::Text { param: 15, text: "8 Band".into() }), since: None, ..control };
        assert!(banded.visible(None, &values(&[(15, 3, "8 BAND")])));
        assert!(!banded.visible(None, &values(&[(15, 0, "10 BAND")])));
        // The device's type names say more than the condition does.
        assert!(banded.visible(None, &values(&[(15, 5, "8 BAND VAR Q")])));
        let listed =
            Control { when: Some(When::Text { param: 0, text: "USA Lead,USA Rhythm, USA Pre LD2".into() }), ..banded };
        assert!(listed.visible(None, &values(&[(0, 1, "USA RHYTHM")])));
        assert!(!listed.visible(None, &values(&[(0, 1, "PLEXI 100W")])));
    }
}
