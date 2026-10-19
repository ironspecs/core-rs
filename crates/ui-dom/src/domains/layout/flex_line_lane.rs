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
//!   `finish_range`; entries are never reordered
//! - an entry's `{first_order_index, item_count}` addresses a contiguous slice
//!   of the order lane's matching container range
//! - ranges are invalidated by `reset()`; ranges never span two updates
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the lane
//! is written and read by the flex solve in step 4 (T4). Until that consumer
//! lands, its API is exercised only by this file's unit tests.

use crate::types::LaneRange;

/// One flex line's summary: a range into the order lane plus the line's
/// outer hypothetical main size and resolved cross size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexLineEntry {
    /// Index into the order lane where this line's items begin.
    pub(crate) first_order_index: u32,
    /// Number of order-lane entries this line consumes.
    pub(crate) item_count: u32,
    pub(crate) outer_hypothetical_main_size_px: f32,
    pub(crate) cross_size_px: f32,
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

    /// Clears all entries for the next update, keeping the retained backing
    /// capacity at its high-water mark.
    pub(crate) fn reset(&mut self) {
        self.entries.clear();
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
mod tests {
    use super::*;

    fn line(first_order_index: u32, item_count: u32) -> FlexLineEntry {
        FlexLineEntry {
            first_order_index,
            item_count,
            outer_hypothetical_main_size_px: 0.0,
            cross_size_px: 0.0,
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
}
