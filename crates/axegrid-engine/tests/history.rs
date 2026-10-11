//! Undo, redo and the choice lists, end to end against the simulated device.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axegrid_engine::Engine;
use serde_json::{json, Value};

fn connected() -> Engine {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    engine
}

fn undo(engine: &mut Engine) -> Value {
    engine.run("edit.undo", &Value::Null).unwrap()
}

fn redo(engine: &mut Engine) -> Value {
    engine.run("edit.redo", &Value::Null).unwrap()
}

fn raw(engine: &mut Engine, block: u64, param: u64) -> Value {
    engine.run("param.get", &json!({ "block": block, "param": param })).unwrap()["value"].clone()
}

fn cell(engine: &Engine, column: u64, row: u64) -> Option<Value> {
    let state = engine.inspect();
    let found = state["grid"].as_array().unwrap().iter().find(|c| c["column"] == column && c["row"] == row);
    found.cloned()
}

fn engaged(engine: &Engine, block: u64) -> Value {
    let state = engine.inspect();
    let found = state["blocks"].as_array().unwrap().iter().find(|b| b["block"] == block);
    found.unwrap()["engaged"].clone()
}

#[test]
fn nothing_to_undo_until_something_is_edited() {
    let mut engine = Engine::new();
    assert_eq!(engine.disabled_reason("edit.undo"), Some("no device connected"));
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    assert_eq!(engine.disabled_reason("edit.undo"), Some("nothing to undo"));
    assert_eq!(engine.disabled_reason("edit.redo"), Some("nothing to redo"));
    assert!(engine.run("edit.undo", &Value::Null).is_err());
    // Reading is not an edit, and neither is setting a value to what it is.
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    engine.run("param.set", &json!({ "param": 1, "value": 0x8000 })).unwrap();
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
}

#[test]
fn a_parameter_edit_is_undone_and_redone() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    engine.run("param.set", &json!({ "param": "DISTORT_DRIVE", "value": 65534 })).unwrap();
    assert_eq!(engine.inspect()["history"]["next_undo"], "Amp 1 Drive");

    let undone = undo(&mut engine);
    assert_eq!((undone["undone"].as_str(), undone["can_redo"].as_u64()), (Some("Amp 1 Drive"), Some(1)));
    assert_eq!(raw(&mut engine, 106, 1), 0x8000);
    // The editor shows what the device now holds.
    assert_eq!(engine.state().params[1].value, 0x8000);
    assert_eq!(redo(&mut engine)["redone"], "Amp 1 Drive");
    assert_eq!(raw(&mut engine, 106, 1), 65534);
    assert_eq!(engine.disabled_reason("edit.redo"), Some("nothing to redo"));
}

#[test]
fn every_way_of_setting_a_parameter_is_remembered() {
    let mut engine = connected();
    // A block that is not open in the editor: its old value is asked for first.
    engine.run("param.set_normalized", &json!({ "block": "Cab 1", "param": 5, "position": 1.0 })).unwrap();
    engine.run("param.nudge", &json!({ "block": 106, "param": 2, "steps": 10 })).unwrap();
    engine.run("param.set_displayed", &json!({ "block": 106, "param": 3, "value": 2.5 })).unwrap();
    engine.run("tempo.set", &json!({ "bpm": 96 })).unwrap();
    let history = engine.run("edit.history", &Value::Null).unwrap();
    assert_eq!(history["undo"].as_array().unwrap().len(), 4);
    assert_eq!(history["undo"][0], "Controllers Tempo");

    undo(&mut engine);
    assert_eq!(engine.inspect()["tempo"], 120);
    undo(&mut engine);
    undo(&mut engine);
    undo(&mut engine);
    for (block, param) in [(108, 5), (106, 2), (106, 3)] {
        assert_eq!(raw(&mut engine, block, param), 0x8000, "block {block} parameter {param}");
    }
    assert_eq!(engine.run("edit.history", &Value::Null).unwrap()["redo"].as_array().unwrap().len(), 4);
    // A new edit closes the way forward.
    engine.run("tempo.set", &json!({ "bpm": 100 })).unwrap();
    assert_eq!(engine.inspect()["history"]["can_redo"], 0);
}

#[test]
fn turning_one_knob_is_one_step_back() {
    let mut engine = connected();
    for value in [40000, 45000, 50000] {
        engine.run("param.set", &json!({ "block": 106, "param": 1, "value": value })).unwrap();
    }
    engine.run("param.set", &json!({ "block": 106, "param": 2, "value": 1000 })).unwrap();
    assert_eq!(engine.inspect()["history"]["can_undo"], 2);
    undo(&mut engine);
    undo(&mut engine);
    assert_eq!((raw(&mut engine, 106, 1), raw(&mut engine, 106, 2)), (json!(0x8000), json!(0x8000)));
}

#[test]
fn bypass_and_xy_go_back_to_the_scene_they_were_changed_in() {
    let mut engine = connected();
    engine.run("block.bypass", &json!({ "block": "Cab 1" })).unwrap();
    engine.run("block.set_xy", &json!({ "block": "Cab 1", "state": "Y" })).unwrap();
    assert_eq!(engine.run("edit.history", &Value::Null).unwrap()["undo"], json!(["Cab 1 to Y", "Bypass Cab 1"]));
    engine.run("scene.select", &json!({ "scene": 3 })).unwrap();

    undo(&mut engine);
    let state = engine.inspect();
    assert_eq!(state["preset"]["scene"], 1);
    assert_eq!(state["blocks"].as_array().unwrap().iter().find(|b| b["block"] == 108).unwrap()["xy"], "X");
    undo(&mut engine);
    assert_eq!(engaged(&engine, 108), true);
    redo(&mut engine);
    assert_eq!(engaged(&engine, 108), false);
}

#[test]
fn grid_edits_are_undone_with_their_cables() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();

    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 5, "row": 3 })).unwrap();
    engine.run("grid.connect_to_input", &json!({ "column": 5, "row": 3 })).unwrap();
    // Amp 1 sits at column 1, row 1, cabled on both sides.
    engine.run("grid.clear_cell", &json!({ "column": 1, "row": 1 })).unwrap();
    engine.run("grid.move_block", &json!({ "column": 2, "row": 1, "to_column": 2, "to_row": 0 })).unwrap();
    // The cab's cable to the next block moved with it; take that one out.
    engine.run("grid.disconnect", &json!({ "column": 3, "row": 1, "from_row": 0 })).unwrap();
    let edited = engine.inspect()["grid"].clone();
    let history = engine.run("edit.history", &Value::Null).unwrap();
    assert_eq!(
        history["undo"],
        json!(["Disconnect Cells", "Move Cab 1", "Remove Amp 1", "Connect to Input", "Place Delay 1"])
    );

    undo(&mut engine);
    undo(&mut engine);
    assert_eq!(cell(&engine, 2, 1).unwrap()["name"], "Cab 1");
    undo(&mut engine);
    // The amp is back where it was, and so are the cables on both sides of it.
    let amp = cell(&engine, 1, 1).unwrap();
    assert_eq!((amp["name"].as_str(), &amp["inputs_from_rows"]), (Some("Amp 1"), &json!([1])));
    assert_eq!(cell(&engine, 2, 1).unwrap()["inputs_from_rows"], json!([1]));
    undo(&mut engine);
    assert!(cell(&engine, 0, 3).is_none());
    undo(&mut engine);
    assert_eq!(engine.inspect()["grid"], original);

    for _ in 0..5 {
        redo(&mut engine);
    }
    assert_eq!(engine.inspect()["grid"], edited);
    // No shunt ID is in two cells.
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
    assert_eq!(placed, shunts.len());
}

#[test]
fn swapped_blocks_are_put_back() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    // Amp 1 and Cab 1 change places by way of a spare cell.
    engine.run("grid.move_block", &json!({ "column": 1, "row": 1, "to_column": 1, "to_row": 0 })).unwrap();
    engine.run("grid.place_block", &json!({ "block": "Cab 1", "column": 1, "row": 1 })).unwrap();
    engine.run("grid.place_block", &json!({ "block": "Amp 1", "column": 2, "row": 1 })).unwrap();
    assert_eq!(cell(&engine, 2, 1).unwrap()["name"], "Amp 1");
    for _ in 0..3 {
        undo(&mut engine);
    }
    assert_eq!(engine.inspect()["grid"], original);
}

#[test]
fn a_rename_is_undone() {
    let mut engine = connected();
    engine.run("preset.rename", &json!({ "name": "Solo" })).unwrap();
    assert_eq!(undo(&mut engine)["undone"], "Rename Preset");
    assert_eq!(engine.inspect()["preset"]["name"], "Clean Chorus");
    redo(&mut engine);
    assert_eq!(engine.inspect()["preset"]["name"], "Solo");
}

#[test]
fn loading_a_preset_empties_the_history() {
    let mut engine = connected();
    engine.run("param.set", &json!({ "block": 106, "param": 1, "value": 1 })).unwrap();
    engine.run("preset.next", &Value::Null).unwrap();
    assert_eq!(
        engine.inspect()["history"],
        json!({ "can_undo": 0, "can_redo": 0, "next_undo": null, "next_redo": null })
    );
    // So does loading the same preset again, and so does disconnecting.
    engine.run("param.set", &json!({ "block": 106, "param": 1, "value": 2 })).unwrap();
    engine.run("preset.select", &json!({ "number": 1 })).unwrap();
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
    engine.run("param.set", &json!({ "block": 106, "param": 1, "value": 3 })).unwrap();
    engine.run("device.disconnect", &Value::Null).unwrap();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
    // Saving keeps it: the edit buffer is still the same.
    engine.run("param.set", &json!({ "block": 106, "param": 1, "value": 4 })).unwrap();
    engine.run("preset.store", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["history"]["can_undo"], 1);
}

fn choices(engine: &Engine, param: &str) -> Value {
    let state = engine.inspect();
    let found = state["params"].as_array().unwrap().iter().find(|p| p["name"] == param);
    found.unwrap()["choices"].clone()
}

#[test]
fn choice_lists_come_from_the_device() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Amp 1" })).unwrap();
    assert_eq!(choices(&engine, "DISTORT_TYPE"), Value::Null);
    let missing = engine.inspect()["lists"]["missing_for_selected_block"].as_u64().unwrap();
    assert!(missing > 5, "{missing}");

    // A few at a time, the way the window asks, then the rest.
    let some = engine.run("block.read_lists", &json!({ "budget": 2 })).unwrap();
    assert_eq!((some["requests"].as_u64(), some["missing"].as_u64()), (Some(2), Some(missing - 2)));
    let all = engine.run("block.read_lists", &Value::Null).unwrap();
    assert_eq!((all["missing"].as_u64(), all["lists"].as_array().map(Vec::len)), (Some(0), Some(missing as usize)));
    assert_eq!(choices(&engine, "DISTORT_TYPE"), json!(["FIRST", "SECOND", "THIRD"]));
    assert_eq!(choices(&engine, "DISTORT_DRIVE"), Value::Null);
    assert_eq!(engine.inspect()["editor"]["model"]["choices"][2], "THIRD");
    // Nothing is asked twice.
    assert_eq!(engine.run("block.read_lists", &Value::Null).unwrap()["requests"], 0);

    // The second amp shares the lists of the first.
    engine.run("grid.place_block", &json!({ "block": "Amp 2", "column": 7, "row": 0 })).unwrap();
    engine.run("block.select", &json!({ "block": "Amp 2" })).unwrap();
    assert_eq!(engine.inspect()["lists"]["missing_for_selected_block"], 0);
    // They belong to the device that was asked.
    engine.run("device.disconnect", &Value::Null).unwrap();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["lists"]["known"], 0);
}

#[test]
fn a_choice_is_set_by_its_text_without_reading_lists_first() {
    let mut engine = connected();
    let set =
        engine.run("param.set_choice", &json!({ "block": "Amp 1", "param": "DISTORT_TYPE", "choice": " second " }));
    assert_eq!(set.unwrap()["value"], 1);
    let unknown = engine.run("param.set_choice", &json!({ "block": 106, "param": 0, "choice": "FOURTH" })).unwrap_err();
    assert_eq!(unknown.kind(), "bad_param");
    // A knob has no choices to pick from.
    let knob = engine.run("param.set_choice", &json!({ "block": 106, "param": 1, "choice": "FIRST" })).unwrap_err();
    assert_eq!(knob.kind(), "not_found");
    assert_eq!(undo(&mut engine)["undone"], "Amp 1 Type");
}

#[test]
fn the_cab_list_is_read_name_by_name() {
    let mut engine = connected();
    engine.run("block.select", &json!({ "block": "Cab 1" })).unwrap();
    let some = engine.run("block.read_lists", &json!({ "budget": 300 })).unwrap();
    assert_eq!(some["requests"], 300);
    assert!(some["missing"].as_u64().unwrap() >= 1);
    assert_eq!(choices(&engine, "CABINET_TYPEL"), Value::Null);
    engine.run("block.read_lists", &Value::Null).unwrap();

    let left = choices(&engine, "CABINET_TYPEL");
    let names = left.as_array().unwrap();
    // 189 factory cabs, 1024 user cabs, four entries after them.
    assert_eq!(names.len(), 1217);
    assert_eq!(
        (names[0].as_str(), names[189].as_str(), names[190].as_str()),
        (Some("Sim Factory 1"), Some("Sim 4x12"), Some("EMPTY"))
    );
    // The user cabs in it are the ones the cab manager lists.
    let library = &engine.inspect()["library"];
    assert_eq!(library["cab_names_known"], 1024);
    assert_eq!(engine.state().cab_names.get(&1).map(String::as_str), Some("Sim 4x12"));
}
