#![allow(dead_code)]

//! Grid item lane — the DOM-ordered item-view lane grid placement iterates.
//!
//! This file owns `GridItemLane` and its entry type `GridItemLaneEntry`. Its
//! purpose is to hold one assembled `GridItemView` per grid child in DOM order,
//! plus the item's auto-placed cell, its border/padding insets, and its
//! resolved size, so placement walks the container's item-lane range directly
//! with DOM order preserved (the flex item lane's discipline, `GRID_MAPPING.md`
//! §3 as corrected 2026-10-20: the ORDER lane carries verbatim; the item lane is
//! type-specific).
//!
//! Invariants and rules:
//!
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes children in DOM order, then closes the range with `finish_range`; a
//!   closed range is never mutated again within that update
//! - `row`/`col` are the item's row-major auto-placed cell (D3); the item
//!   resolution phase writes them plus the resolved content width and the row
//!   cross contribution (the refold result for a flagged-and-width-changed item)
//! - `horizontal_border_padding_px`/`vertical_border_padding_px` are the item's
//!   own insets (read at measure fold), which placement subtracts to derive the
//!   stretched content size and which row sizing adds to the content cross
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation

use crate::domains::layout::grid::GridItemView;
use crate::types::LaneRange;

#[cfg(test)]
use crate::domains::layout::flex_item_source::FlexItemSource;
#[cfg(test)]
use crate::domains::style::computed::{GridLine, JustifySelfKw};
#[cfg(test)]
use crate::types::DomNodeSlotKey;

/// One grid item's item-lane slot: the assembled view plus its auto-placed cell
/// and resolved size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridItemLaneEntry {
    /// The assembled item view (T1's `GridItemView`), read by track sizing,
    /// row sizing, and placement via the order lane's `dom_index`.
    pub(crate) view: GridItemView,
    /// The item's horizontal (left + right) border + padding.
    pub(crate) horizontal_border_padding_px: f32,
    /// The item's vertical (top + bottom) border + padding.
    pub(crate) vertical_border_padding_px: f32,
    /// The item's auto-placed row (D3: `row = i / col_count`).
    pub(crate) row: u32,
    /// The item's auto-placed column (D3: `col = i % col_count`).
    pub(crate) col: u32,
    /// The item's resolved content width (the cell track width minus insets
    /// under stretch, the definite width otherwise), written by item resolution.
    pub(crate) resolved_content_width_px: f32,
    /// The item's row-size cross contribution (border-box content height at the
    /// resolved width — the refold result for a flagged-and-width-changed item).
    pub(crate) resolved_cross_contribution_px: f32,
}

/// Vec-backed storage for DOM-ordered grid item views, sliced per container via
/// `{start, len}` ranges.
#[derive(Debug, Default)]
pub(crate) struct GridItemLane {
    entries: Vec<GridItemLaneEntry>,
}

impl GridItemLane {
    /// Opens a range at the current lane end and returns its start index.
    pub(crate) fn begin_range(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("grid item lane length should fit into u32")
    }

    /// Appends one assembled view in DOM order (with its border/padding insets)
    /// and returns its absolute lane index (the `dom_index` the order-lane entry
    /// stores). The cell and resolved-size fields are zeroed; the resolution
    /// phases fill them.
    pub(crate) fn push(
        &mut self,
        view: GridItemView,
        horizontal_border_padding_px: f32,
        vertical_border_padding_px: f32,
    ) -> u32 {
        let dom_index = self.entries.len();
        self.entries.push(GridItemLaneEntry {
            view,
            horizontal_border_padding_px,
            vertical_border_padding_px,
            row: 0,
            col: 0,
            resolved_content_width_px: 0.0,
            resolved_cross_contribution_px: 0.0,
        });
        u32::try_from(dom_index).expect("grid item lane length should fit into u32")
    }

    /// Closes the range opened by a matching `begin_range` call and returns
    /// the `{start, len}` descriptor for the entries pushed since.
    pub(crate) fn finish_range(&self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("grid item lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "grid item lane range start must not exceed lane length"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("grid item lane range length should fit into u32"),
        }
    }

    /// Returns the entries claimed by `range`, in DOM order.
    pub(crate) fn slice(&self, range: LaneRange) -> &[GridItemLaneEntry] {
        let start =
            usize::try_from(range.start).expect("grid item lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("grid item lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Returns the entries claimed by `range` for in-place mutation (the item
    /// resolution phase writes `row`/`col` and the resolved sizes here).
    pub(crate) fn slice_mut(&mut self, range: LaneRange) -> &mut [GridItemLaneEntry] {
        let start =
            usize::try_from(range.start).expect("grid item lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("grid item lane range length should fit into usize");
        &mut self.entries[start..start + len]
    }

    /// Returns the entry at the absolute `dom_index` (the order-lane entry's
    /// back-reference target).
    pub(crate) fn get(&self, dom_index: u32) -> &GridItemLaneEntry {
        let index =
            usize::try_from(dom_index).expect("grid item lane dom_index should fit into usize");
        &self.entries[index]
    }

    /// Returns the entry at the absolute `dom_index` for in-place mutation.
    pub(crate) fn get_mut(&mut self, dom_index: u32) -> &mut GridItemLaneEntry {
        let index =
            usize::try_from(dom_index).expect("grid item lane dom_index should fit into usize");
        &mut self.entries[index]
    }

    /// Clears all entries for the next update, keeping the retained backing
    /// capacity at its high-water mark.
    pub(crate) fn reset(&mut self) {
        self.entries.clear();
    }

    /// Truncates the lane back to `len` entries (the staged nested-measurement
    /// cleanup seam).
    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    /// The number of entries currently held.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The retained backing capacity (the high-water mark after warmup).
    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // entry field assertions compare exact literals
mod tests {
    use super::*;

    fn slot_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn view(order: i32) -> GridItemView {
        GridItemView {
            source: FlexItemSource::BlockItem(slot_key()),
            order,
            content_main_px: 10.0,
            content_cross_px: 10.0,
            h_min_content_px: 10.0,
            h_max_content_px: 10.0,
            margins: [0.0; 4],
            auto_margins: 0,
            replaced_ratio: false,
            main_influences_cross: false,
            column_start: GridLine::Auto,
            column_end: GridLine::Auto,
            row_start: GridLine::Auto,
            row_end: GridLine::Auto,
            justify_self: JustifySelfKw::Auto,
            align_self: None,
            col_span: 1,
            row_span: 1,
        }
    }

    #[test]
    fn range_claim_push_and_reset_round_trip() {
        let mut lane = GridItemLane::default();
        let start = lane.begin_range();
        assert_eq!(start, 0);

        assert_eq!(lane.push(view(0), 2.0, 4.0), 0);
        assert_eq!(lane.push(view(1), 0.0, 0.0), 1);

        let range = lane.finish_range(start);
        assert_eq!((range.start, range.len), (0, 2));
        assert_eq!(lane.slice(range).len(), 2);
        assert_eq!(lane.slice(range)[0].view.order, 0);
        assert_eq!(lane.slice(range)[0].vertical_border_padding_px, 4.0);

        let capacity_before = lane.capacity();
        lane.reset();
        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");
    }

    #[test]
    fn resolution_writes_cell_and_sizes_in_place() {
        let mut lane = GridItemLane::default();
        let start = lane.begin_range();
        let first = lane.push(view(0), 0.0, 0.0);
        let second = lane.push(view(1), 0.0, 0.0);
        let range = lane.finish_range(start);

        for (index, entry) in lane.slice_mut(range).iter_mut().enumerate() {
            entry.row = u32::try_from(index).unwrap();
            entry.col = 2;
            entry.resolved_content_width_px = 100.0;
            entry.resolved_cross_contribution_px = 40.0;
        }

        assert_eq!(lane.get(first).row, 0);
        assert_eq!(lane.get(second).row, 1);
        assert_eq!(lane.get(first).col, 2);
        assert_eq!(lane.get(second).resolved_content_width_px, 100.0);
        assert_eq!(lane.get(second).resolved_cross_contribution_px, 40.0);
    }
}
