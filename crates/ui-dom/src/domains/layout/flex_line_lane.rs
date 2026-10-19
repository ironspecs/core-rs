#![allow(dead_code)]

//! Flex line lane — one summary entry per built flex line, range-indexed into
//! the order lane.
//!
//! This file owns `FlexLineLane` and its entry type `FlexLineEntry`. Its
//! purpose is to hold the per-line summary the line builder produces for one
//! container's solve (as ranges into the order lane), without owning the
//! line-building algorithm (flex step 4's) or the placement pass that reads
//! these summaries back.
//!
//! Invariants and rules:
//!
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes one entry per built line, then closes the range with
//!   `finish_range`; entries are never reordered (except the wrap-reverse
//!   reversal, which flips a closed range in place before placement)
//! - an entry's `{first_order_index, item_count}` addresses a contiguous slice
//!   of the order lane's matching container range
//! - `cross_offset_px` and `resolved_cross_size_px` are the line-packing
//!   results the resolve phase writes in place after line building (replacing
//!   the former side-carried `line_cross_offsets_px` vector and the solve's
//!   `line_cross_sizes_px` field); `cross_size_px` stays the measured line
//!   cross size for the wrapped-placement invariant
//! - `placed_cross_size_px` is the per-line running maximum of placed item
//!   cross sizes, written in place by the placement pass via `slice_mut`
//!   (replacing the former per-placement `vec![0.0; line_count]` transient
//!   accumulator); the wrapped-placement invariant reads it back against
//!   `cross_size_px`. It is re-zeroed on every push, so it never carries
//!   state across a solve.
//! - ranges are invalidated by `reset()`; ranges never span two updates
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the lane
//! is written and read by the flex solve in step 4 (T4). Until that consumer
//! lands, its API is exercised only by this file's unit tests.

use crate::types::LaneRange;

/// One flex line's summary: a range into the order lane, the line's outer
/// hypothetical main size, its measured and resolved cross sizes, and its
/// resolved cross offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexLineEntry {
    /// Index into the order lane where this line's items begin.
    pub(crate) first_order_index: u32,
    /// Number of order-lane entries this line consumes.
    pub(crate) item_count: u32,
    pub(crate) outer_hypothetical_main_size_px: f32,
    /// The measured line cross size (largest item outer cross size), kept
    /// untouched so placement can verify wrapped lines preserved measurement.
    pub(crate) cross_size_px: f32,
    /// The resolved line cross offset (packing), written by the resolve phase.
    pub(crate) cross_offset_px: f32,
    /// The resolved line cross size (single-line definite cross size, or the
    /// measured size plus `align-content: stretch`), written by the resolve
    /// phase.
    pub(crate) resolved_cross_size_px: f32,
    /// The per-line running maximum of placed item cross sizes, written in
    /// place by the placement pass (replacing the former per-placement
    /// `vec![0.0; line_count]` accumulator); re-zeroed on every push.
    pub(crate) placed_cross_size_px: f32,
}

/// Vec-backed storage for per-line summaries, sliced per container via
/// `{start, len}` ranges into this lane.
#[derive(Debug, Default)]
pub(crate) struct FlexLineLane {
    entries: Vec<FlexLineEntry>,
}

impl FlexLineLane {
    /// Opens a range at the current lane end and returns its start index.
    pub(crate) fn begin_range(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("line lane length should fit into u32")
    }

    /// Appends one line summary in build order.
    pub(crate) fn push(&mut self, entry: FlexLineEntry) {
        self.entries.push(entry);
    }

    /// Closes the range opened by a matching `begin_range` call and returns
    /// the `{start, len}` descriptor for the entries pushed since.
    pub(crate) fn finish_range(&self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("line lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "line lane range start must not exceed lane length"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("line lane range length should fit into u32"),
        }
    }

    /// Returns the line summaries claimed by `range`, in build order.
    pub(crate) fn slice(&self, range: LaneRange) -> &[FlexLineEntry] {
        let start =
            usize::try_from(range.start).expect("line lane range start should fit into usize");
        let len = usize::try_from(range.len).expect("line lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Returns the line summary at the absolute `index` (the item lane's
    /// `line_index` back-reference target).
    pub(crate) fn get(&self, index: u32) -> &FlexLineEntry {
        let index = usize::try_from(index).expect("line lane index should fit into usize");
        &self.entries[index]
    }

    /// Returns the line summaries claimed by `range` for in-place mutation (the
    /// resolve phase writes `cross_offset_px` and `resolved_cross_size_px`
    /// here).
    pub(crate) fn slice_mut(&mut self, range: LaneRange) -> &mut [FlexLineEntry] {
        let start =
            usize::try_from(range.start).expect("line lane range start should fit into usize");
        let len = usize::try_from(range.len).expect("line lane range length should fit into usize");
        &mut self.entries[start..start + len]
    }

    /// Reverses the entries claimed by `range` in place (the wrap-reverse line
    /// reversal, applied after line building and before placement).
    pub(crate) fn reverse_range(&mut self, range: LaneRange) {
        let start =
            usize::try_from(range.start).expect("line lane range start should fit into usize");
        let len = usize::try_from(range.len).expect("line lane range length should fit into usize");
        self.entries[start..start + len].reverse();
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn line(first_order_index: u32, item_count: u32) -> FlexLineEntry {
        FlexLineEntry {
            first_order_index,
            item_count,
            outer_hypothetical_main_size_px: 0.0,
            cross_size_px: 0.0,
            cross_offset_px: 0.0,
            resolved_cross_size_px: 0.0,
            placed_cross_size_px: 0.0,
        }
    }

    #[test]
    fn range_claim_slice_and_reset_round_trip() {
        let mut lane = FlexLineLane::default();
        let start = lane.begin_range();
        assert_eq!(start, 0);

        lane.push(line(0, 2));
        lane.push(line(2, 3));
        let range = lane.finish_range(start);
        assert_eq!((range.start, range.len), (0, 2));

        let slice = lane.slice(range);
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].first_order_index, 0);
        assert_eq!(slice[0].item_count, 2);
        assert_eq!(slice[1].first_order_index, 2);
        assert_eq!(slice[1].item_count, 3);

        // A second container claims a disjoint range after the first.
        let start_b = lane.begin_range();
        assert_eq!(start_b, 2);
        lane.push(line(5, 1));
        let range_b = lane.finish_range(start_b);
        assert_eq!((range_b.start, range_b.len), (2, 1));
        assert_eq!(lane.slice(range_b)[0].first_order_index, 5);
        // The first range still reads only its own entries.
        assert_eq!(lane.slice(range).len(), 2);

        let capacity_before = lane.capacity();
        lane.reset();
        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");
        assert!(lane.slice(lane.finish_range(lane.begin_range())).is_empty());
    }

    #[test]
    fn resolve_writes_packing_fields_in_place() {
        let mut lane = FlexLineLane::default();
        let start = lane.begin_range();
        lane.push(line(0, 2));
        lane.push(line(2, 3));
        let range = lane.finish_range(start);

        for entry in lane.slice_mut(range).iter_mut() {
            entry.cross_offset_px = 15.0;
            entry.resolved_cross_size_px = 40.0;
        }

        let slice = lane.slice(range);
        assert_eq!(slice[0].cross_offset_px, 15.0);
        assert_eq!(slice[0].resolved_cross_size_px, 40.0);
        assert_eq!(slice[1].cross_offset_px, 15.0);
        assert_eq!(slice[1].resolved_cross_size_px, 40.0);
        // The measured cross size is untouched by the packing write.
        assert_eq!(slice[0].cross_size_px, 0.0);
    }

    #[test]
    fn placement_writes_placed_cross_size_as_running_max_in_place() {
        let mut lane = FlexLineLane::default();
        let start = lane.begin_range();
        lane.push(line(0, 2));
        lane.push(line(2, 3));
        let range = lane.finish_range(start);

        // The placement pass writes, per line, the running maximum of placed
        // item cross sizes through `slice_mut` (indexed by line offset).
        for (line_offset, placed_cross_size_px) in
            [10.0f32, 8.0, 12.0, 14.0, 11.0].iter().enumerate()
        {
            let slot = line_offset % usize::try_from(range.len).expect("line count fits usize");
            let entry = &mut lane.slice_mut(range)[slot];
            entry.placed_cross_size_px = entry.placed_cross_size_px.max(*placed_cross_size_px);
        }

        let slice = lane.slice(range);
        assert_eq!(slice[0].placed_cross_size_px, 12.0);
        assert_eq!(slice[1].placed_cross_size_px, 14.0);
        // The placement write is independent of the resolve-phase packing
        // fields and the measured cross size.
        assert_eq!(slice[0].cross_size_px, 0.0);
        assert_eq!(slice[0].resolved_cross_size_px, 0.0);
    }

    #[test]
    fn reverse_range_reverses_only_the_claimed_entries() {
        let mut lane = FlexLineLane::default();

        let start_a = lane.begin_range();
        lane.push(line(0, 1));
        lane.push(line(1, 1));
        let range_a = lane.finish_range(start_a);

        // A sibling container's range after A's; reversal must not touch it.
        let start_b = lane.begin_range();
        lane.push(line(2, 2));
        let range_b = lane.finish_range(start_b);

        lane.reverse_range(range_a);

        let slice_a = lane.slice(range_a);
        assert_eq!(slice_a[0].first_order_index, 1);
        assert_eq!(slice_a[1].first_order_index, 0);

        let slice_b = lane.slice(range_b);
        assert_eq!(slice_b[0].first_order_index, 2);
    }
}
