//! Per-node intrinsic measurement record for the shared layout up fold.
//!
//! This file owns `MeasurementRecord`, the shared exit-time computation
//! `compute_node_measurement`, the node-kind routing to the retained record
//! slot (`node_measurement_of`/`node_measurement_mut`), and the two
//! equality-gated write gates (`replace_measurement_if_changed` for a live
//! mutable borrow; `write_measurement_if_changed` for a node store + key).
//! Its purpose is to give each node one retained slot that the layout pass
//! writes exactly once per dirty pass (the block world at `exit_node`, the
//! flex subtree at the measure fold), carrying the subtree-computed content
//! sizes the flex solver reads in the same pass.
//!
//! Record assembly is unified here: both epilogues pass their shared outputs
//! (the entered-node layout, the min/max content-width contribution pair, and
//! the resolved content size) into one `compute_node_measurement` entry point —
//! one computation, one comparison, and a pass OUTPUT, never a dirt source.
//!
//! Invariants and rules:
//!
//! - only basis-independent intrinsics live in the `h_*`/`v_*` content slots;
//!   `content_height_at_width_px` is this-pass measured and therefore
//!   basis-dependent by design (it is the answer placement needs now)
//! - the `v_*` slots stay `None` until their named consumer lands in flex
//!   step 4; nothing computes what nothing reads
//! - record writes are equality-gated through `replace_measurement_if_changed`
//!   (live borrow) or `write_measurement_if_changed` (store + key, read-first)
//!   and are a pass OUTPUT: they never mark style/layout/paint dirty

use crate::domains::layout::measurement_flags::MeasurementFlags;
use crate::domains::style::computed::FlexDirectionKw;
use crate::numeric::usize_to_f32;
use crate::types::dom_node::{DomIntrinsic, DomNode, DomNodeSlotKey};
use crate::types::root_dom_nodes::RootDomNodes;
use crate::types::{DomError, DomWhiteSpace, RootDom};

use super::flex_anonymous_run::node_is_excluded_flex_child;
use super::flow::{EnteredNodeLayout, ResolvedContentSizePx};
use super::style::LayoutDisplay;

#[cfg(test)]
use crate::domains::style::computed::DisplayKw;
#[cfg(test)]
use crate::types::RootDomNodesMetricName;
#[cfg(test)]
use crate::types::{DomTag, RootDomContext};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasurementRecord {
    /// Min-content inline size, computed bottom-up (the block fold's
    /// `min_width_px`).
    pub h_min_content_px: f32,
    /// Max-content inline size, computed bottom-up (the block fold's
    /// `max_width_px`).
    pub h_max_content_px: f32,
    /// Content height at this pass's laid-out width.
    pub content_height_at_width_px: f32,
    /// The width at which `content_height_at_width_px` was measured (the flex
    /// measure fold's wrap width for a text node). This is the record-serving
    /// placement freshness key (D3): a width match lets placement read the
    /// wrapped height instead of re-shaping.
    pub content_height_measured_at_width_px: f32,
    /// The per-update layout pass in which this record was written (the flex
    /// measure fold's `LayoutScratch::layout_pass`). Zero for the block world's
    /// `exit_node` writes, which the flex freshness check never reads.
    pub measure_pass: u32,
    /// Min-content block size. Uncomputed in step 1; lands with its named
    /// consumer in flex step 4.
    pub v_min_content_px: Option<f32>,
    /// Max-content block size. Uncomputed in step 1; lands with its named
    /// consumer in flex step 4.
    pub v_max_content_px: Option<f32>,
    /// Fixed measurement class flags; see `MeasurementFlags`.
    pub flags: MeasurementFlags,
}

impl MeasurementRecord {
    /// Equality-gated record write: stores `next` only when it differs from
    /// the current value, returning whether a write occurred. This is the
    /// write gate the layout up fold uses at node exit; a write is a pass
    /// output and never marks style/layout/paint dirty.
    pub fn replace_measurement_if_changed(&mut self, next: MeasurementRecord) -> bool {
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}

/// Returns a node's retained measurement record (read side). The layout up
/// fold writes it once per node per dirty pass; readers see children the fold
/// has already exited, so freshness is structural. This is the canonical
/// node-kind routing for the retained record slot.
pub(super) fn node_measurement_of(node: &DomNode) -> MeasurementRecord {
    match node {
        DomNode::Document(node) => node.base.measurement,
        DomNode::DocumentFragment(node) => node.base.measurement,
        DomNode::Element(node) => node.base.measurement,
        DomNode::Text(node) => node.base.measurement,
    }
}

/// Returns a node's retained measurement record slot (write side). This helper
/// only routes to the right node-kind base; callers gate the write through
/// `write_measurement_if_changed` (store + key) or
/// `replace_measurement_if_changed` (a live `&mut MeasurementRecord`).
pub(super) fn node_measurement_mut(node: &mut DomNode) -> &mut MeasurementRecord {
    match node {
        DomNode::Document(node) => &mut node.base.measurement,
        DomNode::DocumentFragment(node) => &mut node.base.measurement,
        DomNode::Element(node) => &mut node.base.measurement,
        DomNode::Text(node) => &mut node.base.measurement,
    }
}

/// Read-first record write gate for the flex measure fold: compares the
/// freshly-computed `next` record against the stored one with ONE immutable
/// node-store read, and takes the mutable borrow only when the value changed.
/// Returns `Ok(true)` iff a write occurred.
///
/// The record is a pass OUTPUT, never a dirt source; an unchanged record costs
/// a read, not a mutation. This is the single-comparison form of
/// `replace_measurement_if_changed` for call sites that hold only the node
/// store + key rather than a live mutable borrow.
pub(super) fn write_measurement_if_changed(
    nodes: &mut RootDomNodes,
    key: DomNodeSlotKey,
    next: MeasurementRecord,
) -> Result<bool, DomError> {
    let stored = node_measurement_of(nodes.get(key).ok_or(DomError::InvalidNodeKey)?);
    if stored == next {
        return Ok(false);
    }
    *node_measurement_mut(nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?) = next;
    Ok(true)
}

/// Computes the exit-time measurement record for one node from the shared
/// block/inline/flex exit epilogue's data.
///
/// `content_width_contribution_px` is the `(min, max)` CONTENT inline-size
/// contribution pair from `content_width_contribution_for_exit` (before this
/// node's own horizontal border+padding — the value `layout_flow_size` adds
/// edges to). `content_size` is the resolved content size the box model
/// derives from; its `viewport_height_px` is the content height at this pass's
/// laid-out width. The `v_min` slot stays `None` here: the flex measure
/// epilogue computes it after this call via `compute_v_min_content_px` (its
/// named consumer is the flex auto-min derivation), while the block world
/// leaves it `None` (nothing reads it). `v_max` stays `None` everywhere (no
/// consumer; D6 discipline).
///
/// Flags are the fixed measurement class bits read from the entered node:
/// `PRESERVE_MODE` from the node's effective white-space class (preserve =
/// `pre`/`pre-wrap`/`break-spaces`; collapse = everything else), and
/// `REPLACED_RATIO` from an img's intrinsic aspect ratio. The effective
/// white-space is read from `entered.inherited_style`, which for an element is
/// its own computed text group and for a text node is the inherited group —
/// the value that actually applies at the node.
///
/// `MAIN_INFLUENCES_CROSS` (bit 2) is set here for a text node whose
/// white-space mode wraps (`pre`/`nowrap` suppress soft wrap, so their
/// cross size is width-independent). The two node-dependent halves of the
/// flag — percentage cross-axis forms and containment propagation — are
/// OR'd in by the flex measure fold (`measure_exited_node_flow_size`,
/// `flex.rs`), which has the node at hand.
pub(super) fn compute_node_measurement(
    entered: &EnteredNodeLayout,
    content_width_contribution_px: (f32, f32),
    content_size: ResolvedContentSizePx,
) -> MeasurementRecord {
    let (min_content_width_px, max_content_width_px) = content_width_contribution_px;
    let content_height_at_width_px = content_size.viewport_height_px;
    let mut flags = MeasurementFlags::empty();
    if white_space_is_preserve_mode(entered.inherited_style.effective_white_space()) {
        flags = flags.with_preserve_mode();
    }
    if entered.img_intrinsic_aspect_ratio.is_some() {
        flags = flags.with_replaced_ratio();
    }
    if entered.is_text_node && white_space_wraps(entered.inherited_style.effective_white_space()) {
        flags = flags.with_main_influences_cross();
    }
    MeasurementRecord {
        h_min_content_px: min_content_width_px,
        h_max_content_px: max_content_width_px,
        content_height_at_width_px,
        content_height_measured_at_width_px: 0.0,
        measure_pass: 0,
        v_min_content_px: None,
        v_max_content_px: None,
        flags,
    }
}

/// Computes the min-content BLOCK size (`v_min_content_px`) for one node at its
/// exit (flex arc step 4, D3). Leaf kinds read their retained intrinsic (text's
/// run-count × line-height; img/control's fixed block size); block, inline, and
/// nested flex containers fold their in-flow children's records bottom-up (the
/// up fold wrote the children first): block/inline/column-flex SUM the stacked
/// children, row-flex takes the MAX (the tallest item). `v_max_content_px`
/// stays `None` (no consumer; D6 discipline).
///
/// This is the flex measure fold's vertical half: the record's `v_min` is only
/// consumed by the flex auto-min derivation (D1), so it is computed here and
/// written by the flex measure epilogue (`measure_exited_node_flow_size` in
/// `flex.rs`), never by the block world's `exit_node` (whose records carry
/// `None` and are never read for `v_min`).
pub(super) fn compute_v_min_content_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
) -> f32 {
    let Some(node) = root_dom.nodes.get(key) else {
        return 0.0;
    };
    match node.intrinsic() {
        DomIntrinsic::Text(text) => text.v_min_content_px,
        DomIntrinsic::Img(image) => image.height_px,
        DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => intrinsic.height_px,
        DomIntrinsic::None | DomIntrinsic::NotMeasured => {
            if matches!(
                entered.style.display,
                LayoutDisplay::Flex | LayoutDisplay::InlineFlex
            ) {
                flex_container_v_min_content_px(root_dom, node)
            } else {
                child_v_min_content_sum_px(root_dom, node)
            }
        }
    }
}

/// A nested flex container's min-content block size (D3): a row's is the
/// tallest item's (items share one line), a column's is the sum (items stack)
/// plus the inter-item `row-gap` × (`item_count` − 1) (CSS Box Alignment §8:
/// the min-content block size includes the gaps between stacked items). Read
/// from the in-flow child records at the inner container's exit.
fn flex_container_v_min_content_px(root_dom: &RootDom, node: &DomNode) -> f32 {
    let flex = node.computed_flex();
    let is_row = matches!(
        flex.direction,
        FlexDirectionKw::Row | FlexDirectionKw::RowReverse
    );
    let mut result: f32 = 0.0;
    let mut item_count: usize = 0;
    let mut child_key = node.first_child_key_for_store();
    while let Some(key) = child_key {
        let Some(child) = root_dom.nodes.get(key) else {
            break;
        };
        let next = child.next_sibling_key_for_store();
        if node_is_excluded_flex_child(child) {
            child_key = next;
            continue;
        }
        let child_v_min = node_measurement_of(child).v_min_content_px.unwrap_or(0.0);
        if is_row {
            result = result.max(child_v_min);
        } else {
            result += child_v_min;
        }
        item_count += 1;
        child_key = next;
    }
    // Row containers take the max over children (no gap term); column items
    // stack, so the block-direction gap (`row_gap`, flex.gap[0]) separates each
    // adjacent pair.
    if !is_row {
        result += flex.gap[0] * usize_to_f32(item_count.saturating_sub(1));
    }
    result
}

/// A block/inline container's min-content block size (D3): the sum of its
/// in-flow children's `v_min` (block stacking; at min-content width nothing
/// shares an inline line). Abs-pos/`display: none` children contribute nothing.
fn child_v_min_content_sum_px(root_dom: &RootDom, node: &DomNode) -> f32 {
    let mut sum = 0.0;
    let mut child_key = node.first_child_key_for_store();
    while let Some(key) = child_key {
        let Some(child) = root_dom.nodes.get(key) else {
            break;
        };
        let next = child.next_sibling_key_for_store();
        if node_is_excluded_flex_child(child) {
            child_key = next;
            continue;
        }
        sum += node_measurement_of(child).v_min_content_px.unwrap_or(0.0);
        child_key = next;
    }
    sum
}

/// The preserve white-space class (`pre`/`pre-wrap`/`break-spaces`) carries
/// literal space runs; every other class (`normal`, `nowrap`, `pre-line`)
/// collapses them. This mirrors the crate's `text_layout_input_for_white_space`
/// grouping so the measurement class flag matches the text shaping path.
fn white_space_is_preserve_mode(white_space: DomWhiteSpace) -> bool {
    matches!(
        white_space,
        DomWhiteSpace::Pre | DomWhiteSpace::PreWrap | DomWhiteSpace::BreakSpaces
    )
}

/// Whether a white-space mode admits soft wrapping (`pre`/`nowrap` suppress
/// it — the same predicate as the text shaping path's
/// `white_space_suppresses_soft_wrap`). A wrapping text node's cross size
/// depends on its width, so it carries `MAIN_INFLUENCES_CROSS`.
fn white_space_wraps(white_space: DomWhiteSpace) -> bool {
    !matches!(white_space, DomWhiteSpace::Nowrap | DomWhiteSpace::Pre)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a block container with `child_count` div children and writes each
    /// child's record `v_min` to the given value, returning the container key.
    fn block_container_with_child_v_mins(root: &mut RootDom, child_v_min: f32) -> DomNodeSlotKey {
        let document = root.document_node().0;
        let container = root
            .create_element(DomTag::Div)
            .expect("container should insert");
        root.append_child(document, container)
            .expect("container should append");
        for _ in 0..2 {
            let child = root
                .create_element(DomTag::Div)
                .expect("child should insert");
            root.append_child(container, child)
                .expect("child should append");
            node_measurement_mut(root.nodes.get_mut(child).expect("child node should exist"))
                .v_min_content_px = Some(child_v_min);
        }
        container
    }

    #[test]
    #[allow(clippy::float_cmp)] // the fold is exact integer arithmetic (10 + 10)
    fn child_v_min_sum_adds_in_flow_child_block_sizes() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let container = block_container_with_child_v_mins(&mut root, 10.0);

        let node = root.nodes.get(container).expect("container node");
        let sum = child_v_min_content_sum_px(&root, node);

        assert_eq!(sum, 20.0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // the fold is exact arithmetic (12, excluded 99)
    fn child_v_min_sum_skips_excluded_children() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.document_node().0;
        let container = root
            .create_element(DomTag::Div)
            .expect("container should insert");
        root.append_child(document, container)
            .expect("container should append");
        let in_flow = root
            .create_element(DomTag::Div)
            .expect("in-flow child should insert");
        root.append_child(container, in_flow)
            .expect("in-flow child should append");
        node_measurement_mut(
            root.nodes
                .get_mut(in_flow)
                .expect("in-flow child node should exist"),
        )
        .v_min_content_px = Some(12.0);
        let hidden = root
            .create_element(DomTag::Div)
            .expect("hidden child should insert");
        root.append_child(container, hidden)
            .expect("hidden child should append");
        // Set the computed display directly (no style pass in this unit test),
        // mirroring the flex classification predicate's read surface.
        match root
            .nodes
            .get_mut(hidden)
            .expect("hidden child node should exist")
        {
            DomNode::Element(element) => {
                element.base.presentation.computed.box_.display = DisplayKw::None;
            }
            _ => panic!("hidden child should be an element"),
        }
        node_measurement_mut(
            root.nodes
                .get_mut(hidden)
                .expect("hidden child node should exist"),
        )
        .v_min_content_px = Some(99.0);

        let node = root.nodes.get(container).expect("container node");
        let sum = child_v_min_content_sum_px(&root, node);

        assert_eq!(sum, 12.0);
    }

    /// Sets the computed flex direction/gap directly on `container` (no style
    /// pass in this unit test), mirroring the fold's read surface, then appends
    /// in-flow div children with the given `v_min` values.
    fn flex_container_with_child_v_mins(
        root: &mut RootDom,
        direction: FlexDirectionKw,
        gap: [f32; 2],
        child_v_mins: impl IntoIterator<Item = f32>,
    ) -> DomNodeSlotKey {
        let document = root.document_node().0;
        let container = root
            .create_element(DomTag::Div)
            .expect("container should insert");
        root.append_child(document, container)
            .expect("container should append");
        match root
            .nodes
            .get_mut(container)
            .expect("container node should exist")
        {
            DomNode::Element(element) => {
                element.base.presentation.computed.flex.direction = direction;
                element.base.presentation.computed.flex.gap = gap;
            }
            _ => panic!("container should be an element"),
        }
        for child_v_min in child_v_mins {
            let child = root
                .create_element(DomTag::Div)
                .expect("child should insert");
            root.append_child(container, child)
                .expect("child should append");
            node_measurement_mut(root.nodes.get_mut(child).expect("child node should exist"))
                .v_min_content_px = Some(child_v_min);
        }
        container
    }

    #[test]
    #[allow(clippy::float_cmp)] // the fold is exact arithmetic (10 + 20 + 10)
    fn flex_column_v_min_sums_children_with_row_gap() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let container = flex_container_with_child_v_mins(
            &mut root,
            FlexDirectionKw::Column,
            [10.0, 0.0],
            [10.0, 20.0],
        );

        let node = root.nodes.get(container).expect("container node");
        let v_min = flex_container_v_min_content_px(&root, node);

        // §9.9/CSS Box Alignment §8: a column's min-content block size includes
        // the row-gap between each adjacent stacked pair — 10 + 20 + 10 = 40.
        assert_eq!(v_min, 40.0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // the fold is exact arithmetic (max(10, 20))
    fn flex_row_v_min_takes_max_without_gap() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let container = flex_container_with_child_v_mins(
            &mut root,
            FlexDirectionKw::Row,
            [10.0, 0.0],
            [10.0, 20.0],
        );

        let node = root.nodes.get(container).expect("container node");
        let v_min = flex_container_v_min_content_px(&root, node);

        // A row's v_min is the tallest item's (items share one line) — no sum,
        // and the row-gap is a cross-direction gap, so it never enters.
        assert_eq!(v_min, 20.0);
    }

    #[test]
    fn default_record_has_none_v_slots_and_zero_flags() {
        let record = MeasurementRecord::default();

        // The derived Default keeps every slot zero and the v slots uncomputed.
        assert_eq!(
            record,
            MeasurementRecord {
                h_min_content_px: 0.0,
                h_max_content_px: 0.0,
                content_height_at_width_px: 0.0,
                content_height_measured_at_width_px: 0.0,
                measure_pass: 0,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags { bits: 0 },
            }
        );
        assert_eq!(record.v_min_content_px, None);
        assert_eq!(record.v_max_content_px, None);
        assert_eq!(record.flags.bits, 0);
        assert_eq!(record.measure_pass, 0);
    }

    #[test]
    fn replace_measurement_if_changed_rejects_equal_write() {
        let mut record = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            content_height_measured_at_width_px: 0.0,
            measure_pass: 0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 1 },
        };
        let equal = record;

        assert!(!record.replace_measurement_if_changed(equal));
        assert_eq!(record, equal);
    }

    #[test]
    fn replace_measurement_if_changed_writes_changed_value() {
        let mut record = MeasurementRecord::default();
        let next = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            content_height_measured_at_width_px: 0.0,
            measure_pass: 0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 2 },
        };

        assert!(record.replace_measurement_if_changed(next));
        assert_eq!(record, next);
    }

    #[test]
    fn replace_measurement_if_changed_treats_none_to_some_v_slot_as_change() {
        let mut record = MeasurementRecord::default();
        let next = MeasurementRecord {
            v_min_content_px: Some(8.0),
            ..MeasurementRecord::default()
        };

        assert!(record.replace_measurement_if_changed(next));
        assert_eq!(record.v_min_content_px, Some(8.0));
        assert_eq!(record.v_max_content_px, None);
        assert_eq!(record.flags.bits, 0);
    }

    #[test]
    fn write_measurement_if_changed_rejects_equal_without_write() {
        let mut nodes = RootDomNodes::with_key();
        let key = nodes.insert_with_key(DomNode::document);
        let record = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            content_height_measured_at_width_px: 0.0,
            measure_pass: 0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 1 },
        };
        *node_measurement_mut(nodes.get_mut(key).expect("node should exist")) = record;
        nodes.reset_metrics();

        let wrote = write_measurement_if_changed(&mut nodes, key, record)
            .expect("write gate should resolve");

        assert!(!wrote);
        // The equal gate costs one immutable read and takes no mutable borrow.
        assert_eq!(nodes.count(RootDomNodesMetricName::Get), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::GetMut), 0);
        assert_eq!(
            node_measurement_of(nodes.get(key).expect("node should exist")),
            record,
        );
    }

    #[test]
    fn write_measurement_if_changed_writes_changed_value() {
        let mut nodes = RootDomNodes::with_key();
        let key = nodes.insert_with_key(DomNode::document);
        let stored = MeasurementRecord::default();
        *node_measurement_mut(nodes.get_mut(key).expect("node should exist")) = stored;
        nodes.reset_metrics();
        let next = MeasurementRecord {
            h_min_content_px: 10.0,
            h_max_content_px: 20.0,
            content_height_at_width_px: 15.0,
            content_height_measured_at_width_px: 0.0,
            measure_pass: 0,
            v_min_content_px: None,
            v_max_content_px: None,
            flags: MeasurementFlags { bits: 2 },
        };

        let wrote =
            write_measurement_if_changed(&mut nodes, key, next).expect("write gate should resolve");

        assert!(wrote);
        // One read + one write; the write is unconditional after the single
        // comparison (no second compare inside).
        assert_eq!(nodes.count(RootDomNodesMetricName::Get), 1);
        assert_eq!(nodes.count(RootDomNodesMetricName::GetMut), 1);
        assert_eq!(
            node_measurement_of(nodes.get(key).expect("node should exist")),
            next,
        );
    }

    #[test]
    fn write_measurement_if_changed_treats_none_to_some_v_slot_as_change() {
        let mut nodes = RootDomNodes::with_key();
        let key = nodes.insert_with_key(DomNode::document);
        nodes.reset_metrics();
        let next = MeasurementRecord {
            v_min_content_px: Some(8.0),
            ..MeasurementRecord::default()
        };

        let wrote =
            write_measurement_if_changed(&mut nodes, key, next).expect("write gate should resolve");

        assert!(wrote);
        assert_eq!(nodes.count(RootDomNodesMetricName::GetMut), 1);
        let written = node_measurement_of(nodes.get(key).expect("node should exist"));
        assert_eq!(written.v_min_content_px, Some(8.0));
        assert_eq!(written.v_max_content_px, None);
        assert_eq!(written.flags.bits, 0);
    }

    #[test]
    fn white_space_preserve_class_matches_text_shaping_grouping() {
        assert!(white_space_is_preserve_mode(DomWhiteSpace::Pre));
        assert!(white_space_is_preserve_mode(DomWhiteSpace::PreWrap));
        assert!(white_space_is_preserve_mode(DomWhiteSpace::BreakSpaces));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::Normal));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::Nowrap));
        assert!(!white_space_is_preserve_mode(DomWhiteSpace::PreLine));
    }

    #[test]
    fn white_space_wraps_class_matches_soft_wrap_suppression() {
        // `pre`/`nowrap` suppress soft wrap; every other admitted mode wraps.
        assert!(white_space_wraps(DomWhiteSpace::Normal));
        assert!(white_space_wraps(DomWhiteSpace::PreWrap));
        assert!(white_space_wraps(DomWhiteSpace::PreLine));
        assert!(white_space_wraps(DomWhiteSpace::BreakSpaces));
        assert!(!white_space_wraps(DomWhiteSpace::Pre));
        assert!(!white_space_wraps(DomWhiteSpace::Nowrap));
    }
}
