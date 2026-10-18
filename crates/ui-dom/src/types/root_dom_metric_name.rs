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
}

pub(crate) const ROOT_DOM_METRIC_NAMES: [RootDomMetricName; 38] = [
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
