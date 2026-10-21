//! Interned grid-template-areas computed value (`GridTemplateAreas` + `AreaRect`).
//!
//! This file owns the areas-map value type (grid T3, `GRID_STEP4_DESIGN.md`
//! D1): the parsed string rows projected into a `[row][col]` cell grid of
//! interned names (`.`, `..`, … dot-runs become `None`) plus the deduped name →
//! bounding-rectangle table. It is plain `Copy` data; the grammar, validation
//! (uniform-column, rectangularity, capacity), and value-dedup interning live in
//! `decl_parse.rs`/`decl_store.rs`, the canonical serialization lives in
//! `decl_value.rs`, and the placement-time resolution (the D4 order, the
//! area-implied `<name>-start`/`<name>-end` lines) lives in `grid.rs`.
//!
//! Invariants and rules:
//!
//! - `row_count` ≤ `GRID_AREA_ROW_CAPACITY` (8), `col_count` ≤
//!   `GRID_AREA_COL_CAPACITY` (12), and `names` holds at most
//!   `GRID_AREA_NAME_CAPACITY` (12) distinct names — all three enforced at
//!   parse with counted drops, never a silent cap
//! - `names` is the deduped name → bounding-rect table; every name's cells tile
//!   exactly one rectangle (the parse-time rectangularity guard), so each entry
//!   is the definitive source for the area's D4 rect and its implied
//!   `<name>-start`/`<name>-end` lines
//! - `AreaRect` bounds are INCLUSIVE (`max_*` is the last row/column index),
//!   so a rect's span is `max − min + 1`

use arrayvec::ArrayVec;

use crate::domains::style::decl_value::TextKey;

/// The maximum rows a template-areas map may hold (the D1 capacity gate).
pub(crate) const GRID_AREA_ROW_CAPACITY: usize = 8;

/// The maximum columns a template-areas map may hold (the D1 capacity gate).
pub(crate) const GRID_AREA_COL_CAPACITY: usize = 12;

/// The maximum distinct names a template-areas map may hold (the D1 capacity
/// gate).
pub(crate) const GRID_AREA_NAME_CAPACITY: usize = 12;

/// One area's inclusive bounding rectangle in area-grid coordinates (0-based).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaRect {
    /// First row of the area.
    pub(crate) min_row: u8,
    /// First column of the area.
    pub(crate) min_col: u8,
    /// Last row of the area (inclusive).
    pub(crate) max_row: u8,
    /// Last column of the area (inclusive).
    pub(crate) max_col: u8,
}

/// A parsed, validated template-areas map: the `[row][col]` cell grid of
/// interned names plus the deduped name → rect table.
#[derive(Clone, Debug, PartialEq)]
pub struct GridTemplateAreas {
    /// `cells[row][col]` — `Some(name)` for a named area cell, `None` for a
    /// dot-run (empty) cell.
    pub(crate) cells:
        ArrayVec<ArrayVec<Option<TextKey>, GRID_AREA_COL_CAPACITY>, GRID_AREA_ROW_CAPACITY>,
    /// The deduped name → bounding-rect table (in first-appearance order).
    pub(crate) names: ArrayVec<(TextKey, AreaRect), GRID_AREA_NAME_CAPACITY>,
    /// The validated row count.
    pub(crate) row_count: u8,
    /// The validated (uniform) column count.
    pub(crate) col_count: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_rect_bounds_are_inclusive() {
        let rect = AreaRect {
            min_row: 0,
            min_col: 1,
            max_row: 2,
            max_col: 1,
        };
        assert_eq!(rect.max_row - rect.min_row + 1, 3);
        assert_eq!(rect.max_col - rect.min_col + 1, 1);
    }

    #[test]
    fn empty_areas_map_has_no_rows_or_columns() {
        let areas = GridTemplateAreas {
            cells: ArrayVec::new(),
            names: ArrayVec::new(),
            row_count: 0,
            col_count: 0,
        };
        assert_eq!(areas.row_count, 0);
        assert_eq!(areas.col_count, 0);
        assert!(areas.names.is_empty());
    }

    #[test]
    fn cells_index_by_row_then_column() {
        let mut cells: ArrayVec<
            ArrayVec<Option<TextKey>, GRID_AREA_COL_CAPACITY>,
            GRID_AREA_ROW_CAPACITY,
        > = ArrayVec::new();
        let mut row = ArrayVec::new();
        row.push(Some(TextKey(1)));
        row.push(None);
        cells.push(row);
        assert_eq!(cells[0][0], Some(TextKey(1)));
        assert_eq!(cells[0][1], None);
    }
}
