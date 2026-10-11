//! The preset and cabinet library commands, and the curves, end to end
//! against the simulated device.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use axegrid_engine::{CommandError, Engine};
use serde_json::{json, Value};

fn connected() -> Engine {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    engine
}

/// A folder of this test's own, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("axegrid-library-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preset_names_are_read_without_loading_anything() {
    let mut engine = connected();
    let read = engine.run("preset.names", &Value::Null).unwrap();
    // The simulator holds three presets and refuses the fourth, which ends the list.
    let names: Vec<&str> = read["names"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Clean Chorus", "Crunch", "Lead Delay"]);
    assert_eq!((read["known"].as_u64(), read["slots"].as_u64()), (Some(3), Some(768)));
    assert_eq!(engine.inspect()["preset"]["number"], 0);
    assert_eq!(engine.inspect()["library"]["preset_names_known"], 3);

    let one = engine.run("preset.names", &json!({ "first": 1, "count": 1 })).unwrap();
    assert_eq!(one["names"], json!([{ "number": 1, "name": "Crunch" }]));
    assert_eq!(engine.state().preset_names.get(&2).map(String::as_str), Some("Lead Delay"));
}

#[test]
fn a_preset_goes_to_a_file_and_back_into_the_edit_buffer() {
    let scratch = Scratch::new("preset");
    let mut engine = connected();
    let path = scratch.file("lead.syx");
    let exported = engine.run("preset.export", &json!({ "number": 2, "path": path })).unwrap();
    assert_eq!((exported["name"].as_str(), exported["number"].as_u64()), (Some("Lead Delay"), Some(2)));
    let report = engine.run("sysex.inspect_file", &json!({ "path": path })).unwrap();
    assert_eq!((report["kind"].as_str(), report["name"].as_str()), (Some("preset"), Some("Lead Delay")));
    assert_eq!(report["problem"], Value::Null);

    // Importing replaces what is being edited, on the preset that is selected.
    let imported = engine.run("preset.import", &json!({ "path": path })).unwrap();
    assert_eq!(imported["preset"], json!({ "number": 0, "name": "Lead Delay", "scene": 1 }));
    assert_eq!(engine.inspect()["blocks"][0]["name"], "Wah 1");
}

#[test]
fn damaged_and_mismatched_files_are_refused() {
    let scratch = Scratch::new("bad");
    let mut engine = connected();
    let path = scratch.file("crunch.syx");
    engine.run("preset.export", &json!({ "number": 1, "path": path })).unwrap();

    // A preset is not a cabinet.
    let err = engine.run("cab.import", &json!({ "cab": 2, "path": path })).unwrap_err();
    assert!(matches!(&err, CommandError::Io(text) if text.contains("holds a preset")), "{err}");

    // One changed data byte, with its message checksum repaired, fails the dump's own checksum.
    let mut bytes = std::fs::read(&path).unwrap();
    let second = bytes.iter().enumerate().filter(|(_, &b)| b == 0xF0).nth(1).unwrap().0;
    let end = second + bytes[second..].iter().position(|&b| b == 0xF7).unwrap();
    bytes[second + 9] ^= 1;
    bytes[end - 1] = axegrid_sysex::checksum(&bytes[second..end - 1]);
    std::fs::write(&path, &bytes).unwrap();
    let err = engine.run("preset.import", &json!({ "path": path })).unwrap_err();
    assert!(matches!(&err, CommandError::Io(text) if text.contains("checksum")), "{err}");
    assert_eq!(engine.inspect()["preset"]["name"], "Clean Chorus");

    let err = engine.run("preset.import", &json!({ "path": scratch.file("missing.syx") })).unwrap_err();
    assert!(matches!(err, CommandError::Io(_)));
}

#[test]
fn copying_a_preset_overwrites_the_target_and_selects_it() {
    let mut engine = connected();
    engine.run("preset.names", &Value::Null).unwrap();
    let copied = engine.run("preset.copy", &json!({ "from": 2, "to": 1 })).unwrap();
    assert_eq!(copied, json!({ "from": 2, "to": 1, "name": "Lead Delay" }));
    assert_eq!(engine.inspect()["preset"]["number"], 1);
    assert_eq!(engine.state().preset_names.get(&1).map(String::as_str), Some("Lead Delay"));
    // The device agrees.
    assert_eq!(
        engine.run("preset.names", &json!({ "first": 1, "count": 1 })).unwrap()["names"][0]["name"],
        "Lead Delay"
    );

    let same = engine.run("preset.copy", &json!({ "from": 1, "to": 1 })).unwrap_err();
    assert!(matches!(same, CommandError::BadParam { name: "to", .. }));
    assert!(engine.run("preset.copy", &json!({ "from": 0, "to": 9999 })).is_err());
}

#[test]
fn saving_to_another_slot_updates_the_known_names() {
    let mut engine = connected();
    engine.run("preset.rename", &json!({ "name": "Mine" })).unwrap();
    engine.run("preset.store", &json!({ "number": 2 })).unwrap();
    assert_eq!(engine.state().preset_names.get(&2).map(String::as_str), Some("Mine"));
}

#[test]
fn a_backup_writes_one_file_per_named_preset() {
    let scratch = Scratch::new("backup");
    let mut engine = connected();
    let folder = scratch.file("presets");
    let done = engine.run("preset.export_range", &json!({ "folder": folder })).unwrap();
    assert_eq!(done["written"], 3);
    let mut files: Vec<String> =
        std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["000 Clean Chorus.syx", "001 Crunch.syx", "002 Lead Delay.syx"]);
    let slice = engine.run("preset.export_range", &json!({ "folder": folder, "first": 1, "count": 1 })).unwrap();
    assert_eq!((slice["written"].as_u64(), slice["files"][0]["number"].as_u64()), (Some(1), Some(1)));
}

#[test]
fn user_cabs_are_listed_exported_and_imported() {
    let scratch = Scratch::new("cab");
    let mut engine = connected();
    let listed = engine.run("cab.names", &json!({ "count": 3 })).unwrap();
    assert_eq!(listed["slots"], 1024);
    assert_eq!(listed["names"][0], json!({ "cab": 1, "name": "Sim 4x12", "empty": false }));
    assert_eq!(listed["names"][1], json!({ "cab": 2, "name": "EMPTY", "empty": true }));

    let path = scratch.file("cab.syx");
    let exported = engine.run("cab.export", &json!({ "cab": 1, "path": path })).unwrap();
    assert_eq!(exported["name"], "Sim 4x12");
    let report = engine.run("sysex.inspect_file", &json!({ "path": path })).unwrap();
    assert_eq!((report["kind"].as_str(), report["name"].as_str()), (Some("cabinet IR"), Some("Sim 4x12")));

    let imported = engine.run("cab.import", &json!({ "cab": 700, "path": path })).unwrap();
    assert_eq!(imported, json!({ "cab": 700, "name": "Sim 4x12", "stored_as": "Sim 4x12" }));
    let again = engine.run("cab.names", &json!({ "first": 700, "count": 1 })).unwrap();
    assert_eq!(again["names"][0]["name"], "Sim 4x12");
    // The neighbours are untouched.
    assert_eq!(engine.run("cab.names", &json!({ "first": 699, "count": 1 })).unwrap()["names"][0]["empty"], true);

    for bad in [json!({ "cab": 0, "path": path }), json!({ "cab": 1025, "path": path }), json!({ "path": path })] {
        assert!(matches!(engine.run("cab.export", &bad), Err(CommandError::BadParam { name: "cab", .. })));
    }
}

#[test]
fn cab_commands_say_so_when_a_model_is_not_known() {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &json!({ "model": "II" })).unwrap();
    let err = engine.run("cab.names", &Value::Null).unwrap_err();
    assert!(matches!(err, CommandError::NotFound(_)));
    assert_eq!(engine.run("preset.names", &Value::Null).unwrap()["slots"], 384);
}

/// The graph of the page called `page` of a freshly placed block.
fn graph_of(block: &str, page: &str) -> Value {
    let mut engine = connected();
    engine.run("grid.place_block", &json!({ "block": block, "column": 8, "row": 3 })).unwrap();
    engine.run("block.select", &json!({ "block": block })).unwrap();
    let editor = engine.inspect()["editor"].clone();
    let pages = editor["pages"].as_array().unwrap();
    let found = pages.iter().find(|p| p["name"] == page).unwrap_or_else(|| panic!("{block} has no page {page}"));
    found["graph"].clone()
}

#[test]
fn every_block_with_a_curve_draws_one() {
    for (block, page) in [
        ("Amp 1", "Speaker"),
        ("Cab 1", "Cabinet"),
        ("Reverb 1", "EQ"),
        ("Delay 1", "EQ"),
        ("Filter 1", "Filter"),
        ("Crossover 1", "Crossover"),
    ] {
        let graph = graph_of(block, page);
        assert_eq!(graph["kind"], "eq", "{block}");
        assert_eq!(graph["points"].as_array().map(Vec::len), Some(96), "{block}");
    }
    assert_eq!(graph_of("Crossover 1", "Crossover")["also"].as_array().map(Vec::len), Some(96));
    for (block, page) in [("Compressor 1", "Studio"), ("Gate/Expander 1", "Gate"), ("Multiband Comp 1", "Bands")] {
        let graph = graph_of(block, page);
        assert_eq!(graph["kind"], "dynamics", "{block}");
        assert!(graph["threshold"].is_number(), "{block}");
    }
    // Pages without a curve stay without one.
    assert_eq!(graph_of("Amp 1", "Basic"), Value::Null);
}

#[test]
fn fixed_blocks_have_their_curves_too() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": 141 })).unwrap();
    let editor = engine.inspect()["editor"].clone();
    let envelope = editor["pages"].as_array().unwrap().iter().find(|p| p["name"] == "ADSR 1").unwrap();
    assert_eq!(envelope["graph"]["kind"], "envelope");
    assert_eq!(envelope["graph"]["points"].as_array().map(Vec::len), Some(5));

    engine.run("block.select", &json!({ "block": 139 })).unwrap();
    let gate = engine.inspect()["editor"]["pages"][0]["graph"].clone();
    assert_eq!(gate["kind"], "dynamics");
}

#[test]
fn finding_a_unit_is_only_offered_while_disconnected() {
    let engine = connected();
    assert_eq!(engine.disabled_reason("device.connect_auto"), Some("a device is already connected"));
    assert_eq!(Engine::new().disabled_reason("device.connect_auto"), None);
}

#[test]
fn a_typed_value_is_set_in_the_unit_it_is_displayed_in() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    // The simulator displays every value from 0.00 to 10.00.
    let set = engine.run("param.set_displayed", &json!({ "param": "DISTORT_DRIVE", "value": 7.5 })).unwrap();
    assert_eq!((set["text"].as_str(), set["value"].as_u64()), (Some("7.50"), Some(49151)));
    // The editor shows it, and a value past the end is limited by the device.
    let shown = engine.inspect()["params"].as_array().unwrap().iter().find(|p| p["name"] == "DISTORT_DRIVE").cloned();
    assert_eq!(shown.unwrap()["text"], "7.50");
    let limited = engine.run("param.set_displayed", &json!({ "param": "DISTORT_DRIVE", "value": 99 })).unwrap();
    assert_eq!(limited["text"], "10.00");

    for bad in [json!({ "param": "DISTORT_DRIVE" }), json!({ "param": "DISTORT_DRIVE", "value": "loud" })] {
        let err = engine.run("param.set_displayed", &bad).unwrap_err();
        assert!(matches!(err, CommandError::BadParam { name: "value", .. }));
    }
}
