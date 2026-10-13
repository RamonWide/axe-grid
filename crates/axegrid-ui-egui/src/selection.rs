//! Which cells of the grid are selected, and where they may be moved to.
//! View state only: moving them is the `grid.move_blocks` command.

use axegrid_model::grid;
use serde_json::{json, Value};

/// A grid cell: column, then row.
pub(crate) type Cell = (usize, usize);

/// The cells picked to be moved together.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Selection {
    /// One bit for each cell, column by column.
    cells: u64,
    /// The cell clicked last without Shift: where a Shift+click reaches from.
    anchor: Option<Cell>,
}

fn bit((column, row): Cell) -> u64 {
    if column < grid::COLUMNS && row < grid::ROWS {
        1 << (column * grid::ROWS + row)
    } else {
        0
    }
}

fn every_cell() -> impl Iterator<Item = Cell> {
    (0..grid::COLUMNS).flat_map(|column| (0..grid::ROWS).map(move |row| (column, row)))
}

/// `cell` moved by an offset, if that is still on the grid.
pub(crate) fn shifted((column, row): Cell, dx: isize, dy: isize) -> Option<Cell> {
    let column = column.checked_add_signed(dx).filter(|&column| column < grid::COLUMNS)?;
    let row = row.checked_add_signed(dy).filter(|&row| row < grid::ROWS)?;
    Some((column, row))
}

/// How far a drag that took hold of `from` has come when the pointer is
/// over `over`, which may be a place beyond the grid's edges.
pub(crate) fn offset(from: Cell, over: (isize, isize)) -> (isize, isize) {
    (over.0.saturating_sub_unsigned(from.0), over.1.saturating_sub_unsigned(from.1))
}

impl Selection {
    pub(crate) fn only(cell: Cell) -> Self {
        Self { cells: bit(cell), anchor: Some(cell) }
    }

    pub(crate) fn of(cells: impl IntoIterator<Item = Cell>) -> Self {
        Self { cells: cells.into_iter().fold(0, |all, cell| all | bit(cell)), anchor: None }
    }

    pub(crate) fn contains(self, cell: Cell) -> bool {
        self.cells & bit(cell) != 0
    }

    pub(crate) fn is_empty(self) -> bool {
        self.cells == 0
    }

    /// The selected cells, left to right and top to bottom within a column.
    pub(crate) fn cells(self) -> impl Iterator<Item = Cell> {
        every_cell().filter(move |&cell| self.contains(cell))
    }

    /// Ctrl+click: takes the cell into the selection, or out of it.
    pub(crate) fn toggle(&mut self, cell: Cell) {
        self.cells ^= bit(cell);
        self.anchor = Some(cell);
    }

    /// Shift+click: selects what `occupied` says is there between the cell
    /// clicked before and this one.
    pub(crate) fn extend_to(&mut self, cell: Cell, occupied: impl Fn(Cell) -> bool) {
        let Some(anchor) = self.anchor else {
            *self = Self::only(cell);
            return;
        };
        let columns = anchor.0.min(cell.0)..=anchor.0.max(cell.0);
        let rows = anchor.1.min(cell.1)..=anchor.1.max(cell.1);
        let between = every_cell().filter(|&(column, row)| columns.contains(&column) && rows.contains(&row));
        self.cells = Self::of(between.filter(|&cell| occupied(cell))).cells;
    }

    /// Without the cells that hold nothing any more.
    pub(crate) fn pruned(self, occupied: impl Fn(Cell) -> bool) -> Self {
        let kept = Self::of(self.cells().filter(|&cell| occupied(cell))).cells;
        Self { cells: kept, anchor: self.anchor.filter(|&anchor| kept & bit(anchor) != 0) }
    }

    /// Whether every selected cell can go `dx` columns and `dy` rows
    /// further: onto the grid, and onto a cell that is free or is being
    /// left by another of them.
    pub(crate) fn can_move(self, dx: isize, dy: isize, occupied: impl Fn(Cell) -> bool) -> bool {
        !self.is_empty()
            && self.cells().all(|cell| shifted(cell, dx, dy).is_some_and(|to| self.contains(to) || !occupied(to)))
    }

    /// The command that moves the selection, if the offset moves anything.
    /// One block alone moves with the command a drag has always used.
    pub(crate) fn move_action(self, dx: isize, dy: isize) -> Option<(&'static str, Value)> {
        if self.is_empty() || (dx, dy) == (0, 0) {
            return None;
        }
        let cells: Vec<Cell> = self.cells().collect();
        if let [(column, row)] = cells.as_slice() {
            let (to_column, to_row) = shifted((*column, *row), dx, dy)?;
            let single = json!({ "column": column, "row": row, "to_column": to_column, "to_row": to_row });
            return Some(("grid.move_block", single));
        }
        let cells: Vec<Value> = cells.iter().map(|(column, row)| json!({ "column": column, "row": row })).collect();
        Some(("grid.move_blocks", json!({ "cells": cells, "dx": dx, "dy": dy })))
    }

    /// Where the selection is once a move made by [`Self::move_action`] has
    /// gone through, so that it stays on the blocks it was on.
    pub(crate) fn after(id: &str, params: &Value) -> Option<Self> {
        let number = |value: &Value| value.as_u64().and_then(|n| usize::try_from(n).ok());
        match id {
            "grid.move_block" => Some(Self::only((number(&params["to_column"])?, number(&params["to_row"])?))),
            "grid.move_blocks" => {
                let step = |key: &str| params[key].as_i64().and_then(|n| isize::try_from(n).ok());
                let (dx, dy) = (step("dx")?, step("dy")?);
                let moved = params["cells"]
                    .as_array()?
                    .iter()
                    .map(|cell| shifted((number(&cell["column"])?, number(&cell["row"])?), dx, dy));
                Some(Self::of(moved.collect::<Option<Vec<Cell>>>()?))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of blocks in columns 1 to 4 of row 1, and one at 6, 2.
    fn occupied(cell: Cell) -> bool {
        matches!(cell, (1..=4, 1) | (6, 2))
    }

    fn cells(selection: Selection) -> Vec<Cell> {
        selection.cells().collect()
    }

    #[test]
    fn clicks_pick_cells() {
        let mut selection = Selection::only((1, 1));
        assert_eq!(cells(selection), [(1, 1)]);
        // Ctrl+click adds, and takes out again.
        selection.toggle((3, 1));
        selection.toggle((6, 2));
        assert_eq!(cells(selection), [(1, 1), (3, 1), (6, 2)]);
        selection.toggle((3, 1));
        assert_eq!(cells(selection), [(1, 1), (6, 2)]);
        assert!(selection.contains((6, 2)) && !selection.contains((3, 1)));
        selection.toggle((1, 1));
        selection.toggle((6, 2));
        assert!(selection.is_empty());
        // A cell that is not on the grid is never selected.
        assert!(Selection::only((12, 0)).is_empty() && Selection::only((0, 4)).is_empty());
        assert!(Selection::of([(99, 99)]).is_empty());
    }

    #[test]
    fn shift_click_reaches_from_the_last_cell_clicked() {
        let mut selection = Selection::only((2, 1));
        selection.extend_to((6, 2), occupied);
        // Everything that is there in the rectangle between the two, nothing that is not.
        assert_eq!(cells(selection), [(2, 1), (3, 1), (4, 1), (6, 2)]);
        // A second Shift+click reaches from the same cell, the other way too.
        selection.extend_to((1, 0), occupied);
        assert_eq!(cells(selection), [(1, 1), (2, 1)]);
        // With nothing clicked before, it selects the one cell.
        let mut fresh = Selection::default();
        fresh.extend_to((4, 1), occupied);
        assert_eq!(cells(fresh), [(4, 1)]);
    }

    #[test]
    fn cells_that_emptied_drop_out() {
        let mut selection = Selection::of([(1, 1), (5, 0), (6, 2)]);
        selection.toggle((9, 3));
        let pruned = selection.pruned(occupied);
        assert_eq!(cells(pruned), [(1, 1), (6, 2)]);
        // The cell a Shift+click would reach from went with it.
        let mut next = pruned;
        next.extend_to((4, 1), occupied);
        assert_eq!(cells(next), [(4, 1)]);
    }

    #[test]
    fn a_drag_is_measured_in_cells() {
        assert_eq!(offset((2, 1), (5, 0)), (3, -1));
        assert_eq!(offset((2, 1), (2, 1)), (0, 0));
        // The pointer may be beyond the grid on any side.
        assert_eq!(offset((2, 1), (-3, 7)), (-5, 6));
        assert_eq!(shifted((2, 1), 3, -1), Some((5, 0)));
        assert_eq!(shifted((2, 1), -3, 0), None);
        assert_eq!(shifted((2, 1), 10, 0), None);
        assert_eq!(shifted((2, 1), 0, 3), None);
        assert_eq!(shifted((11, 3), 0, 0), Some((11, 3)));
    }

    #[test]
    fn a_selection_moves_only_where_all_of_it_fits() {
        let pair = Selection::of([(1, 1), (2, 1)]);
        // Up, down and away are free.
        assert!(pair.can_move(0, -1, occupied) && pair.can_move(0, 2, occupied) && pair.can_move(4, 0, occupied));
        // To the right the second would land on a block that stays.
        assert!(!pair.can_move(1, 0, occupied));
        // Off the grid on either side.
        assert!(!pair.can_move(-2, 0, occupied) && !pair.can_move(0, -2, occupied) && !pair.can_move(0, 3, occupied));
        // The whole row may shift along itself: each lands where another is leaving.
        let row = Selection::of([(1, 1), (2, 1), (3, 1), (4, 1)]);
        assert!(row.can_move(1, 0, occupied) && row.can_move(-1, 0, occupied));
        // Onto the block at 6, 2 it may not.
        assert!(!Selection::of([(4, 1)]).can_move(2, 1, occupied));
        assert!(!Selection::default().can_move(1, 0, occupied));
    }

    #[test]
    fn a_move_is_a_command_the_engine_has() {
        let pair = Selection::of([(1, 1), (2, 1)]);
        let (id, params) = pair.move_action(2, -1).unwrap_or_default();
        assert_eq!(id, "grid.move_blocks");
        assert_eq!(
            params,
            json!({ "cells": [{ "column": 1, "row": 1 }, { "column": 2, "row": 1 }], "dx": 2, "dy": -1 })
        );
        // One block alone moves the way it always has.
        let (single, to) = Selection::only((4, 1)).move_action(1, 2).unwrap_or_default();
        assert_eq!((single, to), ("grid.move_block", json!({ "column": 4, "row": 1, "to_column": 5, "to_row": 3 })));
        for id in [id, single] {
            assert!(axegrid_engine::find(id).is_some(), "{id}");
        }
        // Nowhere to go, nothing to move, or off the grid: no command.
        assert!(pair.move_action(0, 0).is_none());
        assert!(Selection::default().move_action(1, 0).is_none());
        assert!(Selection::only((4, 1)).move_action(0, 3).is_none());
    }

    #[test]
    fn the_selection_follows_the_blocks_it_moved() {
        let pair = Selection::of([(1, 1), (2, 1)]);
        let (id, params) = pair.move_action(2, -1).unwrap_or_default();
        assert_eq!(Selection::after(id, &params).map(cells), Some(vec![(3, 0), (4, 0)]));
        let (id, params) = Selection::only((4, 1)).move_action(1, 2).unwrap_or_default();
        assert_eq!(Selection::after(id, &params).map(cells), Some(vec![(5, 3)]));
        // Anything else, or parameters that make no sense, leaves it alone.
        assert_eq!(Selection::after("grid.clear_cell", &params), None);
        assert_eq!(
            Selection::after("grid.move_blocks", &json!({ "cells": [{ "column": 1 }], "dx": 1, "dy": 0 })),
            None
        );
        assert_eq!(Selection::after("grid.move_blocks", &json!({ "cells": 3 })), None);
    }
}
