//! Flex resolution — the §9.7 sizing machinery.
//!
//! This file owns the resolution phase: §9.2 line building, the §9.7 freeze
//! loop (seed at the floored hypothetical, one-class freeze by signed total
//! violation), base/hypothetical sizing, flex factor distribution, item cross
//! at resolved main (the refold), line cross sizing, and the main/cross packing
//! offsets written into the item/line lanes.
//!
//! Invariants and rules:
//!
//! - the freeze loop seeds every item at its floored hypothetical, never the
//!   flex base; the constructor freeze marks factor-0 and entry-violation items
//!   frozen at that hypothetical
//! - one violation class freezes per iteration, chosen by the signed total
//! - item cross sizing runs after main resolution; flagged-and-width-changed
//!   items re-fold via the measure phase's walk (cross-phase call)
//! - line cross offsets and per-item main offsets are written in-place in the
//!   lanes for the placement phase to read

use crate::types::{DomError, LaneRange, LayoutScratch, RootDom, RootDomMetricName};

use super::DomLayoutResolver;
use super::flex::{
    FlexAxis, FlexContainerSolve, FlexContainerStyle, FlexItemView, FlexLineCrossAxisAlignment,
    FlexMainAxisAlignment, cross_axis_end_margin_from_sides, cross_axis_flow_size,
    cross_axis_start_margin_from_sides, flex_axis_sizes_match, flex_line_gap_count,
    non_negative_f32_from_usize, non_negative_flex_axis_size_px, resolved_cross_axis_is_reversed,
    resolved_line_cross_axis_alignment, resolved_main_axis_alignment,
};
use super::flex_formation::blockified_flex_item_style;
use super::flex_item_lane::FlexItemLane;
use super::flex_item_source::FlexItemSource;
use super::flex_line_lane::{FlexLineEntry, FlexLineLane};
use super::flex_measure::measure_tree_walker_node_without_layout_write;
use super::flex_order_lane::FlexOrderLaneEntry;
use super::flex_resolution_lane::{FlexResolutionEntry, ViolationClass};
use super::flex_wrap_mode::FlexWrapMode;
use super::flow::{LayoutContext, resolved_node_style};

pub(crate) fn build_measured_flex_lines(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    wrap_mode: FlexWrapMode,
    container_main_size_px: Option<f32>,
    gap_between_items_px: f32,
) -> LaneRange {
    // CSS Flexbox 9.2 (#algo-line-breaking): collect items into lines by
    // adding each item's outer hypothetical main size plus the inter-item
    // gap and breaking when the line would exceed the container's inner
    // main size. This is the same greedy line break as Blink's
    // `flex_line_breaker.cc` and Gecko's line-building loop in
    // nsFlexContainerFrame.cpp (`GenerateFlexLines`).
    let order_entries = scratch.order.slice(order_range);
    let items = &scratch.items;
    let line_range_start = scratch.lines.begin_range();
    if order_entries.is_empty() {
        return scratch.lines.finish_range(line_range_start);
    }
    if !wrap_mode.is_wrapping() || container_main_size_px.is_none() {
        push_measured_flex_line(
            items,
            order_entries,
            order_range.start,
            gap_between_items_px,
            &mut scratch.lines,
        );
        return scratch.lines.finish_range(line_range_start);
    }
    let line_break_length_px = container_main_size_px.expect("line break length should exist");
    let mut line_start_index = 0usize;
    let mut line_outer_main_size_px: f32 = 0.0;
    for (index, entry) in order_entries.iter().enumerate() {
        let item = items.get(entry.dom_index).view;
        let gap_before_item_px = if line_outer_main_size_px == 0.0 {
            0.0
        } else {
            gap_between_items_px
        };
        let next_outer_main_size_px =
            line_outer_main_size_px + gap_before_item_px + item.outer_hypothetical_main_size_px;
        if line_outer_main_size_px > 0.0 && next_outer_main_size_px > line_break_length_px {
            push_measured_flex_line(
                items,
                &order_entries[line_start_index..index],
                order_range.start
                    + u32::try_from(line_start_index)
                        .expect("line start index should fit into u32"),
                gap_between_items_px,
                &mut scratch.lines,
            );
            line_start_index = index;
            line_outer_main_size_px = item.outer_hypothetical_main_size_px;
            continue;
        }
        line_outer_main_size_px += gap_before_item_px + item.outer_hypothetical_main_size_px;
    }
    push_measured_flex_line(
        items,
        &order_entries[line_start_index..],
        order_range.start
            + u32::try_from(line_start_index).expect("line start index should fit into u32"),
        gap_between_items_px,
        &mut scratch.lines,
    );
    let line_range = scratch.lines.finish_range(line_range_start);
    if wrap_mode.is_cross_reversed() {
        scratch.lines.reverse_range(line_range);
    }
    line_range
}

pub(crate) fn push_measured_flex_line(
    items: &FlexItemLane,
    order_slice: &[FlexOrderLaneEntry],
    first_order_index: u32,
    gap_between_items_px: f32,
    lines: &mut FlexLineLane,
) {
    let mut outer_hypothetical_main_size_px: f32 = 0.0;
    let mut cross_size_px: f32 = 0.0;
    for (index, entry) in order_slice.iter().enumerate() {
        if index > 0 {
            outer_hypothetical_main_size_px += gap_between_items_px;
        }
        let item = items.get(entry.dom_index).view;
        outer_hypothetical_main_size_px += item.outer_hypothetical_main_size_px;
        cross_size_px = cross_size_px.max(
            item.measured_cross_size_input_px
                + item.margin_cross_start_px
                + item.margin_cross_end_px,
        );
    }
    lines.push(FlexLineEntry {
        first_order_index,
        item_count: u32::try_from(order_slice.len())
            .expect("flex line item count should fit into u32"),
        outer_hypothetical_main_size_px,
        cross_size_px,
        cross_offset_px: 0.0,
        resolved_cross_size_px: 0.0,
        placed_cross_size_px: 0.0,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_active_flex_container<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    mut measured_solve: FlexContainerSolve,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    container_cross_size_px: Option<f32>,
    resolver: &mut R,
) -> Result<FlexContainerSolve, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let order_range = measured_solve.order_range;
    let line_range = measured_solve.line_range;
    // Claim the resolution range parallel to the order range. Per-line order
    // (flex arc step 7, D1): main resolve (§9.7) → item cross at the resolved
    // main (ratio / refold / measured) → line cross = max item outer cross →
    // align-content packs the lines. The pre-flex line-cross sizing has moved
    // AFTER main resolution and item cross sizing.
    let resolution_range = claim_resolution_range(&mut root_dom.layout_scratch, order_range);
    measured_solve.resolution_range = resolution_range;
    resolve_flex_lines(
        &mut root_dom.layout_scratch,
        order_range,
        resolution_range,
        line_range,
        container_main_size_px,
        container_style,
    );
    resolve_item_cross_sizes(
        root_dom,
        tree_size,
        child_context,
        order_range,
        resolution_range,
        line_range,
        container_style,
        resolver,
    )?;
    resolved_line_cross_sizes(
        &mut root_dom.layout_scratch,
        line_range,
        container_style,
        container_cross_size_px,
    );
    // D3 (flex arc step 10): after the post-resolution line cross, distribute
    // each item's remaining cross space equally among its auto cross margins
    // (align-items/align-self then has no effect on that item).
    resolve_flex_item_cross_auto_margins(&mut root_dom.layout_scratch, order_range, line_range);
    line_cross_offsets_px(
        &mut root_dom.layout_scratch,
        line_range,
        container_style,
        container_cross_size_px,
    );
    measured_solve.main_size_px = container_main_size_px;
    measured_solve.cross_size_px = total_line_cross_size_px_from_lane(
        &root_dom.layout_scratch,
        line_range,
        container_style.gap_sizes.between_lines_px,
    );
    Ok(measured_solve)
}

pub(crate) fn claim_resolution_range(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
) -> LaneRange {
    let start = scratch.resolution.begin_range();
    for _ in 0..order_range.len {
        scratch.resolution.push(FlexResolutionEntry {
            resolved_main_size_px: 0.0,
            is_frozen: false,
            violation: ViolationClass::None,
        });
    }
    scratch.resolution.finish_range(start)
}

/// Flex arc step 7, D1.2/D1.3: item cross at the resolved main, then the line
/// cross = max item outer cross. For each item in order-modified sequence, the
/// resolved cross is: a replaced-ratio item keeps its measured cross (the
/// ratio-derived cross lands at placement via the width override + no height
/// override); a `main_influences_cross`-flagged item whose resolved main
/// differs from its hypothetical main REFOLDS (re-measures its subtree at the
/// resolved main width, counted by `NodesRefolded`); everything else keeps
/// the measured cross. The line lane's measured `cross_size_px` is then
/// overwritten with the max resolved item cross + cross margins.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_item_cross_sizes<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    order_range: LaneRange,
    resolution_range: LaneRange,
    line_range: LaneRange,
    container_style: FlexContainerStyle,
    resolver: &mut R,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    // Phase 1: refold flagged + width-changed items. Per-item scoped borrows —
    // read the item's info, drop the scratch borrow, re-measure the subtree,
    // then write the resolved cross back. The resolution lane is parallel to
    // the order lane (same relative index), NOT the same absolute index.
    let order_len =
        usize::try_from(order_range.len).expect("order range len should fit into usize");
    let mut order_index = 0usize;
    while order_index < order_len {
        let (dom_index, needs_refold, source, resolved_main_content_px) = {
            let order_entries = root_dom.layout_scratch.order.slice(order_range);
            let order_entry = order_entries[order_index];
            let item_entry = root_dom.layout_scratch.items.get(order_entry.dom_index);
            let view = item_entry.view;
            let resolution_index = resolution_range.start
                + u32::try_from(order_index).expect("order index should fit into u32");
            let resolved_main = root_dom
                .layout_scratch
                .resolution
                .get(resolution_index)
                .resolved_main_size_px;
            let needs = view.main_influences_cross
                && !flex_axis_sizes_match(resolved_main, view.hypothetical_main_size_px);
            (
                order_entry.dom_index,
                needs,
                view.source,
                resolved_main - view.main_axis_border_padding_px,
            )
        };
        order_index += 1;
        if !needs_refold {
            continue;
        }
        let refolded_cross = refold_item_at_resolved_main(
            root_dom,
            tree_size,
            child_context,
            source,
            resolved_main_content_px,
            container_style.main_axis,
            resolver,
        )?;
        root_dom
            .layout_scratch
            .items
            .get_mut(dom_index)
            .view
            .resolved_cross_size_px = refolded_cross;
        root_dom.increment_metric(RootDomMetricName::NodesRefolded);
    }
    // Phase 2: line cross = max resolved item cross + cross margins.
    update_line_cross_sizes(&mut root_dom.layout_scratch, order_range, line_range);
    Ok(())
}

/// Flex arc step 7, D1.3: overwrites each line's measured `cross_size_px` with
/// the max resolved item cross + cross margins (the item cross is
/// `view.resolved_cross_size_px`, seeded to the measured cross and overwritten
/// by the refold for flagged + width-changed items).
pub(crate) fn update_line_cross_sizes(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    line_range: LaneRange,
) {
    let order_entries = scratch.order.slice(order_range);
    let line_count = usize::try_from(line_range.len).expect("line range len should fit into usize");
    let mut line_pos = 0usize;
    while line_pos < line_count {
        let line = *scratch
            .lines
            .get(line_range.start + u32::try_from(line_pos).expect("line pos fits u32"));
        let start = usize::try_from(line.first_order_index - order_range.start)
            .expect("line first order index should fall within the container's order range");
        let count =
            usize::try_from(line.item_count).expect("line item count should fit into usize");
        let mut max_cross_px = 0.0f32;
        for order_entry in &order_entries[start..start + count] {
            let view = scratch.items.get(order_entry.dom_index).view;
            max_cross_px = max_cross_px.max(
                view.resolved_cross_size_px + view.margin_cross_start_px + view.margin_cross_end_px,
            );
        }
        scratch.lines.slice_mut(line_range)[line_pos].cross_size_px = max_cross_px;
        line_pos += 1;
    }
}

/// Re-measures one flagged flex item's subtree at its resolved main content
/// width (the D1.2 refold): text re-shapes at the new width and the subtree's
/// records update (equality-gated). Returns the item's resolved cross size in
/// BORDER-BOX terms — content + border/padding, no cross margins — matching
/// the measured seed (`measured_cross_size_px` in `measure_one_flex_item`) and
/// every cross-size consumer, which add the cross margins on top.
pub(crate) fn refold_item_at_resolved_main<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    child_context: &LayoutContext,
    source: FlexItemSource,
    resolved_main_content_px: f32,
    main_axis: FlexAxis,
    resolver: &mut R,
) -> Result<f32, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let key = source.key();
    let (child_style, child_inherited_style) = resolved_node_style(root_dom, key, child_context)?;
    let child_style = blockified_flex_item_style(child_style);
    // `cross_axis_flow_size` returns the folded `FlowSize`'s cross axis, which
    // `layout_flow_size` defines as the OUTER size (border box + cross margins).
    // The seed and every cross consumer treat `resolved_cross_size_px` as
    // border-box (they add the cross margins themselves), so subtract the cross
    // margins the flow included to keep the terms consistent and avoid
    // double-counting them (refold cross stays border-box, consistent with the
    // measured seed).
    let cross_margins_px = cross_axis_start_margin_from_sides(child_style.sides, main_axis)
        + cross_axis_end_margin_from_sides(child_style.sides, main_axis);
    let refold_context = LayoutContext {
        available_width_px: Some(resolved_main_content_px.max(0.0)),
        ..child_context.clone()
    };
    let child_flow = measure_tree_walker_node_without_layout_write(
        root_dom,
        tree_size,
        key,
        &refold_context,
        Some(resolved_main_content_px.max(0.0)),
        child_style,
        child_inherited_style,
        resolver,
        RootDomMetricName::FlexNodesMeasured,
    )?;
    Ok(cross_axis_flow_size(main_axis, child_flow) - cross_margins_px)
}

pub(crate) fn resolved_line_cross_sizes(
    scratch: &mut LayoutScratch,
    line_range: LaneRange,
    container_style: FlexContainerStyle,
    container_cross_size_px: Option<f32>,
) {
    // CSS Flexbox 9.4: single-line containers always give their one line the
    // container's inner cross size (#algo-cross-line); multi-line containers
    // with `align-content: stretch` distribute the leftover free space
    // equally into each line's cross size (#algo-line-stretch). The leftover
    // subtracts the inter-line gaps: Gecko's `CrossAxisPositionTracker`
    // constructor in nsFlexContainerFrame.cpp and Blink's
    // flex_layout_algorithm.cc both compute
    // `content_size - sum(line cross sizes) - gap * (line count - 1)`
    // before stretching.
    let lines = scratch.lines.slice_mut(line_range);
    if !container_style.wrap_mode.is_wrapping()
        && let Some(container_cross_size_px) = container_cross_size_px
    {
        for line in lines.iter_mut() {
            line.resolved_cross_size_px = container_cross_size_px;
        }
        return;
    }
    for line in lines.iter_mut() {
        line.resolved_cross_size_px = line.cross_size_px;
    }
    if !container_style.wrap_mode.is_wrapping()
        || resolved_line_cross_axis_alignment(container_style.align_content, container_style)
            != FlexLineCrossAxisAlignment::Stretch
    {
        return;
    }
    let Some(container_cross_size_px) = container_cross_size_px else {
        return;
    };
    if lines.is_empty() {
        return;
    }
    let remaining_cross_space_px = container_cross_size_px
        - total_line_cross_size_px(lines, container_style.gap_sizes.between_lines_px);
    if remaining_cross_space_px <= 0.0 {
        return;
    }
    let extra_cross_size_per_line_px =
        remaining_cross_space_px / non_negative_f32_from_usize(lines.len());
    for line in lines.iter_mut() {
        line.resolved_cross_size_px += extra_cross_size_per_line_px;
    }
}

/// CSS Flexbox §8.1 (D3, flex arc step 10): after the post-resolution line
/// cross, each item's remaining cross space distributes equally among that
/// item's auto CROSS margins (`free_cross = line_cross − item.outer_cross`;
/// `outer_cross` includes the cross margins, still auto=0 here). Negative/zero
/// free space leaves the auto cross margins at their 0.0 placeholder and
/// align-items/align-self proceeds as today. Placement reads the absorbed
/// values (via the auto flags) for the item's cross position and box model.
pub(crate) fn resolve_flex_item_cross_auto_margins(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    line_range: LaneRange,
) {
    let order_entries = scratch.order.slice(order_range);
    let line_count = usize::try_from(line_range.len).expect("line range len should fit into usize");
    let mut line_pos = 0usize;
    while line_pos < line_count {
        let line = *scratch
            .lines
            .get(line_range.start + u32::try_from(line_pos).expect("line pos fits u32"));
        let start = usize::try_from(line.first_order_index - order_range.start)
            .expect("line first order index should fall within the container's order range");
        let count =
            usize::try_from(line.item_count).expect("line item count should fit into usize");
        for order_entry in &order_entries[start..start + count] {
            let view = scratch.items.get(order_entry.dom_index).view;
            let mut auto_count = 0usize;
            if view.margin_cross_start_auto {
                auto_count += 1;
            }
            if view.margin_cross_end_auto {
                auto_count += 1;
            }
            if auto_count == 0 {
                continue;
            }
            let free_cross_px = line.resolved_cross_size_px
                - (view.resolved_cross_size_px
                    + view.margin_cross_start_px
                    + view.margin_cross_end_px);
            if free_cross_px <= 0.0 {
                continue;
            }
            let share_px = free_cross_px / non_negative_f32_from_usize(auto_count);
            let view = &mut scratch.items.get_mut(order_entry.dom_index).view;
            if view.margin_cross_start_auto {
                view.margin_cross_start_px += share_px;
            }
            if view.margin_cross_end_auto {
                view.margin_cross_end_px += share_px;
            }
        }
        line_pos += 1;
    }
}

pub(crate) fn line_cross_offsets_px(
    scratch: &mut LayoutScratch,
    line_range: LaneRange,
    container_style: FlexContainerStyle,
    container_cross_size_px: Option<f32>,
) {
    let lines = scratch.lines.slice_mut(line_range);
    let remaining_cross_space_px = remaining_cross_axis_line_pack_space_px(
        lines,
        container_style.gap_sizes.between_lines_px,
        container_cross_size_px,
        container_style.wrap_mode,
    );
    let mut next_line_cross_offset_px = initial_cross_axis_line_pack_offset_px(
        container_style,
        remaining_cross_space_px,
        lines.len(),
    );
    let extra_space_between_lines_px = extra_cross_axis_space_between_lines_px(
        resolved_line_cross_axis_alignment(container_style.align_content, container_style),
        remaining_cross_space_px,
        lines.len(),
    );
    for line in lines.iter_mut() {
        line.cross_offset_px = next_line_cross_offset_px;
        next_line_cross_offset_px += line.resolved_cross_size_px
            + container_style.gap_sizes.between_lines_px
            + extra_space_between_lines_px;
    }
}

pub(crate) fn remaining_cross_axis_line_pack_space_px(
    lines: &[FlexLineEntry],
    gap_between_lines_px: f32,
    container_cross_size_px: Option<f32>,
    wrap_mode: FlexWrapMode,
) -> f32 {
    if !wrap_mode.is_wrapping() {
        return 0.0;
    }
    let Some(container_cross_size_px) = container_cross_size_px else {
        return 0.0;
    };
    (container_cross_size_px - total_line_cross_size_px(lines, gap_between_lines_px)).max(0.0)
}

pub(crate) fn initial_cross_axis_line_pack_offset_px(
    container_style: FlexContainerStyle,
    remaining_cross_space_px: f32,
    line_count: usize,
) -> f32 {
    let align_content =
        resolved_line_cross_axis_alignment(container_style.align_content, container_style);
    let is_cross_reversed = resolved_cross_axis_is_reversed(container_style);
    match align_content {
        FlexLineCrossAxisAlignment::Start | FlexLineCrossAxisAlignment::End => {
            panic!("line pack alignment should resolve logical start/end before placement")
        }
        FlexLineCrossAxisAlignment::FlexStart | FlexLineCrossAxisAlignment::Stretch => {
            if is_cross_reversed {
                remaining_cross_space_px
            } else {
                0.0
            }
        }
        FlexLineCrossAxisAlignment::FlexEnd => {
            if is_cross_reversed {
                0.0
            } else {
                remaining_cross_space_px
            }
        }
        FlexLineCrossAxisAlignment::Center => remaining_cross_space_px * 0.5,
        FlexLineCrossAxisAlignment::SpaceBetween => {
            if remaining_cross_space_px > 0.0 && line_count > 1 {
                return 0.0;
            }
            if is_cross_reversed {
                remaining_cross_space_px
            } else {
                0.0
            }
        }
        FlexLineCrossAxisAlignment::SpaceAround => {
            if line_count == 0 {
                return 0.0;
            }
            if remaining_cross_space_px <= 0.0 {
                return remaining_cross_space_px * 0.5;
            }
            remaining_cross_space_px / (2.0 * non_negative_f32_from_usize(line_count))
        }
        FlexLineCrossAxisAlignment::SpaceEvenly => {
            if line_count == 0 {
                return 0.0;
            }
            remaining_cross_space_px / non_negative_f32_from_usize(line_count.saturating_add(1))
        }
    }
}

pub(crate) fn extra_cross_axis_space_between_lines_px(
    align_content: FlexLineCrossAxisAlignment,
    remaining_cross_space_px: f32,
    line_count: usize,
) -> f32 {
    if remaining_cross_space_px <= 0.0 || line_count <= 1 {
        return 0.0;
    }
    match align_content {
        FlexLineCrossAxisAlignment::SpaceBetween => {
            remaining_cross_space_px / non_negative_f32_from_usize(line_count - 1)
        }
        FlexLineCrossAxisAlignment::SpaceAround => {
            remaining_cross_space_px / non_negative_f32_from_usize(line_count)
        }
        FlexLineCrossAxisAlignment::SpaceEvenly => {
            remaining_cross_space_px / non_negative_f32_from_usize(line_count.saturating_add(1))
        }
        FlexLineCrossAxisAlignment::Start | FlexLineCrossAxisAlignment::End => {
            panic!("line pack alignment should resolve logical start/end before spacing")
        }
        FlexLineCrossAxisAlignment::FlexStart
        | FlexLineCrossAxisAlignment::Stretch
        | FlexLineCrossAxisAlignment::FlexEnd
        | FlexLineCrossAxisAlignment::Center => 0.0,
    }
}

pub(crate) fn total_line_cross_size_px(lines: &[FlexLineEntry], gap_between_lines_px: f32) -> f32 {
    let gap_count = flex_line_gap_count(lines.len());
    let gap_total_px = gap_between_lines_px * non_negative_f32_from_usize(gap_count);
    non_negative_flex_axis_size_px(
        lines
            .iter()
            .map(|line| line.resolved_cross_size_px)
            .sum::<f32>()
            + gap_total_px,
    )
}

pub(crate) fn total_line_cross_size_px_from_lane(
    scratch: &LayoutScratch,
    line_range: LaneRange,
    gap_between_lines_px: f32,
) -> f32 {
    total_line_cross_size_px(scratch.lines.slice(line_range), gap_between_lines_px)
}

pub(crate) fn resolve_flex_lines(
    scratch: &mut LayoutScratch,
    order_range: LaneRange,
    resolution_range: LaneRange,
    line_range: LaneRange,
    container_main_size_px: f32,
    container_style: FlexContainerStyle,
) {
    let order_entries = scratch.order.slice(order_range);
    let line_entries = scratch.lines.slice(line_range);
    let resolution_entries = scratch.resolution.slice_mut(resolution_range);
    for (line_offset, line) in line_entries.iter().enumerate() {
        let line_start = usize::try_from(line.first_order_index - order_range.start)
            .expect("line first order index should fall within the container's order range");
        let item_count =
            usize::try_from(line.item_count).expect("line item count should fit into usize");
        let line_order = &order_entries[line_start..line_start + item_count];
        let line_resolution = &mut resolution_entries[line_start..line_start + item_count];
        resolve_flex_line_item_resolution_states(
            line_order,
            &scratch.items,
            line_resolution,
            container_main_size_px,
            container_style.gap_sizes.between_items_px,
        );
        assign_flex_line_main_offsets(
            line_order,
            &mut scratch.items,
            line_resolution,
            container_main_size_px,
            container_style,
            line_range.start + u32::try_from(line_offset).expect("line offset should fit into u32"),
        );
    }
}

pub(crate) fn assign_flex_line_main_offsets(
    line_order: &[FlexOrderLaneEntry],
    items: &mut FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    container_main_size_px: f32,
    container_style: FlexContainerStyle,
    line_index: u32,
) {
    if line_order.is_empty() {
        return;
    }
    let gap_between_items_px = container_style.gap_sizes.between_items_px;
    let line_main_size_px =
        resolved_flex_line_main_size_px(line_order, items, line_resolution, gap_between_items_px);
    let free_space_px = container_main_size_px - line_main_size_px;
    // CSS Flexbox §8.1 (D2, flex arc step 10): after §9.7, positive free space
    // distributes EQUALLY among the line's auto MAIN margins, and
    // justify-content has no effect (the auto margins take all of it). The free
    // space is the §9.7 value above (auto margins are still 0 here), reused
    // without recompute. Negative/zero free space leaves the auto margins at
    // their 0.0 placeholder and justify proceeds as today.
    let mut auto_main_margin_count = 0usize;
    for order_entry in line_order {
        let view = items.get(order_entry.dom_index).view;
        if view.margin_main_start_auto {
            auto_main_margin_count += 1;
        }
        if view.margin_main_end_auto {
            auto_main_margin_count += 1;
        }
    }
    let absorbs_free_space = free_space_px > 0.0 && auto_main_margin_count > 0;
    if absorbs_free_space {
        let share_px = free_space_px / non_negative_f32_from_usize(auto_main_margin_count);
        for order_entry in line_order {
            let view = &mut items.get_mut(order_entry.dom_index).view;
            if view.margin_main_start_auto {
                view.margin_main_start_px += share_px;
            }
            if view.margin_main_end_auto {
                view.margin_main_end_px += share_px;
            }
        }
    }
    let mut main_offset_px = if absorbs_free_space {
        0.0
    } else {
        initial_main_axis_alignment_offset_px(container_style, free_space_px, line_order.len())
    };
    let extra_space_between_items_px = if absorbs_free_space {
        0.0
    } else {
        extra_main_axis_space_between_items_px(
            resolved_main_axis_alignment(container_style.justify_content, container_style),
            free_space_px,
            line_order.len(),
        )
    };
    for (index, order_entry) in line_order.iter().enumerate() {
        let view = items.get(order_entry.dom_index).view;
        let resolved_main_size_px = line_resolution[index].resolved_main_size_px;
        let entry = items.get_mut(order_entry.dom_index);
        entry.main_offset_px = main_offset_px;
        entry.line_index = line_index;
        main_offset_px +=
            view.margin_main_start_px + resolved_main_size_px + view.margin_main_end_px;
        if index + 1 < line_order.len() {
            main_offset_px += gap_between_items_px + extra_space_between_items_px;
        }
    }
}

pub(crate) fn resolved_flex_line_main_size_px(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    gap_between_items_px: f32,
) -> f32 {
    let gap_count = flex_line_gap_count(line_order.len());
    let resolved_item_main_size_px = line_order
        .iter()
        .enumerate()
        .map(|(index, order_entry)| {
            let view = items.get(order_entry.dom_index).view;
            view.margin_main_start_px
                + line_resolution[index].resolved_main_size_px
                + view.margin_main_end_px
        })
        .sum::<f32>();
    resolved_item_main_size_px + gap_between_items_px * non_negative_f32_from_usize(gap_count)
}

pub(crate) fn initial_main_axis_alignment_offset_px(
    container_style: FlexContainerStyle,
    remaining_main_space_px: f32,
    item_count: usize,
) -> f32 {
    let justify_content =
        resolved_main_axis_alignment(container_style.justify_content, container_style);
    match justify_content {
        FlexMainAxisAlignment::Start | FlexMainAxisAlignment::End => {
            panic!("main-axis alignment should resolve logical start/end before placement")
        }
        FlexMainAxisAlignment::FlexStart => 0.0,
        FlexMainAxisAlignment::FlexEnd => remaining_main_space_px,
        FlexMainAxisAlignment::Center => remaining_main_space_px.max(0.0) / 2.0,
        FlexMainAxisAlignment::SpaceBetween => {
            if item_count <= 1 || remaining_main_space_px <= 0.0 {
                return 0.0;
            }
            0.0
        }
        FlexMainAxisAlignment::SpaceAround => {
            if item_count == 0 {
                return 0.0;
            }
            remaining_main_space_px.max(0.0) / (2.0 * non_negative_f32_from_usize(item_count))
        }
        FlexMainAxisAlignment::SpaceEvenly => {
            if item_count == 0 {
                return 0.0;
            }
            remaining_main_space_px.max(0.0)
                / non_negative_f32_from_usize(item_count.saturating_add(1))
        }
    }
}

pub(crate) fn extra_main_axis_space_between_items_px(
    justify_content: FlexMainAxisAlignment,
    remaining_main_space_px: f32,
    item_count: usize,
) -> f32 {
    if remaining_main_space_px <= 0.0 || item_count <= 1 {
        return 0.0;
    }
    match justify_content {
        FlexMainAxisAlignment::SpaceBetween => {
            remaining_main_space_px / non_negative_f32_from_usize(item_count - 1)
        }
        FlexMainAxisAlignment::SpaceAround => {
            remaining_main_space_px / non_negative_f32_from_usize(item_count)
        }
        FlexMainAxisAlignment::SpaceEvenly => {
            remaining_main_space_px / non_negative_f32_from_usize(item_count.saturating_add(1))
        }
        FlexMainAxisAlignment::Start | FlexMainAxisAlignment::End => {
            panic!("main-axis alignment should resolve logical start/end before spacing")
        }
        FlexMainAxisAlignment::FlexStart
        | FlexMainAxisAlignment::FlexEnd
        | FlexMainAxisAlignment::Center => 0.0,
    }
}

pub(crate) fn resolve_flex_line_item_resolution_states(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
    container_main_size_px: f32,
    gap_between_items_px: f32,
) {
    // CSS Flexbox 9.7 (#resolve-flexible-lengths): seed at the floored
    // hypothetical, freeze the constructor's inflexible items AT hypothetical,
    // then loop the free-space distribution with per-item min/max/auto-min
    // violation clamping and a ONE-CLASS freeze per iteration chosen by the
    // signed total violation. Blink mirrors this in
    // `LineFlexer::ResolveFlexibleLengths`/`FreezeItems` (line_flexer.cc) and
    // Gecko in `FlexLine::ResolveFlexibleLengths` (nsFlexContainerFrame.cpp).
    let is_using_flex_grow = flex_line_uses_flex_grow(
        line_order,
        items,
        container_main_size_px,
        gap_between_items_px,
    );
    // D2: seed at hypothetical, then the constructor freeze (factor-0 + entry
    // violations) marks items frozen AT that hypothetical (never base).
    initialize_flex_line_resolution(line_order, items, line_resolution);
    freeze_flex_line_inflexible_items(line_order, items, line_resolution, is_using_flex_grow);
    let initial_free_space_px = remaining_flex_line_free_space(
        line_order,
        items,
        line_resolution,
        container_main_size_px,
        gap_between_items_px,
    );
    loop {
        // D2.3: a fully-ctor-frozen line resolves to its seeded hypotheticals —
        // the violation clamp IS the constructor freeze, nothing is skipped.
        if line_resolution.iter().all(|entry| entry.is_frozen) {
            return;
        }
        // D3 FreezeItems reset: unfrozen items return to hypothetical before
        // the next distribution; frozen items keep their current (clamped)
        // value, which the free-space recompute below reads as their
        // contribution (Blink `FlexedMarginBoxSize`, line_flexer.cc:64).
        reset_unfrozen_flex_line_resolution(line_order, items, line_resolution);
        let remaining_free_space_px = remaining_flex_line_free_space(
            line_order,
            items,
            line_resolution,
            container_main_size_px,
            gap_between_items_px,
        );
        let remaining_free_space_px = remaining_flex_line_free_space_limited_by_factor_sum(
            line_order,
            items,
            line_resolution,
            initial_free_space_px,
            remaining_free_space_px,
            is_using_flex_grow,
        );
        // D3 no-free-space early exits (engine shape).
        if is_using_flex_grow && remaining_free_space_px <= 0.0 {
            return;
        }
        if !is_using_flex_grow && remaining_free_space_px >= 0.0 {
            return;
        }
        if !distribute_flex_line_free_space(
            line_order,
            items,
            line_resolution,
            remaining_free_space_px,
            is_using_flex_grow,
        ) {
            return;
        }
        // D3 clamp phase + one-class freeze: `false` means the total violation
        // is zero (every remaining item fits its candidate) — resolve and stop.
        if !freeze_flex_line_violation_class(line_order, items, line_resolution) {
            return;
        }
        // One class froze; re-iterate.
    }
}

pub(crate) fn flex_line_uses_flex_grow(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    container_main_size_px: f32,
    gap_between_items_px: f32,
) -> bool {
    let total_outer_hypothetical_main_size_px: f32 = line_order
        .iter()
        .map(|entry| {
            items
                .get(entry.dom_index)
                .view
                .outer_hypothetical_main_size_px
        })
        .sum();
    total_outer_hypothetical_main_size_px
        + flex_line_gap_total_px(line_order.len(), gap_between_items_px)
        < container_main_size_px
}

pub(crate) fn initialize_flex_line_resolution(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
) {
    // D2 (flex arc step 5): seed every item at its (D1-floored) hypothetical
    // main size, NOT the flex base. The constructor freeze below marks factor-0
    // and entry-violation items frozen AT this hypothetical, so the §9.7 step-3
    // clamp the hypothetical carries is never skipped.
    for (entry, resolution) in line_order.iter().zip(line_resolution.iter_mut()) {
        let item = items.get(entry.dom_index).view;
        resolution.resolved_main_size_px = item.hypothetical_main_size_px;
        resolution.is_frozen = false;
        resolution.violation = ViolationClass::None;
    }
}

/// D3's `FreezeItems` reset (Blink `line_flexer.cc:69`): unfrozen items return to
/// their hypothetical main size before the next distribution. The reset is
/// transient — the free-space recompute reads BASE for unfrozen items and the
/// distribution overwrites the value with `base + share` — but keeping it here
/// preserves the engine's loop shape so a future change to the free-space
/// computation cannot silently read a stale clamped value for an unfrozen item.
pub(crate) fn reset_unfrozen_flex_line_resolution(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
) {
    for (entry, resolution) in line_order.iter().zip(line_resolution.iter_mut()) {
        if resolution.is_frozen {
            continue;
        }
        let item = items.get(entry.dom_index).view;
        resolution.resolved_main_size_px = item.hypothetical_main_size_px;
    }
}

pub(crate) fn freeze_flex_line_inflexible_items(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
    is_using_flex_grow: bool,
) {
    for (entry, resolution) in line_order.iter().zip(line_resolution.iter_mut()) {
        let item = items.get(entry.dom_index).view;
        if flex_line_item_is_inflexible(item, is_using_flex_grow) {
            resolution.is_frozen = true;
        }
    }
}

pub(crate) fn flex_line_item_is_inflexible(item: FlexItemView, is_using_flex_grow: bool) -> bool {
    if is_using_flex_grow {
        if item.flex_grow_factor == 0.0 {
            return true;
        }
        return item.flex_base_size_px > item.hypothetical_main_size_px;
    }
    if item.flex_shrink_factor == 0.0 {
        return true;
    }
    item.flex_base_size_px < item.hypothetical_main_size_px
}

pub(crate) fn remaining_flex_line_free_space(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    container_main_size_px: f32,
    gap_between_items_px: f32,
) -> f32 {
    let total_resolved_main_size_px: f32 = line_order
        .iter()
        .zip(line_resolution.iter())
        .map(|(entry, resolution)| {
            let item = items.get(entry.dom_index).view;
            let main_size_px = if resolution.is_frozen {
                resolution.resolved_main_size_px
            } else {
                item.flex_base_size_px
            };
            main_size_px + item.margin_main_start_px + item.margin_main_end_px
        })
        .sum();
    container_main_size_px
        - total_resolved_main_size_px
        - flex_line_gap_total_px(line_order.len(), gap_between_items_px)
}

pub(crate) fn flex_line_gap_total_px(item_count: usize, gap_between_items_px: f32) -> f32 {
    gap_between_items_px * non_negative_f32_from_usize(flex_line_gap_count(item_count))
}

pub(crate) fn distribute_flex_line_free_space(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
    remaining_free_space_px: f32,
    is_using_flex_grow: bool,
) -> bool {
    let flex_factor_sum =
        unfrozen_flex_line_factor_sum(line_order, items, line_resolution, is_using_flex_grow);
    if flex_factor_sum <= 0.0 {
        return false;
    }
    for (entry, resolution) in line_order.iter().zip(line_resolution.iter_mut()) {
        if resolution.is_frozen {
            continue;
        }
        let item = items.get(entry.dom_index).view;
        let flex_factor = flex_line_flex_factor(item, is_using_flex_grow);
        let size_delta_px = remaining_free_space_px * (flex_factor / flex_factor_sum);
        resolution.resolved_main_size_px = item.flex_base_size_px + size_delta_px;
    }
    true
}

pub(crate) fn remaining_flex_line_free_space_limited_by_factor_sum(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    initial_free_space_px: f32,
    remaining_free_space_px: f32,
    is_using_flex_grow: bool,
) -> f32 {
    let flex_factor_sum =
        unfrozen_raw_flex_line_factor_sum(line_order, items, line_resolution, is_using_flex_grow);
    if !(0.0..1.0).contains(&flex_factor_sum) {
        return remaining_free_space_px;
    }
    let fractional_free_space_px = initial_free_space_px * flex_factor_sum;
    if fractional_free_space_px.abs() < remaining_free_space_px.abs() {
        return fractional_free_space_px;
    }
    remaining_free_space_px
}

pub(crate) fn freeze_flex_line_violation_class(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &mut [FlexResolutionEntry],
) -> bool {
    // The §9.7 clamp phase + one-class freeze (Blink line_flexer.cc:140-181):
    // clamp every unfrozen item's distribution candidate, record its violation
    // class (`None` | `Min` | `Max`), sum the signed violations, and freeze
    // exactly ONE class by the total's sign (Min when ≥ 0, Max when < 0).
    // Returns `false` when the total violation is zero — every remaining item
    // fits its candidate, so the line is resolved; `true` when one class froze
    // (re-iterate).
    let mut total_violation_px = 0.0f32;
    for (entry, resolution) in line_order.iter().zip(line_resolution.iter_mut()) {
        if resolution.is_frozen {
            continue;
        }
        let item = items.get(entry.dom_index).view;
        let candidate = resolution.resolved_main_size_px;
        let clamped = clamped_main_size_after_flex_constraints(item, candidate);
        resolution.resolved_main_size_px = clamped;
        resolution.violation = if clamped > candidate {
            ViolationClass::Min
        } else if clamped < candidate {
            ViolationClass::Max
        } else {
            ViolationClass::None
        };
        total_violation_px += clamped - candidate;
    }
    if flex_axis_sizes_match(total_violation_px, 0.0) {
        return false;
    }
    let freeze_class = if total_violation_px >= 0.0 {
        ViolationClass::Min
    } else {
        ViolationClass::Max
    };
    for resolution in line_resolution.iter_mut() {
        if !resolution.is_frozen && resolution.violation == freeze_class {
            resolution.is_frozen = true;
        }
    }
    true
}

pub(crate) fn clamped_main_size_after_flex_constraints(
    item: FlexItemView,
    resolved_main_size_px: f32,
) -> f32 {
    // The automatic minimum is CONTENT-BOX while `resolved_main_size_px` is
    // border-box, so the floor adds the main-axis border+padding to convert
    // auto-min into the border-box terms of the value it floors. The separate
    // `.max(border_padding)` stays: it guards the content box at zero (a
    // negative-content resolution), independent of the auto-min floor.
    let mut clamped_main_size_px = resolved_main_size_px
        .max(item.automatic_min_main_size_px + item.main_axis_border_padding_px)
        .max(item.main_axis_border_padding_px);
    if let Some(min_main_size_px) = item.min_main_size_px {
        clamped_main_size_px = clamped_main_size_px.max(min_main_size_px);
    }
    if let Some(max_main_size_px) = item.max_main_size_px {
        clamped_main_size_px = clamped_main_size_px.min(max_main_size_px);
    }
    clamped_main_size_px
}

pub(crate) fn unfrozen_flex_line_factor_sum(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    is_using_flex_grow: bool,
) -> f32 {
    line_order
        .iter()
        .zip(line_resolution.iter())
        .filter(|(_, resolution)| !resolution.is_frozen)
        .map(|(entry, _)| {
            flex_line_flex_factor(items.get(entry.dom_index).view, is_using_flex_grow)
        })
        .sum()
}

pub(crate) fn unfrozen_raw_flex_line_factor_sum(
    line_order: &[FlexOrderLaneEntry],
    items: &FlexItemLane,
    line_resolution: &[FlexResolutionEntry],
    is_using_flex_grow: bool,
) -> f32 {
    line_order
        .iter()
        .zip(line_resolution.iter())
        .filter(|(_, resolution)| !resolution.is_frozen)
        .map(|(entry, _)| {
            raw_flex_line_flex_factor(items.get(entry.dom_index).view, is_using_flex_grow)
        })
        .sum()
}

pub(crate) fn raw_flex_line_flex_factor(item: FlexItemView, is_using_flex_grow: bool) -> f32 {
    if is_using_flex_grow {
        return item.flex_grow_factor;
    }
    item.flex_shrink_factor
}

pub(crate) fn flex_line_flex_factor(item: FlexItemView, is_using_flex_grow: bool) -> f32 {
    if is_using_flex_grow {
        return item.flex_grow_factor;
    }
    item.flex_shrink_factor * item.flex_base_size_px
}
#[cfg(test)]
use crate::domains::layout::flex::{
    MeasuredRowItemBoxEdges, TestLine, append_div, build_lines_from_items,
    build_solve_for_container, built_lines, flex_container_with_context, measured_row_item,
    measured_row_item_with_automatic_min, measured_row_item_with_box_edges,
    measured_row_item_with_limits, resolve_active_flex_container_test,
    resolved_items_in_order_modified_order, resolved_line_cross_sizes_in_solve,
    row_container_style, solve_from_items_and_lines, source, test_keys, wrap_row_container_style,
};

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn line_membership_uses_outer_hypothetical_main_sizes() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("flex-wrap", "wrap")
            .expect("wrap should set");
        root.style_mut(container)
            .set("width", "50px")
            .expect("width should set");
        let first = append_div(&mut root, container);
        let second = append_div(&mut root, container);
        let third = append_div(&mut root, container);
        root.style_mut(first)
            .set("width", "30px")
            .expect("width should set");
        root.style_mut(first)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(second)
            .set("width", "30px")
            .expect("width should set");
        root.style_mut(second)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(third)
            .set("width", "20px")
            .expect("width should set");
        root.style_mut(third)
            .set("height", "10px")
            .expect("height should set");

        let solve = build_solve_for_container(&mut root, container, context);
        let lines = built_lines(&root.layout_scratch, solve.order_range, solve.line_range);

        assert_eq!(
            lines,
            vec![
                TestLine {
                    item_sources: vec![source(first)],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(second), source(third)],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
    }

    #[test]
    fn gap_is_included_in_line_breaking_only_once_per_adjacency() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 10.0, 10.0),
        ];

        let mut scratch = LayoutScratch::default();
        let lines =
            build_lines_from_items(&mut scratch, &items, FlexWrapMode::Wrap, Some(25.0), 5.0);

        assert_eq!(
            lines,
            vec![
                TestLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[2])],
                    outer_hypothetical_main_size_px: 10.0,
                    cross_size_px: 10.0,
                },
            ],
        );
    }

    #[test]
    fn no_wrap_keeps_measured_items_in_a_single_line() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];

        let mut scratch = LayoutScratch::default();
        let lines =
            build_lines_from_items(&mut scratch, &items, FlexWrapMode::NoWrap, Some(50.0), 0.0);

        assert_eq!(
            lines,
            vec![TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>(),
                outer_hypothetical_main_size_px: 80.0,
                cross_size_px: 10.0,
            }],
        );
    }

    #[test]
    fn line_membership_is_stable_across_repeated_solves_with_unchanged_inputs() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];

        let mut first_scratch = LayoutScratch::default();
        let mut second_scratch = LayoutScratch::default();
        let first_lines = build_lines_from_items(
            &mut first_scratch,
            &items,
            FlexWrapMode::Wrap,
            Some(50.0),
            0.0,
        );
        let second_lines = build_lines_from_items(
            &mut second_scratch,
            &items,
            FlexWrapMode::Wrap,
            Some(50.0),
            0.0,
        );

        assert_eq!(first_lines, second_lines);
    }

    #[test]
    fn wrap_reverse_reverses_collected_line_order_without_reordering_items_within_a_line() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 10.0, 10.0),
        ];

        let mut scratch = LayoutScratch::default();
        let lines = build_lines_from_items(
            &mut scratch,
            &items,
            FlexWrapMode::WrapReverse,
            Some(25.0),
            5.0,
        );

        assert_eq!(
            lines,
            vec![
                TestLine {
                    item_sources: vec![source(keys[2])],
                    outer_hypothetical_main_size_px: 10.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 10.0,
                },
            ],
        );
    }

    #[test]
    fn resolve_active_flex_container_assigns_cross_offsets_per_line() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            wrap_row_container_style(),
            50.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            keys,
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 0.0, 30.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 10.0, 10.0],
        );
        assert_eq!(solve.cross_size_px, 20.0);
    }

    #[test]
    fn resolve_active_flex_container_centers_wrapped_lines_inside_definite_cross_size() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::Center;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![10.0, 20.0, 20.0],
        );
        assert_eq!(solve.cross_size_px, 20.0);
    }

    #[test]
    fn resolve_active_flex_container_packs_wrapped_lines_to_cross_end() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::FlexEnd;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![20.0, 30.0, 30.0],
        );
        assert_eq!(solve.cross_size_px, 20.0);
    }

    #[test]
    fn resolve_active_flex_container_distributes_remaining_cross_space_between_wrapped_lines() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceBetween;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 30.0, 30.0],
        );
        assert_eq!(solve.cross_size_px, 20.0);
    }

    #[test]
    fn resolve_active_flex_container_packs_items_to_main_end() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: vec![source(keys[0]), source(keys[1])],
                outer_hypothetical_main_size_px: 20.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::FlexEnd;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            60.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![40.0, 50.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_remaining_main_space_between_items() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 30.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::SpaceBetween;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            60.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 25.0, 50.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_main_space_around_items() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 30.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::SpaceAround;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            100.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![35.0 / 3.0, 45.0, 235.0 / 3.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_cross_space_around_wrapped_lines() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceAround;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![5.0, 25.0, 25.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_main_space_evenly() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 30.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::SpaceEvenly;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            100.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![17.5, 45.0, 72.5],
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_cross_space_evenly_between_wrapped_lines() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceEvenly;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        let resolved_cross_offsets = resolved_items_in_order_modified_order(&scratch, solve)
            .iter()
            .map(|item| item.cross_offset_px)
            .collect::<Vec<_>>();
        let expected_cross_offsets = [20.0 / 3.0, 70.0 / 3.0, 70.0 / 3.0];
        for (index, (resolved_cross_offset, expected_cross_offset)) in resolved_cross_offsets
            .iter()
            .zip(expected_cross_offsets.iter())
            .enumerate()
        {
            assert!(
                (resolved_cross_offset - expected_cross_offset).abs() < 0.0001,
                "space-evenly wrapped line {index} should start at {expected_cross_offset}, got {resolved_cross_offset}",
            );
        }
    }

    #[test]
    fn resolve_active_flex_container_stretches_wrapped_line_cross_sizes_to_fill_container() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::Stretch;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            50.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_line_cross_sizes_in_solve(&scratch, solve),
            vec![20.0, 20.0]
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 20.0, 20.0],
        );
        assert_eq!(solve.cross_size_px, 40.0);
    }

    #[test]
    fn resolve_active_flex_container_distributes_positive_free_space_by_grow_factor() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 1.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 1.0, 1.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 2.0, 1.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 30.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            90.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![10.0, 30.0, 50.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 10.0, 40.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_limits_positive_free_space_when_grow_sum_is_below_one() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item(source(keys[0]), 0.5, 1.0, 20.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 1.0, 20.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 40.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            100.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![50.0, 20.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 50.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_redistributes_growth_after_max_main_size_freeze() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item_with_limits(source(keys[0]), 1.0, 1.0, 10.0, 10.0, None, Some(20.0)),
            measured_row_item_with_limits(source(keys[1]), 1.0, 1.0, 10.0, 10.0, None, None),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 20.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            100.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![20.0, 80.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 20.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_redistributes_shrink_after_min_main_size_freeze() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item_with_limits(source(keys[0]), 0.0, 1.0, 20.0, 10.0, Some(15.0), None),
            measured_row_item_with_limits(source(keys[1]), 0.0, 1.0, 20.0, 10.0, None, None),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 40.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            20.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![15.0, 5.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 15.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_subtracts_gap_total_before_distributing_growth() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 16.0, 16.0),
            measured_row_item(source(keys[1]), 1.0, 1.0, 0.0, 18.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 16.0, 16.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 32.0,
                cross_size_px: 18.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.gap_sizes.between_items_px = 2.0;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            120.0,
            Some(18.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![16.0, 84.0, 16.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 18.0, 104.0],
        );
    }

    #[test]
    fn build_measured_flex_lines_breaks_on_outer_size_from_padding_margin_and_gap() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item_with_box_edges(
                source(keys[0]),
                MeasuredRowItemBoxEdges {
                    main_size: 18.0,
                    cross_size: 10.0,
                    main_border_padding: 8.0,
                    main_start_margin: 3.0,
                    main_end_margin: 2.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[1]),
                MeasuredRowItemBoxEdges {
                    main_size: 12.0,
                    cross_size: 10.0,
                    main_border_padding: 2.0,
                    main_start_margin: 4.0,
                    main_end_margin: 2.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[2]),
                MeasuredRowItemBoxEdges {
                    main_size: 12.0,
                    cross_size: 10.0,
                    main_border_padding: 2.0,
                    main_start_margin: 4.0,
                    main_end_margin: 2.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
        ];

        let mut scratch = LayoutScratch::default();
        let lines =
            build_lines_from_items(&mut scratch, &items, FlexWrapMode::Wrap, Some(45.0), 5.0);

        assert_eq!(
            lines,
            vec![
                TestLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 23.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 41.0,
                    cross_size_px: 10.0,
                },
            ],
        );
    }

    #[test]
    fn resolve_active_flex_container_places_gap_after_item_margins_and_padding() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item_with_box_edges(
                source(keys[0]),
                MeasuredRowItemBoxEdges {
                    main_size: 18.0,
                    cross_size: 10.0,
                    main_border_padding: 8.0,
                    main_start_margin: 3.0,
                    main_end_margin: 2.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[1]),
                MeasuredRowItemBoxEdges {
                    main_size: 16.0,
                    cross_size: 10.0,
                    main_border_padding: 6.0,
                    main_start_margin: 4.0,
                    main_end_margin: 1.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 49.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.gap_sizes.between_items_px = 5.0;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            80.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 28.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![18.0, 16.0],
        );
    }

    #[test]
    fn build_measured_flex_line_cross_size_uses_largest_outer_cross_size_before_row_gap() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item_with_box_edges(
                source(keys[0]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 8.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 1.0,
                    cross_end_margin: 3.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[1]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 9.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 4.0,
                    cross_end_margin: 2.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[2]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 7.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
        ];

        let mut scratch = LayoutScratch::default();
        let lines = build_lines_from_items(&mut scratch, &items, FlexWrapMode::NoWrap, None, 5.0);

        assert_eq!(
            lines,
            vec![TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>(),
                outer_hypothetical_main_size_px: 40.0,
                cross_size_px: 15.0,
            }],
        );
    }

    #[test]
    fn resolve_active_flex_container_adds_row_gap_after_outer_cross_size_lines() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item_with_box_edges(
                source(keys[0]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 8.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 1.0,
                    cross_end_margin: 3.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[1]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 9.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 4.0,
                    cross_end_margin: 2.0,
                },
            ),
            measured_row_item_with_box_edges(
                source(keys[2]),
                MeasuredRowItemBoxEdges {
                    main_size: 10.0,
                    cross_size: 7.0,
                    main_border_padding: 0.0,
                    main_start_margin: 0.0,
                    main_end_margin: 0.0,
                    cross_start_margin: 0.0,
                    cross_end_margin: 0.0,
                },
            ),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[
                TestLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 15.0,
                },
                TestLine {
                    item_sources: vec![source(keys[2])],
                    outer_hypothetical_main_size_px: 10.0,
                    cross_size_px: 7.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.gap_sizes.between_lines_px = 5.0;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            25.0,
            Some(40.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 0.0, 20.0],
        );
        assert_eq!(solve.cross_size_px, 27.0);
    }

    #[test]
    fn resolve_active_flex_container_distributes_negative_free_space_by_scaled_shrink_factor() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 2.0, 40.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 3.0, 40.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 80.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            50.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![28.0, 22.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 28.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_clamps_shrink_result_to_automatic_min_main_size() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item_with_automatic_min(source(keys[0]), 0.0, 1.0, 40.0, 10.0, 40.0),
            measured_row_item_with_automatic_min(source(keys[1]), 0.0, 1.0, 40.0, 10.0, 0.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 80.0,
                cross_size_px: 10.0,
            }],
        );

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            20.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![40.0, 0.0],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 40.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_floors_shrunk_content_box_at_zero() {
        let keys = test_keys(1);
        let mut item =
            measured_row_item_with_automatic_min(source(keys[0]), 0.0, 1.0, 8.0, 10.0, 0.0);
        item.main_axis_border_padding_px = 8.0;

        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &[item],
            &[TestLine {
                item_sources: vec![source(keys[0])],
                outer_hypothetical_main_size_px: 8.0,
                cross_size_px: 10.0,
            }],
        );
        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            row_container_style(),
            4.0,
            Some(10.0),
        );
        let resolved = resolved_items_in_order_modified_order(&scratch, solve);

        assert_eq!(resolved[0].resolved_main_size_px, 8.0);
        assert_eq!(resolved[0].main_axis_border_padding_px, 8.0);
    }

    #[test]
    fn resolve_active_flex_container_centers_each_line_after_main_axis_sizing() {
        let keys = test_keys(2);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 10.0, 10.0),
        ];
        let mut scratch = LayoutScratch::default();
        let measured_solve = solve_from_items_and_lines(
            &mut scratch,
            &items,
            &[TestLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>()
                    .clone(),
                outer_hypothetical_main_size_px: 20.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::Center;

        let solve = resolve_active_flex_container_test(
            &mut scratch,
            measured_solve,
            container_style,
            100.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![40.0, 50.0],
        );
    }

    #[test]
    fn resolve_active_flex_container_uses_order_modified_sequence_for_offsets() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "30px")
            .expect("width should set");
        let first = append_div(&mut root, container);
        let second = append_div(&mut root, container);
        let third = append_div(&mut root, container);
        root.style_mut(first)
            .set("flex-basis", "10px")
            .expect("basis should set");
        root.style_mut(first)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(first)
            .set("order", "3")
            .expect("order should set");
        root.style_mut(second)
            .set("flex-basis", "10px")
            .expect("basis should set");
        root.style_mut(second)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(second)
            .set("order", "1")
            .expect("order should set");
        root.style_mut(third)
            .set("flex-basis", "10px")
            .expect("basis should set");
        root.style_mut(third)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(third)
            .set("order", "2")
            .expect("order should set");

        let measured_solve = build_solve_for_container(&mut root, container, context);
        let solve = resolve_active_flex_container_test(
            &mut root.layout_scratch,
            measured_solve,
            row_container_style(),
            30.0,
            Some(10.0),
        );

        assert_eq!(
            resolved_items_in_order_modified_order(&root.layout_scratch, solve)
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![second, third, first],
        );
        assert_eq!(
            resolved_items_in_order_modified_order(&root.layout_scratch, solve)
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 10.0, 20.0],
        );
    }
}
