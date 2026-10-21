#![allow(dead_code)]

//! Grid track lane — the per-axis track sizing state the grid solver claims.
//!
//! This file owns `GridTrackEntry` and `GridTrackLane`. Its purpose is to hold
//! the per-container track sizing state (`GRID_MAPPING.md` D1, Gecko's expanded
//! `TrackSize` shape) for both axes: the explicit column tracks and the
//! implicit row tracks are claimed as two disjoint ranges during one solve,
//! sized by the track sizing algorithm, and read back at placement.
//!
//! Invariants and rules:
//!
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes its track entries, then closes the range with `finish_range`; a
//!   closed range is never mutated again within that update (the sizing phases
//!   mutate entries IN PLACE via `slice_mut`, within the closed range)
//! - `base_px` is the resolved base size after the current phase and, once
//!   sizing completes, the final track size; `limit_px` is the growth limit
//!   (the `minmax` max side, `f32::INFINITY` for unbounded growth); the pair is
//!   Gecko's `mBase`/`mLimit` (`nsGridContainerFrame.cpp`)
//! - `flex_factor` is the `fr` value (0.0 for non-flexible tracks); `is_flexible`
//!   mirrors it as a flag so the fr expand phase can test membership without a
//!   float comparison
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation

use crate::types::LaneRange;

/// One grid track's sizing state (per `GRID_MAPPING.md` D1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GridTrackEntry {
    /// Resolved base size after the current sizing phase; the final track size
    /// once sizing completes.
    pub(crate) base_px: f32,
    /// Growth limit (`minmax` max side; `f32::INFINITY` = unbounded).
    pub(crate) limit_px: f32,
    /// Per-axis state bits (reserved for the T2 re-resolve; T1 leaves them 0).
    pub(crate) state_bits: u8,
    /// The `fr` value (0.0 = non-flexible).
    pub(crate) flex_factor: f32,
    /// Whether this track is flexible (an `fr` track).
    pub(crate) is_flexible: bool,
}

/// Vec-backed storage for grid track entries, sliced per container and per
/// axis via `{start, len}` ranges.
#[derive(Debug, Default)]
pub(crate) struct GridTrackLane {
    entries: Vec<GridTrackEntry>,
}

impl GridTrackLane {
    /// Opens a range at the current lane end and returns its start index.
    pub(crate) fn begin_range(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("track lane length should fit into u32")
    }

    /// Appends one track entry and returns its absolute lane index.
    pub(crate) fn push(&mut self, entry: GridTrackEntry) -> u32 {
        let index = self.entries.len();
        self.entries.push(entry);
        u32::try_from(index).expect("track lane length should fit into u32")
    }

    /// Closes the range opened by a matching `begin_range` call and returns the
    /// `{start, len}` descriptor for the entries pushed since.
    pub(crate) fn finish_range(&self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("track lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "track lane range start must not exceed lane length"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("track lane range length should fit into u32"),
        }
    }

    /// Returns the entries claimed by `range`, in push order.
    pub(crate) fn slice(&self, range: LaneRange) -> &[GridTrackEntry] {
        let start =
            usize::try_from(range.start).expect("track lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("track lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Returns the entries claimed by `range` for in-place mutation (the
    /// sizing phases write `base_px`/`limit_px` here).
    pub(crate) fn slice_mut(&mut self, range: LaneRange) -> &mut [GridTrackEntry] {
        let start =
            usize::try_from(range.start).expect("track lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("track lane range length should fit into usize");
        &mut self.entries[start..start + len]
    }

    /// Returns the entry at the absolute `index`.
    pub(crate) fn get(&self, index: u32) -> &GridTrackEntry {
        let index = usize::try_from(index).expect("track lane index should fit into usize");
        &self.entries[index]
    }

    /// Returns the entry at the absolute `index` for in-place mutation.
    pub(crate) fn get_mut(&mut self, index: u32) -> &mut GridTrackEntry {
        let index = usize::try_from(index).expect("track lane index should fit into usize");
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

    fn entry(base_px: f32, limit_px: f32, flex_factor: f32, is_flexible: bool) -> GridTrackEntry {
        GridTrackEntry {
            base_px,
            limit_px,
            state_bits: 0,
            flex_factor,
            is_flexible,
        }
    }

    #[test]
    fn range_claim_push_and_reset_round_trip() {
        let mut lane = GridTrackLane::default();
        let start = lane.begin_range();
        assert_eq!(start, 0);

        assert_eq!(lane.push(entry(100.0, 100.0, 0.0, false)), 0);
        assert_eq!(lane.push(entry(0.0, f32::INFINITY, 1.0, true)), 1);

        let range = lane.finish_range(start);
        assert_eq!((range.start, range.len), (0, 2));

        let slice = lane.slice(range);
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].base_px, 100.0);
        assert!(!slice[0].is_flexible);
        assert_eq!(slice[1].flex_factor, 1.0);
        assert!(slice[1].is_flexible);

        let capacity_before = lane.capacity();
        lane.reset();
        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");
    }

    #[test]
    fn sibling_axes_claim_disjoint_ranges() {
        let mut lane = GridTrackLane::default();

        // Column range: three entries.
        let col_start = lane.begin_range();
        for base in [0.0f32, 0.0, 0.0] {
            lane.push(entry(base, f32::INFINITY, 1.0, true));
        }
        let col_range = lane.finish_range(col_start);

        // Row range: two entries.
        let row_start = lane.begin_range();
        assert_eq!(row_start, 3);
        lane.push(entry(40.0, 40.0, 0.0, false));
        lane.push(entry(40.0, 40.0, 0.0, false));
        let row_range = lane.finish_range(row_start);

        assert_eq!((col_range.start, col_range.len), (0, 3));
        assert_eq!((row_range.start, row_range.len), (3, 2));

        // In-place mutation is scoped to the row range.
        for entry in lane.slice_mut(row_range).iter_mut() {
            entry.base_px = 60.0;
        }
        assert_eq!(lane.slice(col_range)[0].base_px, 0.0);
        assert_eq!(lane.get(3).base_px, 60.0);
        assert_eq!(lane.get(4).base_px, 60.0);
    }
}
