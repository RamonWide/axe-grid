//! Block instances and the effect type each one belongs to.
//!
//! Block IDs and their symbolic names come from `data/blocks.json`
//! (spec section 5). The pairing of a block family with an effect type and
//! the display names are this project's own.

use std::sync::OnceLock;

/// First and last block ID that stand for a plain pass-through cell.
pub const SHUNT_IDS: std::ops::RangeInclusive<u16> = 200..=235;
pub const EMPTY: u16 = 0;
/// Blocks every preset has, outside the grid: the gate at the input, the
/// mixer at the output, and the controllers (tempo, LFOs, envelopes).
pub const INPUT_GATE: u16 = 139;
pub const OUTPUT: u16 = 140;
pub const CONTROLLERS: u16 = 141;

/// Whether the block exists in every preset instead of being placed.
pub fn is_fixed(id: u16) -> bool {
    matches!(id, INPUT_GATE | OUTPUT | CONTROLLERS)
}

/// `(symbolic family, effect type, display name)`.
const FAMILIES: &[(&str, u8, &str)] = &[
    ("COMP", 2, "Compressor"),
    ("GRAPHEQ", 3, "Graphic EQ"),
    ("PARAEQ", 4, "Parametric EQ"),
    ("DISTORT", 5, "Amp"),
    ("CAB", 6, "Cab"),
    ("REVERB", 7, "Reverb"),
    ("DELAY", 8, "Delay"),
    ("MULTITAP", 9, "Multi Delay"),
    ("CHORUS", 10, "Chorus"),
    ("FLANGER", 11, "Flanger"),
    ("ROTARY", 12, "Rotary"),
    ("PHASER", 13, "Phaser"),
    ("WAH", 14, "Wah"),
    ("FORMANT", 15, "Formant"),
    ("TREMOLO", 16, "Pan/Trem"),
    ("PITCH", 17, "Pitch"),
    ("FILTER", 18, "Filter"),
    ("FUZZ", 19, "Drive"),
    ("ENHANCER", 20, "Enhancer"),
    ("LOOP", 21, "FX Loop"),
    ("MIXER", 22, "Mixer"),
    ("FBSEND", 23, "Feedback Send"),
    ("FBRETURN", 24, "Feedback Return"),
    ("SYNTH", 25, "Synth"),
    ("VOCODER", 26, "Vocoder"),
    ("MEGATAP", 27, "Mega Tap"),
    ("CROSSOVER", 28, "Crossover"),
    ("GATE", 29, "Gate/Expander"),
    ("RINGMOD", 30, "Ring Mod"),
    ("MULTICOMP", 31, "Multiband Comp"),
    ("QUADCHORUS", 32, "Quad Chorus"),
    ("RESONATOR", 33, "Resonator"),
    ("VOLUME", 34, "Volume/Pan"),
    ("NOISEGATE", 35, "Input Gate"),
    ("OUT", 36, "Output"),
    ("CONTROL", 37, "Controllers"),
    ("LOOPER", 38, "Looper"),
    ("EQMATCH", 39, "Tone Match"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: u16,
    pub effect_type: u8,
    /// Display name including the instance number, e.g. `"Amp 1"`.
    pub name: String,
}

#[allow(clippy::expect_used)] // embedded data, checked by tests
fn table() -> &'static [Block] {
    static TABLE: OnceLock<Vec<Block>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: Vec<(u16, String)> =
            serde_json::from_str(include_str!("../data/blocks.json")).expect("embedded blocks.json is valid");
        raw.into_iter()
            .filter_map(|(id, symbol)| {
                let symbol = symbol.strip_prefix("ID_")?;
                let family = symbol.trim_end_matches(|c: char| c.is_ascii_digit());
                let instance = &symbol[family.len()..];
                let &(_, effect_type, display) = FAMILIES.iter().find(|f| f.0 == family)?;
                let multiple = instance != "1" || has_sibling(family);
                let name =
                    if instance.is_empty() || !multiple { display.to_owned() } else { format!("{display} {instance}") };
                Some(Block { id, effect_type, name })
            })
            .collect()
    })
}

/// Families with more than one instance, whose first block is numbered too.
fn has_sibling(family: &str) -> bool {
    include_str!("../data/blocks.json").contains(&format!("\"ID_{family}2\""))
}

/// All placeable blocks, in ID order.
pub fn all() -> &'static [Block] {
    table()
}

pub fn by_id(id: u16) -> Option<&'static Block> {
    table().iter().find(|b| b.id == id)
}

pub fn is_shunt(id: u16) -> bool {
    SHUNT_IDS.contains(&id)
}

/// Short label for a grid cell.
pub fn label(id: u16) -> String {
    match by_id(id) {
        Some(block) => block.name.clone(),
        None if id == EMPTY => String::new(),
        None if is_shunt(id) => "Shunt".to_owned(),
        None => format!("#{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_block_resolves() {
        assert_eq!(all().len(), 71);
        assert_eq!(all().first().map(|b| b.id), Some(100));
        assert_eq!(all().last().map(|b| b.id), Some(170));
    }

    #[test]
    fn names_and_types() {
        let amp = by_id(106).unwrap();
        assert_eq!((amp.name.as_str(), amp.effect_type), ("Amp 1", 5));
        assert_eq!(by_id(107).unwrap().name, "Amp 2");
        assert_eq!(by_id(165).unwrap().name, "Filter 4");
        // Single-instance blocks carry no number.
        assert_eq!(by_id(126).unwrap().name, "Formant");
        assert_eq!(by_id(141).unwrap().name, "Controllers");
        assert_eq!(by_id(170).unwrap().name, "Tone Match");
        // Pitch 2 is far from Pitch 1 in ID order but shares its type.
        assert_eq!(by_id(153).unwrap().effect_type, by_id(130).unwrap().effect_type);
    }

    #[test]
    fn fixed_blocks() {
        assert_eq!(by_id(INPUT_GATE).unwrap().name, "Input Gate");
        assert_eq!(by_id(OUTPUT).unwrap().name, "Output");
        assert_eq!(by_id(CONTROLLERS).unwrap().effect_type, 37);
        assert!(is_fixed(OUTPUT) && !is_fixed(106));
    }

    #[test]
    fn labels_for_non_blocks() {
        assert_eq!(label(0), "");
        assert_eq!(label(200), "Shunt");
        assert_eq!(label(99), "#99");
    }
}
