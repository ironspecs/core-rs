//! Root-owned metric vocabulary for `ui-dom`.
//!
//! This module defines the root-specific metric vocabulary for retained
//! `ui-dom` mutation profiling. Allocation metrics are semantic counters for
//! named mutating `RootDom` work sites, while the shared storage and history
//! mechanics live in `collections::metrics`.
//!
//! Intended usage:
//!
//! - `RootDom::increment_metric(name)` must be called only after a measured
//!   mutating operation changes retained `RootDom` state.
//! - Metrics must not be incremented from selector queries, hit-test reads,
//!   style-map reads, or any other read-only traversal.
//! - Metrics must not be incremented for no-op calls that leave `RootDom` state
//!   unchanged.
//! - `RootDom::count(name)` is the aggregate-counter read API.
//! - `RootDom::history()` is the retained increment-event read API.
//! - `RootDom::reset_metrics()` starts a new profiling window.
//!
//! Invariants and requirements:
//!
//! - Metric names must be enum variants, not strings or public struct fields.
//! - Metric increments must require mutable access to `RootDom`.
//! - Counted operations must mutate retained `RootDom` state.
//! - Root-local read APIs must remain read-only; do not change them to
//!   `&mut self` to support metrics.
//! - The metrics tally must remain an attachment on `RootDom`.
//! - Metrics must not be passed down through lower-level function parameters or
//!   returned through lower-level result types.
//! - The metrics tally must not use interior mutability such as `Cell`, `Arc`,
//!   or mutexes to bypass the root mutation boundary.
//! - The root-owned tally must perform no heap allocation after `RootDom`
//!   creation.
//! - Metric counters must be fixed arrays indexed by `RootDomMetricName`.
//! - History must use `ROOT_DOM_METRIC_HISTORY_CAPACITY`; once full, new
//!   increments overwrite the oldest retained entries.
//! - `history()` must return retained entries oldest-to-newest without
//!   allocating.
//! - Allocation metrics must count intentional mutating `ui-dom` work sites, not
//!   global allocator calls, bytes, or capacity-growth details.
//!
//! Example:
//!
//! ```rust
//! # use ui_dom::{PixelDensity, RootDom, RootDomContext, RootDomMetricName};
//! let mut root = RootDom::new(RootDomContext::new_cpu());
//! root.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
//! assert_eq!(
//!     root.count(RootDomMetricName::TextIntrinsicMeasurement),
//!     1
//! );
//! assert_eq!(
//!     root.history().next().map(|entry| entry.name),
//!     Some(RootDomMetricName::TextIntrinsicMeasurement)
//! );
//! root.reset_metrics();
//! assert_eq!(root.history().next(), None);
//! ```

use collections::metrics::{
    MetricHistoryEntry, MetricHistoryIter, MetricName, MetricSnapshot, Metrics,
};

/// Number of metric history entries retained by one `RootDom`.
pub const ROOT_DOM_METRIC_HISTORY_CAPACITY: usize = 128;

/// Names one `ui-dom` metric counter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RootDomMetricName {
    /// Number of completed hover pseudo-state set mutations.
    HoverPseudoStateSet,
    /// Number of completed hover pseudo-state clear mutations.
    HoverPseudoStateCleared,
    /// Number of started style phase executions for dirty layer roots.
    StyleUpdated,
    /// Number of started layout phase executions for dirty layer roots.
    LayoutUpdated,
    /// Number of started paint phase executions for dirty layer roots.
    PaintUpdated,
    /// Number of completed paint-plan builds for dirty layer roots.
    PaintPlanBuilt,
    /// Number of started retained layer presentation updates after layout.
    LayerPresented,
    /// Number of retained clip updates pushed into presented base materials.
    ClipUpdated,
    /// Number of scroll-only presented layer translations.
    LayerTranslated,
    /// Number of completed layer-root paint executions.
    LayerPainted,
    /// Number of retained rect ops published by completed layer paints.
    LayerPaintRectOp,
    /// Number of retained stroke-path ops published by completed layer paints.
    LayerPaintStrokePathOp,
    /// Number of retained text-run ops published by completed layer paints.
    LayerPaintTextRunOp,
    /// Number of retained SVG ops published by completed layer paints.
    LayerPaintSvgOp,
    /// Number of retained image ops published by completed layer paints.
    LayerPaintImageOp,
    /// Number of layer image allocations performed while spawning presented
    /// layer entities.
    LayerImageCreated,
    /// Number of times presented layer z order was renumbered after layer-tree
    /// creation or removal.
    RenumberedLayerZOrder,
    /// Number of text intrinsic measurements performed during node creation or
    /// explicit text-content mutation.
    TextIntrinsicMeasurement,
    /// Number of times one `Img` node reached a fully measured intrinsic state.
    ImgIntrinsicRecomputation,
    /// Number of times one `Img` node's intrinsic state was refreshed, whether
    /// that resulted in `NotMeasured` or a fully measured intrinsic record.
    ImgIntrinsicStateUpdate,
    /// Number of root-owned node slot insertions.
    NodeSlotAllocation,
    /// Number of text storage creations or replacements.
    TextStorageAllocation,
    /// Number of root-owned event-listener registration calls run.
    EventListenerRegistrationAllocation,
    /// Number of root-emitted event records appended after internal dispatch.
    EmittedEventAllocation,
    /// Number of click dispatches completed for default click behavior paths.
    ClickDispatched,
    /// Number of provisional activation state changes applied before click dispatch.
    ActivationPreApplied,
    /// Number of provisional activation state changes reverted after canceled click dispatch.
    ActivationCanceled,
    /// Number of committed activation paths completed after uncanceled click dispatch.
    ActivationCommitted,
    /// Number of node transition schedule insertions or replacements.
    TransitionScheduleAllocation,
    /// Number of transition event dispatch paths built.
    TransitionEventPathAllocation,
    /// Number of hover layer-chain collections accepted into root pointer state.
    HoverLayerChainAllocation,
    /// Number of stylesheet-owned removed-attribute lists built.
    StylesheetRemovedAttributeListAllocation,
    /// Number of stylesheet tracked-attribute sets created or refreshed.
    StylesheetTrackedAttributeSetAllocation,
    /// Number of fragment top-level key lists built during import.
    FragmentTopLevelKeyListAllocation,
    /// Number of fragment import validation stacks built.
    FragmentImportValidationStackAllocation,
    /// Number of removed subtree key lists built.
    RemovedSubtreeKeyListAllocation,
}

pub(crate) const ROOT_DOM_METRIC_NAMES: [RootDomMetricName; 36] = [
    RootDomMetricName::HoverPseudoStateSet,
    RootDomMetricName::HoverPseudoStateCleared,
    RootDomMetricName::StyleUpdated,
    RootDomMetricName::LayoutUpdated,
    RootDomMetricName::PaintUpdated,
    RootDomMetricName::PaintPlanBuilt,
    RootDomMetricName::LayerPresented,
    RootDomMetricName::ClipUpdated,
    RootDomMetricName::LayerTranslated,
    RootDomMetricName::LayerPainted,
    RootDomMetricName::LayerPaintRectOp,
    RootDomMetricName::LayerPaintStrokePathOp,
    RootDomMetricName::LayerPaintTextRunOp,
    RootDomMetricName::LayerPaintSvgOp,
    RootDomMetricName::LayerPaintImageOp,
    RootDomMetricName::LayerImageCreated,
    RootDomMetricName::RenumberedLayerZOrder,
    RootDomMetricName::TextIntrinsicMeasurement,
    RootDomMetricName::ImgIntrinsicRecomputation,
    RootDomMetricName::ImgIntrinsicStateUpdate,
    RootDomMetricName::NodeSlotAllocation,
    RootDomMetricName::TextStorageAllocation,
    RootDomMetricName::EventListenerRegistrationAllocation,
    RootDomMetricName::EmittedEventAllocation,
    RootDomMetricName::ClickDispatched,
    RootDomMetricName::ActivationPreApplied,
    RootDomMetricName::ActivationCanceled,
    RootDomMetricName::ActivationCommitted,
    RootDomMetricName::TransitionScheduleAllocation,
    RootDomMetricName::TransitionEventPathAllocation,
    RootDomMetricName::HoverLayerChainAllocation,
    RootDomMetricName::StylesheetRemovedAttributeListAllocation,
    RootDomMetricName::StylesheetTrackedAttributeSetAllocation,
    RootDomMetricName::FragmentTopLevelKeyListAllocation,
    RootDomMetricName::FragmentImportValidationStackAllocation,
    RootDomMetricName::RemovedSubtreeKeyListAllocation,
];

impl RootDomMetricName {
    /// Number of defined `RootDomMetricName` variants.
    pub const COUNT: usize = ROOT_DOM_METRIC_NAMES.len();

    /// Returns the fixed counter index for this metric.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::HoverPseudoStateSet => 0,
            Self::HoverPseudoStateCleared => 1,
            Self::StyleUpdated => 2,
            Self::LayoutUpdated => 3,
            Self::PaintUpdated => 4,
            Self::PaintPlanBuilt => 5,
            Self::LayerPresented => 6,
            Self::ClipUpdated => 7,
            Self::LayerTranslated => 8,
            Self::LayerPainted => 9,
            Self::LayerPaintRectOp => 10,
            Self::LayerPaintStrokePathOp => 11,
            Self::LayerPaintTextRunOp => 12,
            Self::LayerPaintSvgOp => 13,
            Self::LayerPaintImageOp => 14,
            Self::LayerImageCreated => 15,
            Self::RenumberedLayerZOrder => 16,
            Self::TextIntrinsicMeasurement => 17,
            Self::ImgIntrinsicRecomputation => 18,
            Self::ImgIntrinsicStateUpdate => 19,
            Self::NodeSlotAllocation => 20,
            Self::TextStorageAllocation => 21,
            Self::EventListenerRegistrationAllocation => 22,
            Self::EmittedEventAllocation => 23,
            Self::ClickDispatched => 24,
            Self::ActivationPreApplied => 25,
            Self::ActivationCanceled => 26,
            Self::ActivationCommitted => 27,
            Self::TransitionScheduleAllocation => 28,
            Self::TransitionEventPathAllocation => 29,
            Self::HoverLayerChainAllocation => 30,
            Self::StylesheetRemovedAttributeListAllocation => 31,
            Self::StylesheetTrackedAttributeSetAllocation => 32,
            Self::FragmentTopLevelKeyListAllocation => 33,
            Self::FragmentImportValidationStackAllocation => 34,
            Self::RemovedSubtreeKeyListAllocation => 35,
        }
    }
}

impl MetricName for RootDomMetricName {
    fn index(self) -> usize {
        RootDomMetricName::index(self)
    }
}

/// Snapshot copy of root-owned metric counter values.
pub type RootDomMetrics = MetricSnapshot<RootDomMetricName, { RootDomMetricName::COUNT }>;

/// One retained root-owned metric increment event.
pub type RootDomMetricHistoryEntry = MetricHistoryEntry<RootDomMetricName>;

/// Non-allocating iterator over retained root-owned metric increment history.
pub type RootDomMetricHistoryIter<'a> =
    MetricHistoryIter<'a, RootDomMetricName, ROOT_DOM_METRIC_HISTORY_CAPACITY>;

/// Root-owned mutable metric counter and fixed history store.
pub(crate) type RootDomMetricTally =
    Metrics<RootDomMetricName, { RootDomMetricName::COUNT }, ROOT_DOM_METRIC_HISTORY_CAPACITY>;

#[cfg(test)]
mod tests {
    use super::{ROOT_DOM_METRIC_NAMES, RootDomMetricName};

    #[test]
    fn metric_name_indices_are_unique_and_in_bounds() {
        let mut seen_indices = [false; RootDomMetricName::COUNT];

        for name in ROOT_DOM_METRIC_NAMES {
            let index = name.index();
            assert!(index < RootDomMetricName::COUNT);
            assert_eq!(ROOT_DOM_METRIC_NAMES[index], name);
            assert!(
                !seen_indices[index],
                "metric index should be assigned to exactly one metric"
            );
            seen_indices[index] = true;
        }

        assert!(seen_indices.into_iter().all(|seen| seen));
    }
}
