#![allow(dead_code)]

//! Flex order lane — the order-sorted child sequence for one container's solve.
//!
//! This file owns `FlexOrderLane` and its entry type `FlexOrderLaneEntry`.
//! Its purpose is to hold the order-sorted child sequence a flex container's
//! solve walks during line building, without owning the order property's
//! source (that stays on the node's computed flex group) or the solve that
//! consumes the lane (that is flex step 4's).
//!
//! Invariants and rules:
//!
//! - append-only per update: a container claims a range with `begin_range`,
//!   pushes its children in DOM order, then closes the range with
//!   `finish_range`; a closed range is never mutated again within that update
//! - insertion is range-scoped: `push_in_order` sorts only within the open
//!   range, so sibling containers' ranges stay disjoint even when their order
//!   values would interleave under a global sort
//! - insertion is stable: equal orders keep DOM insertion order (do not
//!   re-sort equal keys); the sort key is `(order, dom insertion sequence)`
//! - ranges are invalidated by `reset()`; ranges never span two updates
//! - update-scoped: `reset()` clears entries (keeping capacity at the
//!   high-water mark) and re-arms the open-range cursor
//!
//! Named bound: insertion is adaptive. Already-sorted input (untouched order
//! values) appends at the range tail in O(n); scrambled input is stable
//! insertion sort, O(n²) worst case. The merge path is the designed,
//! evidence-gated fallback if a pathological fan-out ever shows up in the
//! perf window — never speculative.
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the lane
//! is written and read by the flex fold and solve in step 4 (T4), and read by
//! paint in step 2. Until those consumers land, its API is exercised only by
//! this file's unit tests.

use crate::types::DomNodeSlotKey;
use crate::types::LaneRange;

/// One flex item's order-lane slot: the DOM-order back-reference into the item
/// lane (`dom_index`), the child key, and its resolved `order`.
///
/// `dom_index` is the item's absolute index in the item lane (the DOM
/// sequence), so the solve reads the item's assembled view via `dom_index`
/// without a key lookup. The sort key remains `(order, dom insertion
/// sequence)`: children are pushed in DOM order, so equal orders stay stable
/// and the `dom_index` tiebreaker is implicit in insertion order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexOrderLaneEntry {
    pub(crate) dom_index: u32,
    pub(crate) child_key: DomNodeSlotKey,
    pub(crate) order: i32,
}

/// Vec-backed storage for the order-sorted child sequence, shared across a
/// document and sliced per container via `{start, len}` ranges.
#[derive(Debug, Default)]
pub(crate) struct FlexOrderLane {
    entries: Vec<FlexOrderLaneEntry>,
    /// Stack of open-range start cursors. `push_in_order` sorts within
    /// `[top, entries.len())` so nested and sibling containers stay disjoint;
    /// `finish_range` pops the matching cursor so a nested measurement returns
    /// the enclosing container's open range to its pre-nested state.
    range_starts: Vec<usize>,
}

impl FlexOrderLane {
    /// Opens a range at the current lane end and returns its start index.
    ///
    /// This pushes the open-range cursor (`&mut self`), which `push_in_order`
    /// needs so insertion stays scoped to the claiming container. A nested
    /// container's `begin_range`/`finish_range` pair restores the enclosing
    /// container's cursor on `finish_range`.
    pub(crate) fn begin_range(&mut self) -> u32 {
        let start = self.entries.len();
        self.range_starts.push(start);
        u32::try_from(start).expect("order lane length should fit into u32")
    }

    /// Inserts `entry` sorted by `(order, dom insertion sequence)` within the
    /// open range and returns the entry's assigned absolute lane index.
    ///
    /// Callers push children in DOM order, so equal orders stay stable and an
    /// untouched order sequence stays non-decreasing: every push then takes
    /// the append fast path (zero scan comparisons), the adaptive O(n) case.
    /// The returned index is where the entry landed, which the fold
    /// back-fills into the item lane's `order_index` cross-reference.
    pub(crate) fn push_in_order(&mut self, entry: FlexOrderLaneEntry) -> u32 {
        let range_start = *self.range_starts.last().unwrap_or(&0);
        // Append fast path: the open range is empty, or its tail is already
        // in order relative to the new entry. The open range is always the
        // lane tail, so `entries.last()` is the open range's last entry.
        if self.entries.len() == range_start
            || self
                .entries
                .last()
                .is_some_and(|last| last.order <= entry.order)
        {
            let index = self.entries.len();
            self.entries.push(entry);
            return u32::try_from(index).expect("order lane length should fit into u32");
        }
        // Scrambled input within the open range: stable insertion sort from
        // the range tail. Shift strictly-greater orders down (equal orders
        // keep DOM insertion order).
        let mut index = self.entries.len();
        while index > range_start {
            let predecessor = self.entries[index - 1];
            if predecessor.order <= entry.order {
                break;
            }
            index -= 1;
        }
        self.entries.insert(index, entry);
        u32::try_from(index).expect("order lane length should fit into u32")
    }

    /// Closes the range opened by a matching `begin_range` call and returns
    /// the `{start, len}` descriptor for the entries pushed since, popping the
    /// open-range cursor so the enclosing container's cursor is restored.
    pub(crate) fn finish_range(&mut self, start: u32) -> LaneRange {
        let start_usize =
            usize::try_from(start).expect("order lane range start should fit into usize");
        assert!(
            start_usize <= self.entries.len(),
            "order lane range start must not exceed lane length"
        );
        let popped = self.range_starts.pop();
        assert_eq!(
            popped,
            Some(start_usize),
            "order lane finish_range should pop the matching begin_range cursor"
        );
        LaneRange {
            start,
            len: u32::try_from(self.entries.len() - start_usize)
                .expect("order lane range length should fit into u32"),
        }
    }

    /// Returns the entries claimed by `range`, in order-sorted sequence.
    pub(crate) fn slice(&self, range: LaneRange) -> &[FlexOrderLaneEntry] {
        let start =
            usize::try_from(range.start).expect("order lane range start should fit into usize");
        let len =
            usize::try_from(range.len).expect("order lane range length should fit into usize");
        &self.entries[start..start + len]
    }

    /// Truncates the lane back to `len` entries (the staged nested-measurement
    /// cleanup seam: a nested container's measurement discards its transient
    /// order entries after computing its intrinsic size, keeping the enclosing
    /// container's open range contiguous).
    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    /// Clears all entries for the next update, keeping the retained backing
    /// capacity at its high-water mark, and re-arms the open-range cursor.
    pub(crate) fn reset(&mut self) {
        self.entries.clear();
        self.range_starts.clear();
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

    /// Distinct slot keys from one fresh store (same-store inserts guarantee
    /// uniqueness, unlike one key per fresh store).
    fn slot_keys(count: usize) -> Vec<DomNodeSlotKey> {
        let mut keys = slotmap::SlotMap::with_key();
        (0..count).map(|_| keys.insert(())).collect()
    }

    fn entry(dom_index: u32, key: DomNodeSlotKey, order: i32) -> FlexOrderLaneEntry {
        FlexOrderLaneEntry {
            dom_index,
            child_key: key,
            order,
        }
    }

    #[test]
    fn empty_lane_has_empty_range() {
        let mut lane = FlexOrderLane::default();
        let start = lane.begin_range();
        let range = lane.finish_range(start);

        assert_eq!(range.start, 0);
        assert_eq!(range.len, 0);
        assert!(lane.slice(range).is_empty());
    }

    #[test]
    fn already_sorted_input_appends_at_range_tail_with_sequential_positions() {
        let mut lane = FlexOrderLane::default();
        let keys = slot_keys(4);
        let start = lane.begin_range();

        for (dom_index, (key, order)) in keys.iter().copied().zip([0i32, 1, 2, 3]).enumerate() {
            assert_eq!(
                lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order)),
                u32::try_from(dom_index).unwrap(),
                "already-sorted insert should append at the range tail (sequential positions)"
            );
        }

        let range = lane.finish_range(start);
        let slice = lane.slice(range);
        let orders: Vec<i32> = slice.iter().map(|entry| entry.order).collect();
        let slice_keys: Vec<DomNodeSlotKey> = slice.iter().map(|entry| entry.child_key).collect();
        let dom_indices: Vec<u32> = slice.iter().map(|entry| entry.dom_index).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
        assert_eq!(slice_keys, keys);
        assert_eq!(dom_indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn reverse_sorted_input_produces_ascending_permutation() {
        let mut lane = FlexOrderLane::default();
        let keys = slot_keys(4);
        let start = lane.begin_range();

        for (dom_index, (key, order)) in keys.iter().copied().zip([3i32, 2, 1, 0]).enumerate() {
            lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order));
        }

        let range = lane.finish_range(start);
        let slice = lane.slice(range);
        let orders: Vec<i32> = slice.iter().map(|entry| entry.order).collect();
        let slice_keys: Vec<DomNodeSlotKey> = slice.iter().map(|entry| entry.child_key).collect();
        let dom_indices: Vec<u32> = slice.iter().map(|entry| entry.dom_index).collect();

        // Ascending order key, and the key sequence is the input reversed.
        assert_eq!(orders, vec![0, 1, 2, 3]);
        let mut expected_keys = keys;
        expected_keys.reverse();
        assert_eq!(slice_keys, expected_keys);
        // The stored `dom_index` back-references follow the sorted order (the
        // final positions the fold reads back after the sort completes).
        assert_eq!(dom_indices, vec![3, 2, 1, 0]);
    }

    #[test]
    fn equal_orders_preserve_dom_insertion_order() {
        let mut lane = FlexOrderLane::default();
        let keys = slot_keys(4);
        let start = lane.begin_range();

        // Two ties at order 1 (keys[1] and keys[2]); keys[0] and keys[3] are
        // the extremes so the ties land in the middle of the sorted range.
        for (dom_index, (key, order)) in keys.iter().copied().zip([2i32, 1, 1, 0]).enumerate() {
            lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order));
        }

        let range = lane.finish_range(start);
        let slice = lane.slice(range);
        let orders: Vec<i32> = slice.iter().map(|entry| entry.order).collect();
        assert_eq!(orders, vec![0, 1, 1, 2]);
        // Stability: the order-1 tie keeps DOM order (keys[1] before keys[2]).
        assert_eq!(slice[1].child_key, keys[1]);
        assert_eq!(slice[2].child_key, keys[2]);
    }

    #[test]
    fn sibling_container_ranges_are_disjoint() {
        let mut lane = FlexOrderLane::default();
        let keys = slot_keys(6);

        // Container A and container B use identical order values; a global
        // sort would interleave them, so this pins range-scoped insertion.
        let start_a = lane.begin_range();
        for (dom_index, (key, order)) in keys[0..3].iter().copied().zip([0i32, 1, 2]).enumerate() {
            lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order));
        }
        let range_a = lane.finish_range(start_a);

        let start_b = lane.begin_range();
        for (dom_index, (key, order)) in keys[3..6].iter().copied().zip([0i32, 1, 2]).enumerate() {
            lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order));
        }
        let range_b = lane.finish_range(start_b);

        assert_eq!((range_a.start, range_a.len), (0, 3));
        assert_eq!((range_b.start, range_b.len), (3, 3));

        let keys_a: Vec<DomNodeSlotKey> = lane
            .slice(range_a)
            .iter()
            .map(|entry| entry.child_key)
            .collect();
        let keys_b: Vec<DomNodeSlotKey> = lane
            .slice(range_b)
            .iter()
            .map(|entry| entry.child_key)
            .collect();

        for key in keys_a {
            assert!(
                keys[0..3].contains(&key),
                "container A range leaks a foreign key"
            );
            assert!(
                !keys[3..6].contains(&key),
                "container A range contains a B key"
            );
        }
        for key in keys_b {
            assert!(
                keys[3..6].contains(&key),
                "container B range leaks a foreign key"
            );
            assert!(
                !keys[0..3].contains(&key),
                "container B range contains an A key"
            );
        }
    }

    #[test]
    fn reset_keeps_capacity_and_zeroes_length() {
        let mut lane = FlexOrderLane::default();
        let keys = slot_keys(5);
        let start = lane.begin_range();
        for (dom_index, (key, order)) in keys.iter().copied().zip([4i32, 3, 2, 1, 0]).enumerate() {
            lane.push_in_order(entry(u32::try_from(dom_index).unwrap(), key, order));
        }
        let _ = lane.finish_range(start);

        let capacity_before = lane.capacity();
        assert!(capacity_before >= 5);
        assert_eq!(lane.len(), 5);

        lane.reset();

        assert_eq!(lane.len(), 0);
        assert_eq!(lane.capacity(), capacity_before, "reset must keep capacity");

        // A fresh range after reset starts at zero and reads empty.
        let new_start = lane.begin_range();
        assert_eq!(new_start, 0);
        let new_range = lane.finish_range(new_start);
        assert!(lane.slice(new_range).is_empty());
    }
}
