//! Parameter metadata and the on-disk definition cache (spec section 6).

use std::fmt;

/// Raw parameter values span `0..=RAW_MAX`.
pub const RAW_MAX: u16 = 65534;
/// Type word of an enumerated parameter.
pub const TYPE_ENUM: u32 = 0x10;

const HEADER_LEN: usize = 46;
const RESERVED_LEN: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    pub id: u16,
    pub type_word: u32,
    pub min: f32,
    pub max: f32,
    /// Multiplier applied to the engineering value for display.
    pub scale: f32,
    pub step: f32,
    pub strings: Vec<String>,
    /// Six trailing bytes of unknown meaning, kept for exact round trips.
    pub reserved: [u8; RESERVED_LEN],
}

impl ParamDef {
    pub fn is_enum(&self) -> bool {
        self.type_word == TYPE_ENUM || !self.strings.is_empty()
    }

    /// Text for a raw value when the device's own formatting is not at hand.
    /// Enumerations are exact; continuous values assume a linear taper, which
    /// is only an approximation for logarithmic parameters.
    pub fn describe(&self, raw: u16) -> String {
        if self.is_enum() {
            return match self.strings.get(usize::from(raw)) {
                Some(text) => text.trim().to_owned(),
                None => raw.to_string(),
            };
        }
        let position = f32::from(raw) / f32::from(RAW_MAX);
        let value = (self.min + (self.max - self.min) * position) * self.scale;
        let decimals = match self.step * self.scale {
            s if s >= 1.0 || s <= 0.0 => 0,
            s if s >= 0.1 => 1,
            s if s >= 0.01 => 2,
            _ => 3,
        };
        format!("{value:.decimals$}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectDef {
    pub effect_type: u32,
    pub params: Vec<ParamDef>,
}

impl EffectDef {
    pub fn param(&self, id: u16) -> Option<&ParamDef> {
        self.params.iter().find(|p| p.id == id)
    }
}

/// Contents of an `effectDefinitions_<model>.cache` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Definitions {
    /// The 46-byte header; its last four bytes hold the effect count and are
    /// rewritten on save.
    pub header: Vec<u8>,
    pub effects: Vec<EffectDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefsError {
    Truncated {
        offset: usize,
    },
    /// A count or length field is larger than the bytes that follow it.
    Implausible {
        offset: usize,
    },
}

impl fmt::Display for DefsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset } => write!(f, "definition cache ends early at byte {offset}"),
            Self::Implausible { offset } => {
                write!(f, "definition cache has an impossible length at byte {offset}")
            }
        }
    }
}

impl std::error::Error for DefsError {}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], DefsError> {
        let end = self.at.checked_add(len).filter(|&end| end <= self.data.len());
        let end = end.ok_or(DefsError::Truncated { offset: self.at })?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DefsError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16, DefsError> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, DefsError> {
        self.array().map(u32::from_le_bytes)
    }

    fn f32(&mut self) -> Result<f32, DefsError> {
        self.u32().map(f32::from_bits)
    }

    /// A count whose items each take at least `min_item` bytes.
    fn count(&mut self, min_item: usize) -> Result<usize, DefsError> {
        let offset = self.at;
        let count = self.u32()? as usize;
        if count.saturating_mul(min_item) > self.data.len() - self.at {
            return Err(DefsError::Implausible { offset });
        }
        Ok(count)
    }
}

impl Definitions {
    pub fn effect(&self, effect_type: u32) -> Option<&EffectDef> {
        self.effects.iter().find(|e| e.effect_type == effect_type)
    }

    pub fn parse(data: &[u8]) -> Result<Self, DefsError> {
        let mut cursor = Cursor { data, at: 0 };
        let header = cursor.take(HEADER_LEN)?.to_vec();
        let mut effects = Vec::new();
        while cursor.at < data.len() {
            let effect_type = cursor.u32()?;
            let count = cursor.count(2 + 4 + 16 + 4 + RESERVED_LEN)?;
            let mut params = Vec::with_capacity(count);
            for _ in 0..count {
                let id = cursor.u16()?;
                let type_word = cursor.u32()?;
                let (min, max) = (cursor.f32()?, cursor.f32()?);
                let (scale, step) = (cursor.f32()?, cursor.f32()?);
                let string_count = cursor.count(4)?;
                let mut strings = Vec::with_capacity(string_count);
                for _ in 0..string_count {
                    let len = cursor.count(1)?;
                    strings.push(String::from_utf8_lossy(cursor.take(len)?).into_owned());
                }
                let reserved = cursor.array()?;
                params.push(ParamDef { id, type_word, min, max, scale, step, strings, reserved });
            }
            effects.push(EffectDef { effect_type, params });
        }
        Ok(Self { header, effects })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header.clone();
        out.resize(HEADER_LEN, 0);
        out[HEADER_LEN - 4..].copy_from_slice(&(self.effects.len() as u32).to_le_bytes());
        for effect in &self.effects {
            out.extend_from_slice(&effect.effect_type.to_le_bytes());
            out.extend_from_slice(&(effect.params.len() as u32).to_le_bytes());
            for p in &effect.params {
                out.extend_from_slice(&p.id.to_le_bytes());
                out.extend_from_slice(&p.type_word.to_le_bytes());
                for value in [p.min, p.max, p.scale, p.step] {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.extend_from_slice(&(p.strings.len() as u32).to_le_bytes());
                for text in &p.strings {
                    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                    out.extend_from_slice(text.as_bytes());
                }
                out.extend_from_slice(&p.reserved);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Definitions {
        let knob = ParamDef {
            id: 0,
            type_word: 0x131,
            min: -80.0,
            max: 0.0,
            scale: 1.0,
            step: 0.1,
            strings: vec![],
            reserved: [0; 6],
        };
        let knee = ParamDef {
            id: 5,
            type_word: TYPE_ENUM,
            min: 0.0,
            max: 3.0,
            scale: 0.0,
            step: 0.0,
            strings: ["HARD", "SOFT", "SOFTER", "SOFTEST"].map(String::from).to_vec(),
            reserved: [0; 6],
        };
        let mut header = vec![0u8; HEADER_LEN];
        header[0] = 0x20;
        header[HEADER_LEN - 4] = 1;
        Definitions { header, effects: vec![EffectDef { effect_type: 2, params: vec![knob, knee] }] }
    }

    #[test]
    fn round_trips() {
        let defs = sample();
        let bytes = defs.to_bytes();
        assert_eq!(Definitions::parse(&bytes).unwrap(), defs);
        // 46 header + 8 effect header + 32 plain record + (32 + 4*4 + 21) enum record
        assert_eq!(bytes.len(), 46 + 8 + 32 + 69);
    }

    #[test]
    fn rejects_damage_without_panicking() {
        let bytes = sample().to_bytes();
        assert_eq!(
            Definitions::parse(&bytes[..bytes.len() - 3]),
            Err(DefsError::Truncated { offset: bytes.len() - 6 })
        );
        let mut huge = bytes.clone();
        huge[50..54].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Definitions::parse(&huge), Err(DefsError::Implausible { offset: 50 }));
        assert!(Definitions::parse(&[]).is_err());
    }

    #[test]
    fn describes_values() {
        let defs = sample();
        let effect = defs.effect(2).unwrap();
        assert_eq!(effect.param(5).unwrap().describe(2), "SOFTER");
        assert_eq!(effect.param(5).unwrap().describe(9), "9");
        assert_eq!(effect.param(0).unwrap().describe(0), "-80.0");
        assert_eq!(effect.param(0).unwrap().describe(RAW_MAX), "0.0");
        assert_eq!(effect.param(0).unwrap().describe(RAW_MAX / 2), "-40.0");
    }

    /// Runs against the cache written by a real Axe-Edit install, when present.
    #[test]
    fn parses_an_installed_cache_exactly() {
        let Some(appdata) = std::env::var_os("APPDATA") else { return };
        let dir = std::path::Path::new(&appdata).join("Fractal Audio").join("Axe-Edit");
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "cache") {
                let bytes = std::fs::read(&path).unwrap();
                let defs = Definitions::parse(&bytes).unwrap();
                assert!(!defs.effects.is_empty());
                assert_eq!(defs.to_bytes(), bytes, "{} must round-trip", path.display());
            }
        }
    }
}
