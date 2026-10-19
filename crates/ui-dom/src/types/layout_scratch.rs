#![allow(dead_code)]

//! Per-document, per-update layout scratch storage for `ui-dom`.
//!
//! This file owns `LaneRange` (the shared `{start, len}` slice descriptor)
//! and `LayoutScratch` (the RootDom-owned owner of the four flex lanes'
//! storage). Its purpose is to give each document one retained scratch block
//! whose lanes are cleared at the start of each update and whose backing
//! capacity is kept at the high-water mark, so the flex solve reaches steady
//! state with zero allocation after warmup.
//!
//! Invariants and rules:
//!
//! - RootDom-owned: per-document, per-update state — NOT the shared
//!   `RootDomContext` (the `FLEX_STEP1_EXECUTION.md` §2 scratch-ownership
//!   amendment; the context is shared across documents and coupling their
//!   scratch lifetimes buys nothing)
//! - update-scoped: `reset_for_update` clears all four lanes' entries at the
//!   start of each update's layout phase; ranges never span two updates
//! - high-water-mark capacity retention: `reset` calls `Vec::clear`, which
//!   keeps capacity, so steady state is zero allocation (the `perf_allocation`
//!   window is the judge)
//!
//! The `#![allow(dead_code)]` is temporary infrastructure allowance: the
//! lanes are written and read by the flex fold and solve in step 4 (T4).
//! Until those consumers land, the capacity accessors are read only by this
//! file's unit tests; they are the designed high-water observability surface
//! the step-6 scenario pin reads (`FLEX_STEP1_EXECUTION.md` §10 R4).

use crate::domains::layout::{FlexItemLane, FlexLineLane, FlexOrderLane, FlexResolutionLane};

#[cfg(test)]
use crate::domains::layout::{
    FlexItemSource, FlexItemView, FlexLineEntry, FlexOrderLaneEntry, FlexResolutionEntry,
};
#[cfg(test)]
use crate::types::DomNodeSlotKey;

/// A `{start, len}` slice descriptor into a lane's backing storage.
///
/// `start` and `len` are both relative to the lane, not to a container: a
/// container claims a range with `begin_range`/`finish_range` and passes the
/// resulting `LaneRange` back to `slice`/`slice_mut`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LaneRange {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

/// RootDom-owned owner of the four flex lanes' storage.
#[derive(Debug, Default)]
pub(crate) struct LayoutScratch {
    pub(crate) items: FlexItemLane,
    pub(crate) order: FlexOrderLane,
    pub(crate) lines: FlexLineLane,
    pub(crate) resolution: FlexResolutionLane,
}

impl LayoutScratch {
    /// Clears all four lanes for the next update, keeping each lane's backing
    /// capacity at its high-water mark.
    pub(crate) fn reset_for_update(&mut self) {
        self.items.reset();
        self.order.reset();
        self.lines.reset();
        self.resolution.reset();
    }

    /// The item lane's retained backing capacity (high-water mark).
    pub(crate) fn items_capacity(&self) -> usize {
        self.items.capacity()
    }

    /// The order lane's retained backing capacity (high-water mark).
    pub(crate) fn order_capacity(&self) -> usize {
        self.order.capacity()
    }

    /// The line lane's retained backing capacity (high-water mark).
    pub(crate) fn lines_capacity(&self) -> usize {
        self.lines.capacity()
    }

    /// The resolution lane's retained backing capacity (high-water mark).
    pub(crate) fn resolution_capacity(&self) -> usize {
        self.resolution.capacity()
    }

    /// The item lane's current entry count.
    pub(crate) fn items_len(&self) -> usize {
        self.items.len()
    }

    /// The order lane's current entry count.
    pub(crate) fn order_len(&self) -> usize {
        self.order.len()
    }

    /// The line lane's current entry count.
    pub(crate) fn lines_len(&self) -> usize {
        self.lines.len()
    }

    /// The resolution lane's current entry count.
    pub(crate) fn resolution_len(&self) -> usize {
        self.resolution.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_keys(count: usize) -> Vec<DomNodeSlotKey> {
        let mut keys = slotmap::SlotMap::with_key();
        (0..count).map(|_| keys.insert(())).collect()
    }

    fn item_view(source: FlexItemSource) -> FlexItemView {
        FlexItemView {
            source,
            order: 0,
            align_self: None,
            flex_grow_factor: 0.0,
            flex_shrink_factor: 0.0,
            flex_base_size_px: 0.0,
            hypothetical_main_size_px: 0.0,
            automatic_min_main_size_px: 0.0,
            min_main_size_px: None,
            max_main_size_px: None,
            main_axis_border_padding_px: 0.0,
            outer_hypothetical_main_size_px: 0.0,
            measured_cross_size_input_px: 0.0,
            margin_main_start_px: 0.0,
            margin_main_end_px: 0.0,
            margin_cross_start_px: 0.0,
            margin_cross_end_px: 0.0,
        }
    }

    #[test]
    fn reset_for_update_zeroes_lengths_and_keeps_capacities() {
        let mut scratch = LayoutScratch::default();
        let keys = slot_keys(4);

        let items_start = scratch.items.begin_range();
        for key in keys.iter().copied() {
            scratch
                .items
                .push(item_view(FlexItemSource::BlockItem(key)));
        }
        let _ = scratch.items.finish_range(items_start);

        let order_start = scratch.order.begin_range();
        for (dom_index, (key, order)) in keys.iter().copied().zip([3i32, 2, 1, 0]).enumerate() {
            scratch.order.push_in_order(FlexOrderLaneEntry {
                dom_index: u32::try_from(dom_index).unwrap(),
                child_key: key,
                order,
            });
        }
        let _ = scratch.order.finish_range(order_start);

        let lines_start = scratch.lines.begin_range();
        scratch.lines.push(FlexLineEntry {
            first_order_index: 0,
            item_count: 4,
            outer_hypothetical_main_size_px: 240.0,
            cross_size_px: 40.0,
            cross_offset_px: 0.0,
            resolved_cross_size_px: 0.0,
        });
        let _ = scratch.lines.finish_range(lines_start);

        let resolution_start = scratch.resolution.begin_range();
        for size in [100.0f32, 80.0, 70.0, 60.0] {
            scratch.resolution.push(FlexResolutionEntry {
                resolved_main_size_px: size,
                is_frozen: false,
            });
        }
        let _ = scratch.resolution.finish_range(resolution_start);

        // High-water capacities captured before the reset.
        let items_capacity = scratch.items_capacity();
        let order_capacity = scratch.order_capacity();
        let lines_capacity = scratch.lines_capacity();
        let resolution_capacity = scratch.resolution_capacity();
        assert!(items_capacity >= 4);
        assert!(order_capacity >= 4);
        assert!(lines_capacity >= 1);
        assert!(resolution_capacity >= 4);

        assert_eq!(scratch.items_len(), 4);
        assert_eq!(scratch.order_len(), 4);
        assert_eq!(scratch.lines_len(), 1);
        assert_eq!(scratch.resolution_len(), 4);

        scratch.reset_for_update();

        assert_eq!(scratch.items_len(), 0);
        assert_eq!(scratch.order_len(), 0);
        assert_eq!(scratch.lines_len(), 0);
        assert_eq!(scratch.resolution_len(), 0);
        assert_eq!(
            scratch.items_capacity(),
            items_capacity,
            "items capacity must persist"
        );
        assert_eq!(
            scratch.order_capacity(),
            order_capacity,
            "order capacity must persist"
        );
        assert_eq!(
            scratch.lines_capacity(),
            lines_capacity,
            "lines capacity must persist"
        );
        assert_eq!(
            scratch.resolution_capacity(),
            resolution_capacity,
            "resolution capacity must persist"
        );
    }
}
