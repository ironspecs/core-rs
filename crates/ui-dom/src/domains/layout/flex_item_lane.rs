#![allow(dead_code)]

//! Flex item lane — the DOM-ordered item-view lane placement iterates.
//!
//! This file owns `FlexItemLane` and its entry type `FlexItemLaneEntry`. Its
//! purpose is to hold one assembled `FlexItemView` per flex child in DOM order,
//! so placement walks the container's item-lane range directly with DOM order
//! preserved (the paint-order divergence's current behavior) instead of
//! re-joining through a key map.
//!
//! Invariants and rules:
//!
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes children in DOM order, then closes the range with `finish_range`; a
//!   closed range is never mutated again within that update
//! - the `order_index` back-reference addresses the order lane's assigned
//!   position for this child; the fold writes it once when the order-lane
//!   insertion returns its position
//! - `main_offset_px` and `line_index` are the per-item packing results the
//!   resolve phase writes after §9.7 (via `get_mut`), keyed by `dom_index`;
//!   they are placement's O(1) read path in DOM order
//! - ranges are invalidated by `reset()`; ranges never span two updates
//! - update-scoped: `reset()` clears entries, keeping the retained backing
//!   capacity at its high-water mark so steady state is zero-allocation
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the lane
//! is written and read by the flex fold and solve in step 4 (T4). Until those
//! consumers land, its API is exercised only by this file's unit tests.

use crate::domains::layout::flex::FlexItemView;
use crate::types::LaneRange;

#[cfg(test)]
use crate::domains::layout::flex_item_source::FlexItemSource;
#[cfg(test)]
use crate::types::DomNodeSlotKey;

/// One flex item's item-lane slot: the assembled view plus its placement
/// cross-references and packing results.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexItemLaneEntry {
    /// The assembled item view (T4a's `FlexItemView`), read by the solve via
    /// the order lane's `dom_index` and by placement directly.
    pub(crate) view: FlexItemView,
    /// The child's assigned position in the order lane (the order-modified
    /// sequence). Placement reads the child's resolution result from the
    /// resolution lane at this index.
    pub(crate) order_index: u32,
    /// The child's main-axis packing offset within its line, written by the
    /// resolve phase's forward pass over the order lane.
    pub(crate) main_offset_px: f32,
    /// The child's absolute line-lane index, so placement reads its line's
    /// cross offset and resolved cross size without a key lookup.
    pub(crate) line_index: u32,
}

/// Vec-backed storage for DOM-ordered item views, sliced per container via
/// `{start, len}` ranges.
#[derive(Debug, Default)]
pub(crate) struct FlexItemLane {
    entries: Vec<FlexItemLaneEntry>,
}

impl FlexItemLane {
    /// Opens a range at the current lane end and returns its start index.
    pub(crate) fn begin_range(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("item lane length should fit into u32")
    }

    /// Appends one assembled view in DOM order and returns its absolute lane
    /// index (the `dom_index` the order-lane entry stores). The cross-reference
    /// and packing fields are zeroed; the fold and resolve phases fill them.
    pub(crate) fn push(&mut self, view: FlexItemView) -> u32 {
        let dom_index = self.entries.len();
        self.entries.push(FlexItemLaneEntry {
            view,
            order_index: 0,
            main_offset_px: 0.0,
            line_index: 0,
        });
        u32::try_from(dom_index).expect("item lane length should fit into u32")
    }

    /// Closes the range opened by a matching `begin_range` call and returns
    /// the `{start, len}` descriptor for the entries pushed since.
    pub(crate) fn finish_range(&self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("item lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "item lane range start must not exceed lane length"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("item lane range length should fit into u32"),
        }
    }

    /// Returns the entries claimed by `range`, in DOM order.
    pub(crate) fn slice(&self, range: LaneRange) -> &[FlexItemLaneEntry] {
        let start =
            usize::try_from(range.start).expect("item lane range start should fit into usize");
        let len = usize::try_from(range.len).expect("item lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Returns the entries claimed by `range` for in-place mutation (the
    /// resolve phase writes `main_offset_px`/`line_index` here).
    pub(crate) fn slice_mut(&mut self, range: LaneRange) -> &mut [FlexItemLaneEntry] {
        let start =
            usize::try_from(range.start).expect("item lane range start should fit into usize");
        let len = usize::try_from(range.len).expect("item lane range length should fit into usize");
        &mut self.entries[start..start + len]
    }

    /// Returns the entry at the absolute `dom_index` (the order-lane entry's
    /// back-reference target).
    pub(crate) fn get(&self, dom_index: u32) -> &FlexItemLaneEntry {
        let index = usize::try_from(dom_index).expect("item lane dom_index should fit into usize");
        &self.entries[index]
    }

    /// Returns the entry at the absolute `dom_index` for in-place mutation.
    pub(crate) fn get_mut(&mut self, dom_index: u32) -> &mut FlexItemLaneEntry {
        let index = usize::try_from(dom_index).expect("item lane dom_index should fit into usize");
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn slot_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn view(order: i32) -> FlexItemView {
        FlexItemView {
            source: FlexItemSource::BlockItem(slot_key()),
            order,
            align_self: None,
            flex_grow_factor: 0.0,
            flex_shrink_factor: 0.0,
            flex_base_size_px: 10.0,
            hypothetical_main_size_px: 10.0,
            automatic_min_main_size_px: 0.0,
            min_main_size_px: None,
            max_main_size_px: None,
            main_axis_border_padding_px: 0.0,
            outer_hypothetical_main_size_px: 10.0,
            measured_cross_size_input_px: 10.0,
            replaced_ratio: false,
            main_influences_cross: false,
            resolved_cross_size_px: 10.0,
            h_min_content_px: 10.0,
            h_max_content_px: 10.0,
            margin_main_start_px: 0.0,
            margin_main_end_px: 0.0,
            margin_cross_start_px: 0.0,
            margin_cross_end_px: 0.0,
        }
    }

    #[test]
    fn range_claim_push_and_reset_round_trip() {
        let mut lane = FlexItemLane::default();
        let start = lane.begin_range();
        assert_eq!(start, 0);

        let first = lane.push(view(0));
        let second = lane.push(view(1));
        assert_eq!(first, 0);
        assert_eq!(second, 1);

        let range = lane.finish_range(start);
        assert_eq!((range.start, range.len), (0, 2));

        let slice = lane.slice(range);
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].view.order, 0);
        assert_eq!(slice[1].view.order, 1);

        // A second container claims a disjoint range after the first.
        let start_b = lane.begin_range();
        assert_eq!(start_b, 2);
        lane.push(view(2));
        let range_b = lane.finish_range(start_b);
        assert_eq!((range_b.start, range_b.len), (2, 1));
        assert_eq!(lane.slice(range).len(), 2);

        let capacity_before = lane.capacity();
        lane.reset();
        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");
        assert!(lane.slice(lane.finish_range(lane.begin_range())).is_empty());
    }

    #[test]
    fn dom_index_and_order_index_backreference_round_trip() {
        let mut lane = FlexItemLane::default();
        let start = lane.begin_range();

        // Push two children in DOM order; back-fill the order-lane positions
        // exactly as the fold does after the order-lane insertion returns.
        let first = lane.push(view(0));
        let second = lane.push(view(1));
        lane.get_mut(first).order_index = 1; // first child lands at order position 1
        lane.get_mut(second).order_index = 0; // second child lands at order position 0

        let range = lane.finish_range(start);
        let slice = lane.slice(range);

        assert_eq!(slice[0].order_index, 1);
        assert_eq!(slice[1].order_index, 0);
        assert_eq!(lane.get(second).order_index, 0);
        assert_eq!(lane.get(first).order_index, 1);
    }

    #[test]
    fn resolve_writes_packing_fields_in_place() {
        let mut lane = FlexItemLane::default();
        let start = lane.begin_range();
        let first = lane.push(view(0));
        let second = lane.push(view(1));
        let range = lane.finish_range(start);

        for entry in lane.slice_mut(range).iter_mut() {
            entry.main_offset_px = 12.0;
            entry.line_index = 3;
        }

        assert_eq!(lane.get(first).main_offset_px, 12.0);
        assert_eq!(lane.get(first).line_index, 3);
        assert_eq!(lane.get(second).main_offset_px, 12.0);
        assert_eq!(lane.get(second).line_index, 3);
    }

    #[test]
    fn dom_index_is_the_dom_sequence() {
        let mut lane = FlexItemLane::default();
        let start = lane.begin_range();
        let indices: Vec<u32> = (0..5).map(|i| lane.push(view(i))).collect();
        let range = lane.finish_range(start);

        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
        let slice = lane.slice(range);
        for (index, entry) in slice.iter().enumerate() {
            assert_eq!(entry.view.order, i32::try_from(index).unwrap());
        }
    }
}
