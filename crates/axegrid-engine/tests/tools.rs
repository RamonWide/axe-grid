//! Scene and block tools, modifiers, the looper's position and the tone
//! match workflow, end to end against the simulated device.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use axegrid_engine::{CommandError, Engine};
use serde_json::{json, Value};

const AMP: u64 = 106;
const DRIVE: u64 = 1;

fn connected() -> Engine {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    engine
}

/// A folder of this test's own, emptied.
fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("axegrid-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

fn block_state(engine: &Engine, block: u64) -> Value {
    let state = engine.inspect();
    let found = state["blocks"].as_array().unwrap().iter().find(|b| b["block"] == block);
    found.unwrap().clone()
}

fn raw(engine: &mut Engine, block: u64, param: u64) -> Value {
    engine.run("param.get", &json!({ "block": block, "param": param })).unwrap()["value"].clone()
}

fn scene(engine: &mut Engine, number: u64) {
    engine.run("scene.select", &json!({ "scene": number })).unwrap();
}

#[test]
fn bypass_and_xy_belong_to_a_scene() {
    let mut engine = connected();
    engine.run("block.bypass", &json!({ "block": AMP })).unwrap();
    engine.run("block.set_xy", &json!({ "block": AMP, "state": "Y" })).unwrap();
    scene(&mut engine, 2);
    assert_eq!(block_state(&engine, AMP)["engaged"], true);
    assert_eq!(block_state(&engine, AMP)["xy"], "X");
    scene(&mut engine, 1);
    assert_eq!(block_state(&engine, AMP)["engaged"], false);
    assert_eq!(block_state(&engine, AMP)["xy"], "Y");
}

#[test]
fn a_scene_is_copied_onto_another_and_the_paste_is_undone() {
    let mut engine = connected();
    assert_eq!(engine.disabled_reason("scene.paste"), Some("no scene has been copied"));
    // Scene 1: amp bypassed and on Y, its own main level and scene controller.
    engine.run("block.bypass", &json!({ "block": AMP })).unwrap();
    engine.run("block.set_xy", &json!({ "block": AMP, "state": "Y" })).unwrap();
    engine.run("param.set", &json!({ "block": "Output", "param": "OUTPUT_MAIN1", "value": 1111 })).unwrap();
    engine
        .run("param.set", &json!({ "block": "Controllers", "param": "CONTROLLERS_SCENE2_VAL1", "value": 222 }))
        .unwrap();
    assert_eq!(engine.run("scene.copy", &Value::Null).unwrap(), json!({ "scene": 1, "blocks": 5 }));
    assert_eq!(engine.inspect()["clipboard"]["scene"], true);

    let pasted = engine.run("scene.paste", &json!({ "scene": 4 })).unwrap();
    assert_eq!(pasted, json!({ "from": 1, "scene": 4, "changes": 4 }));
    assert_eq!(engine.inspect()["preset"]["scene"], 4);
    assert_eq!(block_state(&engine, AMP)["engaged"], false);
    assert_eq!(block_state(&engine, AMP)["xy"], "Y");
    assert_eq!(engine.run("param.get", &json!({ "block": "Output", "param": "OUTPUT_MAIN4" })).unwrap()["value"], 1111);
    let controller = json!({ "block": "Controllers", "param": "CONTROLLERS_SCENE2_VAL4" });
    assert_eq!(engine.run("param.get", &controller).unwrap()["value"], 222);
    // Pasting again changes nothing, so there is nothing more to undo.
    assert_eq!(engine.run("scene.paste", &Value::Null).unwrap()["changes"], 0);

    let undone = engine.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(undone["undone"], "Paste Scene 1 into 4");
    assert_eq!(block_state(&engine, AMP)["engaged"], true);
    assert_eq!(block_state(&engine, AMP)["xy"], "X");
    assert_ne!(engine.run("param.get", &json!({ "block": "Output", "param": "OUTPUT_MAIN4" })).unwrap()["value"], 1111);
    // The scene it was copied from is as it was.
    scene(&mut engine, 1);
    assert_eq!(block_state(&engine, AMP)["engaged"], false);
    engine.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["preset"]["scene"], 4);
    assert_eq!(block_state(&engine, AMP)["xy"], "Y");
}

#[test]
fn scene_names_are_kept_on_this_computer() {
    let folder = scratch("names");
    let mut engine = Engine::with_data_dir(&folder);
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    let names = engine.run("scene.rename", &json!({ "scene": 3, "name": "  Solo " })).unwrap();
    assert_eq!(names[2], json!({ "scene": 3, "name": "Solo", "current": false }));
    engine.run("scene.rename", &json!({ "name": "Clean" })).unwrap();
    assert_eq!(engine.inspect()["scenes"][0], json!({ "scene": 1, "name": "Clean", "current": true }));
    // A later session finds them in the file.
    let mut later = Engine::with_data_dir(&folder);
    later.run("device.connect_simulator", &Value::Null).unwrap();
    assert_eq!(later.inspect()["scenes"][2]["name"], "Solo");
    // An empty name takes one away.
    later.run("scene.rename", &json!({ "scene": 3, "name": "" })).unwrap();
    assert_eq!(later.inspect()["scenes"][2]["name"], "");
    engine.run("scene.rename", &json!({ "scene": 3, "name": "Solo" })).unwrap();
    // Another preset has none; coming back shows them again.
    engine.run("preset.next", &Value::Null).unwrap();
    assert!(engine.inspect()["scenes"].as_array().unwrap().iter().all(|s| s["name"] == ""));
    engine.run("preset.previous", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["scenes"][2]["name"], "Solo");
    // They follow the preset when it is renamed, and when it is saved elsewhere.
    engine.run("preset.rename", &json!({ "name": "Cleaner" })).unwrap();
    assert_eq!(engine.inspect()["scenes"][2]["name"], "Solo");
    engine.run("preset.store", &json!({ "number": 2 })).unwrap();
    engine.run("preset.select", &json!({ "number": 2 })).unwrap();
    assert_eq!(engine.run("scene.names", &Value::Null).unwrap()[0]["name"], "Clean");

    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn scene_names_are_checked_and_need_no_folder() {
    let mut engine = connected();
    let long = "x".repeat(25);
    for bad in [json!({ "name": long }), json!({ "name": "a\nb" }), json!({ "scene": 0, "name": "a" }), json!({})] {
        let err = engine.run("scene.rename", &bad).unwrap_err();
        assert!(matches!(err, CommandError::BadParam { .. }), "{bad}");
    }
    // Without a folder the names last as long as the engine.
    engine.run("scene.rename", &json!({ "scene": 8, "name": "Outro" })).unwrap();
    assert_eq!(engine.inspect()["scenes"][7]["name"], "Outro");
}

#[test]
fn a_block_is_copied_into_another_of_its_kind() {
    let mut engine = connected();
    assert_eq!(engine.disabled_reason("block.copy"), Some("no block is selected"));
    engine.run("grid.place_block", &json!({ "block": "Amp 2", "column": 0, "row": 3 })).unwrap();
    engine.run("param.set", &json!({ "block": AMP, "param": "DISTORT_TYPE", "value": 2 })).unwrap();
    engine.run("param.set", &json!({ "block": AMP, "param": DRIVE, "value": 4321 })).unwrap();
    engine.run("block.select", &json!({ "block": AMP })).unwrap();
    assert_eq!(engine.disabled_reason("block.paste"), Some("no block has been copied"));
    let copied = engine.run("block.copy", &Value::Null).unwrap();
    assert_eq!((copied["name"].as_str(), &copied["sets"]), (Some("Amp 1"), &json!(["X"])));

    // A block of another kind takes nothing.
    let err = engine.run("block.paste", &json!({ "block": "Cab 1" })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "block", .. }), "{err}");
    let pasted = engine.run("block.paste", &json!({ "block": "Amp 2" })).unwrap();
    assert_eq!(pasted["sets_changed"], 1);
    assert_eq!(raw(&mut engine, 107, DRIVE), 4321);
    assert_eq!(raw(&mut engine, 107, 0), 2);
    // The same again changes nothing and leaves nothing to undo.
    assert_eq!(engine.run("block.paste", &json!({ "block": "Amp 2" })).unwrap()["sets_changed"], 0);
    assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Paste into Amp 2");
    assert_ne!(raw(&mut engine, 107, DRIVE), 4321);
    assert_eq!(raw(&mut engine, 107, 0), 0);
    engine.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(raw(&mut engine, 107, DRIVE), 4321);
    // Bypass is the scene's, not the block's: it is not pasted.
    engine.run("block.bypass", &json!({ "block": AMP })).unwrap();
    engine.run("block.copy", &Value::Null).unwrap();
    engine.run("block.paste", &json!({ "block": "Amp 2" })).unwrap();
    assert_eq!(block_state(&engine, 107)["engaged"], true);
}

#[test]
fn both_sets_are_copied_and_one_set_is_copied_over_the_other() {
    let mut engine = connected();
    engine.run("grid.place_block", &json!({ "block": "Amp 2", "column": 0, "row": 3 })).unwrap();
    engine.run("block.select", &json!({ "block": AMP })).unwrap();
    engine.run("param.set", &json!({ "param": DRIVE, "value": 1000 })).unwrap();
    engine.run("block.set_xy", &json!({ "state": "Y" })).unwrap();
    engine.run("param.set", &json!({ "param": DRIVE, "value": 2000 })).unwrap();

    let copied = engine.run("block.copy", &json!({ "sets": "both" })).unwrap();
    assert_eq!(copied["sets"], json!(["X", "Y"]));
    // The block is back on the set it was on.
    assert_eq!(block_state(&engine, AMP)["xy"], "Y");
    assert_eq!(engine.run("block.paste", &json!({ "block": "Amp 2" })).unwrap()["sets_changed"], 2);
    assert_eq!(block_state(&engine, 107)["xy"], "X");
    assert_eq!(raw(&mut engine, 107, DRIVE), 1000);
    engine.run("block.set_xy", &json!({ "block": 107, "state": "Y" })).unwrap();
    assert_eq!(raw(&mut engine, 107, DRIVE), 2000);
    let err = engine.run("block.copy", &json!({ "sets": "all" })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "sets", .. }));

    // X over Y on the amp, which is on Y and stays there.
    assert_eq!(engine.run("block.copy_x_to_y", &Value::Null).unwrap()["sets_changed"], 1);
    assert_eq!(block_state(&engine, AMP)["xy"], "Y");
    assert_eq!(raw(&mut engine, AMP, DRIVE), 1000);
    assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Copy Amp 1 X to Y");
    assert_eq!(raw(&mut engine, AMP, DRIVE), 2000);
    // And Y over X.
    engine.run("block.copy_y_to_x", &Value::Null).unwrap();
    engine.run("block.set_xy", &json!({ "state": "X" })).unwrap();
    assert_eq!(raw(&mut engine, AMP, DRIVE), 2000);
    // The output mixer has one set only.
    let err = engine.run("block.copy_x_to_y", &json!({ "block": "Output" })).unwrap_err();
    assert!(matches!(err, CommandError::NotFound(_)), "{err}");
}

#[test]
fn a_modifier_is_attached_edited_and_removed() {
    let mut engine = connected();
    assert_eq!(engine.disabled_reason("modifier.close"), Some("no modifier is open"));
    let opened = engine.run("modifier.open", &json!({ "block": AMP, "param": "DISTORT_DRIVE" })).unwrap();
    assert_eq!((opened["attached"].as_bool(), opened["label"].as_str()), (Some(false), Some("Drive")));
    let fields = opened["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 13);
    assert_eq!(
        fields[0],
        json!({ "field": 0, "key": "source", "label": "Source", "value": 0, "text": "NONE",
                                  "kind": "step", "max": 23 })
    );
    assert_eq!(opened["graph"]["points"][0], json!([0.0, 0.0]));
    assert_eq!(engine.inspect()["modifier"]["param"], DRIVE);

    // Without a source the device keeps no field, so none is sent.
    let early = engine.run("modifier.set", &json!({ "field": "start", "value": 9 })).unwrap_err();
    assert!(matches!(early, CommandError::BadParam { name: "field", .. }), "{early}");

    // The open modifier is the one the other commands act on.
    let attached = engine.run("modifier.attach", &json!({ "source": 3 })).unwrap();
    assert_eq!(attached["attached"], true);
    assert_eq!(attached["source"], json!({ "value": 3, "text": "SIM PEDAL" }));
    let known = engine.run("modifier.sources", &Value::Null).unwrap();
    assert!(known.as_array().unwrap().contains(&json!({ "source": 3, "name": "SIM PEDAL" })));
    // A name the device has shown works too.
    engine.run("modifier.attach", &json!({ "source": "sim pedal" })).unwrap();
    let err = engine.run("modifier.attach", &json!({ "source": "theremin" })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "source", .. }));
    assert!(engine.run("modifier.attach", &json!({ "source": 0 })).is_err());
    // No choice is sent past the last one the family is documented to have.
    let past = engine.run("modifier.attach", &json!({ "source": 28 })).unwrap_err();
    assert!(matches!(past, CommandError::BadParam { name: "source", .. }), "{past}");
    let past = engine.run("modifier.set", &json!({ "field": "pc_reset", "value": 2 })).unwrap_err();
    assert!(matches!(past, CommandError::BadParam { name: "value", .. }), "{past}");

    // The curve falls once start and end are turned round.
    let set = engine.run("modifier.set", &json!({ "field": "start", "value": 65534 })).unwrap();
    assert_eq!(set["text"], "100.0 %");
    engine.run("modifier.set", &json!({ "field": "end", "value": 0 })).unwrap();
    let graph = engine.inspect()["modifier"]["graph"].clone();
    assert_eq!((&graph["points"][0], &graph["points"][64]), (&json!([0.0, 1.0]), &json!([1.0, 0.0])));
    let err = engine.run("modifier.set", &json!({ "field": "end_marker", "value": 0 })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "field", .. }));

    // Each step is undone by itself.
    assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Modifier End of Amp 1 Drive");
    let removed = engine.run("modifier.remove", &Value::Null).unwrap();
    assert_eq!(removed["attached"], false);
    assert_eq!(engine.inspect()["modifier"]["attached"], false);
    // The device put the rest back to its defaults; one undo returns all of it.
    let start = |modifier: &Value| {
        let fields = modifier["fields"].as_array().unwrap();
        fields.iter().find(|field| field["key"] == "start").unwrap()["value"].clone()
    };
    assert_eq!(start(&engine.inspect()["modifier"]), 0);
    assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Remove modifier of Amp 1 Drive");
    let again = engine.run("modifier.open", &json!({ "block": AMP, "param": DRIVE })).unwrap();
    assert_eq!(again["attached"], true);
    assert_eq!(start(&again), 65534);
    // Stepping the source down to none removes it the same way.
    let stepped = engine.run("modifier.set", &json!({ "field": "source", "value": 0 })).unwrap();
    assert_eq!((stepped["value"].as_u64(), start(&engine.inspect()["modifier"])), (Some(0), json!(0)));
    engine.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(engine.run("modifier.open", &json!({ "block": AMP, "param": DRIVE })).unwrap()["attached"], true);
    engine.run("modifier.close", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["modifier"], Value::Null);
}

#[test]
fn a_modifier_needs_a_parameter_that_can_have_one() {
    let mut engine = connected();
    // The amp's type is a list: nothing is asked of the device.
    let err = engine.run("modifier.open", &json!({ "block": AMP, "param": "DISTORT_TYPE" })).unwrap_err();
    assert_eq!(err.kind(), "bad_param");
    assert!(engine.run("modifier.open", &json!({ "block": AMP })).is_err());
    // Loading another preset closes the window.
    engine.run("modifier.open", &json!({ "block": AMP, "param": DRIVE })).unwrap();
    engine.run("preset.next", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["modifier"], Value::Null);
}

#[test]
fn the_looper_reports_how_far_the_loop_has_played() {
    let mut engine = connected();
    engine.run("grid.place_block", &json!({ "block": "Looper", "column": 6, "row": 0 })).unwrap();
    engine.run("block.select", &json!({ "block": "Looper" })).unwrap();
    // Nothing is running: no position.
    assert_eq!(engine.inspect()["looper"]["position"], Value::Null);
    engine.run("looper.press", &json!({ "button": "play" })).unwrap();
    engine.run("device.cpu_load", &Value::Null).unwrap();
    engine.run("device.poll", &Value::Null).unwrap();
    let looper = engine.inspect()["looper"].clone();
    assert_eq!(looper["active"], json!(["play"]));
    let first = looper["position"].as_u64().unwrap();
    assert!((1..100).contains(&first), "{first}");
    assert_eq!(engine.inspect()["editor"]["looper"]["position"], first);
    engine.run("device.cpu_load", &Value::Null).unwrap();
    engine.run("device.poll", &Value::Null).unwrap();
    assert!(engine.inspect()["looper"]["position"].as_u64().unwrap() > first);
    // Stopped, the bar goes away.
    engine.run("looper.press", &json!({ "button": "play" })).unwrap();
    engine.run("device.poll", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["looper"]["position"], Value::Null);
}

#[test]
fn the_tone_match_workflow_runs_its_three_steps() {
    let mut engine = connected();
    assert_eq!(engine.disabled_reason("tonematch.match"), Some("the current preset has no Tone Match block"));
    engine.run("grid.place_block", &json!({ "block": "Tone Match", "column": 6, "row": 0 })).unwrap();
    let idle = engine.run("tonematch.status", &Value::Null).unwrap();
    assert_eq!(idle, json!({ "block": 170, "capturing_reference": false, "capturing_local": false }));
    let reference = engine.run("tonematch.capture", &json!({ "signal": "reference" })).unwrap();
    assert_eq!((&reference["capturing_reference"], &reference["capturing_local"]), (&json!(true), &json!(false)));
    engine.run("tonematch.capture", &json!({ "signal": "reference", "on": false })).unwrap();
    let both = engine.run("tonematch.capture", &json!({ "signal": "both" })).unwrap();
    assert_eq!((&both["capturing_reference"], &both["capturing_local"]), (&json!(true), &json!(true)));
    // The editor shows it once the block is open.
    engine.run("block.select", &json!({ "block": "Tone Match" })).unwrap();
    assert_eq!(engine.inspect()["editor"]["tone_match"]["capturing_local"], true);
    let matched = engine.run("tonematch.match", &Value::Null).unwrap();
    assert_eq!(matched["matched"], true);
    assert_eq!((&matched["capturing_reference"], &matched["capturing_local"]), (&json!(false), &json!(false)));
    // Captures are not edits: there is nothing to undo but placing the block.
    assert_eq!(engine.inspect()["history"]["can_undo"], 1);
    let err = engine.run("tonematch.capture", &json!({ "signal": "room" })).unwrap_err();
    assert!(matches!(err, CommandError::BadParam { name: "signal", .. }));
}
