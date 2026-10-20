#![allow(dead_code)]

//! Flex resolution lane — per-item resolved main sizes, parallel to the order
//! lane.
//!
//! This file owns `FlexResolutionLane` and its entry type
//! `FlexResolutionEntry`. Its purpose is to hold the per-item main-axis
//! resolution the resolve loop mutates in place (flex step 4's §9.7 loop),
//! parallel-indexed with the order lane's container range, without owning the
//! distribution algorithm itself.
//!
//! Invariants and rules:
//!
//! - parallel indexing with the order lane: one container claims matching
//!   ranges in both lanes; `item_count` from the line lane bounds the slice
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes one entry per item in order-lane sequence, then closes the range
//!   with `finish_range`
//! - `slice_mut` is the in-place mutation seam the §9.7 resolve loop uses at
//!   flex step 4 (T4); `slice` is the read seam
//! - ranges are invalidated by `reset()`; ranges never span two updates
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation
//!
//! Step-5 shape note: the entry carries `resolved_main_size_px`, `is_frozen`,
//! and the transient `violation: ViolationClass` scratch the §9.7 clamp phase
//! writes and the one-class freeze selection reads. The addition is free
//! because the lane is transient — update-scoped, reconstructed every update —
//! so this shape is not a retained layout contract.
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the lane
//! is written and mutated by the flex resolve loop in step 4 (T4). Until that
//! consumer lands, its API is exercised only by this file's unit tests.

use crate::types::LaneRange;

/// The per-item violation class the §9.7 clamp phase records and the freeze
/// selection reads (flex arc step 5, D4): `None` when the clamped size equals
/// the distribution candidate, `Min` when the clamp RAISED the candidate
/// (clamped > candidate), `Max` when the clamp LOWERED it (clamped < candidate).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViolationClass {
    None,
    Min,
    Max,
}

/// One flex item's main-axis resolution: its resolved size, frozen flag, and
/// transient violation class (step-5 scratch, written in the clamp phase and
/// read at the one-class freeze selection; never a retained layout contract).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexResolutionEntry {
    pub(crate) resolved_main_size_px: f32,
    pub(crate) is_frozen: bool,
    pub(crate) violation: ViolationClass,
}

/// Vec-backed storage for per-item resolutions, sliced per container via
/// `{start, len}` ranges that parallel the order lane's ranges.
#[derive(Debug, Default)]
pub(crate) struct FlexResolutionLane {
    entries: Vec<FlexResolutionEntry>,
}

impl FlexResolutionLane {
    /// Opens a range at the current lane end and returns its start index.
    pub(crate) fn begin_range(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("resolution lane length should fit into u32")
    }

    /// Appends one resolution entry in order-lane sequence.
    pub(crate) fn push(&mut self, entry: FlexResolutionEntry) {
        self.entries.push(entry);
    }

    /// Closes the range opened by a matching `begin_range` call and returns
    /// the `{start, len}` descriptor for the entries pushed since.
    pub(crate) fn finish_range(&self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("resolution lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "resolution lane range start must not exceed lane length"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("resolution lane range length should fit into u32"),
        }
    }

    /// Returns the resolutions claimed by `range`, in order-lane sequence.
    pub(crate) fn slice(&self, range: LaneRange) -> &[FlexResolutionEntry] {
        let start = usize::try_from(range.start)
            .expect("resolution lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("resolution lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Returns the resolution entry at the absolute `index` (the item lane's
    /// `order_index` back-reference target, parallel to the order lane).
    pub(crate) fn get(&self, index: u32) -> &FlexResolutionEntry {
        let index = usize::try_from(index).expect("resolution lane index should fit into usize");
        &self.entries[index]
    }

    /// Returns the resolutions claimed by `range` for in-place mutation (the
    /// §9.7 resolve loop writes resolved sizes, the frozen flag, and the
    /// transient violation class here).
    pub(crate) fn slice_mut(&mut self, range: LaneRange) -> &mut [FlexResolutionEntry] {
        let start = usize::try_from(range.start)
            .expect("resolution lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("resolution lane range length should fit into usize");
        &mut self.entries[start..start + len]
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

    fn entry(resolved_main_size_px: f32) -> FlexResolutionEntry {
        FlexResolutionEntry {
            resolved_main_size_px,
            is_frozen: false,
            violation: ViolationClass::None,
        }
    }

    #[test]
    fn range_claim_slice_and_reset_round_trip() {
        let mut lane = FlexResolutionLane::default();
        let start = lane.begin_range();
        assert_eq!(start, 0);

        lane.push(entry(120.0));
        lane.push(entry(80.0));
        let range = lane.finish_range(start);
        assert_eq!((range.start, range.len), (0, 2));

        let slice = lane.slice(range);
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].resolved_main_size_px, 120.0);
        assert_eq!(slice[1].resolved_main_size_px, 80.0);

        let capacity_before = lane.capacity();
        lane.reset();
        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");
        assert!(lane.slice(lane.finish_range(lane.begin_range())).is_empty());
    }

    #[test]
    fn slice_mut_mutates_in_place_within_range() {
        let mut lane = FlexResolutionLane::default();

        let start_a = lane.begin_range();
        lane.push(entry(100.0));
        lane.push(entry(100.0));
        let range_a = lane.finish_range(start_a);

        // A sibling container's range sits after A's; slice_mut must not
        // touch it.
        let start_b = lane.begin_range();
        lane.push(entry(50.0));
        let range_b = lane.finish_range(start_b);

        for item in lane.slice_mut(range_a).iter_mut() {
            item.resolved_main_size_px *= 0.5;
            item.is_frozen = true;
        }

        let slice_a = lane.slice(range_a);
        assert_eq!(slice_a[0].resolved_main_size_px, 50.0);
        assert!(slice_a[0].is_frozen);
        assert_eq!(slice_a[1].resolved_main_size_px, 50.0);
        assert!(slice_a[1].is_frozen);

        // B's entry is untouched by A's in-place mutation.
        let slice_b = lane.slice(range_b);
        assert_eq!(slice_b[0].resolved_main_size_px, 50.0);
        assert!(!slice_b[0].is_frozen);
    }
}
