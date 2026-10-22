//! Root-owned metric name vocabulary for `ui-dom`.
//!
//! This file represents the fixed metric vocabulary used to profile retained
//! `ui-dom` mutations. Metric names are enum variants indexed into fixed
//! counter arrays; the shared storage and history mechanics live in
//! `collections::metrics`.
//!
//! Invariants and rules:
//!
//! - this file owns only the `RootDomMetricName` enum, its companion name
//!   table, and its `impl` blocks
//! - metric names must be enum variants, not strings or public struct fields
//! - the name table must stay a fixed array indexed by `RootDomMetricName`

use collections::metrics::MetricName;

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
    /// Number of fragment top-level key lists built during import.
    FragmentTopLevelKeyListAllocation,
    /// Number of fragment import validation stacks built.
    FragmentImportValidationStackAllocation,
    /// Number of removed subtree key lists built.
    RemovedSubtreeKeyListAllocation,
    /// Number of `style_mut` write calls that changed an inline declaration block.
    StyleDeclarationUpsert,
    /// Number of declarations dropped by a drop-and-continue declaration-list
    /// parse on the style-attribute route (CSS 2.1 §4.1.7; every drop is
    /// counted, never a graceful silent fallback).
    StyleDeclarationDropped,
    /// Number of elements resolved by the style recompute pass (winners fold +
    /// group resolution).
    StylePassNodeVisited,
    /// Number of resolved elements whose computed groups changed (group diff
    /// was not NONE).
    StylePassNodeChanged,
    /// Number of nodes whose measurement record was computed in the flex
    /// measurement fold. This is a WORK counter: one increment per
    /// `compute_node_measurement` call on the flex measure path (per node
    /// whose record is computed), not one per record write — an unchanged
    /// record still costs a compute.
    FlexNodesMeasured,
    /// Number of child relayouts performed during active flex placement (one
    /// increment per `relayout_active_flex_child` call, i.e. per child the
    /// placement walk relayouts).
    FlexNodesPlaced,
    /// Number of flagged-and-width-changed flex items re-measured at their
    /// resolved main size during the item-cross-at-resolved-main step (one
    /// increment per refold — D1.2's `main_influences_cross` re-measure, flex
    /// arc step 7). Width-unchanged items never refold, so an unchanged
    /// scenario's refold fraction is zero by construction. Shared with the grid
    /// solver (the T1 row-sizing refold of a flagged-and-width-changed item at
    /// its cell track width, `GRID_STEP2_DESIGN.md` D4): one mechanism, one
    /// counter — the rename from `FlexNodesRefolded` lands with the grid T1
    /// arc's documented re-pin.
    NodesRefolded,
    /// Number of nodes whose measurement record was computed in the grid
    /// measurement fold. One increment per `compute_node_measurement` on the
    /// grid measure path (the grid analog of `FlexNodesMeasured`).
    GridNodesMeasured,
    /// Number of child relayouts performed during active grid placement (one
    /// increment per `relayout_active_grid_child` call, i.e. per child the grid
    /// placement walk relayouts).
    GridNodesPlaced,
    /// Number of times an auto-fill/fit repetition count clamped to the
    /// track-lane capacity (12 tracks) instead of the fill formula's larger
    /// integer (grid T3.5's layout-time capacity gate, `grid_repetition_count_capped`).
    GridRepetitionCountCapped,
}

pub(crate) const ROOT_DOM_METRIC_NAMES: [RootDomMetricName; 44] = [
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
    RootDomMetricName::FragmentTopLevelKeyListAllocation,
    RootDomMetricName::FragmentImportValidationStackAllocation,
    RootDomMetricName::RemovedSubtreeKeyListAllocation,
    RootDomMetricName::StyleDeclarationUpsert,
    RootDomMetricName::StyleDeclarationDropped,
    RootDomMetricName::StylePassNodeVisited,
    RootDomMetricName::StylePassNodeChanged,
    RootDomMetricName::FlexNodesMeasured,
    RootDomMetricName::FlexNodesPlaced,
    RootDomMetricName::NodesRefolded,
    RootDomMetricName::GridNodesMeasured,
    RootDomMetricName::GridNodesPlaced,
    RootDomMetricName::GridRepetitionCountCapped,
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
            Self::FragmentTopLevelKeyListAllocation => 31,
            Self::FragmentImportValidationStackAllocation => 32,
            Self::RemovedSubtreeKeyListAllocation => 33,
            Self::StyleDeclarationUpsert => 34,
            Self::StyleDeclarationDropped => 35,
            Self::StylePassNodeVisited => 36,
            Self::StylePassNodeChanged => 37,
            Self::FlexNodesMeasured => 38,
            Self::FlexNodesPlaced => 39,
            Self::NodesRefolded => 40,
            Self::GridNodesMeasured => 41,
            Self::GridNodesPlaced => 42,
            Self::GridRepetitionCountCapped => 43,
        }
    }
}

impl MetricName for RootDomMetricName {
    fn index(self) -> usize {
        RootDomMetricName::index(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
