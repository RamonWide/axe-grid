//! Moving several blocks together, end to end against the simulated device.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axegrid_engine::{CommandError, Engine};
use serde_json::{json, Value};

fn connected() -> Engine {
    let mut engine = Engine::new();
    engine.run("device.connect_simulator", &Value::Null).unwrap();
    engine
}

fn cell(engine: &Engine, column: u64, row: u64) -> Option<Value> {
    let state = engine.inspect();
    let found = state["grid"].as_array().unwrap().iter().find(|c| c["column"] == column && c["row"] == row);
    found.cloned()
}

fn name(engine: &Engine, column: u64, row: u64) -> String {
    cell(engine, column, row).map(|c| c["name"].as_str().unwrap().to_owned()).unwrap_or_default()
}

fn inputs(engine: &Engine, column: u64, row: u64) -> Value {
    cell(engine, column, row).unwrap()["inputs_from_rows"].clone()
}

/// Fails if a block or shunt ID sits in two cells.
fn assert_no_repeated_ids(engine: &Engine) {
    let state = engine.inspect();
    let mut ids: Vec<u64> = state["grid"].as_array().unwrap().iter().filter_map(|c| c["block"].as_u64()).collect();
    let placed = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(placed, ids.len(), "an ID is in two cells");
    // The simulated chain: five blocks and seven shunts.
    assert_eq!(placed, 12);
}

// The first preset is one chain along row 1: five blocks in columns 0 to 4,
// Amp 1 and Cab 1 second and third, and shunts to the output after them.
const AMP_AND_CAB: [(u64, u64); 2] = [(1, 1), (2, 1)];

fn cells(of: &[(u64, u64)]) -> Value {
    of.iter().map(|&(column, row)| json!({ "column": column, "row": row })).collect()
}

#[test]
fn a_group_moved_within_its_columns_keeps_every_cable() {
    let mut engine = connected();
    let moved = engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dy": -1 })).unwrap();
    assert_eq!(moved["moved"].as_array().unwrap().len(), 2);
    assert_eq!((name(&engine, 1, 0), name(&engine, 2, 0)), ("Amp 1".to_owned(), "Cab 1".to_owned()));
    assert!(cell(&engine, 1, 1).is_none() && cell(&engine, 2, 1).is_none());
    // The cable between the two, and the ones to the neighbours that stayed.
    assert_eq!(inputs(&engine, 2, 0), json!([0]));
    assert_eq!(inputs(&engine, 1, 0), json!([1]));
    assert_eq!(inputs(&engine, 3, 1), json!([0]));
    // The rest of the chain is as it was.
    assert_eq!(inputs(&engine, 4, 1), json!([1]));
    assert_no_repeated_ids(&engine);
    assert_eq!(engine.inspect()["history"]["next_undo"], "Move 2 Blocks");
}

#[test]
fn a_group_moved_to_other_columns_keeps_its_own_cables_only() {
    let mut engine = connected();
    engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dx": 1, "dy": 1 })).unwrap();
    assert_eq!((name(&engine, 2, 2), name(&engine, 3, 2)), ("Amp 1".to_owned(), "Cab 1".to_owned()));
    assert_eq!(inputs(&engine, 3, 2), json!([2]));
    // Nothing reaches the amp any more, and the cab no longer feeds the chain.
    assert_eq!(inputs(&engine, 2, 2), json!([]));
    assert_eq!(inputs(&engine, 3, 1), json!([]));
    assert_eq!(inputs(&engine, 4, 1), json!([1]));
    assert_no_repeated_ids(&engine);
}

#[test]
fn a_group_may_land_on_cells_it_is_leaving() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dy": -1 })).unwrap();
    // One column to the right along its own row: the amp goes where the cab was.
    engine.run("grid.move_blocks", &json!({ "cells": cells(&[(1, 0), (2, 0)]), "dx": 1 })).unwrap();
    assert_eq!((name(&engine, 2, 0), name(&engine, 3, 0)), ("Amp 1".to_owned(), "Cab 1".to_owned()));
    assert!(cell(&engine, 1, 0).is_none());
    assert_eq!(inputs(&engine, 3, 0), json!([0]));
    assert_eq!(inputs(&engine, 2, 0), json!([]));
    assert_no_repeated_ids(&engine);
    // Both are still among the preset's blocks.
    let state = engine.inspect();
    let names: Vec<&str> = state["blocks"].as_array().unwrap().iter().filter_map(|b| b["name"].as_str()).collect();
    assert!(names.contains(&"Amp 1") && names.contains(&"Cab 1"));

    engine.run("edit.undo", &Value::Null).unwrap();
    engine.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["grid"], original);
}

#[test]
fn shunts_move_with_their_ids_and_cables() {
    let mut engine = connected();
    let ids = |engine: &Engine, row: u64| -> Vec<Value> {
        (9..12).map(|column| cell(engine, column, row).unwrap()["block"].clone()).collect()
    };
    let before = ids(&engine, 1);
    engine.run("grid.move_blocks", &json!({ "cells": cells(&[(9, 1), (10, 1), (11, 1)]), "dy": 2 })).unwrap();
    assert_eq!(ids(&engine, 3), before);
    assert_eq!(name(&engine, 10, 3), "Shunt");
    assert!(cell(&engine, 10, 1).is_none());
    assert_eq!(inputs(&engine, 10, 3), json!([3]));
    assert_eq!(inputs(&engine, 11, 3), json!([3]));
    // The shunt in column 8 stayed and still feeds the first of them.
    assert_eq!(inputs(&engine, 9, 3), json!([1]));
    assert_no_repeated_ids(&engine);
}

#[test]
fn a_taken_cell_refuses_the_whole_move() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    // The amp alone would land on the cab, which is not moving.
    let taken = engine.run("grid.move_blocks", &json!({ "cells": cells(&[(1, 1)]), "dx": 1 })).unwrap_err();
    assert!(matches!(&taken, CommandError::BadParam { name: "dx", .. }), "{taken}");
    assert!(taken.to_string().contains("Cab 1"), "{taken}");
    assert_eq!(engine.inspect()["grid"], original);
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
    // Two blocks of which only one is in the way of something: neither moves.
    engine.run("grid.place_block", &json!({ "block": "Delay 1", "column": 2, "row": 3 })).unwrap();
    let placed = engine.inspect()["grid"].clone();
    let blocked = engine.run("grid.move_blocks", &json!({ "cells": cells(&[(2, 3), (1, 1)]), "dy": -2 }));
    assert!(blocked.is_err());
    let blocked = engine.run("grid.move_blocks", &json!({ "cells": cells(&[(1, 1), (2, 3)]), "dx": 1 }));
    assert!(blocked.is_err());
    assert_eq!(engine.inspect()["grid"], placed);
    assert_eq!(engine.inspect()["history"]["can_undo"], 1);
}

#[test]
fn leaving_the_grid_is_refused() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    let up = engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dy": -2 })).unwrap_err();
    assert!(matches!(up, CommandError::BadParam { name: "dy", .. }));
    let left = engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dx": -2 })).unwrap_err();
    assert!(matches!(left, CommandError::BadParam { name: "dx", .. }));
    for (dx, dy) in [(11, 0), (0, 3), (-12, 0), (0, 4), (99, 0)] {
        let far = engine.run("grid.move_blocks", &json!({ "cells": cells(&AMP_AND_CAB), "dx": dx, "dy": dy }));
        assert!(far.is_err(), "{dx}, {dy}");
    }
    assert_eq!(engine.inspect()["grid"], original);
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
}

#[test]
fn bad_parameters_are_reported() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    for params in [
        json!({ "dx": 1 }),
        json!({ "cells": [], "dx": 1 }),
        json!({ "cells": "all", "dx": 1 }),
        json!({ "cells": [{ "column": 12, "row": 0 }], "dy": 1 }),
        json!({ "cells": [{ "column": 1 }], "dy": 1 }),
        json!({ "cells": [7], "dy": 1 }),
        json!({ "cells": cells(&AMP_AND_CAB), "dy": 0.5 }),
        json!({ "cells": cells(&AMP_AND_CAB), "dx": "left" }),
    ] {
        let refused = engine.run("grid.move_blocks", &params).unwrap_err();
        assert!(matches!(refused, CommandError::BadParam { .. }), "{params}: {refused}");
    }
    // An empty cell has nothing to move.
    let empty = engine.run("grid.move_blocks", &json!({ "cells": cells(&[(1, 1), (6, 3)]), "dy": 1 })).unwrap_err();
    assert!(matches!(empty, CommandError::NotFound(_)));
    // No offset is no edit, and a cell named twice moves once.
    let twice = engine.run("grid.move_blocks", &json!({ "cells": cells(&[(1, 1), (1, 1)]) })).unwrap();
    assert_eq!(twice["moved"].as_array().unwrap().len(), 1);
    assert_eq!(engine.inspect()["grid"], original);
    assert_eq!(engine.inspect()["history"]["can_undo"], 0);
}

#[test]
fn a_group_move_is_one_undo_step() {
    let mut engine = connected();
    let original = engine.inspect()["grid"].clone();
    let group = cells(&[(1, 1), (2, 1), (3, 1), (9, 1)]);
    engine.run("grid.move_blocks", &json!({ "cells": group, "dx": 2, "dy": -1 })).unwrap();
    let edited = engine.inspect()["grid"].clone();
    assert_ne!(edited, original);
    assert_eq!(inputs(&engine, 4, 0), json!([0]));
    assert_eq!(inputs(&engine, 5, 0), json!([0]));
    assert_eq!(engine.inspect()["history"]["can_undo"], 1);
    assert_eq!(engine.inspect()["history"]["next_undo"], "Move 4 Blocks");

    assert_eq!(engine.run("edit.undo", &Value::Null).unwrap()["undone"], "Move 4 Blocks");
    assert_eq!(engine.inspect()["grid"], original);
    assert_no_repeated_ids(&engine);
    engine.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(engine.inspect()["grid"], edited);
    assert_no_repeated_ids(&engine);
    // One block alone is named in the menu.
    engine.run("grid.move_blocks", &json!({ "cells": cells(&[(0, 1)]), "dy": 1 })).unwrap();
    assert!(engine.inspect()["history"]["next_undo"].as_str().unwrap().starts_with("Move "));
    assert_ne!(engine.inspect()["history"]["next_undo"], "Move 1 Blocks");
}
