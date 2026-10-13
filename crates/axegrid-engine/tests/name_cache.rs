//! Preset and cab names kept between sessions, end to end against the
//! simulated device. Every engine here keeps its data in a folder of the
//! test's own, never in the user's.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use axegrid_engine::{CommandError, Engine};
use serde_json::{json, Value};

/// Positions in the simulator's cab list that are cached: 189 factory cabs and 1024 user cabs.
const CABS: u64 = 1213;
/// The simulator's XL claims 768 preset slots and holds three presets.
const PRESETS: u64 = 3;

/// A folder of this test's own, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("axegrid-names-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// An engine that keeps its data here, connected to a fresh simulator.
    fn connect(&self, model: &str) -> Engine {
        let mut engine = Engine::with_data_dir(&self.0);
        engine.run("device.connect_simulator", &json!({ "model": model })).unwrap();
        engine
    }

    /// The cache files written so far.
    fn files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.0.join("name-cache")) else { return Vec::new() };
        let mut files: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
        files.sort();
        files
    }

    /// Rewrites the one cache file as text.
    fn edit(&self, change: impl FnOnce(String) -> String) {
        let files = self.files();
        assert_eq!(files.len(), 1, "{files:?}");
        let text = std::fs::read_to_string(&files[0]).unwrap();
        std::fs::write(&files[0], change(text)).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(engine: &Engine) -> Value {
    engine.inspect()["library"]["names"].clone()
}

fn preset(engine: &Engine, number: u16) -> Option<&str> {
    engine.state().preset_names.get(&number).map(String::as_str)
}

fn cab(engine: &Engine, number: u16) -> Option<&str> {
    engine.state().cab_names.get(&number).map(String::as_str)
}

/// A session that reads everything, so that the folder holds a full cache.
fn fill(scratch: &Scratch) {
    let mut engine = scratch.connect("XL");
    assert_eq!(engine.run("library.scan", &Value::Null).unwrap()["phase"], "verified");
    engine.run("device.disconnect", &Value::Null).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)]
fn names_are_there_at_once_the_second_time() {
    let scratch = Scratch::new("second-time");
    let mut first = scratch.connect("XL");
    // Nothing is known of a unit seen for the first time, and nothing was asked for yet.
    assert_eq!(names(&first)["phase"], "reading");
    assert_eq!((names(&first)["checked"].as_u64(), names(&first)["total"].as_u64()), (Some(0), Some(768 + CABS)));
    assert!(first.state().preset_names.is_empty() && first.state().cab_names.is_empty());
    assert!(scratch.files().is_empty());

    // The names arrive a few at a time, from where the last call stopped.
    let step = first.run("library.scan", &json!({ "budget": 2 })).unwrap();
    assert_eq!((step["read"].as_u64(), step["phase"].as_str()), (Some(2), Some("reading")));
    assert_eq!(first.state().preset_names.len(), 2);
    assert_eq!(first.state().name_scan.presets_checked, 2);
    // The third preset, the refusal that ends the simulator's list, and four cabs.
    let step = first.run("library.scan", &json!({ "budget": 6 })).unwrap();
    assert_eq!(step["presets"], json!({ "checked": 3, "slots": 3, "done": true }));
    assert_eq!(step["cabs"]["checked"], 4);
    // What was read so far is already on disk, should the session end here.
    assert_eq!(scratch.files().len(), 1);

    let done = first.run("library.scan", &Value::Null).unwrap();
    assert_eq!((done["phase"].as_str(), done["verified"].as_bool()), (Some("verified"), Some(true)));
    assert_eq!((done["checked"].as_u64(), done["total"].as_u64()), (Some(PRESETS + CABS), Some(PRESETS + CABS)));
    assert_eq!((done["changed"].as_u64(), done["from_cache"].as_bool()), (Some(0), Some(false)));
    assert_eq!(first.state().cab_names.len(), 1024);
    assert!(first.state().name_scan.verified());
    // Once everything is read there is nothing left to ask.
    assert_eq!(first.run("library.scan", &Value::Null).unwrap()["read"], 0);
    first.run("device.disconnect", &Value::Null).unwrap();
    assert_eq!(names(&first)["phase"], "none");

    // The next session has the names before it has asked for a single one.
    let mut second = scratch.connect("XL");
    let before = names(&second);
    assert_eq!((before["phase"].as_str(), before["checked"].as_u64()), (Some("cached"), Some(0)));
    assert_eq!((before["from_cache"].as_bool(), before["verified"].as_bool()), (Some(true), Some(false)));
    assert!(before["cache_file"].as_str().unwrap().ends_with("names-06-fw9.03-simulator.json"));
    assert_eq!((preset(&second, 0), preset(&second, 2)), (Some("Clean Chorus"), Some("Lead Delay")));
    assert_eq!(
        (cab(&second, 1), cab(&second, 2), cab(&second, 1024)),
        (Some("Sim 4x12"), Some("EMPTY"), Some("EMPTY"))
    );
    assert_eq!(second.inspect()["library"]["preset_names_known"], 3);
    assert_eq!(second.inspect()["library"]["cab_names_known"], 1024);

    // The cab picker has its list without reading the cabs again: only the
    // few entries past the user cabs, which are not kept, are asked for.
    second.run("grid.place_block", &json!({ "block": "Cab 1", "column": 8, "row": 3 })).unwrap();
    second.run("block.select", &json!({ "block": "Cab 1" })).unwrap();
    let lists = second.run("block.read_lists", &Value::Null).unwrap();
    assert!(lists["requests"].as_u64().unwrap() < 40, "{lists}");
    assert_eq!(lists["missing"], 0);

    // Checking finds nothing to correct and ends verified.
    let checked = second.run("library.scan", &Value::Null).unwrap();
    assert_eq!((checked["phase"].as_str(), checked["changed"].as_u64()), (Some("verified"), Some(0)));
    assert_eq!(checked["corrected"], json!([]));
}

#[test]
fn what_changed_on_the_device_is_corrected() {
    let scratch = Scratch::new("corrected");
    fill(&scratch);
    // As if the unit had been changed since: the file remembers other names,
    // and a preset in a slot that no longer exists.
    scratch.edit(|text| {
        assert!(text.contains("\"Crunch\"") && text.contains("\"Sim 4x12\"") && text.contains("\"2\": \"Lead Delay\""));
        text.replace("\"Crunch\"", "\"Old Crunch\"")
            .replace("\"Sim 4x12\"", "\"Old Cab\"")
            .replace("\"2\": \"Lead Delay\"", "\"2\": \"Lead Delay\", \"3\": \"Gone\"")
    });

    let mut engine = scratch.connect("XL");
    // The remembered names are shown first, wrong as they are.
    assert_eq!(
        (preset(&engine, 1), preset(&engine, 3), cab(&engine, 1)),
        (Some("Old Crunch"), Some("Gone"), Some("Old Cab"))
    );
    assert_eq!(names(&engine)["phase"], "cached");

    // Checking the presets corrects the one that differs and drops the one that is gone.
    let presets = engine.run("library.scan", &json!({ "budget": 4 })).unwrap();
    assert_eq!(presets["corrected"], json!([{ "list": "presets", "slot": 1, "was": "Old Crunch", "name": "Crunch" }]));
    assert_eq!((preset(&engine, 1), preset(&engine, 3)), (Some("Crunch"), None));
    assert_eq!((presets["phase"].as_str(), presets["changed"].as_u64()), (Some("cached"), Some(1)));
    // The cab is still the remembered one until its turn comes.
    assert_eq!(cab(&engine, 1), Some("Old Cab"));
    let rest = engine.run("library.scan", &Value::Null).unwrap();
    assert_eq!(rest["corrected"], json!([{ "list": "cabs", "slot": 189, "was": "Old Cab", "name": "Sim 4x12" }]));
    assert_eq!(
        (cab(&engine, 1), rest["phase"].as_str(), rest["changed"].as_u64()),
        (Some("Sim 4x12"), Some("verified"), Some(2))
    );

    // And the file was put right, for next time.
    let mut later = scratch.connect("XL");
    assert_eq!((preset(&later, 1), preset(&later, 3), cab(&later, 1)), (Some("Crunch"), None, Some("Sim 4x12")));
    assert_eq!(later.run("library.scan", &Value::Null).unwrap()["changed"], 0);
}

#[test]
fn a_damaged_file_is_ignored_and_then_replaced() {
    let scratch = Scratch::new("damaged");
    fill(&scratch);
    let file = scratch.files().remove(0);
    let good = std::fs::read(&file).unwrap();
    let damaged: [Vec<u8>; 5] = [
        Vec::new(),
        vec![0; good.len()],
        good[..good.len() / 3].to_vec(),
        b"{ \"format\": \"axegrid-name-cache\", \"version\": 99 }".to_vec(),
        b"\xFF\xFEgarbage".to_vec(),
    ];
    for bytes in damaged {
        std::fs::write(&file, &bytes).unwrap();
        let mut engine = scratch.connect("XL");
        assert_eq!(names(&engine)["phase"], "reading", "{:?}", String::from_utf8_lossy(&bytes));
        assert!(engine.state().preset_names.is_empty() && engine.state().cab_names.is_empty());
        // Reading goes on as if there had never been a file, and writes a good one.
        assert_eq!(engine.run("library.scan", &Value::Null).unwrap()["phase"], "verified");
        assert_eq!(names(&scratch.connect("XL"))["phase"], "cached");
    }
}

#[test]
fn another_unit_does_not_get_these_names() {
    let scratch = Scratch::new("identity");
    fill(&scratch);
    // Another model starts with nothing and gets a file of its own.
    let mut plus = scratch.connect("XL+");
    assert_eq!(names(&plus)["phase"], "reading");
    assert!(plus.state().preset_names.is_empty() && plus.state().cab_names.is_empty());
    plus.run("library.scan", &Value::Null).unwrap();
    assert_eq!(scratch.files().len(), 2);

    // Neither does a unit of the same model on other firmware, even when
    // its file is a copy of this unit's: the file says whose names it holds.
    let file = scratch.files().remove(0);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"simulated\": true"), "{text}");
    std::fs::write(&file, text.replace("\"simulated\": true", "\"simulated\": false")).unwrap();
    let other = scratch.connect("XL");
    assert_eq!(names(&other)["phase"], "reading");
    assert!(other.state().preset_names.is_empty());
}

#[test]
fn a_model_without_known_cab_memory_is_never_asked_for_cabs() {
    let scratch = Scratch::new("mark-two");
    let mut engine = scratch.connect("II");
    assert_eq!(names(&engine)["cabs"], json!({ "checked": 0, "slots": 0, "done": true }));
    let done = engine.run("library.scan", &Value::Null).unwrap();
    // Three names and the refusal that ends the list: four requests, none of them for a cab.
    assert_eq!((done["phase"].as_str(), done["read"].as_u64()), (Some("verified"), Some(3)));
    assert!(engine.state().cab_names.is_empty());
    assert_eq!(names(&scratch.connect("II"))["phase"], "cached");
}

#[test]
fn what_the_app_stores_is_remembered_without_a_rescan() {
    let scratch = Scratch::new("own-actions");
    fill(&scratch);
    let mut engine = scratch.connect("XL");
    engine.run("preset.rename", &json!({ "name": "Mine" })).unwrap();
    // A new name is only in the edit buffer until it is stored.
    assert_eq!(preset(&engine, 0), Some("Clean Chorus"));
    engine.run("preset.store", &json!({ "number": 2 })).unwrap();
    engine.run("preset.copy", &json!({ "from": 1, "to": 0 })).unwrap();
    let path = scratch.0.join("cab.syx").display().to_string();
    engine.run("cab.export", &json!({ "cab": 1, "path": path })).unwrap();
    engine.run("cab.import", &json!({ "cab": 700, "path": path })).unwrap();
    assert_eq!(
        (preset(&engine, 0), preset(&engine, 2), cab(&engine, 700)),
        (Some("Crunch"), Some("Mine"), Some("Sim 4x12"))
    );
    // No scan and no disconnect: the session ends as if the program had been closed.
    drop(engine);

    // The simulator starts afresh, so only the file can know these names.
    let later = scratch.connect("XL");
    assert_eq!(
        (preset(&later, 0), preset(&later, 2), cab(&later, 700)),
        (Some("Crunch"), Some("Mine"), Some("Sim 4x12"))
    );
    assert_eq!(names(&later)["phase"], "cached");
}

#[test]
fn a_rescan_starts_over_and_keeps_the_names_on_show() {
    let scratch = Scratch::new("rescan");
    fill(&scratch);
    let mut engine = scratch.connect("XL");
    engine.run("library.scan", &Value::Null).unwrap();
    assert_eq!(names(&engine)["phase"], "verified");

    // Starting over reads nothing by itself when told to leave the reading to library.scan.
    let again = engine.run("library.rescan", &json!({ "budget": 0 })).unwrap();
    assert_eq!(
        (again["read"].as_u64(), again["checked"].as_u64(), again["phase"].as_str()),
        (Some(0), Some(0), Some("cached"))
    );
    assert_eq!((preset(&engine, 1), engine.state().cab_names.len()), (Some("Crunch"), 1024));
    // The slot the unit refused is not asked about again: the list ends where it ended.
    assert_eq!(again["presets"]["slots"], 3);
    // The cabs can be taken first, for a window that shows them.
    let cabs = engine.run("library.scan", &json!({ "budget": 5, "first": "cabs" })).unwrap();
    assert_eq!((cabs["cabs"]["checked"].as_u64(), cabs["presets"]["checked"].as_u64()), (Some(5), Some(0)));
    // Without a budget a rescan reads everything.
    let full = engine.run("library.rescan", &Value::Null).unwrap();
    assert_eq!((full["phase"].as_str(), full["read"].as_u64()), (Some("verified"), Some(PRESETS + CABS)));

    for bad in [json!({ "first": "amps" }), json!({ "budget": -1 }), json!({ "budget": "all" })] {
        assert!(matches!(engine.run("library.scan", &bad), Err(CommandError::BadParam { .. })), "{bad}");
    }
}

#[test]
fn the_commands_need_a_unit_and_an_engine_without_a_folder_writes_nothing() {
    let mut engine = Engine::new();
    for id in ["library.scan", "library.rescan"] {
        assert_eq!(engine.disabled_reason(id), Some("no device connected"));
        assert!(matches!(engine.run(id, &Value::Null), Err(CommandError::Disabled { .. })));
    }
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    let done = engine.run("library.scan", &Value::Null).unwrap();
    assert_eq!((done["phase"].as_str(), &done["cache_file"]), (Some("verified"), &Value::Null));
}
