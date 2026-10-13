//! The names of stored presets and cabs, kept in a file on this computer
//! so that the lists are there the moment a unit connects. The unit is
//! still read afterwards, a little at a time, and what differs is corrected
//! (`names_cmds.rs`); this module is only the file.
//!
//! One file per kind of unit, in the folder `name-cache` of the data
//! folder. A file that is missing, damaged, cut short, of another version
//! or written for another unit counts as no file at all: reading never
//! fails. Writing goes to a draft beside the file that is then moved over
//! it, so a crash leaves the old file or the new one, never half of one.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// Folder of the cache files inside the data folder.
pub(crate) const FOLDER: &str = "name-cache";
const FORMAT: &str = "axegrid-name-cache";
/// Raised whenever the meaning of the file changes; older files are then ignored.
const VERSION: u64 = 1;
/// A cache file is some tens of kilobytes. Anything far larger is not one.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// The unit's names have 32 characters at most; longer text is not a name.
const MAX_NAME_CHARS: usize = 64;

/// What the app knows of the identity of a unit. The device reports no
/// serial number, so two units of one model on one firmware share a file;
/// the check against the unit after connecting sorts that out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unit {
    /// The model byte of the unit's messages.
    pub model: u8,
    /// Major and minor firmware version, when the unit told.
    pub firmware: Option<(u8, u8)>,
    /// The built-in stand-in, whose names must never pass for a unit's.
    pub simulated: bool,
}

impl Unit {
    fn file_name(&self) -> String {
        let firmware =
            self.firmware.map_or_else(|| "unknown".to_owned(), |(major, minor)| format!("{major}.{minor:02}"));
        let kind = if self.simulated { "-simulator" } else { "" };
        format!("names-{:02X}-fw{firmware}{kind}.json", self.model)
    }

    fn to_json(self) -> Value {
        json!({
            "model": self.model,
            "firmware": self.firmware.map(|(major, minor)| json!([major, minor])),
            "simulated": self.simulated,
        })
    }
}

/// How many slots a unit has. A name filed under a slot at or past these
/// is dropped when a file is read, so that nothing read from a file can
/// ever lead to a question about a slot the unit does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Preset slots, numbered from 0.
    pub presets: u16,
    /// Positions of the cab list that are cached, numbered from 0.
    pub cabs: u16,
}

/// What a cache file holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Cached {
    /// Preset names by slot.
    pub presets: BTreeMap<u16, String>,
    /// Cab names by position in the unit's cab list.
    pub cabs: BTreeMap<u16, String>,
}

/// The file that holds the names of `unit` under `folder`.
pub(crate) fn path(folder: &Path, unit: &Unit) -> PathBuf {
    folder.join(unit.file_name())
}

fn names(value: &Value, limit: u16) -> BTreeMap<u16, String> {
    let entries = value.as_object().into_iter().flatten();
    entries
        .filter_map(|(slot, name)| {
            let slot = slot.parse::<u16>().ok().filter(|&slot| slot < limit)?;
            let name = name.as_str()?;
            let plain = name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(char::is_control);
            plain.then(|| (slot, name.to_owned()))
        })
        .collect()
}

/// Reads the names kept for `unit`. `None` when there is nothing usable.
pub(crate) fn load(folder: &Path, unit: &Unit, limits: Limits) -> Option<Cached> {
    let file = path(folder, unit);
    let size = std::fs::metadata(&file).ok()?.len();
    if size == 0 || size > MAX_FILE_BYTES {
        return None;
    }
    let doc: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
    if doc["format"] != FORMAT || doc["version"].as_u64() != Some(VERSION) || doc["unit"] != unit.to_json() {
        return None;
    }
    let cached = Cached { presets: names(&doc["presets"], limits.presets), cabs: names(&doc["cabs"], limits.cabs) };
    (!(cached.presets.is_empty() && cached.cabs.is_empty())).then_some(cached)
}

fn to_object(names: &BTreeMap<u16, String>) -> Value {
    Value::Object(names.iter().map(|(slot, name)| (slot.to_string(), json!(name))).collect::<Map<String, Value>>())
}

/// Writes the names of `unit`, replacing the file in one step.
pub(crate) fn save(
    folder: &Path,
    unit: &Unit,
    presets: &BTreeMap<u16, String>,
    cabs: &BTreeMap<u16, String>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let saved = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let doc = json!({
        "format": FORMAT,
        "version": VERSION,
        "unit": unit.to_json(),
        "saved_unix": saved,
        "presets": to_object(presets),
        "cabs": to_object(cabs),
    });
    let text = serde_json::to_vec_pretty(&doc)?;
    let file = path(folder, unit);
    // The draft is this process's own, so two programs saving at once do not share one.
    let draft = folder.join(format!("{}.{}.tmp", unit.file_name(), std::process::id()));
    let written = std::fs::File::create(&draft).and_then(|mut out| {
        out.write_all(&text)?;
        // On the disk before it takes the old file's place.
        out.sync_all()
    });
    let moved = written.and_then(|()| std::fs::rename(&draft, &file));
    if moved.is_err() {
        let _ = std::fs::remove_file(&draft);
    }
    moved
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const UNIT: Unit = Unit { model: 6, firmware: Some((9, 4)), simulated: false };
    const LIMITS: Limits = Limits { presets: 768, cabs: 1213 };

    /// A folder of this test's own, removed when it goes out of scope.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("axegrid-name-cache-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn some_names() -> Cached {
        Cached {
            presets: BTreeMap::from([
                (0, "Clean".to_owned()),
                (767, "Last \"one\" / é".to_owned()),
                (5, String::new()),
            ]),
            cabs: BTreeMap::from([(0, "Factory 1".to_owned()), (189, "EMPTY".to_owned()), (1212, "Mine".to_owned())]),
        }
    }

    #[test]
    fn names_come_back_as_they_were_saved() {
        let scratch = Scratch::new("round-trip");
        let names = some_names();
        assert_eq!(load(&scratch.0, &UNIT, LIMITS), None);
        save(&scratch.0, &UNIT, &names.presets, &names.cabs).unwrap();
        assert_eq!(load(&scratch.0, &UNIT, LIMITS), Some(names.clone()));
        // Saving again replaces the file and leaves no draft behind.
        let fewer = BTreeMap::from([(1, "Only".to_owned())]);
        save(&scratch.0, &UNIT, &fewer, &BTreeMap::new()).unwrap();
        assert_eq!(load(&scratch.0, &UNIT, LIMITS), Some(Cached { presets: fewer, cabs: BTreeMap::new() }));
        let files: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(files, ["names-06-fw9.04.json"]);
    }

    #[test]
    fn another_unit_does_not_get_these_names() {
        let scratch = Scratch::new("identity");
        let names = some_names();
        save(&scratch.0, &UNIT, &names.presets, &names.cabs).unwrap();
        let others = [
            Unit { model: 7, ..UNIT },
            Unit { firmware: Some((9, 3)), ..UNIT },
            Unit { firmware: None, ..UNIT },
            Unit { simulated: true, ..UNIT },
        ];
        for other in others {
            assert_ne!(path(&scratch.0, &other), path(&scratch.0, &UNIT));
            assert_eq!(load(&scratch.0, &other, LIMITS), None);
            // Not even when its file is a copy of this unit's.
            std::fs::copy(path(&scratch.0, &UNIT), path(&scratch.0, &other)).unwrap();
            assert_eq!(load(&scratch.0, &other, LIMITS), None, "{other:?}");
        }
    }

    #[test]
    fn a_damaged_file_is_no_file() {
        let scratch = Scratch::new("damaged");
        let names = some_names();
        save(&scratch.0, &UNIT, &names.presets, &names.cabs).unwrap();
        let file = path(&scratch.0, &UNIT);
        let good = std::fs::read(&file).unwrap();
        let text = String::from_utf8(good.clone()).unwrap();
        let damaged: Vec<Vec<u8>> = vec![
            Vec::new(),
            // What a power cut left of a file on this very project.
            vec![0; good.len()],
            good[..good.len() / 2].to_vec(),
            b"not json at all".to_vec(),
            b"[1, 2, 3]".to_vec(),
            b"{}".to_vec(),
            vec![0xFF, 0xFE, 0x00, 0x7B],
            text.replace("\"version\": 1", "\"version\": 2").into_bytes(),
            text.replace("\"version\": 1", "\"version\": \"1\"").into_bytes(),
            text.replace(FORMAT, "something-else").into_bytes(),
        ];
        for bytes in damaged {
            std::fs::write(&file, &bytes).unwrap();
            assert_eq!(load(&scratch.0, &UNIT, LIMITS), None, "{:?}", String::from_utf8_lossy(&bytes));
        }
        // A folder where the file should be is no file either.
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir_all(&file).unwrap();
        assert_eq!(load(&scratch.0, &UNIT, LIMITS), None);
    }

    #[test]
    fn entries_that_cannot_be_names_are_dropped() {
        let scratch = Scratch::new("entries");
        std::fs::create_dir_all(&scratch.0).unwrap();
        let doc = json!({
            "format": FORMAT, "version": VERSION, "unit": UNIT.to_json(),
            "presets": { "0": "Kept", "768": "past the last slot", "9999": "far past", "-1": "negative",
                         "x": "no number", "3": 7, "4": "x".repeat(65), "5": "bell\u{7}", "6": "Also kept" },
            "cabs": [1, 2, 3],
        });
        std::fs::write(path(&scratch.0, &UNIT), serde_json::to_vec(&doc).unwrap()).unwrap();
        let cached = load(&scratch.0, &UNIT, LIMITS).unwrap();
        assert_eq!(cached.presets, BTreeMap::from([(0, "Kept".to_owned()), (6, "Also kept".to_owned())]));
        assert!(cached.cabs.is_empty());
        // A model with fewer slots takes fewer names from the same text.
        let small = Limits { presets: 6, cabs: 0 };
        assert_eq!(load(&scratch.0, &UNIT, small).unwrap().presets.len(), 1);
    }
}
