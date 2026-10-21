//! Grid `CellMap` — the §8.5 auto-placement occupancy bitmap.
//!
//! This file owns the `GridCellMap` structure (`GRID_STEP3_DESIGN.md` D4): a
//! row-major bitmap of claimed cells, generalizing T1's row-major fill into the
//! full §8.5 auto-placement (definite → locked-to-major → remaining autos). It
//! is scratch-owned, high-water-mark, update-scoped: its backing `Vec<u64>` is
//! retained across updates (`reset` keeps capacity), and the only resizes are
//! appending zeroed rows / widening the row stride, both amortized into the
//! high-water mark so steady state is allocation-free.
#![allow(dead_code)] // capacity() is the high-water observability surface, read only by tests
//!
//! Invariants and rules:
//!
//! - row-major: bit `(row * 64 + col)` is the cell `(row, col)`; each row spans
//!   `words_per_row = ceil(width / 64)` `u64` words
//! - `is_free`/`claim` are the two closed operations the placement algorithm
//!   uses; `is_free` treats out-of-bounds cells as free (cells beyond the
//!   current map are implicitly unoccupied)
//! - `claim` grows the map (`ensure_size`) before setting bits; a width growth
//!   re-strides rows in place so the backing capacity is reused (no per-solve
//!   allocation after warmup)

/// The §8.5 occupancy bitmap: a row-major `u64` bitset over claimed cells.
#[derive(Debug, Default)]
pub(crate) struct GridCellMap {
    /// Row-major bitset backing (retained at high-water mark across updates).
    bits: Vec<u64>,
    /// The current column count (cells per row).
    width: usize,
    /// The current row count.
    height: usize,
    /// `u64` words per row (`width.div_ceil(64)`), kept in sync with `width`.
    words_per_row: usize,
}

impl GridCellMap {
    /// Clears every cell for the next update, keeping the backing capacity at
    /// its high-water mark.
    pub(crate) fn reset(&mut self) {
        self.bits.clear();
        self.width = 0;
        self.height = 0;
        self.words_per_row = 0;
    }

    /// The current column count.
    pub(crate) fn width(&self) -> usize {
        self.width
    }

    /// The current row count.
    pub(crate) fn height(&self) -> usize {
        self.height
    }

    /// The retained backing capacity in `u64` words (the high-water mark).
    pub(crate) fn capacity(&self) -> usize {
        self.bits.capacity()
    }

    /// True when the rectangular region `(row, col, row_span, col_span)` holds
    /// no claimed cell. Out-of-bounds cells are implicitly free.
    pub(crate) fn is_free(&self, row: usize, col: usize, row_span: usize, col_span: usize) -> bool {
        for r in row..row.saturating_add(row_span) {
            if r >= self.height {
                continue; // rows beyond the map are free
            }
            for c in col..col.saturating_add(col_span) {
                if c >= self.width {
                    continue; // columns beyond the map are free
                }
                let index = self.bit_index(r, c);
                if self.bits[index] & Self::bit_mask(c) != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// Claims the rectangular region `(row, col, row_span, col_span)`, growing
    /// the map as needed.
    pub(crate) fn claim(&mut self, row: usize, col: usize, row_span: usize, col_span: usize) {
        self.ensure_size(row + row_span, col + col_span);
        for r in row..row + row_span {
            for c in col..col + col_span {
                let index = self.bit_index(r, c);
                self.bits[index] |= Self::bit_mask(c);
            }
        }
    }

    /// Grows the map to at least `rows` × `cols`, zeroing any new cells. Width
    /// growth re-strides rows in place; row growth appends zeroed words.
    fn ensure_size(&mut self, rows: usize, cols: usize) {
        if cols > self.width {
            self.grow_width(cols);
        }
        if rows > self.height {
            self.grow_rows(rows);
        }
    }

    /// Appends zeroed rows until the map has `rows` rows.
    fn grow_rows(&mut self, rows: usize) {
        self.bits.resize(rows * self.words_per_row, 0);
        self.height = rows;
    }

    /// Widens the map to `cols` columns, re-striding the existing rows in place
    /// (bottom-up, right-to-left, so no source word is overwritten before it is
    /// read; the newly widened columns stay zeroed). Backing capacity is reused.
    fn grow_width(&mut self, cols: usize) {
        let new_words_per_row = cols.div_ceil(64);
        let old_words_per_row = self.words_per_row;
        if new_words_per_row == old_words_per_row {
            // No re-stride needed: the new columns fit in the existing words,
            // whose freshly-added bits are already zero (never claimed while
            // out of bounds).
            self.width = cols;
            return;
        }
        let old_height = self.height;
        debug_assert!(new_words_per_row > old_words_per_row);
        self.bits.resize(new_words_per_row * old_height, 0);
        for row in (0..old_height).rev() {
            for word in (0..old_words_per_row).rev() {
                let src = row * old_words_per_row + word;
                let dst = row * new_words_per_row + word;
                self.bits[dst] = self.bits[src];
                if dst != src {
                    self.bits[src] = 0;
                }
            }
        }
        self.words_per_row = new_words_per_row;
        self.width = cols;
    }

    /// The backing word index for cell `(row, col)`.
    fn bit_index(&self, row: usize, col: usize) -> usize {
        row * self.words_per_row + col / 64
    }

    /// The bit within a word for column `col`.
    fn bit_mask(col: usize) -> u64 {
        1u64 << (col % 64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_and_is_free_round_trip() {
        let mut map = GridCellMap::default();
        assert!(map.is_free(0, 0, 1, 1));
        map.claim(0, 0, 1, 1);
        assert!(!map.is_free(0, 0, 1, 1));
        assert!(map.is_free(0, 1, 1, 1));
        assert!(map.is_free(1, 0, 1, 1));
    }

    #[test]
    fn claim_spans_multiple_cells() {
        let mut map = GridCellMap::default();
        map.claim(1, 2, 2, 3); // rows 1..3, cols 2..5
        assert!(!map.is_free(1, 2, 1, 1));
        assert!(!map.is_free(2, 4, 1, 1));
        assert!(!map.is_free(1, 2, 2, 3));
        assert!(map.is_free(1, 5, 1, 1));
        assert!(map.is_free(3, 2, 1, 1));
        assert_eq!(map.width(), 5);
        assert_eq!(map.height(), 3);
    }

    #[test]
    fn out_of_bounds_is_free() {
        let map = GridCellMap::default();
        assert!(map.is_free(100, 100, 1, 1));
    }

    #[test]
    fn reset_keeps_backing_capacity() {
        let mut map = GridCellMap::default();
        map.claim(3, 3, 2, 2);
        let capacity_before = map.capacity();
        assert!(capacity_before >= 1);
        map.reset();
        assert_eq!(map.width(), 0);
        assert_eq!(map.height(), 0);
        assert!(map.is_free(0, 0, 1, 1));
        assert_eq!(map.capacity(), capacity_before, "reset must keep capacity");
    }

    #[test]
    fn width_growth_restrides_rows_without_losing_occupancy() {
        let mut map = GridCellMap::default();
        // Occupy a cell in the first word region, then widen past 64 columns.
        map.claim(0, 1, 1, 1);
        assert!(!map.is_free(0, 1, 1, 1));
        map.claim(0, 70, 1, 1); // forces a width growth to 71 columns
        assert!(
            !map.is_free(0, 1, 1, 1),
            "old occupancy survives re-striding"
        );
        assert!(!map.is_free(0, 70, 1, 1));
        assert!(map.is_free(0, 0, 1, 1));
    }
}
