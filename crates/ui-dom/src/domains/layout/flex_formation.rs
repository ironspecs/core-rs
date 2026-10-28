//! Flex item formation — the item-formation pass of the flex solver.
//!
//! This file owns the DOM-children → flex-items formation stage (flex arc step
//! 3): classifying direct children into anonymous inline runs, block items, and
//! excluded abs-pos/`display: none` children; blockifying an inline-flex item's
//! outer display; assembling the anonymous run's virtual `FlexItemView`; and
//! writing items into the item/order lanes at formation.
//!
//! Invariants and rules:
//!
//! - formation runs once per container inside the measurement fold; it never
//!   measures children itself — it delegates each member's measurement to the
//!   measure phase's walk
//! - the order lane is built here (push-in-order + back-fill), in DOM order
//! - excluded children (abs-pos/`display: none`) never enter the lanes
//! - the anonymous run's item view carries the CSS initials (order 0, grow 0,
//!   shrink 1) and no box model

use crate::types::{
    DomError, DomNodeSlotKey, DomWhiteSpace, LaneRange, LayoutScratch, RootDom, RootDomMetricName,
};

use super::DomLayoutResolver;
use super::flex::{FlexAxis, FlexItemView, node_for_key};
use super::flex_anonymous_run::{
    RunAccumulator, accumulate_run, empty_run, member_metrics_from_element_atom,
    member_metrics_from_text, node_is_excluded_flex_child, node_is_inline_run_member,
};
use super::flex_item_source::FlexItemSource;
use super::flex_measure::measure_tree_walker_node_without_layout_write;
use super::flex_order_lane::FlexOrderLaneEntry;
use super::flow::{
    InlineFlowState, LayoutContext, next_inline_child_context, next_sibling_key,
    resolved_node_style,
};
use super::style::{LayoutDisplay, NodeStyle};

/// After the order lane is fully insertion-sorted, writes each item-lane
/// entry's `order_index` back-reference to its FINAL order-lane index
/// (insertion shifts earlier indices on later pushes, so the at-push
/// index is not the final one).
pub(crate) fn backfill_order_indices(scratch: &mut LayoutScratch, order_range: LaneRange) {
    let entries = scratch.order.slice(order_range);
    for (index, entry) in entries.iter().enumerate() {
        scratch.items.get_mut(entry.dom_index).order_index =
            order_range.start + u32::try_from(index).expect("order lane index should fit into u32");
    }
}

/// Pushes one assembled item view into the item lane and its order-lane entry
/// (the `child_key` is the item's identity anchor: the child's own key for a
/// block item, the run's first-child key for an anonymous inline group).
pub(crate) fn push_flex_item_into_lanes(
    scratch: &mut LayoutScratch,
    item: FlexItemView,
    child_key: DomNodeSlotKey,
) {
    let dom_index = scratch.items.push(item);
    scratch.order.push_in_order(FlexOrderLaneEntry {
        dom_index,
        child_key,
        order: item.order,
    });
}

/// The virtual item view for an anonymous inline run (`FLEX_MAPPING` §3.2, D5):
/// order 0, `align_self` None, grow 0 / shrink 1 (the CSS INITIALS — an
/// anonymous wrapper inherits nothing, so `flex-shrink` keeps its initial 1),
/// basis auto (base = the run's max-content), zero margins/border/padding, no
/// min/max, `auto_min` = the run's min-content (the widest-segment cursor).
///
/// The run's content sizes come from the accumulator, NOT a node record (there
/// is no node) — the one view exempt from the record-read contract.
///
/// The auto-min walk is bypassed for run items — a run has no node to walk —
/// so the cursor min-content IS the automatic minimum. shrink 1 lets the run
/// shrink in negative free space, clamped at that min-content floor (never
/// below it); grow 0 keeps it frozen against positive free space. The
/// horizontal-main cross is the D4 joint-measured height at the measure width
/// (a shrunk run wraps and grows its cross); the vertical-main axis maps the
/// strut to the main size (the run's single-line height) and the max-content
/// width to the cross size.
pub(crate) fn anonymous_inline_run_item_view(
    source: FlexItemSource,
    run: RunAccumulator,
    measured_joint_height_px: f32,
    main_axis: FlexAxis,
) -> FlexItemView {
    let max_content_width_px = run.max_content_sum_px;
    let min_content_width_px = run.widest_segment_px;
    let strut_line_height_px = run.strut_line_height_px;
    let (flex_base_size_px, automatic_min_main_size_px, measured_cross_size_input_px) =
        match main_axis {
            FlexAxis::Horizontal => (
                max_content_width_px,
                min_content_width_px,
                measured_joint_height_px,
            ),
            FlexAxis::Vertical => (
                strut_line_height_px,
                strut_line_height_px,
                max_content_width_px,
            ),
        };
    FlexItemView {
        source,
        order: 0,
        align_self: None,
        flex_grow_factor: 0.0,
        flex_shrink_factor: 1.0,
        flex_base_size_px,
        hypothetical_main_size_px: flex_base_size_px,
        automatic_min_main_size_px,
        min_main_size_px: None,
        max_main_size_px: None,
        main_axis_border_padding_px: 0.0,
        outer_hypothetical_main_size_px: flex_base_size_px,
        measured_cross_size_input_px,
        replaced_ratio: false,
        main_influences_cross: false,
        resolved_cross_size_px: measured_cross_size_input_px,
        // D2a: a run's inline min/max-content are the 3a cursors (widest
        // segment / max-content sum), axis- and wrap-agnostic.
        h_min_content_px: min_content_width_px,
        h_max_content_px: max_content_width_px,
        margin_main_start_px: 0.0,
        margin_main_end_px: 0.0,
        margin_cross_start_px: 0.0,
        margin_cross_end_px: 0.0,
        // A run has no node style, so it carries no auto margins.
        margin_main_start_auto: false,
        margin_main_end_auto: false,
        margin_cross_start_auto: false,
        margin_cross_end_auto: false,
    }
}

/// Measures a maximal run of contiguous inline-level children starting at
/// `first_child_key`: the D3 fold (intrinsic cursors) interleaved with the D4
/// cross measurement (the existing inline accumulation — one measure per child
/// writing its record, `InlineFlowState`/`next_inline_child_context` reducing
/// the available width by the used line width). Returns the folded accumulator,
/// the key of the first run-breaker after the run (`None` at the end of the
/// siblings), and the run's joint laid-out height at the measure width (the
/// D4 wrapper's `flow.line_height_px` — the max child height, the existing
/// inline accumulation). Joint line-breaking happens in the placement path,
/// not here. `first_child_key` is a run member (the caller classified it);
/// subsequent siblings are classified here.
pub(crate) fn measure_inline_run<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    first_child_key: DomNodeSlotKey,
    child_context: &LayoutContext,
    white_space: DomWhiteSpace,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<(RunAccumulator, Option<DomNodeSlotKey>, f32), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let mut run = empty_run(first_child_key);
    let mut flow = InlineFlowState::default();
    let mut child_key = Some(first_child_key);
    while let Some(current_child_key) = child_key {
        // D1 (flex arc step 3b): an excluded child (abs-pos/`display: none`) is
        // NOT a run-breaker — the run joins across it (engines skip OOF/none
        // children at formation, so a text–abs–text sequence is one contiguous
        // run). It contributes no width and is not folded.
        {
            let current_node = node_for_key(root_dom, current_child_key)?;
            if node_is_excluded_flex_child(current_node) {
                child_key = next_sibling_key(root_dom, current_child_key)?;
                continue;
            }
        }
        let is_text_node = {
            let current_node = node_for_key(root_dom, current_child_key)?;
            if !node_is_inline_run_member(current_node) {
                return Ok((run, Some(current_child_key), flow.line_height_px));
            }
            current_node.as_text().is_some()
        };
        // D4: measure the member at the remaining inline width — the same
        // per-child loop as `measure_inline_node_without_layout_write`, with a
        // different child source (the run's sibling range, not a node's
        // children). The measure writes the member's record, so an inline
        // element's record is fresh for the atom fold immediately below.
        let (child_style, child_inherited_style) =
            resolved_node_style(root_dom, current_child_key, child_context)?;
        let next_child_context = next_inline_child_context(child_context.clone(), flow);
        let child_flow = measure_tree_walker_node_without_layout_write(
            root_dom,
            tree_size,
            current_child_key,
            &next_child_context,
            None,
            child_style,
            child_inherited_style,
            resolver,
            metric,
        )?;
        flow.has_children = true;
        flow.inline_left_px += child_flow.width_px;
        flow.line_height_px = flow.line_height_px.max(child_flow.height_px);
        // D3: fold the member's fragment metrics (text reads the retained
        // intrinsic; an inline element reads its just-written record as one
        // atom whose edges never join).
        let current_node = node_for_key(root_dom, current_child_key)?;
        let member = if is_text_node {
            member_metrics_from_text(current_node)
        } else {
            member_metrics_from_element_atom(current_node, child_flow.height_px)
        };
        accumulate_run(&mut run, member, white_space);
        child_key = next_sibling_key(root_dom, current_child_key)?;
    }
    Ok((run, None, flow.line_height_px))
}

/// CSS Display 3 §2.7: blockify a flex item's `inline-flex` outer display to
/// `flex`. The solver hosts only block-level flex containers, and the flex
/// item path (`measure_one_flex_item`) applies this before measurement so a
/// nested `inline-flex` item behaves identically to `display: flex`.
///
/// The blockification map stays `InlineFlex → Flex` (`FLEX_STEP3C_DESIGN.md` D2):
/// `inline-block` is already block-routed (it reaches this function's caller
/// as a block-measured item, never `InlineBlock` here), and `display: inline`
/// elements are anonymous-run members (flex arc step 3a-ii) — never flex items,
/// so `inline → block` has no application.
pub(crate) fn blockified_flex_item_style(mut style: NodeStyle) -> NodeStyle {
    if style.display == LayoutDisplay::InlineFlex {
        style.display = LayoutDisplay::Flex;
    }
    style
}
#[cfg(test)]
use crate::domains::layout::flex::{
    append_div, append_span, append_text, build_solve_for_container, collect_items_for_container,
    flex_container_with_context, item_views_in_order_modified_order, node_record,
    text_intrinsic_triple, text_intrinsic_widths,
};

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn item_view_text_run_reads_text_intrinsic_fragment_metrics() {
        let (mut root, container, context) = flex_container_with_context();
        let text = append_text(&mut root, container, "Flex label");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("text run item");
        let (min_width_px, max_width_px, strut_px) = text_intrinsic_triple(&root, text);

        // A bare text node is now ONE anonymous inline-run item (CSS Flexbox
        // §4, flex arc step 3a-ii), not a block item: its base is the run's
        // max-content, its automatic minimum the run's min-content (the cursor
        // value, bypassing the auto-min walk), and its cross the strut line
        // height. The run item carries zero border/padding/margins.
        assert_eq!(
            item.source,
            FlexItemSource::AnonymousInlineGroup {
                first_child_key: text,
                child_count: 1,
            },
        );
        assert_eq!(item.flex_base_size_px, max_width_px);
        assert_eq!(item.hypothetical_main_size_px, max_width_px);
        assert_eq!(item.automatic_min_main_size_px, min_width_px);
        assert_eq!(item.measured_cross_size_input_px, strut_px);
        assert_eq!(item.main_axis_border_padding_px, 0.0);
        assert_eq!(item.outer_hypothetical_main_size_px, max_width_px);
    }

    /// D9 pin (f): the run's min-content (the automatic minimum) is the widest
    /// joined segment — `"foo"+"bar"` (no space) joins to `w("foobar")`, while
    /// `"foo "+"bar"` breaks at the collapsible space to `max(w("foo"),w("bar"))`.
    #[test]
    fn item_view_joined_text_run_min_content_is_the_joined_segment() {
        let (mut root, container, context) = flex_container_with_context();
        let foo = append_text(&mut root, container, "foo");
        let bar = append_text(&mut root, container, "bar");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("run item");
        let (_, foo_max) = text_intrinsic_widths(&root, foo);
        let (_, bar_max) = text_intrinsic_widths(&root, bar);

        assert_eq!(
            item.source,
            FlexItemSource::AnonymousInlineGroup {
                first_child_key: foo,
                child_count: 2,
            },
        );
        // Joined across the no-space boundary: min-content == w("foobar").
        assert_eq!(item.automatic_min_main_size_px, foo_max + bar_max);
        assert_eq!(item.flex_base_size_px, foo_max + bar_max);
    }

    #[test]
    fn item_view_broken_text_run_min_content_is_the_widest_word() {
        let (mut root, container, context) = flex_container_with_context();
        let foo = append_text(&mut root, container, "foo ");
        let bar = append_text(&mut root, container, "bar");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("run item");
        let (foo_min, _) = text_intrinsic_widths(&root, foo);
        let (bar_min, _) = text_intrinsic_widths(&root, bar);

        // Broken at the collapsible space: min-content == max(w("foo"), w("bar")).
        assert_eq!(item.automatic_min_main_size_px, foo_min.max(bar_min));
    }

    #[test]
    fn item_view_inline_run_reads_span_fragment_metrics() {
        let (mut root, container, context) = flex_container_with_context();
        let span = append_span(&mut root, container);
        let first = append_text(&mut root, span, "one two");
        let second = append_text(&mut root, span, "three four");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("inline run item");
        let (_, first_max) = text_intrinsic_widths(&root, first);
        let (_, second_max) = text_intrinsic_widths(&root, second);

        // A `display: inline` element is now ONE anonymous inline-run item
        // (CSS Flexbox §4): its base is the run's max-content (the sum of the
        // span's text children's max-content) and its automatic minimum the
        // span's own min-content. The span is an ATOM (D1) — its whole subtree
        // measures as one inline unit, so the run min-content reads the span's
        // measured min-content (max child min-content, no fragment joining
        // across the element boundary).
        assert_eq!(
            item.source,
            FlexItemSource::AnonymousInlineGroup {
                first_child_key: span,
                child_count: 1,
            },
        );
        assert_eq!(item.flex_base_size_px, first_max + second_max);
        assert_eq!(
            item.automatic_min_main_size_px,
            node_record(&root, span).h_min_content_px,
        );
        assert_eq!(item.main_axis_border_padding_px, 0.0);
    }

    #[test]
    fn nested_inline_flex_child_blockifies_to_flex_container_measurement() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "200px")
            .expect("width should set");
        root.style_mut(container)
            .set("height", "40px")
            .expect("height should set");

        let inline_flex = append_div(&mut root, container);
        root.style_mut(inline_flex)
            .set("display", "inline-flex")
            .expect("inline-flex display should set");
        let inline_flex_child = append_div(&mut root, inline_flex);
        root.style_mut(inline_flex_child)
            .set("width", "12px")
            .expect("width should set");
        root.style_mut(inline_flex_child)
            .set("height", "18px")
            .expect("height should set");

        let flex_control = append_div(&mut root, container);
        root.style_mut(flex_control)
            .set("display", "flex")
            .expect("flex display should set");
        let flex_control_child = append_div(&mut root, flex_control);
        root.style_mut(flex_control_child)
            .set("width", "12px")
            .expect("width should set");
        root.style_mut(flex_control_child)
            .set("height", "18px")
            .expect("height should set");

        let items = collect_items_for_container(&mut root, container, context);

        assert_eq!(
            items.len(),
            2,
            "both children should be collected as flex items",
        );
        let inline_flex_item = &items[0];
        let flex_control_item = &items[1];
        assert_eq!(inline_flex_item.source.key(), inline_flex);
        assert_eq!(flex_control_item.source.key(), flex_control);
        // Blockification (CSS Display 3 §2.7): the nested `inline-flex` item
        // measures identically to the equivalent `display: flex` item — both
        // stretch as block-level flex containers rather than shrink-wrapping.
        assert_eq!(
            inline_flex_item.flex_base_size_px, flex_control_item.flex_base_size_px,
            "blockified inline-flex item should share the flex item's base size",
        );
        assert_eq!(
            inline_flex_item.automatic_min_main_size_px,
            flex_control_item.automatic_min_main_size_px,
            "blockified inline-flex item should share the flex item's auto-min",
        );
        assert!(inline_flex_item.flex_base_size_px > 0.0);
    }

    #[test]
    fn order_modified_item_collection_does_not_mutate_dom_sibling_order() {
        let (mut root, container, context) = flex_container_with_context();
        let first = append_div(&mut root, container);
        let second = append_div(&mut root, container);
        let third = append_div(&mut root, container);
        root.style_mut(first)
            .set("width", "10px")
            .expect("width should set");
        root.style_mut(second)
            .set("width", "10px")
            .expect("width should set");
        root.style_mut(third)
            .set("width", "10px")
            .expect("width should set");
        root.style_mut(second)
            .set("order", "2")
            .expect("order should set");
        root.style_mut(third)
            .set("order", "-1")
            .expect("order should set");

        let solve = build_solve_for_container(&mut root, container, context);
        let ordered_items = item_views_in_order_modified_order(&root.layout_scratch, solve);

        assert_eq!(
            next_sibling_key(&root, first).expect("first sibling"),
            Some(second)
        );
        assert_eq!(
            next_sibling_key(&root, second).expect("second sibling"),
            Some(third)
        );
        assert_eq!(
            ordered_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![third, first, second],
        );
    }
}
