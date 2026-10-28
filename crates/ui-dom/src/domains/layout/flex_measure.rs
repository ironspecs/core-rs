//! Flex measurement fold — the staged measurement solve.
//!
//! This file owns the measurement fold (flex arc steps 3–6): the per-child
//! measurement walk, item-view assembly for block items, the measurement-record
//! interplay (read-first write, auto-min derivation, min-content block fold),
//! the §9.9 intrinsic contribution helpers, and the measured-container solve
//! assembly (widest line main + summed line cross).
//!
//! Invariants and rules:
//!
//! - the fold writes each child's `MeasurementRecord` (equality-gated) and
//!   never retained layout/box-model state
//! - item-view CONTENT-SIZE fields derive from the just-written record
//! - nested containers claim transient lane ranges and truncate them back after
//!   their intrinsic size is computed
//! - the fold delegates item formation to the formation phase and §9.2 line
//!   breaking to the resolve phase (cross-phase calls)
#![allow(clippy::needless_pass_by_value)] // LayoutContext is passed by value as a child's owned context, mirroring flow.rs

use crate::domains::style::computed::{FlexBasisKw, OverflowKw};
use crate::domains::visitor::inherited_style::InheritedStyleContext;
use crate::types::{
    DomBoxModel, DomError, DomNode, DomNodeSlotKey, DomRect, LaneRange, LayoutScratch, RootDom,
    RootDomMetricName,
};

use super::DomLayoutResolver;
use super::MeasurementRecord;
use super::flex::{
    FlexAxis, FlexBasisSource, FlexContainerSolve, FlexContainerStyle, FlexItemView,
    MeasuredBlockFlowState, StagedMeasurementRequest, active_flex_content_height_px,
    active_flex_content_width_px, auto_margin_sides, cross_axis_border_padding_size_px,
    cross_axis_end_margin, cross_axis_start_margin, entered_container_cross_size,
    entered_container_main_size, flex_axis_sizes_match, item_align_self, item_order_from_node,
    main_axis_border_padding_size_px, main_axis_end_margin, main_axis_start_margin, node_for_key,
    non_negative_f32_from_usize, staged_flex_container_style,
};
use super::flex_anonymous_run::{
    container_mode_is_collapse, node_is_excluded_flex_child, node_is_inline_run_member,
};
use super::flex_formation::{
    anonymous_inline_run_item_view, backfill_order_indices, blockified_flex_item_style,
    measure_inline_run, push_flex_item_into_lanes,
};
use super::flex_item_source::FlexItemSource;
use super::flex_resolve::{build_measured_flex_lines, resolve_active_flex_container};
use super::flow::{
    EnteredNodeLayout, FlowSize, InlineFlowState, LayoutContext, auto_content_width_for_exit,
    border_box_height, border_box_width, child_layout_context, content_width_contribution_for_exit,
    display_is_inline_outer, enter_node, entered_content_size_property_style, first_child_key,
    intrinsic_content_width, layout_flow_size, next_inline_child_context, next_sibling_key,
    non_negative_auto_content_height, resolved_content_size_px, resolved_node_style,
    wrapped_text_content_height_px,
};
use super::measurement_record::{
    compute_node_measurement, node_measurement_of, write_measurement_if_changed,
};
use super::style::{LayoutDisplay, NodeStyle, PositionMode};

#[cfg(test)]
pub(crate) fn build_staged_measurement_solve<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<FlexContainerSolve, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let container_node = node_for_key(root_dom, key)?;
    let container_style = staged_flex_container_style(container_node);
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context, entered.clone());
    let container_main_size_px = entered_container_main_size(&entered, container_style.main_axis);
    build_staged_measurement_solve_from_container(
        root_dom,
        tree_size,
        &StagedMeasurementRequest {
            key,
            child_context,
            container_style,
            container_main_size_px,
        },
        resolver,
        RootDomMetricName::FlexNodesMeasured,
    )
}

pub(crate) fn build_staged_measurement_solve_from_container<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &StagedMeasurementRequest,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlexContainerSolve, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    // The fold claims disjoint item-lane and order-lane ranges for this
    // container, pushes each child in DOM order (measure → item lane → order
    // lane → back-fill), then builds lines over the order lane. Nested
    // containers claim their own disjoint ranges during their own measurement
    // fold (T3-verified discipline).
    //
    // Item formation (CSS Flexbox §4, flex arc step 3a-ii): a maximal run of
    // contiguous inline-level in-flow children (text nodes and inline
    // elements) contributes ONE virtual item — `measure_inline_run` folds the
    // run's fragment metrics into the three cursors (D3) while measuring the
    // run's cross at the measure width (D4). Block-level children (block,
    // inline-block, flex, inline-flex, img, element controls) and
    // abs-pos/`display: none` children break the run and keep today's
    // per-child item treatment. A whitespace-only run is dropped when — and
    // only when — its characters are collapsible AND the container's
    // white-space mode collapses spaces (the D3 emit gate).
    let item_range_start = root_dom.layout_scratch.items.begin_range();
    let order_range_start = root_dom.layout_scratch.order.begin_range();
    let white_space = request
        .child_context
        .inherited_style
        .effective_white_space();
    let mut child_key = first_child_key(root_dom, request.key)?;
    while let Some(current_child_key) = child_key {
        let current_node = node_for_key(root_dom, current_child_key)?;
        if node_is_inline_run_member(current_node) {
            let (run, run_breaker_key, measured_joint_height_px) = measure_inline_run(
                root_dom,
                tree_size,
                current_child_key,
                &request.child_context,
                white_space,
                resolver,
                metric,
            )?;
            if !(run.all_members_text_and_all_whitespace && container_mode_is_collapse(white_space))
            {
                let source = FlexItemSource::AnonymousInlineGroup {
                    first_child_key: run.first_child_key,
                    child_count: run.child_count,
                };
                let item = anonymous_inline_run_item_view(
                    source,
                    run,
                    measured_joint_height_px,
                    request.container_style.main_axis,
                );
                push_flex_item_into_lanes(&mut root_dom.layout_scratch, item, current_child_key);
            }
            child_key = run_breaker_key;
        } else if node_is_excluded_flex_child(current_node) {
            // D1 (flex arc step 3b): abs-pos/`display: none` children are
            // EXCLUDED from item formation — no item view, no gap adjacency,
            // no line membership, no justify count. The lanes (item, order,
            // line, resolution) never carry them, so justify distribution and
            // gap insertion read only real items.
            child_key = next_sibling_key(root_dom, current_child_key)?;
        } else {
            let next_child_key = next_sibling_key(root_dom, current_child_key)?;
            let source = FlexItemSource::BlockItem(current_child_key);
            let item = measure_one_flex_item(
                root_dom,
                tree_size,
                source,
                &request.child_context,
                request.container_style.main_axis,
                resolver,
                metric,
            )?;
            push_flex_item_into_lanes(&mut root_dom.layout_scratch, item, current_child_key);
            child_key = next_child_key;
        }
    }
    let item_range = root_dom.layout_scratch.items.finish_range(item_range_start);
    let order_range = root_dom
        .layout_scratch
        .order
        .finish_range(order_range_start);
    backfill_order_indices(&mut root_dom.layout_scratch, order_range);
    let line_range = build_measured_flex_lines(
        &mut root_dom.layout_scratch,
        order_range,
        request.container_style.wrap_mode,
        request.container_main_size_px,
        request.container_style.gap_sizes.between_items_px,
    );
    Ok(resolve_measured_flex_container(
        &root_dom.layout_scratch,
        item_range,
        order_range,
        line_range,
    ))
}

pub(crate) fn measure_one_flex_item<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    source: FlexItemSource,
    child_context: &LayoutContext,
    main_axis: FlexAxis,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlexItemView, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let child_key = source.key();
    let (child_style, child_inherited_style) =
        resolved_node_style(root_dom, child_key, child_context)?;
    // CSS Display 3 §2.7: blockify a flex item's inline outer display so a
    // nested `inline-flex` item resolves its auto size and auto-min exactly
    // like a `display: flex` item (stretching as block-level) rather than
    // shrink-wrapping as inline-level. This keeps the measurement below on
    // the same path the dispatcher routes `InlineFlex` onto.
    let child_style = blockified_flex_item_style(child_style);
    measure_tree_walker_node_without_layout_write(
        root_dom,
        tree_size,
        child_key,
        child_context,
        None,
        child_style,
        child_inherited_style,
        resolver,
        metric,
    )?;
    let node = node_for_key(root_dom, child_key)?;
    // The child's record was written by the measurement walk just exited (a
    // pass output, fresh by construction); the content sizes below read it
    // instead of re-deriving from the now-discarded `FlowSize`.
    let record = node_measurement_of(node);
    let measured_main_size_px = record_content_main_size(main_axis, record)
        + main_axis_border_padding_size_px(child_style.sides, main_axis);
    let measured_cross_size_px = record_content_cross_size(main_axis, record)
        + cross_axis_border_padding_size_px(child_style.sides, main_axis);
    let order = item_order_from_node(node);
    let align_self = item_align_self(node);
    let flex_grow_factor = item_flex_grow_factor_from_node(node);
    let flex_shrink_factor = item_flex_shrink_factor_from_node(node);
    let main_axis_border_padding_px =
        main_axis_border_padding_size_px(child_style.sides, main_axis);
    let flex_base_size_px =
        measured_flex_base_size_px(node, child_style, main_axis, measured_main_size_px);
    // D1 (flex arc step 4): the automatic minimum main size is DERIVED at the
    // item view's seed — `min(min_content_main, definite_specified_main_size)`
    // for the overflow-visible subset, 0 otherwise (CSS Flexbox §4.5). Two
    // reads (record + authored style) and one `min`; no recursion, no gates.
    // The derivation reads the record during the measurement fold (pre-
    // overwrite, by construction — R6's read discipline): the outer solve
    // reads nested records before placement overwrites them, so no future
    // consumer may drift into reading end-of-pass values.
    let automatic_min_main_size_px =
        automatic_min_main_size_px(node, child_style, main_axis, record);
    // D1 (flex arc step 5): the hypothetical main size FIELD gains the automatic
    // minimum floor — `clamp(base, min, max).max(auto_min + bp)` — equivalent to
    // the engines' clamp with `min' = max(min, auto_min)` (including the
    // `auto_min > max` case, where the floor wins exactly as `min > max` wins
    // today). The floor is now on the field itself, so the §9.7 seed (step 5's
    // hypothetical-seed switch) and the inflexibility check (base < hypothetical)
    // both see the floored value — the entire fix for
    // `off_spec_inflexible_items_skip_hypothetical_clamps`. The OUTER form below
    // simplifies to `field + margins` (the former outer-site `max()` is a no-op
    // once the field floors).
    //
    // The automatic minimum is CONTENT-BOX while `hypothetical_main_size_px` is
    // border-box, so the floor adds the main-axis border+padding to convert
    // auto-min into the border-box terms of the value it floors.
    let hypothetical_main_size_px = clamp_main_axis_size(child_style, main_axis, flex_base_size_px)
        .max(automatic_min_main_size_px + main_axis_border_padding_px);
    let (min_main_size_px, max_main_size_px) = main_axis_limits(child_style, main_axis);
    let margin_main_start_px = main_axis_start_margin(child_style, main_axis);
    let margin_main_end_px = main_axis_end_margin(child_style, main_axis);
    let margin_cross_start_px = cross_axis_start_margin(child_style, main_axis);
    let margin_cross_end_px = cross_axis_end_margin(child_style, main_axis);
    // D1 (flex arc step 10): read the auto-margin flags from the node's computed
    // box group at item-view assembly (no new resolution) — a side is auto iff
    // its computed margin is `BoxLength::Auto`.
    let (
        margin_main_start_auto,
        margin_main_end_auto,
        margin_cross_start_auto,
        margin_cross_end_auto,
    ) = auto_margin_sides(node.computed_box().margin, main_axis);
    let outer_hypothetical_main_size_px =
        hypothetical_main_size_px + margin_main_start_px + margin_main_end_px;
    Ok(FlexItemView {
        source,
        order,
        align_self,
        flex_grow_factor,
        flex_shrink_factor,
        flex_base_size_px,
        hypothetical_main_size_px,
        automatic_min_main_size_px,
        min_main_size_px,
        max_main_size_px,
        main_axis_border_padding_px,
        outer_hypothetical_main_size_px,
        measured_cross_size_input_px: measured_cross_size_px,
        replaced_ratio: record.flags.replaced_ratio(),
        main_influences_cross: record.flags.main_influences_cross(),
        resolved_cross_size_px: measured_cross_size_px,
        // D2a: the item's record inline min/max-content, carried into the view
        // for the inner container's §9.9 intrinsic fold.
        h_min_content_px: record.h_min_content_px,
        h_max_content_px: record.h_max_content_px,
        margin_main_start_px,
        margin_main_end_px,
        margin_cross_start_px,
        margin_cross_end_px,
        margin_main_start_auto,
        margin_main_end_auto,
        margin_cross_start_auto,
        margin_cross_end_auto,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_tree_walker_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    wrap_width_override: Option<f32>,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    if style.display == LayoutDisplay::None {
        return Ok(FlowSize::default());
    }
    if matches!(style.position, PositionMode::Absolute | PositionMode::Fixed) {
        return Ok(FlowSize::default());
    }
    match style.display {
        LayoutDisplay::Block | LayoutDisplay::InlineBlock => {
            measure_block_node_without_layout_write(
                root_dom,
                tree_size,
                key,
                context,
                wrap_width_override,
                style,
                inherited_style,
                resolver,
                metric,
            )
        }
        LayoutDisplay::Grid | LayoutDisplay::InlineGrid => {
            super::grid::measure_grid_node_without_layout_write(
                root_dom,
                tree_size,
                key,
                context,
                wrap_width_override,
                style,
                inherited_style,
                resolver,
                metric,
            )
        }
        LayoutDisplay::Inline => measure_inline_node_without_layout_write(
            root_dom,
            tree_size,
            key,
            context,
            wrap_width_override,
            style,
            inherited_style,
            resolver,
            metric,
        ),
        // CSS Display 3 §2.7 blockification: a flex item's `inline-flex`
        // outer display blockifies to `flex` (like `inline-block` → `block`),
        // so an inline-flex child reaches the flex container measurement path
        // rather than panicking. The `InlineFlex` arm below is the one
        // remaining display value that could previously reach this match; the
        // other admitted displays are all handled above (`None` and
        // absolute/fixed-position children return before the match).
        LayoutDisplay::Flex | LayoutDisplay::InlineFlex => measure_flex_node_without_layout_write(
            root_dom,
            tree_size,
            key,
            context,
            wrap_width_override,
            style,
            inherited_style,
            resolver,
            metric,
        ),
        LayoutDisplay::None => unreachable!("display none handled before measurement dispatch"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_flex_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    _wrap_width_override: Option<f32>,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    // CSS Flexbox 9.2/9.7 split flex item collection from final layout:
    // https://www.w3.org/TR/css-flexbox-1/#algo-main-item
    // https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths
    // Gecko mirrors this in `GenerateFlexItemForChild`,
    // `FlexLine::ResolveFlexibleLengths`, and final reflow in
    // nsFlexContainerFrame.cpp; Blink mirrors it with
    // `ConstructAndAppendFlexItems`, `LineFlexer::ResolveFlexibleLengths`,
    // then `GiveItemsFinalPositionAndSize`/`BuildSpaceForLayout` in
    // flex_layout_algorithm.cc. ui-dom uses its existing entry/exit model for
    // the measuring phase and avoids writing retained layout state here.
    //
    // Staged nested measurement claims transient lane ranges during its fold
    // and resolve; those ranges are truncated back to their entry lengths
    // after the intrinsic size is computed so the enclosing container's open
    // item/order ranges stay contiguous (nested containers' transient entries
    // must not interleave the enclosing container's DOM sequence).
    let saved_item_len = root_dom.layout_scratch.items.len();
    let saved_order_len = root_dom.layout_scratch.order.len();
    let saved_line_len = root_dom.layout_scratch.lines.len();
    let saved_resolution_len = root_dom.layout_scratch.resolution.len();
    let container_node = node_for_key(root_dom, key)?;
    let container_style = staged_flex_container_style(container_node);
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context.clone(), entered.clone());
    let container_main_size_px = entered_container_main_size(&entered, container_style.main_axis);
    let measured_solve = build_staged_measurement_solve_from_container(
        root_dom,
        tree_size,
        &StagedMeasurementRequest {
            key,
            child_context: child_context.clone(),
            container_style,
            container_main_size_px,
        },
        resolver,
        metric,
    )?;
    let resolved_container_main_size_px =
        container_main_size_px.unwrap_or(measured_solve.main_size_px);
    let resolved_solve = resolve_active_flex_container(
        root_dom,
        tree_size,
        &child_context,
        measured_solve,
        container_style,
        resolved_container_main_size_px,
        entered_container_cross_size(&entered, container_style.main_axis),
        resolver,
    )?;
    let content_width_px = active_flex_content_width_px(
        container_style.main_axis,
        resolved_solve.main_size_px,
        resolved_solve.cross_size_px,
    );
    let auto_content_width_px = auto_content_width_for_exit(
        &entered,
        context.available_width_px,
        content_width_px,
        content_width_px,
        content_width_px,
    );
    // D2a (flex arc step 4): the inner container's record inline min/max-content
    // sizes are the §9.9 intrinsic main sizes, folded from the item views'
    // carried record sizes + inline margins — NOT the resolved/stretched width
    // the block model would otherwise write. This is what the auto-min
    // derivation (D1) and the §9.2 auto-basis base read.
    let (h_min_content_px, h_max_content_px) =
        flex_container_intrinsic_inline_sizes(root_dom, container_style, resolved_solve.item_range);
    let flow = measure_exited_node_flow_size(
        root_dom,
        key,
        &entered,
        auto_content_width_px,
        (h_min_content_px, h_max_content_px),
        active_flex_content_height_px(
            container_style.main_axis,
            resolved_solve.main_size_px,
            resolved_solve.cross_size_px,
        ),
        metric,
    )?;
    root_dom.layout_scratch.items.truncate(saved_item_len);
    root_dom.layout_scratch.order.truncate(saved_order_len);
    root_dom.layout_scratch.lines.truncate(saved_line_len);
    root_dom
        .layout_scratch
        .resolution
        .truncate(saved_resolution_len);
    Ok(flow)
}

/// The inner flex container's §9.9 intrinsic inline (horizontal) min/max-content
/// sizes, folded from the item lane's carried record sizes (D2a, flex arc step
/// 4). Arithmetic over the views already assembled by the measurement fold:
/// each item contributes its record `h_min`/`h_max` plus its inline-axis margins
/// (main margins for a row, cross margins for a column — the axis of the `h_*`
/// size). `gap` is the main-axis inter-item gap; run items carry zero margins.
///
/// ```text
/// h_min = row nowrap    => Σ contribution_min + gap × (n−1)
///         row wrap      => max contribution_min
///         column        => max contribution_min
/// h_max = row           => Σ contribution_max + gap × (n−1)
///         column        => max contribution_max
/// ```
///
/// Empty container: both 0.
pub(crate) fn flex_container_intrinsic_inline_sizes(
    root_dom: &RootDom,
    container_style: FlexContainerStyle,
    item_range: LaneRange,
) -> (f32, f32) {
    let is_row = container_style.main_axis == FlexAxis::Horizontal;
    let mut min_sum: f32 = 0.0;
    let mut max_sum: f32 = 0.0;
    let mut min_max: f32 = 0.0;
    let mut max_max: f32 = 0.0;
    let mut item_count = 0usize;
    for entry in root_dom.layout_scratch.items.slice(item_range) {
        let view = entry.view;
        let inline_margins = match container_style.main_axis {
            FlexAxis::Horizontal => view.margin_main_start_px + view.margin_main_end_px,
            FlexAxis::Vertical => view.margin_cross_start_px + view.margin_cross_end_px,
        };
        // §9.9.3 flex item intrinsic contribution: floored by the flex base
        // size if the item is not shrinkable (an inflexible item won't shrink
        // below its base). The floor applies to the intrinsic MAIN size; for a
        // column container the flex base is vertical and never belongs in a
        // horizontal contribution, so the floor is gated to the horizontal
        // main axis. Content-box flex base, matching `h_min`/`h_max`.
        let flex_base_floored =
            container_style.main_axis == FlexAxis::Horizontal && view.flex_shrink_factor == 0.0;
        let flex_base_content_px = view.flex_base_size_px - view.main_axis_border_padding_px;
        let contribution_min = if flex_base_floored {
            view.h_min_content_px.max(flex_base_content_px)
        } else {
            view.h_min_content_px
        };
        let contribution_max = if flex_base_floored {
            view.h_max_content_px.max(flex_base_content_px)
        } else {
            view.h_max_content_px
        };
        let contribution_min = contribution_min + inline_margins;
        let contribution_max = contribution_max + inline_margins;
        min_sum += contribution_min;
        max_sum += contribution_max;
        min_max = min_max.max(contribution_min);
        max_max = max_max.max(contribution_max);
        item_count += 1;
    }
    if item_count == 0 {
        return (0.0, 0.0);
    }
    let gap_total_px =
        container_style.gap_sizes.between_items_px * non_negative_f32_from_usize(item_count - 1);
    let (h_min, h_max) = if is_row {
        let h_min = if container_style.wrap_mode.is_wrapping() {
            min_max
        } else {
            min_sum + gap_total_px
        };
        (h_min, max_sum + gap_total_px)
    } else {
        (min_max, max_max)
    };
    (h_min, h_max)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_block_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    wrap_width_override: Option<f32>,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context.clone(), entered.clone());
    let mut flow = MeasuredBlockFlowState::default();
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let (child_style, child_inherited_style) =
            resolved_node_style(root_dom, current_child_key, &child_context)?;
        let next_child_context =
            measured_block_child_context(child_context.clone(), flow, child_style.display);
        let child_flow = measure_tree_walker_node_without_layout_write(
            root_dom,
            tree_size,
            current_child_key,
            &next_child_context,
            wrap_width_override,
            child_style,
            child_inherited_style,
            resolver,
            metric,
        )?;
        flow.has_children = true;
        update_measured_block_flow(&mut flow, child_flow, child_style.display);
        child_key = next_child_key;
    }
    let min_content_width_px = measured_block_min_content_width(&entered, flow);
    let max_content_width_px = measured_block_max_content_width(&entered, flow);
    let auto_content_width_px = measured_block_content_width(
        &entered,
        context.available_width_px,
        flow,
        min_content_width_px,
        max_content_width_px,
    );
    let auto_content_height_px =
        measured_block_content_height(entered.intrinsic_content_height_px, flow);
    measure_exited_node_flow_size(
        root_dom,
        key,
        &entered,
        auto_content_width_px,
        (min_content_width_px, max_content_width_px),
        auto_content_height_px,
        metric,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_inline_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    wrap_width_override: Option<f32>,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let child_context = child_layout_context(context.clone(), entered.clone());
    let mut flow = InlineFlowState::default();
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let (child_style, child_inherited_style) =
            resolved_node_style(root_dom, current_child_key, &child_context)?;
        let next_child_context = next_inline_child_context(child_context.clone(), flow);
        let child_flow = measure_tree_walker_node_without_layout_write(
            root_dom,
            tree_size,
            current_child_key,
            &next_child_context,
            wrap_width_override,
            child_style,
            child_inherited_style,
            resolver,
            metric,
        )?;
        flow.has_children = true;
        flow.inline_left_px += child_flow.width_px;
        flow.line_height_px = flow.line_height_px.max(child_flow.height_px);
        flow.min_width_px = flow.min_width_px.max(child_flow.min_width_px);
        flow.max_content_width_px += child_flow.max_width_px;
        child_key = next_child_key;
    }
    let min_content_width_px = measured_inline_min_content_width(&entered, flow);
    let max_content_width_px = measured_inline_max_content_width(&entered, flow);
    let auto_content_width_px = measured_inline_content_width(&entered, flow, wrap_width_override);
    let auto_content_height_px = if flow.has_children {
        non_negative_auto_content_height(flow.line_height_px)
    } else {
        entered.intrinsic_content_height_px.unwrap_or(0.0)
    };
    measure_exited_node_flow_size(
        root_dom,
        key,
        &entered,
        auto_content_width_px,
        (min_content_width_px, max_content_width_px),
        auto_content_height_px,
        metric,
    )
}

pub(crate) fn measure_exited_node_flow_size(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    auto_content_width_px: f32,
    auto_content_width_contribution_px: (f32, f32),
    auto_content_height_px: f32,
    metric: RootDomMetricName,
) -> Result<FlowSize, DomError> {
    let (auto_min_content_width_px, auto_max_content_width_px) = auto_content_width_contribution_px;
    // Record-serving placement (flex arc step 7, D3): a width-unchanged text
    // node's record was written this pass at the same width by the outer fold
    // (fresh by construction). Placement's re-entry into the nested flex
    // solver reads the record instead of re-shaping the text — no shape, no
    // record recompute, no work counter. The refold path (a flagged item whose
    // width changed) re-measures at a different width, so the width mismatch
    // routes it through the normal measure below.
    if entered.is_text_node {
        let node = node_for_key(root_dom, key)?;
        let record = node_measurement_of(node);
        let current_pass = root_dom.layout_scratch.layout_pass();
        if record.measure_pass == current_pass
            && flex_axis_sizes_match(
                record.content_height_measured_at_width_px,
                auto_content_width_px,
            )
        {
            let content_height_px = record.content_height_at_width_px;
            let content_size = resolved_content_size_px(
                entered_content_size_property_style(entered),
                entered.img_intrinsic_aspect_ratio,
                None,
                auto_content_width_px,
                content_height_px,
            );
            let (min_content_width_px, max_content_width_px) = content_width_contribution_for_exit(
                entered,
                content_size.width_px,
                auto_min_content_width_px,
                auto_max_content_width_px,
            );
            return Ok(layout_flow_size(
                entered.style,
                measured_box_model(entered.box_model, entered.style, content_size),
                min_content_width_px,
                max_content_width_px,
            ));
        }
    }
    let content_height_px = measured_auto_content_height(
        root_dom,
        key,
        entered,
        auto_content_width_px,
        auto_content_height_px,
    )?;
    let content_size = resolved_content_size_px(
        entered_content_size_property_style(entered),
        entered.img_intrinsic_aspect_ratio,
        None,
        auto_content_width_px,
        content_height_px,
    );
    let (min_content_width_px, max_content_width_px) = content_width_contribution_for_exit(
        entered,
        content_size.width_px,
        auto_min_content_width_px,
        auto_max_content_width_px,
    );
    // Read-first record write (the T4a ruling, FLEX_STEP1_EXECUTION.md §10):
    // compare the freshly-computed record against the stored one with an
    // immutable read, and take the mutable borrow only when the value changed.
    // The record is a pass output, never a dirt source; unchanged records cost
    // a read, not a mutation. Record assembly is unified through
    // `compute_node_measurement` (one computation, one comparison; pass
    // outputs, never a dirt source), and the write gate is the single-comparison
    // `write_measurement_if_changed`.
    let mut next_record = compute_node_measurement(
        entered,
        (min_content_width_px, max_content_width_px),
        content_size,
    );
    // D3 (flex arc step 4): the flex measure fold computes the record's
    // min-content BLOCK size bottom-up (`compute_v_min_content_px`, in
    // measurement_record.rs) and writes it here; the block world's `exit_node`
    // leaves it `None` (nothing reads it). The vertical auto-min derivation
    // (D1) is its named consumer.
    next_record.v_min_content_px = Some(super::measurement_record::compute_v_min_content_px(
        root_dom, key, entered,
    ));
    // Flex arc step 7, D2/D3: stamp the measure pass and measure width on the
    // record (the record-serving placement freshness keys), and propagate the
    // `main_influences_cross` flag bottom-up — a container whose in-flow child
    // is flagged is text-bearing/percentage-cross, so its own cross is
    // width-dependent too.
    next_record.measure_pass = root_dom.layout_scratch.layout_pass();
    next_record.content_height_measured_at_width_px = auto_content_width_px;
    if !entered.is_text_node && node_has_main_influences_cross_child(root_dom, key)? {
        next_record.flags = next_record.flags.with_main_influences_cross();
    }
    // The measure counter (FlexNodesMeasured on the flex path, GridNodesMeasured
    // on the grid path) is a work counter, so it increments per COMPUTE, not
    // per record write: an unchanged record below still costs one compute on
    // the measure path.
    root_dom.increment_metric(metric);
    let _ = write_measurement_if_changed(&mut root_dom.nodes, key, next_record)?;
    Ok(layout_flow_size(
        entered.style,
        measured_box_model(entered.box_model, entered.style, content_size),
        min_content_width_px,
        max_content_width_px,
    ))
}

/// Whether a node has any in-flow child whose record carries
/// `main_influences_cross` (wrapping text / percentage cross) or
/// `replaced_ratio`. The flag is propagated bottom-up so a container's own
/// record inherits the width-dependence of its content.
pub(crate) fn node_has_main_influences_cross_child(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let current_node = node_for_key(root_dom, current_child_key)?;
        if node_is_excluded_flex_child(current_node) {
            child_key = next_sibling_key(root_dom, current_child_key)?;
            continue;
        }
        let flags = node_measurement_of(current_node).flags;
        if flags.main_influences_cross() || flags.replaced_ratio() {
            return Ok(true);
        }
        child_key = next_sibling_key(root_dom, current_child_key)?;
    }
    Ok(false)
}

pub(crate) fn measured_auto_content_height(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    content_width_px: f32,
    fallback_content_height_px: f32,
) -> Result<f32, DomError> {
    if !entered.is_text_node {
        return Ok(non_negative_auto_content_height(fallback_content_height_px));
    }
    let node = node_for_key(root_dom, key)?;
    let content_height_px =
        wrapped_text_content_height_px(node, &entered.inherited_style, content_width_px, root_dom)
            .unwrap_or(fallback_content_height_px.max(0.0));
    Ok(non_negative_auto_content_height(content_height_px))
}

pub(crate) fn measured_box_model(
    box_model: DomBoxModel,
    style: NodeStyle,
    content_size: super::flow::ResolvedContentSizePx,
) -> DomBoxModel {
    DomBoxModel {
        border_box: DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: border_box_width(style.sides, content_size.width_px),
            height_px: border_box_height(style.sides, content_size.viewport_height_px),
        },
        margin: box_model.margin,
        border: box_model.border,
        padding: box_model.padding,
    }
}

pub(crate) fn measured_block_child_context(
    parent: LayoutContext,
    flow: MeasuredBlockFlowState,
    child_display: LayoutDisplay,
) -> LayoutContext {
    if display_is_inline_outer(child_display) {
        return LayoutContext {
            flow_left_px: parent.flow_left_px + flow.inline_left_px,
            flow_top_px: parent.flow_top_px + flow.block_top_px,
            available_width_px: parent
                .available_width_px
                .map(|width| (width - flow.inline_left_px).max(0.0)),
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            ..parent
        };
    }
    let next_block_top_px = flow.block_top_px + flow.line_height_px;
    LayoutContext {
        flow_left_px: parent.flow_left_px,
        flow_top_px: parent.flow_top_px + next_block_top_px,
        available_width_px: parent.available_width_px,
        available_height_px: parent
            .available_height_px
            .map(|height| (height - next_block_top_px).max(0.0)),
        node_content_width_override_px: None,
        node_content_height_override_px: None,
        ..parent
    }
}

pub(crate) fn update_measured_block_flow(
    flow: &mut MeasuredBlockFlowState,
    child_flow: FlowSize,
    child_display: LayoutDisplay,
) {
    if display_is_inline_outer(child_display) {
        flow.inline_left_px += child_flow.width_px;
        flow.line_height_px = flow.line_height_px.max(child_flow.height_px);
        flow.max_width_px = flow.max_width_px.max(flow.inline_left_px);
        flow.line_min_width_px = flow.line_min_width_px.max(child_flow.min_width_px);
        flow.line_max_width_px += child_flow.max_width_px;
        flow.min_width_px = flow.min_width_px.max(flow.line_min_width_px);
        flow.max_content_width_px = flow.max_content_width_px.max(flow.line_max_width_px);
        return;
    }
    flow.block_top_px += flow.line_height_px;
    flow.line_height_px = 0.0;
    flow.inline_left_px = 0.0;
    flow.line_min_width_px = 0.0;
    flow.line_max_width_px = 0.0;
    flow.block_top_px += child_flow.height_px;
    flow.max_width_px = flow.max_width_px.max(child_flow.width_px);
    flow.min_width_px = flow.min_width_px.max(child_flow.min_width_px);
    flow.max_content_width_px = flow.max_content_width_px.max(child_flow.max_width_px);
}

pub(crate) fn measured_block_content_height(
    intrinsic_content_height_px: Option<f32>,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return non_negative_auto_content_height(flow.block_top_px + flow.line_height_px);
    }
    intrinsic_content_height_px.unwrap_or(0.0)
}

pub(crate) fn measured_block_content_width(
    entered: &EnteredNodeLayout,
    available_width_px: Option<f32>,
    flow: MeasuredBlockFlowState,
    min_content_width_px: f32,
    max_content_width_px: f32,
) -> f32 {
    if entered.style.display == LayoutDisplay::InlineBlock {
        return auto_content_width_for_exit(
            entered,
            available_width_px,
            flow.max_width_px,
            min_content_width_px,
            max_content_width_px,
        );
    }
    if flow.has_children {
        return flow.max_width_px;
    }
    entered.content_width_px
}

pub(crate) fn measured_block_min_content_width(
    entered: &EnteredNodeLayout,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

pub(crate) fn measured_block_max_content_width(
    entered: &EnteredNodeLayout,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

pub(crate) fn measured_inline_content_width(
    entered: &EnteredNodeLayout,
    flow: InlineFlowState,
    wrap_width_override: Option<f32>,
) -> f32 {
    if !flow.has_children && entered.is_text_node {
        // A bare text node's content width is its intrinsic (max-content)
        // width in the normal measure fold. The refold (flex arc step 7, D1.2)
        // passes a wrap-width override so a shrunk item's text re-shapes at the
        // resolved width instead of the unshrunk max-content.
        if let Some(override_px) = wrap_width_override {
            return override_px.max(0.0);
        }
        return intrinsic_content_width(entered.box_model);
    }
    auto_content_width_for_exit(
        entered,
        None,
        flow.inline_left_px,
        flow.min_width_px,
        flow.max_content_width_px,
    )
}

pub(crate) fn measured_inline_min_content_width(
    entered: &EnteredNodeLayout,
    flow: InlineFlowState,
) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

pub(crate) fn measured_inline_max_content_width(
    entered: &EnteredNodeLayout,
    flow: InlineFlowState,
) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

pub(crate) fn resolve_measured_flex_container(
    scratch: &LayoutScratch,
    item_range: LaneRange,
    order_range: LaneRange,
    line_range: LaneRange,
) -> FlexContainerSolve {
    let mut main_size_px: f32 = 0.0;
    let mut cross_size_px: f32 = 0.0;
    for line in scratch.lines.slice(line_range) {
        main_size_px = main_size_px.max(line.outer_hypothetical_main_size_px);
        cross_size_px += line.cross_size_px;
    }
    FlexContainerSolve {
        item_range,
        order_range,
        line_range,
        resolution_range: LaneRange::default(),
        main_size_px,
        cross_size_px,
    }
}

pub(crate) fn item_flex_grow_factor_from_node(node: &DomNode) -> f32 {
    node.computed_flex().grow
}

pub(crate) fn item_flex_shrink_factor_from_node(node: &DomNode) -> f32 {
    node.computed_flex().shrink
}

pub(crate) fn flex_basis_source_from_kw(kw: FlexBasisKw) -> FlexBasisSource {
    match kw {
        FlexBasisKw::Auto => FlexBasisSource::Auto,
        FlexBasisKw::Content => FlexBasisSource::Content,
        FlexBasisKw::Length(0.0) => FlexBasisSource::Zero,
        FlexBasisKw::Length(px) => FlexBasisSource::Length(px),
    }
}

pub(crate) fn flex_basis_source_from_node(node: &DomNode) -> FlexBasisSource {
    flex_basis_source_from_kw(node.computed_flex().basis)
}

pub(crate) fn preferred_flex_basis_px(node: &DomNode) -> Option<f32> {
    match flex_basis_source_from_node(node) {
        FlexBasisSource::Auto | FlexBasisSource::Content => None,
        FlexBasisSource::Zero => Some(0.0),
        FlexBasisSource::Length(length_px) => Some(length_px),
    }
}

pub(crate) fn measured_flex_base_size_px(
    node: &DomNode,
    style: NodeStyle,
    main_axis: FlexAxis,
    measured_border_box_main_size_px: f32,
) -> f32 {
    preferred_flex_basis_px(node).map_or(measured_border_box_main_size_px, |basis_px| {
        basis_px + main_axis_border_padding_size_px(style.sides, main_axis)
    })
}

/// The automatic minimum main size (flex arc step 4, D1): the CSS Flexbox §4.5
/// formula derived at the item view's seed — `min(min_content_main,
/// definite_specified_main_size)` for the overflow-visible subset, `0`
/// otherwise. Two reads (the record's main-axis min-content and the authored
/// definite main size) and one `min`; no recursion, no shape gates.
pub(crate) fn automatic_min_main_size_px(
    node: &DomNode,
    style: NodeStyle,
    main_axis: FlexAxis,
    record: MeasurementRecord,
) -> f32 {
    if node.computed_box().overflow != OverflowKw::Visible {
        return 0.0;
    }
    let content_min = min_content_main_size_px(main_axis, record);
    match definite_specified_main_size(style, main_axis) {
        Some(definite) => content_min.min(definite),
        None => content_min,
    }
}

/// The record's min-content size in the item's main axis (D2/D3): horizontal
/// reads `h_min_content_px` (mode-aware for text, fold-computed for the rest);
/// vertical reads `v_min_content_px` (the step-4 fold's min-content block size).
pub(crate) fn min_content_main_size_px(main_axis: FlexAxis, record: MeasurementRecord) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => record.h_min_content_px,
        FlexAxis::Vertical => record.v_min_content_px.unwrap_or(0.0),
    }
}

/// The authored definite main size (D1): the `width`/`height` resolved to a
/// definite `BoxLength::Px` (`style.width_px`/`style.height_px` already carry
/// the down-fold-resolved definite value; a percentage against a definite
/// down-fold basis resolves to `Px`, an indefinite percentage to `auto`).
/// `flex-basis` does NOT count as definite.
pub(crate) fn definite_specified_main_size(style: NodeStyle, main_axis: FlexAxis) -> Option<f32> {
    match main_axis {
        FlexAxis::Horizontal => style.width_px,
        FlexAxis::Vertical => style.height_px,
    }
}

pub(crate) fn clamp_main_axis_size(style: NodeStyle, main_axis: FlexAxis, value_px: f32) -> f32 {
    let (min_px, max_px) = main_axis_limits(style, main_axis);
    let mut clamped = value_px;
    if let Some(min_px) = min_px {
        clamped = clamped.max(min_px);
    }
    if let Some(max_px) = max_px {
        clamped = clamped.min(max_px);
    }
    clamped
}

pub(crate) fn main_axis_limits(
    style: NodeStyle,
    main_axis: FlexAxis,
) -> (Option<f32>, Option<f32>) {
    match main_axis {
        FlexAxis::Horizontal => (style.min_width_px, style.max_width_px),
        FlexAxis::Vertical => (style.min_height_px, style.max_height_px),
    }
}

/// The record's MAIN-axis content size for an item: the max-content inline
/// contribution for a horizontal main axis, the content height at measure width
/// for a vertical main axis.
pub(crate) fn record_content_main_size(main_axis: FlexAxis, record: MeasurementRecord) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => record.h_max_content_px,
        FlexAxis::Vertical => record.content_height_at_width_px,
    }
}

/// The record's CROSS-axis content size for an item (the inverse of
/// `record_content_main_size`): content height for a horizontal main axis,
/// max-content inline contribution for a vertical main axis.
pub(crate) fn record_content_cross_size(main_axis: FlexAxis, record: MeasurementRecord) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => record.content_height_at_width_px,
        FlexAxis::Vertical => record.h_max_content_px,
    }
}
#[cfg(test)]
use super::flow::resolved_node_style_with_basis;
#[cfg(test)]
use crate::domains::layout::flex::{
    TestLine, append_div, append_span, append_text, assert_item_view_reads_record, base_context,
    build_solve_for_container, built_lines, collect_items_for_container,
    configure_row_reverse_item, configure_rtl_row_item, configure_wrap_item,
    flex_container_with_context, item_views_in_dom_order, item_views_in_order_modified_order,
    measured_row_item, node_box, node_layout, node_layout_dirty, node_record,
    solve_from_items_and_lines, source, test_keys,
};
#[cfg(test)]
use crate::types::dom_node::measure_text_intrinsic_from_content_and_font_size;
#[cfg(test)]
use crate::types::{DomAttribute, DomAttributeValue, DomIntrinsic, DomTag, RootDomContext};

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn measured_item_record_uses_explicit_width_and_height() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("width", "12px")
            .expect("width should set");
        root.style_mut(child)
            .set("height", "18px")
            .expect("height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.source.key(), child);
        assert_eq!(item.flex_base_size_px, 12.0);
        assert_eq!(item.hypothetical_main_size_px, 12.0);
        assert_eq!(item.outer_hypothetical_main_size_px, 12.0);
        assert_eq!(item.measured_cross_size_input_px, 18.0);
        assert_eq!(item.automatic_min_main_size_px, 12.0);
    }

    #[test]
    fn measured_item_record_adds_padding_to_explicit_flex_basis() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("height", "18px")
            .expect("height should set");
        root.style_mut(child)
            .set("flex-basis", "12px")
            .expect("basis should set");
        root.style_mut(child)
            .set("padding-left", "4px")
            .expect("left padding should set");
        root.style_mut(child)
            .set("padding-right", "6px")
            .expect("right padding should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.flex_base_size_px, 22.0);
        assert_eq!(item.main_axis_border_padding_px, 10.0);
        assert_eq!(item.hypothetical_main_size_px, 22.0);
        assert_eq!(item.outer_hypothetical_main_size_px, 22.0);
    }

    #[test]
    fn measured_item_record_uses_text_child_subtree_measurement() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        append_text(&mut root, child, "Flex label");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.source.key(), child);
        assert!(item.flex_base_size_px > 0.0);
        assert!(item.flex_base_size_px < 320.0);
        assert!(item.hypothetical_main_size_px > 0.0);
        assert!(item.measured_cross_size_input_px > 0.0);
        assert_eq!(item.flex_base_size_px, item.hypothetical_main_size_px);
        assert_eq!(
            item.outer_hypothetical_main_size_px,
            item.hypothetical_main_size_px + item.margin_main_start_px + item.margin_main_end_px,
        );
    }

    #[test]
    fn measured_item_record_ignores_authored_inline_span_size_when_measuring_text_subtree() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        let span = append_span(&mut root, child);
        root.style_mut(span)
            .set("width", "20px")
            .expect("span width should set");
        root.style_mut(span)
            .set("height", "40px")
            .expect("span height should set");
        let text = append_text(&mut root, span, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");
        let DomIntrinsic::Text(text_intrinsic) =
            root.node(text).expect("text node should exist").intrinsic()
        else {
            panic!("flex measurement target should be measured text");
        };

        assert!(text_intrinsic.max_content_width_px > 20.0);
        assert_eq!(item.flex_base_size_px, text_intrinsic.max_content_width_px);
        assert_eq!(
            item.automatic_min_main_size_px,
            text_intrinsic.max_content_width_px
        );
    }

    #[test]
    fn measured_item_record_uses_zero_automatic_min_main_size_for_scroll_overflow() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "scroll")
            .expect("overflow should set");
        append_text(&mut root, child, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.automatic_min_main_size_px, 0.0);
        assert!(item.flex_base_size_px > 0.0);
    }

    #[test]
    fn measured_item_record_uses_zero_automatic_min_main_size_for_hidden_overflow() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "hidden")
            .expect("overflow should set");
        append_text(&mut root, child, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.automatic_min_main_size_px, 0.0);
        assert!(item.flex_base_size_px > 0.0);
    }

    #[test]
    fn measured_item_record_uses_zero_automatic_min_main_size_for_clip_overflow() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "clip")
            .expect("overflow should set");
        append_text(&mut root, child, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.automatic_min_main_size_px, 0.0);
        assert!(item.flex_base_size_px > 0.0);
    }

    #[test]
    fn measured_item_record_uses_content_based_automatic_min_main_size_for_visible_overflow() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "visible")
            .expect("overflow should set");
        append_text(&mut root, child, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        assert_eq!(item.automatic_min_main_size_px, item.flex_base_size_px);
        assert!(item.automatic_min_main_size_px > 0.0);
    }

    #[test]
    fn measured_item_record_uses_text_min_content_width_for_visible_overflow_auto_min() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "20px")
            .expect("width should set");
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "visible")
            .expect("overflow should set");
        let text = append_text(&mut root, child, "tiny Supercalifragilistic tiny");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");
        let (text_style, _) = resolved_node_style_with_basis(
            &root,
            text,
            &InheritedStyleContext::default(),
            0.0,
            None,
        )
        .expect("text style");
        let expected_segment_width_px = super::measure_text_intrinsic_from_content_and_font_size(
            "Supercalifragilistic",
            text_style.font_size_px,
        )
        .max_content_width_px;

        assert_eq!(item.automatic_min_main_size_px, expected_segment_width_px);
    }

    #[test]
    fn measured_item_record_uses_intrinsic_leaf_width_for_visible_overflow_auto_min() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document_key = root.document_node().0;
        let container = append_div(&mut root, document_key);
        root.style_mut(container)
            .set("display", "flex")
            .expect("display should set");
        let input = root
            .create_element(DomTag::Input)
            .expect("input should insert");
        root.append_child(container, input)
            .expect("input should append");
        root.style_mut(input)
            .set("overflow", "visible")
            .expect("overflow should set");

        let items = collect_items_for_container(&mut root, container, base_context());
        let item = items.first().expect("input item");

        assert!(item.flex_base_size_px > 0.0);
        assert_eq!(item.automatic_min_main_size_px, item.flex_base_size_px);
    }

    #[test]
    fn measured_item_record_uses_v_min_for_vertical_visible_overflow_auto_min() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("flex-direction", "column")
            .expect("direction should set");
        root.style_mut(container)
            .set("width", "20px")
            .expect("width should set");
        root.style_mut(container)
            .set("height", "40px")
            .expect("height should set");
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("overflow", "visible")
            .expect("overflow should set");
        root.style_mut(child)
            .set("width", "20px")
            .expect("width should set");
        root.style_mut(child)
            .set("flex-basis", "10px")
            .expect("basis should set");
        let nested = append_div(&mut root, child);
        root.style_mut(nested)
            .set("width", "20px")
            .expect("nested width should set");
        root.style_mut(nested)
            .set("height", "30px")
            .expect("nested height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("child item");

        // D3 (flex arc step 4): the child's vertical auto-min is its v_min (the
        // min-content block size). The nested 30px div is a SPECIFIED-height
        // block — a definite used size, not an intrinsic — so its v_min is its
        // content sum (0, empty content); the child's v_min is therefore 0, not
        // the 30px measured height the old vertical gate returned.
        assert_eq!(item.automatic_min_main_size_px, 0.0);
        assert_eq!(item.flex_base_size_px, 10.0);
    }

    #[test]
    fn measured_item_record_uses_measured_main_size_for_column_flex_item_auto_min() {
        let (mut root, container, context) = flex_container_with_context();
        let item = append_div(&mut root, container);
        root.style_mut(item)
            .set("display", "flex")
            .expect("display should set");
        root.style_mut(item)
            .set("flex-direction", "column")
            .expect("direction should set");
        // No explicit width and default visible overflow: the column shape is
        // outside the supported content-based auto-min subset, so the auto-min
        // falls back to the measured main size instead of panicking.
        let nested = append_div(&mut root, item);
        root.style_mut(nested)
            .set("width", "30px")
            .expect("nested width should set");
        root.style_mut(nested)
            .set("height", "20px")
            .expect("nested height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item_record = items.first().expect("child item");

        assert!(item_record.flex_base_size_px > 0.0);
        assert_eq!(
            item_record.automatic_min_main_size_px, item_record.flex_base_size_px,
            "unsupported column flex item auto-min should equal its measured main size",
        );
    }

    #[test]
    fn measured_item_record_uses_measured_main_size_for_reversed_and_wrapped_flex_item_auto_min() {
        for configure in [
            configure_row_reverse_item,
            configure_wrap_item,
            configure_rtl_row_item,
        ] {
            let (mut root, container, context) = flex_container_with_context();
            let item = append_div(&mut root, container);
            root.style_mut(item)
                .set("display", "flex")
                .expect("display should set");
            configure(&mut root, item);
            let nested = append_div(&mut root, item);
            root.style_mut(nested)
                .set("width", "30px")
                .expect("nested width should set");
            root.style_mut(nested)
                .set("height", "20px")
                .expect("nested height should set");

            let items = collect_items_for_container(&mut root, container, context);
            let item_record = items.first().expect("child item");

            assert!(item_record.flex_base_size_px > 0.0);
            assert_eq!(
                item_record.automatic_min_main_size_px, item_record.flex_base_size_px,
                "reversed/wrapped/rtl flex item auto-min should equal its measured main size",
            );
        }
    }

    #[test]
    fn measured_item_record_computes_content_based_auto_min_for_supported_row_flex_item() {
        let (mut root, container, context) = flex_container_with_context();
        let item = append_div(&mut root, container);
        root.style_mut(item)
            .set("display", "flex")
            .expect("display should set");
        // Supported shape: horizontal, non-reversed, no-wrap. The auto-min must
        // stay the content-based value (the text child's min-content width),
        // not degrade to the 10px flex-basis measured base.
        root.style_mut(item)
            .set("flex-basis", "10px")
            .expect("basis should set");
        let text = append_text(&mut root, item, "UnbreakableFlexLabel");

        let items = collect_items_for_container(&mut root, container, context);
        let item_record = items.first().expect("child item");
        let (text_style, _) = resolved_node_style_with_basis(
            &root,
            text,
            &InheritedStyleContext::default(),
            0.0,
            None,
        )
        .expect("text style");
        let expected_segment_width_px = super::measure_text_intrinsic_from_content_and_font_size(
            "UnbreakableFlexLabel",
            text_style.font_size_px,
        )
        .max_content_width_px;

        assert_eq!(item_record.flex_base_size_px, 10.0);
        assert_eq!(
            item_record.automatic_min_main_size_px,
            expected_segment_width_px
        );
    }

    #[test]
    fn measured_item_record_grandchild_unsupported_flex_shape_degrades_gracefully() {
        let (mut root, container, context) = flex_container_with_context();
        // row item (supported) -> row child (supported) -> column grandchild
        // (unsupported). The grandchild's unsupported shape must degrade to the
        // measured size through the recursion instead of panicking.
        let item = append_div(&mut root, container);
        root.style_mut(item)
            .set("display", "flex")
            .expect("item display should set");
        let child = append_div(&mut root, item);
        root.style_mut(child)
            .set("display", "flex")
            .expect("child display should set");
        let grandchild = append_div(&mut root, child);
        root.style_mut(grandchild)
            .set("display", "flex")
            .expect("grandchild display should set");
        root.style_mut(grandchild)
            .set("flex-direction", "column")
            .expect("grandchild direction should set");
        let leaf = append_div(&mut root, grandchild);
        root.style_mut(leaf)
            .set("width", "30px")
            .expect("leaf width should set");
        root.style_mut(leaf)
            .set("height", "20px")
            .expect("leaf height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item_record = items.first().expect("child item");

        // The unsupported column grandchild degrades to the measured size
        // threaded through the recursion (the enclosing row item's measured
        // main size), which over-estimates the grandchild's true ~30px
        // min-content instead of collapsing it or panicking.
        assert_eq!(
            item_record.automatic_min_main_size_px, item_record.flex_base_size_px,
            "grandchild degradation should thread the enclosing item's measured size",
        );
        assert!(
            item_record.automatic_min_main_size_px >= 30.0,
            "grandchild degradation must not collapse the grandchild's 30px leaf",
        );
    }

    #[test]
    fn measured_item_record_growing_child_uses_factor_blind_content_min() {
        let (mut root, container, context) = flex_container_with_context();
        // D6 gate 2 (flex arc step 4): a growing child no longer forces the
        // measured-size fallback — the record is grow-blind. A growing child
        // (flex-grow 1, height 10, no width/basis) has empty main-axis content,
        // so it contributes 0 to the item's h_min/h_max and the item's base and
        // auto-min are both 0 (not the stretched width the old gate returned).
        let item = append_div(&mut root, container);
        root.style_mut(item)
            .set("display", "flex")
            .expect("item display should set");
        let growing = append_div(&mut root, item);
        root.style_mut(growing)
            .set("flex-grow", "1")
            .expect("grow should set");
        root.style_mut(growing)
            .set("height", "10px")
            .expect("height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item_record = items.first().expect("child item");

        assert_eq!(item_record.flex_base_size_px, 0.0);
        assert_eq!(item_record.automatic_min_main_size_px, 0.0);
    }

    #[test]
    fn measured_item_record_growing_grandchild_degrades_to_measured_size() {
        let (mut root, container, context) = flex_container_with_context();
        // row item (supported) -> row child (supported) -> growing grandchild.
        // The growing grandchild must degrade to the measured size threaded
        // through the recursion instead of panicking.
        let item = append_div(&mut root, container);
        root.style_mut(item)
            .set("display", "flex")
            .expect("item display should set");
        let child = append_div(&mut root, item);
        root.style_mut(child)
            .set("display", "flex")
            .expect("child display should set");
        let grandchild = append_div(&mut root, child);
        root.style_mut(grandchild)
            .set("flex-grow", "1")
            .expect("grandchild grow should set");
        root.style_mut(grandchild)
            .set("height", "10px")
            .expect("grandchild height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item_record = items.first().expect("child item");

        assert_eq!(
            item_record.automatic_min_main_size_px, item_record.flex_base_size_px,
            "growing grandchild should thread the measured size instead of panicking",
        );
    }

    #[test]
    fn item_view_block_with_children_kind_reads_record_with_block_flow_sizes() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        let first = append_div(&mut root, child);
        root.style_mut(first)
            .set("width", "50px")
            .expect("first width should set");
        root.style_mut(first)
            .set("height", "20px")
            .expect("first height should set");
        let second = append_div(&mut root, child);
        root.style_mut(second)
            .set("width", "80px")
            .expect("second width should set");
        root.style_mut(second)
            .set("height", "30px")
            .expect("second height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("block item");
        let record = node_record(&root, child);

        // Widest child content contribution (80px) and stacked child heights.
        assert_eq!(record.h_min_content_px, 80.0);
        assert_eq!(record.h_max_content_px, 80.0);
        assert_eq!(record.content_height_at_width_px, 50.0);
        assert_item_view_reads_record(item, record, FlexAxis::Horizontal, 0.0, 0.0);
    }

    #[test]
    fn item_view_img_kind_reads_record_with_intrinsic_and_replaced_ratio_flag() {
        let (mut root, container, context) = flex_container_with_context();
        let img_key = root.create_element(DomTag::Img).expect("img should insert");
        root.append_child(container, img_key)
            .expect("img should append");
        root.set_attr(
            img_key,
            DomAttribute::Src,
            Some(DomAttributeValue::String("memory://a.png".to_string())),
        )
        .expect("src should set");
        root.style_mut(img_key)
            .set("width", "320px")
            .expect("width should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("img item");
        let record = node_record(&root, img_key);

        // Explicit width substitutes the intrinsic width for both content
        // contributions; the intrinsic ratio (160/90) derives the height.
        assert_eq!(record.h_min_content_px, 320.0);
        assert_eq!(record.h_max_content_px, 320.0);
        assert_eq!(record.content_height_at_width_px, 180.0);
        assert!(record.flags.replaced_ratio());
        assert!(!record.flags.preserve_mode());
        assert_item_view_reads_record(item, record, FlexAxis::Horizontal, 0.0, 0.0);
    }

    #[test]
    fn item_view_element_control_kind_reads_record_with_preferred_size_intrinsic() {
        let (mut root, container, context) = flex_container_with_context();
        let input_key = root
            .create_element(DomTag::Input)
            .expect("input should insert");
        root.append_child(container, input_key)
            .expect("input should append");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("input item");
        let record = node_record(&root, input_key);
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) =
            root.node(input_key).expect("input node").intrinsic()
        else {
            panic!("input should carry a preferred-size intrinsic");
        };

        assert_eq!(record.h_min_content_px, intrinsic.width_px);
        assert_eq!(record.h_max_content_px, intrinsic.width_px);
        assert_eq!(record.content_height_at_width_px, intrinsic.height_px);
        assert_item_view_reads_record(item, record, FlexAxis::Horizontal, 0.0, 0.0);
    }

    #[test]
    fn item_view_nested_flex_kind_reads_record_with_inner_resolved_container_sizes() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "100px")
            .expect("container width should set");
        root.style_mut(container)
            .set("height", "40px")
            .expect("container height should set");
        let nested = append_div(&mut root, container);
        root.style_mut(nested)
            .set("display", "flex")
            .expect("nested display should set");
        root.style_mut(nested)
            .set("width", "30px")
            .expect("nested width should set");
        root.style_mut(nested)
            .set("height", "20px")
            .expect("nested height should set");
        let nested_child = append_div(&mut root, nested);
        root.style_mut(nested_child)
            .set("width", "10px")
            .expect("nested child width should set");
        root.style_mut(nested_child)
            .set("height", "10px")
            .expect("nested child height should set");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("nested flex item");
        let record = node_record(&root, nested);

        // The inner container's record carries the inner solve's resolved
        // content sizes: h_min == h_max == resolved content width.
        assert_eq!(record.h_min_content_px, 30.0);
        assert_eq!(record.h_max_content_px, 30.0);
        assert_eq!(record.content_height_at_width_px, 20.0);
        assert_item_view_reads_record(item, record, FlexAxis::Horizontal, 0.0, 0.0);
    }

    #[test]
    fn item_view_reads_record_content_sizes_before_adding_own_border_padding() {
        let (mut root, container, context) = flex_container_with_context();
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("padding-left", "4px")
            .expect("left padding should set");
        root.style_mut(child)
            .set("padding-right", "6px")
            .expect("right padding should set");
        root.style_mut(child)
            .set("padding-top", "2px")
            .expect("top padding should set");
        root.style_mut(child)
            .set("padding-bottom", "3px")
            .expect("bottom padding should set");
        append_text(&mut root, child, "Flex label");

        let items = collect_items_for_container(&mut root, container, context);
        let item = items.first().expect("padded block item");
        let record = node_record(&root, child);
        let main_bp = 4.0 + 6.0;
        let cross_bp = 2.0 + 3.0;

        assert_eq!(item.main_axis_border_padding_px, main_bp);
        assert_item_view_reads_record(item, record, FlexAxis::Horizontal, main_bp, cross_bp);
    }

    #[test]
    // Purity pin: the staged measurement solve writes only each node's
    // measurement record (the sanctioned measurement output), never node
    // layout/box-model state.
    fn staged_measurement_solve_measures_nested_flex_without_writing_layout() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "90px")
            .expect("width should set");
        root.style_mut(container)
            .set("height", "20px")
            .expect("height should set");
        let nested = append_div(&mut root, container);
        let nested_child = append_div(&mut root, nested);
        root.style_mut(nested)
            .set("display", "flex")
            .expect("nested display should set");
        root.style_mut(nested)
            .set("height", "20px")
            .expect("nested height should set");
        root.style_mut(nested)
            .set("flex-basis", "20px")
            .expect("nested basis should set");
        root.style_mut(nested)
            .set("overflow", "scroll")
            .expect("nested overflow should set");
        root.style_mut(nested_child)
            .set("flex-basis", "10px")
            .expect("nested child basis should set");
        root.style_mut(nested_child)
            .set("height", "10px")
            .expect("nested child height should set");
        root.style_mut(nested_child)
            .set("overflow", "scroll")
            .expect("nested child overflow should set");

        let nested_layout_before = node_layout(&root, nested);
        let nested_box_before = node_box(&root, nested);
        let nested_dirty_before = node_layout_dirty(&root, nested);
        let nested_child_layout_before = node_layout(&root, nested_child);
        let nested_child_box_before = node_box(&root, nested_child);
        let nested_child_dirty_before = node_layout_dirty(&root, nested_child);

        let solve = build_solve_for_container(&mut root, container, context);

        let items = item_views_in_dom_order(&root.layout_scratch, solve);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].flex_base_size_px, 20.0);
        assert_eq!(node_layout(&root, nested), nested_layout_before);
        assert_eq!(node_box(&root, nested), nested_box_before);
        assert_eq!(node_layout_dirty(&root, nested), nested_dirty_before);
        assert_eq!(node_layout(&root, nested_child), nested_child_layout_before);
        assert_eq!(node_box(&root, nested_child), nested_child_box_before);
        assert_eq!(
            node_layout_dirty(&root, nested_child),
            nested_child_dirty_before
        );
    }

    #[test]
    // Purity pin: the staged measurement solve writes only each node's
    // measurement record (the sanctioned measurement output), never node
    // layout/box-model state.
    fn staged_measurement_solve_does_not_write_node_layout_or_box_model() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "40px")
            .expect("width should set");
        root.style_mut(container)
            .set("flex-wrap", "wrap")
            .expect("wrap should set");
        let child = append_div(&mut root, container);
        root.style_mut(child)
            .set("width", "24px")
            .expect("width should set");
        root.style_mut(child)
            .set("height", "10px")
            .expect("height should set");
        let text = append_text(&mut root, child, "Flex label");

        let container_layout_before = node_layout(&root, container);
        let container_box_before = node_box(&root, container);
        let container_dirty_before = node_layout_dirty(&root, container);
        let child_layout_before = node_layout(&root, child);
        let child_box_before = node_box(&root, child);
        let child_dirty_before = node_layout_dirty(&root, child);
        let text_layout_before = node_layout(&root, text);
        let text_box_before = node_box(&root, text);
        let text_dirty_before = node_layout_dirty(&root, text);

        let solve = build_solve_for_container(&mut root, container, context);

        let items = item_views_in_dom_order(&root.layout_scratch, solve);
        assert_eq!(items.len(), 1);
        assert_eq!(node_layout(&root, container), container_layout_before);
        assert_eq!(node_box(&root, container), container_box_before);
        assert_eq!(node_layout_dirty(&root, container), container_dirty_before);
        assert_eq!(node_layout(&root, child), child_layout_before);
        assert_eq!(node_box(&root, child), child_box_before);
        assert_eq!(node_layout_dirty(&root, child), child_dirty_before);
        assert_eq!(node_layout(&root, text), text_layout_before);
        assert_eq!(node_box(&root, text), text_box_before);
        assert_eq!(node_layout_dirty(&root, text), text_dirty_before);
    }

    #[test]
    fn staged_measurement_solve_orders_items_before_collecting_lines() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("width", "35px")
            .expect("width should set");
        root.style_mut(container)
            .set("flex-wrap", "wrap")
            .expect("wrap should set");
        let first = append_div(&mut root, container);
        let second = append_div(&mut root, container);
        let third = append_div(&mut root, container);
        root.style_mut(first)
            .set("width", "20px")
            .expect("width should set");
        root.style_mut(first)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(first)
            .set("order", "2")
            .expect("order should set");
        root.style_mut(second)
            .set("width", "15px")
            .expect("width should set");
        root.style_mut(second)
            .set("height", "10px")
            .expect("height should set");
        root.style_mut(second)
            .set("order", "-1")
            .expect("order should set");
        root.style_mut(third)
            .set("width", "20px")
            .expect("width should set");
        root.style_mut(third)
            .set("height", "10px")
            .expect("height should set");

        let solve = build_solve_for_container(&mut root, container, context);

        assert_eq!(
            next_sibling_key(&root, first).expect("first sibling"),
            Some(second),
        );
        assert_eq!(
            next_sibling_key(&root, second).expect("second sibling"),
            Some(third),
        );
        let dom_items = item_views_in_dom_order(&root.layout_scratch, solve);
        let ordered_items = item_views_in_order_modified_order(&root.layout_scratch, solve);
        assert_eq!(
            dom_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![first, second, third],
        );
        assert_eq!(
            ordered_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![second, third, first],
        );
        assert_eq!(
            built_lines(&root.layout_scratch, solve.order_range, solve.line_range),
            vec![
                TestLine {
                    item_sources: vec![source(second), source(third)],
                    outer_hypothetical_main_size_px: 35.0,
                    cross_size_px: 10.0,
                },
                TestLine {
                    item_sources: vec![source(first)],
                    outer_hypothetical_main_size_px: 20.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        assert_eq!(solve.main_size_px, 35.0);
        assert_eq!(solve.cross_size_px, 20.0);
    }

    #[test]
    fn resolve_measured_flex_container_uses_widest_line_and_total_cross_size() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 1.0, 10.0, 12.0),
            measured_row_item(source(keys[1]), 0.0, 1.0, 16.0, 18.0),
        ];
        let lines = vec![
            TestLine {
                item_sources: vec![source(keys[0])],
                outer_hypothetical_main_size_px: 10.0,
                cross_size_px: 12.0,
            },
            TestLine {
                item_sources: vec![source(keys[1])],
                outer_hypothetical_main_size_px: 16.0,
                cross_size_px: 18.0,
            },
        ];

        let mut scratch = LayoutScratch::default();
        let solve = solve_from_items_and_lines(&mut scratch, &items, &lines);

        assert_eq!(
            item_views_in_dom_order(&scratch, solve)
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            built_lines(&scratch, solve.order_range, solve.line_range),
            lines
        );
        assert_eq!(solve.main_size_px, 16.0);
        assert_eq!(solve.cross_size_px, 30.0);
        assert_eq!(solve.resolution_range, LaneRange::default());
    }
}
