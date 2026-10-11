//! Parameter IDs and their symbolic names per effect type (spec section 5).

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Bypass switch present on every effect: 0 is engaged, 1 is bypassed.
pub const SHARED_BYPASS: u16 = 255;

struct Names {
    effects: BTreeMap<u8, Vec<(u16, String)>>,
    shared: Vec<(u16, String)>,
}

#[allow(clippy::expect_used)] // embedded data, checked by tests
fn names() -> &'static Names {
    static NAMES: OnceLock<Names> = OnceLock::new();
    NAMES.get_or_init(|| {
        let doc: serde_json::Value =
            serde_json::from_str(include_str!("../data/params.json")).expect("embedded params.json is valid");
        let list = |value: &serde_json::Value| -> Vec<(u16, String)> {
            serde_json::from_value(value.clone()).expect("list of [id, name] pairs")
        };
        let effects = doc["effects"]
            .as_object()
            .expect("effects object")
            .iter()
            .map(|(key, value)| (key.parse().expect("numeric effect type"), list(value)))
            .collect();
        Names { effects, shared: list(&doc["shared"]) }
    })
}

/// Symbolic name of a parameter, e.g. `(5, 1)` is `"DISTORT_DRIVE"`.
pub fn name(effect_type: u8, param: u16) -> Option<&'static str> {
    let names = names();
    let own = names.effects.get(&effect_type).into_iter().flatten();
    own.chain(&names.shared).find(|(id, _)| *id == param).map(|(_, n)| n.as_str())
}

/// Reverse lookup: the effect's own table first, then the shared one.
pub fn id(effect_type: u8, name: &str) -> Option<u16> {
    let names = names();
    let own = names.effects.get(&effect_type).into_iter().flatten();
    own.chain(&names.shared).find(|(_, n)| n == name).map(|(id, _)| *id)
}

/// The effect's own parameters in table order.
pub fn all(effect_type: u8) -> impl Iterator<Item = (u16, &'static str)> {
    let own = names().effects.get(&effect_type).into_iter().flatten();
    own.map(|(id, name)| (*id, name.as_str()))
}

/// A readable label derived from the symbolic name: the effect prefix is
/// dropped and the rest title-cased, so `DISTORT_DRIVE` becomes `Drive`.
pub fn label(effect_type: u8, param: u16) -> String {
    match name(effect_type, param) {
        Some(symbol) => {
            let rest = symbol.split_once('_').map_or(symbol, |(_, rest)| rest);
            let mut chars = rest.chars();
            match chars.next() {
                Some(first) => first.to_string() + &chars.as_str().to_ascii_lowercase(),
                None => String::new(),
            }
        }
        None => format!("Param {param}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_both_ways() {
        assert_eq!(name(5, 0), Some("DISTORT_TYPE"));
        assert_eq!(name(5, 1), Some("DISTORT_DRIVE"));
        assert_eq!(id(2, "COMP_THRESH"), Some(0));
        assert_eq!(id(34, "VOLUME_TAPER").and_then(|p| name(34, p)), Some("VOLUME_TAPER"));
        assert_eq!(name(5, 9999), None);
        assert_eq!(id(200, "DISTORT_DRIVE"), None);
    }

    #[test]
    fn shared_table_is_a_fallback() {
        assert_eq!(id(7, "GLOBAL_BYPCTRL"), Some(SHARED_BYPASS));
        assert_eq!(name(7, SHARED_BYPASS), Some("GLOBAL_BYPCTRL"));
    }

    #[test]
    fn own_table_excludes_shared() {
        assert_eq!(all(23).count(), 2);
    }

    #[test]
    fn labels() {
        assert_eq!(label(5, 1), "Drive");
        assert_eq!(label(5, 9999), "Param 9999");
    }
}
