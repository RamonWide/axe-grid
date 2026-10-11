//! Commands end to end against the simulated device.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axegrid_engine::{find, registry, CommandError, Engine};
use serde_json::{json, Value};

fn connected() -> Engine {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    engine
}

fn block_state(engine: &Engine, block: u64) -> Value {
    let state = engine.inspect();
    let found = state["blocks"].as_array().unwrap().iter().find(|b| b["block"] == block);
    found.unwrap().clone()
}

#[test]
fn registry_is_well_formed() {
    let ids: Vec<&str> = registry().iter().map(|c| c.id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "command ids must be unique");
    for command in registry() {
        assert!(command.id.contains('.'), "{} needs a namespace", command.id);
        assert!(!command.label.is_empty() && !command.doc.is_empty(), "{} is undocumented", command.id);
        // A menu item cannot prompt for required parameters.
        let promptless = command.params.iter().all(|p| !p.required);
        assert!(command.menu.is_empty() || promptless, "{} is a menu item with required params", command.id);
    }
    assert!(find("preset.next").is_some());
}

#[test]
fn connecting_reads_the_preset() {
    let engine = connected();
    let state = engine.inspect();
    assert_eq!(state["connected"], true);
    assert_eq!(state["model"], "Axe-Fx II XL");
    assert_eq!(state["firmware"], "9.03");
    assert_eq!(state["preset"], json!({ "number": 0, "name": "Clean Chorus", "scene": 1 }));
    assert_eq!(state["grid"].as_array().unwrap().len(), 12);
    assert_eq!(state["blocks"][1]["name"], "Amp 1");
}

#[test]
fn device_commands_are_disabled_until_connected() {
    let mut engine = Engine::new();
    let err = engine.run("preset.next", &Value::Null).unwrap_err();
    assert!(matches!(err, CommandError::Disabled { id: "preset.next", .. }));
    assert_eq!(engine.disabled_reason("preset.next"), Some("no device connected"));
    assert_eq!(engine.disabled_reason("catalog.blocks"), None);
}

#[test]
fn preset_and_scene_navigation() {
    let mut engine = connected();
    assert_eq!(engine.run("preset.next", &Value::Null).unwrap()["name"], "Crunch");
    assert_eq!(engine.run("preset.select", &json!({ "number": 2 })).unwrap()["name"], "Lead Delay");
    assert_eq!(engine.run("scene.previous", &Value::Null).unwrap()["scene"], 8);
    assert_eq!(engine.run("scene.select", &json!({ "scene": 3 })).unwrap()["scene"], 3);
    engine.run("preset.rename", &json!({ "name": "Solo" })).unwrap();
    assert_eq!(engine.inspect()["preset"]["name"], "Solo");
    assert_eq!(engine.run("preset.store", &Value::Null).unwrap()["stored_to"], 2);
}

#[test]
fn editing_a_parameter_by_name() {
    let mut engine = connected();
    let selected = engine.run("block.select", &json!({ "block": "amp 1" })).unwrap();
    assert_eq!(selected["selected_block"], 106);
    assert_eq!(selected["params"], 118);

    // `block` defaults to the selection; `param` may be symbolic.
    let set = engine.run("param.set", &json!({ "param": "DISTORT_DRIVE", "value": 65534 })).unwrap();
    assert_eq!(set["param"], 1);
    assert_eq!(set["label"], "Drive");
    assert_eq!(set["text"], "10.00");
    assert_eq!(engine.state().params[1].value, 65534);

    let nudged = engine.run("param.nudge", &json!({ "param": 1, "steps": -50 })).unwrap();
    assert_eq!(nudged["value"], 32767);
    let quarter = json!({ "block": 106, "param": 2, "position": 0.25 });
    assert_eq!(engine.run("param.set_normalized", &quarter).unwrap()["value"], 16384);
}

#[test]
fn bypass_and_xy_show_up_in_block_states() {
    let mut engine = connected();
    engine.run("block.bypass", &json!({ "block": "Cab 1" })).unwrap();
    engine.run("block.set_xy", &json!({ "block": 108, "state": "Y" })).unwrap();
    let cab = block_state(&engine, 108);
    assert_eq!(cab["engaged"], false);
    assert_eq!(cab["xy"], "Y");

    engine.run("block.select", &json!({ "block": 108 })).unwrap();
    engine.run("block.toggle_bypass", &Value::Null).unwrap();
    engine.run("block.toggle_xy", &Value::Null).unwrap();
    let cab = block_state(&engine, 108);
    assert_eq!(cab["engaged"], true);
    assert_eq!(cab["xy"], "X");
}

#[test]
fn hostile_parameters_are_errors_not_panics() {
    let mut engine = connected();
    let bad = [
        ("preset.select", json!({ "number": -1 })),
        ("preset.select", json!({ "number": 1e30 })),
        ("preset.select", json!({})),
        ("preset.rename", json!({ "name": "" })),
        ("preset.rename", json!({ "name": "x".repeat(33) })),
        ("scene.select", json!({ "scene": 0 })),
        ("scene.select", json!({ "scene": 9 })),
        ("param.set", json!({ "block": 106, "param": 1, "value": 65535 })),
        ("param.set", json!({ "block": 106, "param": "NOT_A_PARAM", "value": 1 })),
        ("param.set", json!({ "block": "Nonexistent", "param": 1, "value": 1 })),
        ("param.set", json!({ "param": 1, "value": 1 })),
        ("param.set_normalized", json!({ "block": 106, "param": 1, "position": 2.5 })),
        ("param.nudge", json!({ "block": 106, "param": 1, "steps": 1e9 })),
        ("block.set_xy", json!({ "block": 106, "state": "Z" })),
        ("grid.place_block", json!({ "block": 106, "column": 12, "row": 0 })),
        ("block.select", json!({ "block": 124 })),
        ("sysex.inspect_file", json!({ "path": "does/not/exist.syx" })),
        ("definitions.load", json!({ "path": 7 })),
        ("preset.next", json!([1, 2, 3])),
        ("no.such.command", json!({})),
    ];
    for (id, params) in bad {
        assert!(engine.run(id, &params).is_err(), "{id} {params} should fail");
    }
    // A wah is not in preset 0, so the device itself refuses.
    let err = engine.run("param.get", &json!({ "block": 124, "param": 0 })).unwrap_err();
    assert_eq!(err.kind(), "device");
    // The engine is still healthy.
    assert_eq!(engine.run("preset.next", &Value::Null).unwrap()["number"], 1);
}

#[test]
fn catalogs_work_without_a_device() {
    let mut engine = Engine::new();
    assert_eq!(engine.run("catalog.blocks", &Value::Null).unwrap().as_array().unwrap().len(), 71);
    let params = engine.run("catalog.params", &json!({ "block": "Reverb 1" })).unwrap();
    assert_eq!(params[0]["name"], "REVERB_TYPE");
    let listed = engine.run("app.commands", &Value::Null).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), registry().len());
}

#[test]
fn inspects_a_sysex_file() {
    let dir = std::env::temp_dir().join(format!("axegrid-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ir.syx");
    let mut data = axegrid_sysex::Frame::new(7, 0x7A, [0x7F, 0x7F, 0, 0x10]).to_bytes();
    data.extend(axegrid_sysex::Frame::new(7, 0x7B, [1, 2, 3]).to_bytes());
    data.extend(axegrid_sysex::Frame::new(7, 0x7C, [1, 2, 3, 4, 5]).to_bytes());
    data.extend([0xF0, 0x00, 0x01, 0x74, 0x07, 0x7B, 0x00, 0x00, 0xF7]);
    std::fs::write(&path, &data).unwrap();
    let report = Engine::new().run("sysex.inspect_file", &json!({ "path": path })).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(report["messages"], 4);
    assert_eq!(report["invalid"], 1);
    assert_eq!(report["kind"], "cabinet IR");
}

fn cell(engine: &Engine, column: u64, row: u64) -> Option<Value> {
    let state = engine.inspect();
    let found = state["grid"].as_array().unwrap().iter().find(|c| c["column"] == column && c["row"] == row);
    found.cloned()
}

#[test]
fn bypass_goes_through_the_shared_parameter() {
    let mut engine = connected();
    engine.run("block.bypass", &json!({ "block": "Amp 1" })).unwrap();
    assert_eq!(block_state(&engine, 106)["engaged"], false);
    let shared = engine.run("param.get", &json!({ "block": 106, "param": 255 })).unwrap();
    assert_eq!(shared["value"], 1);
    engine.run("block.engage", &json!({ "block": "Amp 1" })).unwrap();
    assert_eq!(block_state(&engine, 106)["engaged"], true);
}

#[test]
fn editing_the_grid() {
    let mut engine = connected();
    let free = json!({ "column": 5, "row": 2 });

    // A new block arrives without cables; one already placed moves.
    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 5, "row": 2 })).unwrap();
    let placed = cell(&engine, 5, 2).unwrap();
    assert_eq!(
        (placed["name"].as_str(), placed["inputs_from_rows"].as_array().map(Vec::len)),
        (Some("Delay 1"), Some(0))
    );
    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 5, "row": 3 })).unwrap();
    assert!(cell(&engine, 5, 2).is_none());
    assert_eq!(cell(&engine, 5, 3).unwrap()["block"], 112);

    engine.run("grid.connect", &json!({ "column": 5, "row": 3, "from_row": 1 })).unwrap();
    assert_eq!(cell(&engine, 5, 3).unwrap()["inputs_from_rows"], json!([1]));
    engine.run("grid.disconnect", &json!({ "column": 5, "row": 3, "from_row": 1 })).unwrap();
    assert_eq!(cell(&engine, 5, 3).unwrap()["inputs_from_rows"], json!([]));

    engine.run("grid.place_shunt", &free).unwrap();
    assert_eq!(cell(&engine, 5, 2).unwrap()["name"], "Shunt");
    engine.run("grid.clear_cell", &free).unwrap();
    assert!(cell(&engine, 5, 2).is_none());

    // The first column has no column to its left.
    let err = engine.run("grid.connect", &json!({ "column": 0, "row": 1, "from_row": 1 })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "column", .. }));
}

#[test]
fn removing_the_selected_block_closes_its_editor() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    engine.run("grid.clear_cell", &json!({ "column": 1, "row": 1 })).unwrap();
    assert_eq!(engine.inspect()["selected_block"], Value::Null);
}

#[test]
fn fixed_blocks_open_without_being_in_the_grid() {
    let mut engine = connected();
    let gate = engine.run("block.select", &json!({ "block": "Input Gate" })).unwrap();
    assert_eq!(gate["selected_block"], 139);
    let editor = engine.inspect()["editor"].clone();
    assert_eq!((editor["name"].as_str(), editor["fixed"].as_bool()), (Some("Input Gate"), Some(true)));
    assert_eq!(editor["pages"][0]["rows"][0]["controls"][0]["label"], "Threshold");
    // A preset refresh keeps a fixed block selected.
    engine.run("device.refresh", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["selected_block"], 139);
    assert_eq!(engine.run("block.select", &json!({ "block": "Output" })).unwrap()["selected_block"], 140);
}

#[test]
fn editor_pages_follow_the_layout() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    let editor = engine.inspect()["editor"].clone();
    let pages: Vec<&str> = editor["pages"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(pages.first(), Some(&"Basic"));
    assert!(pages.contains(&"Power Amp"));
    let drive = &editor["pages"][0]["rows"][0]["controls"][0];
    assert_eq!(
        (drive["label"].as_str(), drive["name"].as_str(), drive["kind"].as_str()),
        (Some("Input Drive"), Some("DISTORT_DRIVE"), Some("knob"))
    );
    assert_eq!(drive["max"], 65534);
    assert_eq!(editor["model"]["name"], "DISTORT_TYPE");
    // Nothing selected: no editor.
    engine.run("block.deselect", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["editor"], Value::Null);
}

#[test]
fn tempo_is_set_read_and_tapped() {
    let mut engine = connected();
    assert_eq!(engine.inspect()["tempo"], 120);
    assert_eq!(engine.run("tempo.set", &json!({ "bpm": 96 })).unwrap()["bpm"], 96);
    assert_eq!(engine.run("tempo.get", &Value::Null).unwrap()["bpm"], 96);
    let slow = engine.run("tempo.set", &json!({ "bpm": 10 })).unwrap_err();
    assert!(matches!(slow, CommandError::BadParam { name: "bpm", .. }));
    assert!(engine.run("tempo.set", &json!({ "bpm": 400 })).is_err());

    let first = engine.run("tempo.tap", &Value::Null).unwrap();
    assert_eq!(first["waiting_for_second_tap"], true);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let bpm = engine.run("tempo.tap", &Value::Null).unwrap()["bpm"].as_u64().unwrap();
    // 300 ms apart is 200 BPM; scheduling may stretch the gap, never shrink it.
    assert!((120..=200).contains(&bpm), "{bpm}");
    assert_eq!(engine.inspect()["tempo"], bpm);
}

#[test]
fn tuner_readings_arrive_through_poll() {
    let mut engine = connected();
    assert_eq!(engine.inspect()["tuner"], json!({ "on": false, "reading": null }));
    engine.run("tuner.on", &Value::Null).unwrap();
    let polled = engine.run("device.poll", &Value::Null).unwrap();
    assert_eq!(polled["tuner"], json!({ "on": true, "reading": { "note": "A", "string": 5, "offset": -5 } }));
    assert_eq!(polled["refreshed"], false);
    engine.run("tuner.toggle", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["tuner"]["on"], false);
    // Disconnecting with the tuner on switches it off first.
    engine.run("tuner.on", &Value::Null).unwrap();
    engine.run("device.disconnect", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["tuner"]["on"], false);
}

#[test]
fn load_is_read_with_the_preset() {
    let mut engine = connected();
    assert_eq!(engine.inspect()["cpu_load"], 42);
    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 5, "row": 2 })).unwrap();
    assert_eq!(engine.inspect()["cpu_load"], 42);
}

#[test]
fn moving_a_block_keeps_its_cables_within_a_column() {
    let mut engine = connected();
    // Amp 1 sits at column 1, row 1, cabled on both sides.
    engine.run("grid.move_block", &json!({ "column": 1, "row": 1, "to_column": 1, "to_row": 3 })).unwrap();
    assert!(cell(&engine, 1, 1).is_none());
    let amp = cell(&engine, 1, 3).unwrap();
    assert_eq!((amp["name"].as_str(), &amp["inputs_from_rows"]), (Some("Amp 1"), &json!([1])));
    assert_eq!(cell(&engine, 2, 1).unwrap()["inputs_from_rows"], json!([3]));

    // Across columns the cables are gone.
    engine.run("grid.move_block", &json!({ "column": 1, "row": 3, "to_column": 6, "to_row": 0 })).unwrap();
    assert_eq!(cell(&engine, 6, 0).unwrap()["inputs_from_rows"], json!([]));
    assert_eq!(cell(&engine, 2, 1).unwrap()["inputs_from_rows"], json!([]));

    // A taken cell refuses, an empty source has nothing to move.
    let taken =
        engine.run("grid.move_block", &json!({ "column": 6, "row": 0, "to_column": 0, "to_row": 1 })).unwrap_err();
    assert!(matches!(taken, CommandError::BadParam { name: "to_column", .. }));
    assert!(engine.run("grid.move_block", &json!({ "column": 9, "row": 3, "to_column": 9, "to_row": 2 })).is_err());
}

#[test]
fn cabling_a_block_to_the_input_and_the_output() {
    let mut engine = connected();
    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 5, "row": 3 })).unwrap();
    let to_input = engine.run("grid.connect_to_input", &json!({ "column": 5, "row": 3 })).unwrap();
    assert_eq!(to_input["shunts_added"], 5);
    let to_output = engine.run("grid.connect_to_output", &json!({ "column": 5, "row": 3 })).unwrap();
    assert_eq!(to_output["shunts_added"], 6);
    for column in 1..12 {
        assert_eq!(cell(&engine, column, 3).unwrap()["inputs_from_rows"], json!([3]), "column {column}");
    }
    assert_eq!(cell(&engine, 0, 3).unwrap()["name"], "Shunt");
    assert_eq!(cell(&engine, 5, 3).unwrap()["name"], "Delay 1");
    // Every shunt has an ID of its own.
    let state = engine.inspect();
    let mut shunts: Vec<u64> = state["grid"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"] == "Shunt")
        .filter_map(|c| c["block"].as_u64())
        .collect();
    let placed = shunts.len();
    shunts.sort_unstable();
    shunts.dedup();
    assert_eq!((placed, shunts.len()), (18, 18));
    // Doing it again changes nothing.
    assert_eq!(engine.run("grid.connect_to_input", &json!({ "column": 5, "row": 3 })).unwrap()["shunts_added"], 0);
    assert!(engine.run("grid.connect_to_output", &json!({ "column": 5, "row": 0 })).is_err());
}

#[test]
fn looper_buttons_send_controllers_and_show_their_state() {
    let mut engine = connected();
    assert_eq!(engine.inspect()["looper"], json!({ "watched": false, "active": null }));
    engine.run("looper.watch", &Value::Null).unwrap();
    assert_eq!(
        engine.run("looper.press", &json!({ "button": "record" })).unwrap(),
        json!({ "button": "record", "on": true })
    );
    engine.run("device.poll", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["looper"]["active"], json!(["record"]));
    // Pressed again it toggles off, from what the device reported.
    assert_eq!(engine.run("looper.press", &json!({ "button": "record" })).unwrap()["on"], false);
    engine.run("looper.press", &json!({ "button": "play", "on": true })).unwrap();
    engine.run("device.poll", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["looper"]["active"], json!(["play"]));
    assert!(engine.run("looper.press", &json!({ "button": "rewind" })).is_err());
}

#[test]
fn parametric_eq_page_carries_a_curve() {
    let mut engine = connected();
    engine.run("grid.place_block", &json!({ "block": "Parametric EQ 1", "column": 7, "row": 0 })).unwrap();
    engine.run("block.select", &json!({ "block": "Parametric EQ 1" })).unwrap();
    let editor = engine.inspect()["editor"].clone();
    let graph = &editor["pages"][0]["graph"];
    assert_eq!(graph["kind"], "eq");
    assert_eq!(graph["points"].as_array().unwrap().len(), 96);
    assert_eq!(graph["bands"].as_array().unwrap().len(), 5);
    assert!(editor["pages"][0]["rows"].as_array().unwrap().iter().any(|row| row["mixer"] == true));
}
