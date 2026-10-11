//! The 12-column, 4-row routing grid (spec section 4.4).

use axegrid_sysex::GridCell;

pub const COLUMNS: usize = 12;
pub const ROWS: usize = 4;
pub const CELLS: usize = COLUMNS * ROWS;

const EMPTY_CELL: GridCell = GridCell { block: 0, routing: 0 };

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    /// Column by column: index = column * ROWS + row.
    cells: [GridCell; CELLS],
}

impl Default for Grid {
    fn default() -> Self {
        Self { cells: [EMPTY_CELL; CELLS] }
    }
}

impl Grid {
    /// Builds a grid from a device reply; missing cells stay empty and
    /// surplus ones are ignored.
    pub fn from_cells(reply: &[GridCell]) -> Self {
        let mut grid = Self::default();
        for (slot, cell) in grid.cells.iter_mut().zip(reply) {
            *slot = *cell;
        }
        grid
    }

    pub fn cell(&self, column: usize, row: usize) -> GridCell {
        self.cells[column * ROWS + row]
    }

    /// Whether the cell takes input from `from_row` of the column to its left.
    pub fn is_fed_by(&self, column: usize, row: usize, from_row: usize) -> bool {
        column > 0 && self.cell(column, row).routing & (1 << from_row) != 0
    }

    /// IDs of the blocks placed in the grid, left to right, without shunts.
    pub fn blocks(&self) -> Vec<u16> {
        let mut ids: Vec<u16> = self
            .cells
            .iter()
            .map(|c| c.block)
            .filter(|&id| id != crate::blocks::EMPTY && !crate::blocks::is_shunt(id))
            .collect();
        ids.dedup();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_is_column_major() {
        let mut reply = vec![EMPTY_CELL; CELLS];
        reply[ROWS + 2] = GridCell { block: 106, routing: 0b0100 };
        reply[2 * ROWS + 2] = GridCell { block: 200, routing: 0b0100 };
        reply[3 * ROWS + 2] = GridCell { block: 108, routing: 0b0101 };
        let grid = Grid::from_cells(&reply);

        assert_eq!(grid.cell(1, 2).block, 106);
        assert!(grid.is_fed_by(3, 2, 0));
        assert!(grid.is_fed_by(3, 2, 2));
        assert!(!grid.is_fed_by(3, 2, 1));
        assert_eq!(grid.blocks(), [106, 108]);
    }

    #[test]
    fn short_reply_leaves_rest_empty() {
        let grid = Grid::from_cells(&[GridCell { block: 100, routing: 0 }]);
        assert_eq!(grid.cell(0, 0).block, 100);
        assert_eq!(grid.cell(11, 3), EMPTY_CELL);
        assert!(!grid.is_fed_by(0, 0, 0));
    }
}
