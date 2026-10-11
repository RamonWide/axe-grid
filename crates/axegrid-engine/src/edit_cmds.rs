//! Blocks and their parameters.

use axegrid_model::defs::RAW_MAX;
use axegrid_model::{blocks, grid, params};
use axegrid_sysex::{ParamAddress, Request, XyState};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::Engine;

/// Effect type of the looper.
const LOOPER: u8 = 38;
/// One nudge step: one percent of the raw range.
const NUDGE: f64 = RAW_MAX as f64 / 100.0;

fn has_selection(engine: &Engine) -> Result<(), &'static str> {
    connected(engine)?;
    engine.state.selected_block.map(|_| ()).ok_or("no block is selected")
}

fn address(engine: &Engine, args: &Args<'_>) -> Result<ParamAddress, CommandError> {
    let block = engine.block_arg(args)?;
    Ok(ParamAddress { block, param: engine.param_arg(args, block)? })
}

/// Sets a parameter and remembers the edit, so that it can be undone.
fn apply(engine: &mut Engine, address: ParamAddress, raw: u16) -> Result<Value, CommandError> {
    let before = engine.value_before(address)?;
    let value = engine.session()?.set_param(address, raw.min(RAW_MAX))?;
    engine.remember(&value);
    engine.record_param(before, &value);
    Ok(engine.param_json(&value))
}

fn select(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    if !blocks::is_fixed(block) && !engine.state.preset.grid.blocks().contains(&block) {
        return Err(CommandError::NotFound(format!(
            "block {block} ({}) is not in the current preset",
            blocks::label(block)
        )));
    }
    engine.state.selected_block = Some(block);
    engine.refresh_params()?;
    if blocks::by_id(block).is_some_and(|b| b.effect_type == LOOPER) {
        // Its buttons show what the device reports; not worth failing over.
        let _ = engine.watch_looper(true);
    }
    Ok(json!({ "selected_block": block, "name": blocks::label(block), "params": engine.state.params.len() }))
}

fn deselect(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.state.selected_block = None;
    engine.state.params.clear();
    Ok(Value::Null)
}

fn refresh(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.refresh_params()?;
    Ok(json!({ "params": engine.state.params.len() }))
}

fn set_bypass(engine: &mut Engine, block: u16, bypassed: bool) -> Result<Value, CommandError> {
    apply(engine, ParamAddress { block, param: params::SHARED_BYPASS }, u16::from(bypassed))?;
    engine.refresh_preset()?;
    Ok(json!({ "block": block, "bypassed": bypassed }))
}

fn bypass(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    set_bypass(engine, block, true)
}

fn engage(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    set_bypass(engine, block, false)
}

fn toggle_bypass(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let engaged = engine.state.preset.states.iter().find(|s| s.block == block).is_none_or(|s| s.engaged);
    set_bypass(engine, block, engaged)
}

fn set_xy(engine: &mut Engine, block: u16, state: XyState) -> Result<Value, CommandError> {
    let before = engine.state.preset.states.iter().find(|s| s.block == block).map(|s| s.xy);
    engine.session()?.request(&Request::BlockXy(block, Some(state)))?;
    engine.refresh_preset()?;
    if let Some(before) = before {
        engine.record_xy(block, before, state);
    }
    Ok(json!({ "block": block, "xy": if state == XyState::X { "X" } else { "Y" } }))
}

fn xy(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let state = match args.str("state")? {
        "X" | "x" => XyState::X,
        "Y" | "y" => XyState::Y,
        other => {
            return Err(CommandError::BadParam {
                name: "state",
                problem: format!("{other:?} is neither \"X\" nor \"Y\""),
            })
        }
    };
    set_xy(engine, block, state)
}

fn toggle_xy(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let current = engine.state.preset.states.iter().find(|s| s.block == block).map(|s| s.xy);
    set_xy(engine, block, if current == Some(XyState::X) { XyState::Y } else { XyState::X })
}

fn get(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    let value = engine.session()?.get_param(address)?;
    engine.remember(&value);
    Ok(engine.param_json(&value))
}

fn set(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    apply(engine, address, args.int("value", u64::from(RAW_MAX))? as u16)
}

/// Largest magnitude a typed value may have; the device limits it further.
const DISPLAYED_LIMIT: f64 = 1.0e6;

fn set_displayed(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    let wanted = args.number("value", -DISPLAYED_LIMIT, DISPLAYED_LIMIT)? as f32;
    let before = engine.value_before(address)?;
    let value = engine.session()?.set_param_displayed(address, wanted)?;
    engine.remember(&value);
    engine.record_param(before, &value);
    Ok(engine.param_json(&value))
}

fn set_normalized(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    let position = args.number("position", 0.0, 1.0)?;
    apply(engine, address, (position * f64::from(RAW_MAX)).round() as u16)
}

fn nudge(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    let steps = args.number("steps", -100.0, 100.0)?;
    let current = f64::from(engine.value_before(address)?);
    let target = (current + steps * NUDGE).round().clamp(0.0, f64::from(RAW_MAX));
    apply(engine, address, target as u16)
}

fn set_choice(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let address = address(engine, args)?;
    let wanted = args.str("choice")?;
    engine.ensure_list(address)?;
    let effect = blocks::by_id(address.block).map(|b| b.effect_type);
    let listed = effect.and_then(|effect| engine.choices(effect, address.param));
    // Failing the device's own list, a loaded definitions file may have one.
    let from_file = effect
        .and_then(|effect| engine.defs.as_ref()?.effect(effect.into()))
        .and_then(|e| e.param(address.param))
        .map(|def| &def.strings);
    let choices = listed
        .or(from_file)
        .ok_or_else(|| CommandError::NotFound("the device has no list of choices for this parameter".into()))?;
    let index = choices.iter().position(|s| s.trim().eq_ignore_ascii_case(wanted.trim())).ok_or_else(|| {
        CommandError::BadParam { name: "choice", problem: format!("{wanted:?} is not one of this parameter's choices") }
    })?;
    apply(engine, address, index as u16)
}

/// Reads the `column` and `row` arguments.
fn cell_arg(args: &Args<'_>) -> Result<(usize, usize), CommandError> {
    let column = args.int("column", grid::COLUMNS as u64 - 1)? as usize;
    let row = args.int("row", grid::ROWS as u64 - 1)? as usize;
    Ok((column, row))
}

fn put(engine: &mut Engine, block: u16, (column, row): (usize, usize)) -> Result<Value, CommandError> {
    let cell = (column * grid::ROWS + row) as u8;
    engine.session()?.request(&Request::PlaceBlock { block, cell })?;
    engine.refresh_preset()?;
    Ok(json!({ "block": block, "column": column, "row": row }))
}

fn place_block(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let block = engine.block_arg(args)?;
    let cell = cell_arg(args)?;
    engine.recording_grid(|| format!("Place {}", blocks::label(block)), |engine| put(engine, block, cell))
}

/// A shunt ID no cell uses yet. Each shunt in a preset has its own; the
/// device has a fixed number of them.
fn free_shunt(engine: &Engine, also_taken: &[u16]) -> Result<u16, CommandError> {
    let view = &engine.state.preset.grid;
    let taken = |id: &u16| {
        let cells = (0..grid::COLUMNS).flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)));
        also_taken.contains(id) || cells.into_iter().any(|(column, row)| view.cell(column, row).block == *id)
    };
    blocks::SHUNT_IDS
        .into_iter()
        .find(|id| !taken(id))
        .ok_or_else(|| CommandError::NotFound("every shunt the device has is already in the grid".into()))
}

fn place_shunt(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let shunt = free_shunt(engine, &[])?;
    let cell = cell_arg(args)?;
    engine.recording_grid(|| "Place Shunt".into(), |engine| put(engine, shunt, cell))
}

fn clear_cell(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let cell = cell_arg(args)?;
    let removed = blocks::label(engine.state.preset.grid.cell(cell.0, cell.1).block);
    engine.recording_grid(|| format!("Remove {removed}"), |engine| put(engine, blocks::EMPTY, cell))
}

fn cable(engine: &mut Engine, args: &Args<'_>, connect: bool) -> Result<Value, CommandError> {
    let label = || if connect { "Connect Cells" } else { "Disconnect Cells" }.to_owned();
    engine.recording_grid(label, |engine| draw_cable(engine, args, connect))
}

fn draw_cable(engine: &mut Engine, args: &Args<'_>, connect: bool) -> Result<Value, CommandError> {
    let (column, row) = cell_arg(args)?;
    let from_row = args.int("from_row", grid::ROWS as u64 - 1)? as usize;
    if column == 0 {
        return Err(CommandError::BadParam {
            name: "column",
            problem: "the first column is fed by the input; cables start at column 1".into(),
        });
    }
    let from = ((column - 1) * grid::ROWS + from_row) as u8;
    let to = (column * grid::ROWS + row) as u8;
    engine.session()?.request(&Request::Cable { from, to, connect })?;
    engine.refresh_preset()?;
    Ok(json!({ "column": column, "row": row, "from_row": from_row, "connected": connect }))
}

fn send_place(engine: &mut Engine, block: u16, column: usize, row: usize) -> Result<(), CommandError> {
    let cell = (column * grid::ROWS + row) as u8;
    engine.session()?.request(&Request::PlaceBlock { block, cell })?;
    Ok(())
}

fn move_block(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (column, row) = cell_arg(args)?;
    let moved = blocks::label(engine.state.preset.grid.cell(column, row).block);
    engine.recording_grid(|| format!("Move {moved}"), |engine| carry_block(engine, args))
}

fn carry_block(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (column, row) = cell_arg(args)?;
    let to_column = args.int("to_column", grid::COLUMNS as u64 - 1)? as usize;
    let to_row = args.int("to_row", grid::ROWS as u64 - 1)? as usize;
    let view = &engine.state.preset.grid;
    let moved = view.cell(column, row).block;
    if moved == blocks::EMPTY {
        return Err(CommandError::NotFound(format!("cell {column}, {row} is empty")));
    }
    if (column, row) == (to_column, to_row) {
        return Ok(json!({ "block": moved, "column": to_column, "row": to_row }));
    }
    if view.cell(to_column, to_row).block != blocks::EMPTY {
        return Err(CommandError::BadParam {
            name: "to_column",
            problem: format!(
                "cell {to_column}, {to_row} is taken by {}",
                blocks::label(view.cell(to_column, to_row).block)
            ),
        });
    }
    // The device drops a cell's cables when the cell empties. Within a
    // column they can follow the block: note them now, redraw them after.
    let same_column = to_column == column;
    let sources: Vec<usize> =
        (0..grid::ROWS).filter(|&from| same_column && view.is_fed_by(column, row, from)).collect();
    let takers: Vec<usize> = (0..grid::ROWS)
        .filter(|&taker| same_column && column + 1 < grid::COLUMNS && view.is_fed_by(column + 1, taker, row))
        .collect();
    // A block moves when it is placed again. A shunt is lifted first, so
    // that its ID is never in two cells at once.
    if blocks::is_shunt(moved) {
        send_place(engine, blocks::EMPTY, column, row)?;
    }
    send_place(engine, moved, to_column, to_row)?;
    let cell = |column: usize, row: usize| (column * grid::ROWS + row) as u8;
    for from in sources {
        let cable = Request::Cable { from: cell(column - 1, from), to: cell(column, to_row), connect: true };
        engine.session()?.request(&cable)?;
    }
    for taker in takers {
        let cable = Request::Cable { from: cell(column, to_row), to: cell(column + 1, taker), connect: true };
        engine.session()?.request(&cable)?;
    }
    engine.refresh_preset()?;
    Ok(json!({ "block": moved, "column": to_column, "row": to_row }))
}

/// Fills the empty cells of `row` across `columns` with shunts and cables
/// every cell in that span to its left-hand neighbour in the same row.
fn run_cable(engine: &mut Engine, row: usize, columns: std::ops::RangeInclusive<usize>) -> Result<Value, CommandError> {
    let before = engine.state.preset.grid.clone();
    let mut added: Vec<u16> = Vec::new();
    // Know that there are enough shunts before placing the first.
    let needed = columns.clone().filter(|&column| before.cell(column, row).block == blocks::EMPTY).count();
    for _ in 0..needed {
        let shunt = free_shunt(engine, &added)?;
        added.push(shunt);
    }
    let mut spare = added.iter().copied();
    for column in columns.clone() {
        if before.cell(column, row).block == blocks::EMPTY {
            if let Some(shunt) = spare.next() {
                send_place(engine, shunt, column, row)?;
            }
        }
        // The first column is fed by the input as soon as something sits in it.
        if column > 0 && column > *columns.start() && !before.is_fed_by(column, row, row) {
            let from = ((column - 1) * grid::ROWS + row) as u8;
            let to = (column * grid::ROWS + row) as u8;
            engine.session()?.request(&Request::Cable { from, to, connect: true })?;
        }
    }
    engine.refresh_preset()?;
    Ok(json!({ "row": row, "from_column": columns.start(), "to_column": columns.end(), "shunts_added": added.len() }))
}

fn occupied(engine: &Engine, args: &Args<'_>) -> Result<(usize, usize), CommandError> {
    let (column, row) = cell_arg(args)?;
    if engine.state.preset.grid.cell(column, row).block == blocks::EMPTY {
        return Err(CommandError::NotFound(format!("cell {column}, {row} is empty")));
    }
    Ok((column, row))
}

fn connect_to_input(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (column, row) = occupied(engine, args)?;
    // Start one column early so the first cable drawn is the one into column 1.
    engine.recording_grid(|| "Connect to Input".into(), |engine| run_cable(engine, row, 0..=column))
}

fn connect_to_output(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (column, row) = occupied(engine, args)?;
    engine.recording_grid(|| "Connect to Output".into(), |engine| run_cable(engine, row, column..=grid::COLUMNS - 1))
}

fn connect_cells(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    cable(engine, args, true)
}

fn disconnect_cells(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    cable(engine, args, false)
}

const BLOCK: crate::ParamDoc =
    opt("block", "integer|string", "Block ID or name such as \"Amp 1\"; defaults to the selected block");
const PARAM: crate::ParamDoc =
    req("param", "integer|string", "Parameter ID or symbolic name such as \"DISTORT_DRIVE\"");

const COLUMN: crate::ParamDoc = req("column", "integer", "0 to 11, left to right");
const ROW: crate::ParamDoc = req("row", "integer", "0 to 3, top to bottom");
const FROM_ROW: crate::ParamDoc =
    req("from_row", "integer", "Row, 0 to 3, of the source cell in the column to the left");

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "block.select",
        label: "Select Block",
        menu: "",
        shortcut: "",
        doc: "Make a block of the current preset the edited one and read all its parameters. \
              \"Input Gate\", \"Output\" and \"Controllers\" are part of every preset.",
        params: &[req("block", "integer|string", "Block ID or name such as \"Amp 1\"")],
        enabled: connected,
        run: select,
    },
    CommandSpec {
        id: "block.deselect",
        label: "Deselect Block",
        menu: "Block/Deselect",
        shortcut: "Escape",
        doc: "Close the block editor.",
        params: &[],
        enabled: has_selection,
        run: deselect,
    },
    CommandSpec {
        id: "block.refresh",
        label: "Refresh Block",
        menu: "Block/Refresh Parameters",
        shortcut: "",
        doc: "Re-read every parameter of the selected block.",
        params: &[],
        enabled: has_selection,
        run: refresh,
    },
    CommandSpec {
        id: "block.bypass",
        label: "Bypass Block",
        menu: "",
        shortcut: "",
        doc: "Bypass a block.",
        params: &[BLOCK],
        enabled: connected,
        run: bypass,
    },
    CommandSpec {
        id: "block.engage",
        label: "Engage Block",
        menu: "",
        shortcut: "",
        doc: "Take a block out of bypass.",
        params: &[BLOCK],
        enabled: connected,
        run: engage,
    },
    CommandSpec {
        id: "block.toggle_bypass",
        label: "Toggle Bypass",
        menu: "Block/Toggle Bypass",
        shortcut: "B",
        doc: "Bypass the block if it is engaged, engage it if it is bypassed.",
        params: &[BLOCK],
        enabled: has_selection,
        run: toggle_bypass,
    },
    CommandSpec {
        id: "block.set_xy",
        label: "Set X/Y State",
        menu: "",
        shortcut: "",
        doc: "Switch a block between its X and Y parameter sets.",
        params: &[BLOCK, req("state", "string", "\"X\" or \"Y\"")],
        enabled: connected,
        run: xy,
    },
    CommandSpec {
        id: "block.toggle_xy",
        label: "Toggle X/Y",
        menu: "Block/Toggle X/Y",
        shortcut: "X",
        doc: "Switch the block to its other parameter set.",
        params: &[BLOCK],
        enabled: has_selection,
        run: toggle_xy,
    },
    CommandSpec {
        id: "param.get",
        label: "Read Parameter",
        menu: "",
        shortcut: "",
        doc: "Read one parameter: raw value plus the device's own display text.",
        params: &[BLOCK, PARAM],
        enabled: connected,
        run: get,
    },
    CommandSpec {
        id: "param.set",
        label: "Set Parameter",
        menu: "",
        shortcut: "",
        doc: "Set a parameter to a raw value and return what the device applied.",
        params: &[BLOCK, PARAM, req("value", "integer", "Raw value, 0 to 65534; for enumerations, the choice index")],
        enabled: connected,
        run: set,
    },
    CommandSpec {
        id: "param.set_displayed",
        label: "Set Parameter (Displayed Value)",
        menu: "",
        shortcut: "",
        doc: "Set a continuous parameter to a value in the unit the device displays it in, such as -40 for \
              \"-40.0 dB\" or 1200 for \"1200 Hz\". The device limits values outside the parameter's range.",
        params: &[BLOCK, PARAM, req("value", "number", "The value as it should read on the device")],
        enabled: connected,
        run: set_displayed,
    },
    CommandSpec {
        id: "param.set_normalized",
        label: "Set Parameter (0 to 1)",
        menu: "",
        shortcut: "",
        doc: "Set a continuous parameter by knob position.",
        params: &[BLOCK, PARAM, req("position", "number", "0.0 (minimum) to 1.0 (maximum)")],
        enabled: connected,
        run: set_normalized,
    },
    CommandSpec {
        id: "param.nudge",
        label: "Nudge Parameter",
        menu: "",
        shortcut: "",
        doc: "Move a continuous parameter up or down in steps of one percent of its range.",
        params: &[BLOCK, PARAM, req("steps", "number", "-100 to 100; negative moves down")],
        enabled: connected,
        run: nudge,
    },
    CommandSpec {
        id: "param.set_choice",
        label: "Set Parameter by Choice",
        menu: "",
        shortcut: "",
        doc: "Set an enumerated parameter by the text of the choice. The list of choices is read from the \
              device if it has not been yet.",
        params: &[BLOCK, PARAM, req("choice", "string", "Choice text, e.g. an amp model name")],
        enabled: connected,
        run: set_choice,
    },
    CommandSpec {
        id: "grid.place_block",
        label: "Place Block",
        menu: "",
        shortcut: "",
        doc: "Put a block in a grid cell of the edit buffer. A block that is already in the grid \
              moves. Only the first column is cabled on arrival; use grid.connect for the rest.",
        params: &[req("block", "integer|string", "Block ID or name"), COLUMN, ROW],
        enabled: connected,
        run: place_block,
    },
    CommandSpec {
        id: "grid.place_shunt",
        label: "Place Shunt",
        menu: "",
        shortcut: "",
        doc: "Put a shunt, a plain pass-through, in a grid cell.",
        params: &[COLUMN, ROW],
        enabled: connected,
        run: place_shunt,
    },
    CommandSpec {
        id: "grid.clear_cell",
        label: "Clear Cell",
        menu: "",
        shortcut: "",
        doc: "Remove whatever block or shunt sits in a grid cell.",
        params: &[COLUMN, ROW],
        enabled: connected,
        run: clear_cell,
    },
    CommandSpec {
        id: "grid.move_block",
        label: "Move Block",
        menu: "",
        shortcut: "",
        doc: "Move a block or shunt to an empty cell. Within a column its cables move with it; \
              across columns the device drops them and they have to be drawn again.",
        params: &[
            COLUMN,
            ROW,
            req("to_column", "integer", "Destination column, 0 to 11"),
            req("to_row", "integer", "Destination row, 0 to 3"),
        ],
        enabled: connected,
        run: move_block,
    },
    CommandSpec {
        id: "grid.connect_to_input",
        label: "Connect to Input",
        menu: "",
        shortcut: "",
        doc: "Run a cable from the input to a block along its row, adding shunts in the empty cells on the way.",
        params: &[COLUMN, ROW],
        enabled: connected,
        run: connect_to_input,
    },
    CommandSpec {
        id: "grid.connect_to_output",
        label: "Connect to Output",
        menu: "",
        shortcut: "",
        doc: "Run a cable from a block to the output along its row, adding shunts in the empty cells on the way.",
        params: &[COLUMN, ROW],
        enabled: connected,
        run: connect_to_output,
    },
    CommandSpec {
        id: "grid.connect",
        label: "Connect Cells",
        menu: "",
        shortcut: "",
        doc: "Cable a cell to a cell in the column on its left.",
        params: &[COLUMN, ROW, FROM_ROW],
        enabled: connected,
        run: connect_cells,
    },
    CommandSpec {
        id: "grid.disconnect",
        label: "Disconnect Cells",
        menu: "",
        shortcut: "",
        doc: "Remove the cable between a cell and a cell in the column on its left.",
        params: &[COLUMN, ROW, FROM_ROW],
        enabled: connected,
        run: disconnect_cells,
    },
];
