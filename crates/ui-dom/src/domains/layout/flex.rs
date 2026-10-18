//! Flex solver entrypoint for the shared `TreeWalker`.
//!
//! This file preserves the existing visitor pattern used by layout solving:
//! enter the current node, recurse through children in tree order, then finish
//! the node on exit from the subtree. It exists to give flex its own solver
//! boundary without introducing a second traversal model or speculative cache.
//!
//! Invariants and rules:
//!
//! - flex solving stays inside the shared walker and shared enter/exit pattern
//! - transient solve state lives only for the current solver call stack
//! - this file must not persist solver scratch on nodes or elsewhere
//! - temporary state here must be algorithmic necessity, not caching for its
//!   own sake
//! - staged measurement helpers return records only
//! - flex container/item style is read visit-locally from the computed
//!   groups (`DomNode::computed_flex`; the container's text direction reads
//!   `DomNode::computed_text`, spine step 3.3f); the group `*Kw` enums map onto
//!   this solver's private `FlexAxis`/`FlexWrapMode`/alignment/basis vocabulary
//!   at one small boundary helper per enum
//! - final node layout writes use active flex placement with anonymous flex
//!   item generation for inline-level children per CSS Flexbox §4
#![allow(clippy::needless_pass_by_value)] // LayoutContext is passed by value as a child's owned context, mirroring flow.rs

use std::collections::HashMap;

use crate::domains::style::computed::{
    AlignContentKw, AlignItemsKw, FlexBasisKw, FlexDirectionKw, FlexWrapKw, JustifyContentKw,
    OverflowKw,
};
use crate::domains::visitor::inherited_style::InheritedStyleContext;
use crate::types::dom_node::measure_text_intrinsic_from_content_and_font_size;
use crate::types::{
    DomBoxModel, DomError, DomIntrinsic, DomNode, DomNodeSlotKey, DomRect, DomTextDirection,
    RootDom,
};

use super::DomLayoutResolver;
use super::flex_item_source::FlexItemSource;
use super::flex_wrap_mode::FlexWrapMode;
use super::flow::{
    EnteredNodeLayout, FlowSize, InlineFlowState, LayoutContext, auto_content_width_for_exit,
    border_box_height, border_box_width, child_layout_context, content_width_contribution_for_exit,
    display_is_inline_outer, enter_node, entered_content_size_property_style, exit_node,
    first_child_key, intrinsic_content_width, layout_flow_size, next_inline_child_context,
    next_sibling_key, non_negative_auto_content_height, relayout_tree_walker_node,
    resolved_content_size_px, resolved_node_style, wrapped_text_content_height_px,
};
use super::style::{BoxSides, LayoutDisplay, NodeStyle, PositionMode};

#[cfg(test)]
use crate::domains::layout::IdentityDomLayoutResolver;
#[cfg(test)]
use crate::domains::layout::style::{layout_dirty_from_ref, node_box_from_ref};
#[cfg(test)]
use crate::domains::style::computed::BoxSizingKw;
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{BoxEdgeInsets, DomLayerKind, DomLayoutState, DomTag, RootDomContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlexAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlexMainAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlexCrossAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlexLineCrossAxisAlignment {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FlexGapSizes {
    between_items_px: f32,
    between_lines_px: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FlexBasisSource {
    Auto,
    Content,
    Zero,
    Length(f32),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct FlexContainerStyle {
    main_axis: FlexAxis,
    is_main_reversed: bool,
    text_direction: DomTextDirection,
    wrap_mode: FlexWrapMode,
    justify_content: FlexMainAxisAlignment,
    align_items: FlexCrossAxisAlignment,
    align_content: FlexLineCrossAxisAlignment,
    gap_sizes: FlexGapSizes,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeasuredFlexItem {
    source: FlexItemSource,
    order: i32,
    main_axis: FlexAxis,
    cross_axis: FlexAxis,
    align_self: Option<FlexCrossAxisAlignment>,
    flex_grow_factor: f32,
    flex_shrink_factor: f32,
    flex_base_size_px: f32,
    hypothetical_main_size_px: f32,
    automatic_min_main_size_px: f32,
    min_main_size_px: Option<f32>,
    max_main_size_px: Option<f32>,
    main_axis_border_padding_px: f32,
    outer_hypothetical_main_size_px: f32,
    measured_cross_size_input_px: f32,
    margin_main_start_px: f32,
    margin_main_end_px: f32,
    margin_cross_start_px: f32,
    margin_cross_end_px: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct FlexLine {
    item_sources: Vec<FlexItemSource>,
    outer_hypothetical_main_size_px: f32,
    cross_size_px: f32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedFlexItem {
    source: FlexItemSource,
    align_self: Option<FlexCrossAxisAlignment>,
    resolved_main_size_px: f32,
    main_axis_border_padding_px: f32,
    resolved_cross_size_px: f32,
    main_offset_px: f32,
    cross_offset_px: f32,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct FlexContainerSolve {
    measured_items_in_dom_order: Vec<MeasuredFlexItem>,
    line_collection_items: Vec<MeasuredFlexItem>,
    lines: Vec<FlexLine>,
    line_cross_sizes_px: Vec<f32>,
    resolved_items: Vec<ResolvedFlexItem>,
    main_size_px: f32,
    cross_size_px: f32,
}

#[derive(Clone, Copy, Debug)]
struct FlexLineItemResolutionState {
    measured_item: MeasuredFlexItem,
    resolved_main_size_px: f32,
    is_frozen: bool,
}

#[derive(Clone, Debug)]
struct PlacedFlexChildren {
    resolved_items: Vec<ResolvedFlexItem>,
    container_cross_size_px: f32,
}

struct StagedMeasurementRequest {
    key: DomNodeSlotKey,
    child_context: LayoutContext,
    container_style: FlexContainerStyle,
    container_main_size_px: Option<f32>,
    use_order_modified_sequence: bool,
}

struct ActiveFlexExitRequest {
    key: DomNodeSlotKey,
    context: LayoutContext,
    entered: EnteredNodeLayout,
    child_context: LayoutContext,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
}

struct ActiveFlexPlacementRequest<'a> {
    key: DomNodeSlotKey,
    child_context: &'a LayoutContext,
    container_style: FlexContainerStyle,
    lines: &'a [FlexLine],
    line_cross_sizes_px: &'a [f32],
    container_main_size_px: f32,
    resolved_items: &'a [ResolvedFlexItem],
}

struct ActiveFlexChildRelayoutRequest<'a> {
    source: FlexItemSource,
    child_context: &'a LayoutContext,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    line_cross_size_px: f32,
    resolved_item: ResolvedFlexItem,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MeasuredBlockFlowState {
    block_top_px: f32,
    inline_left_px: f32,
    line_height_px: f32,
    max_width_px: f32,
    line_min_width_px: f32,
    line_max_width_px: f32,
    min_width_px: f32,
    max_content_width_px: f32,
    has_children: bool,
}

pub(super) fn relayout_flex_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    relayout_active_flex_node(
        root_dom,
        tree_size,
        key,
        context,
        style,
        inherited_style,
        resolver,
    )
}

fn relayout_active_flex_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
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
    let container_main_size_px = entered_container_main_size(&entered, container_style.main_axis);
    let child_context = child_layout_context(context.clone(), entered.clone());
    let container_cross_size_px = entered_container_cross_size(&entered, container_style.main_axis);
    let measured_solve = build_staged_measurement_solve_from_container(
        root_dom,
        tree_size,
        &StagedMeasurementRequest {
            key,
            child_context: child_context.clone(),
            container_style,
            container_main_size_px,
            use_order_modified_sequence: true,
        },
        resolver,
    )?;
    let resolved_container_main_size_px =
        container_main_size_px.unwrap_or(measured_solve.main_size_px);
    let resolved_solve = resolve_active_flex_container(
        measured_solve,
        container_style,
        resolved_container_main_size_px,
        container_cross_size_px,
    );
    exit_active_flex_container(
        root_dom,
        tree_size,
        ActiveFlexExitRequest {
            key,
            context,
            entered,
            child_context,
            container_style,
            solve: resolved_solve,
        },
        resolver,
    )
}

#[cfg(test)]
fn build_staged_measurement_solve<R>(
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
            use_order_modified_sequence: true,
        },
        resolver,
    )
}

fn build_staged_measurement_solve_from_container<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &StagedMeasurementRequest,
    resolver: &mut R,
) -> Result<FlexContainerSolve, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let measured_items_in_dom_order = collect_measured_flex_items_in_dom_order(
        root_dom,
        tree_size,
        request.key,
        &request.child_context,
        request.container_style,
        resolver,
    )?;
    let line_collection_items = if request.use_order_modified_sequence {
        order_modified_measured_items(&measured_items_in_dom_order)
    } else {
        measured_items_in_dom_order.clone()
    };
    let lines = build_measured_flex_lines(
        &line_collection_items,
        request.container_style.wrap_mode,
        request.container_main_size_px,
        request.container_style.gap_sizes.between_items_px,
    );
    Ok(resolve_measured_flex_container(
        measured_items_in_dom_order,
        line_collection_items,
        lines,
    ))
}

fn collect_measured_flex_items_in_dom_order<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    child_context: &LayoutContext,
    container_style: FlexContainerStyle,
    resolver: &mut R,
) -> Result<Vec<MeasuredFlexItem>, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let mut items = Vec::new();
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let source = FlexItemSource::BlockItem(current_child_key);
        let item = measure_one_flex_item(
            root_dom,
            tree_size,
            source,
            child_context,
            container_style.main_axis,
            resolver,
        )?;
        items.push(item);
        child_key = next_child_key;
    }
    Ok(items)
}

fn measure_one_flex_item<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    source: FlexItemSource,
    child_context: &LayoutContext,
    main_axis: FlexAxis,
    resolver: &mut R,
) -> Result<MeasuredFlexItem, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let child_key = source.key();
    let (child_style, child_inherited_style) =
        resolved_node_style(root_dom, child_key, &child_context.inherited_style)?;
    let measured_flow = measure_tree_walker_node_without_layout_write(
        root_dom,
        tree_size,
        child_key,
        child_context,
        child_style,
        child_inherited_style,
        resolver,
    )?;
    let measured_main_size_px =
        main_axis_border_box_size_from_flow(main_axis, measured_flow, child_style);
    let measured_cross_size_px =
        cross_axis_border_box_size_from_flow(main_axis, measured_flow, child_style);
    let node = node_for_key(root_dom, child_key)?;
    let order = item_order_from_node(node);
    let align_self = item_align_self(node);
    let flex_grow_factor = item_flex_grow_factor_from_node(node);
    let flex_shrink_factor = item_flex_shrink_factor_from_node(node);
    let main_axis_border_padding_px =
        main_axis_border_padding_size_px(child_style.sides, main_axis);
    let flex_base_size_px =
        measured_flex_base_size_px(node, child_style, main_axis, measured_main_size_px);
    let automatic_min_main_size_px = supported_automatic_min_main_size_px(
        root_dom,
        child_key,
        node,
        child_style,
        &child_inherited_style,
        main_axis,
        measured_main_size_px,
    )?;
    let hypothetical_main_size_px = clamp_main_axis_size(child_style, main_axis, flex_base_size_px);
    let (min_main_size_px, max_main_size_px) = main_axis_limits(child_style, main_axis);
    let margin_main_start_px = main_axis_start_margin(child_style, main_axis);
    let margin_main_end_px = main_axis_end_margin(child_style, main_axis);
    let margin_cross_start_px = cross_axis_start_margin(child_style, main_axis);
    let margin_cross_end_px = cross_axis_end_margin(child_style, main_axis);
    Ok(MeasuredFlexItem {
        source,
        order,
        main_axis,
        cross_axis: cross_axis(main_axis),
        align_self,
        flex_grow_factor,
        flex_shrink_factor,
        flex_base_size_px,
        hypothetical_main_size_px,
        automatic_min_main_size_px,
        min_main_size_px,
        max_main_size_px,
        main_axis_border_padding_px,
        outer_hypothetical_main_size_px: hypothetical_main_size_px
            + margin_main_start_px
            + margin_main_end_px,
        measured_cross_size_input_px: measured_cross_size_px,
        margin_main_start_px,
        margin_main_end_px,
        margin_cross_start_px,
        margin_cross_end_px,
    })
}

fn measure_tree_walker_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
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
                style,
                inherited_style,
                resolver,
            )
        }
        LayoutDisplay::Inline => measure_inline_node_without_layout_write(
            root_dom,
            tree_size,
            key,
            context,
            style,
            inherited_style,
            resolver,
        ),
        LayoutDisplay::Flex => measure_flex_node_without_layout_write(
            root_dom,
            tree_size,
            key,
            context,
            style,
            inherited_style,
            resolver,
        ),
        LayoutDisplay::InlineFlex => {
            unreachable!("pure flex measurement only supports block-level flex containers",)
        }
        LayoutDisplay::None => unreachable!("display none handled before measurement dispatch"),
    }
}

fn measure_flex_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
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
            child_context,
            container_style,
            container_main_size_px,
            use_order_modified_sequence: true,
        },
        resolver,
    )?;
    let resolved_container_main_size_px =
        container_main_size_px.unwrap_or(measured_solve.main_size_px);
    let resolved_solve = resolve_active_flex_container(
        measured_solve,
        container_style,
        resolved_container_main_size_px,
        entered_container_cross_size(&entered, container_style.main_axis),
    );
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
    measure_exited_node_flow_size(
        root_dom,
        key,
        &entered,
        auto_content_width_px,
        (auto_content_width_px, auto_content_width_px),
        active_flex_content_height_px(
            container_style.main_axis,
            resolved_solve.main_size_px,
            resolved_solve.cross_size_px,
        ),
    )
}

fn measure_block_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
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
            resolved_node_style(root_dom, current_child_key, &child_context.inherited_style)?;
        let next_child_context =
            measured_block_child_context(child_context.clone(), flow, child_style.display);
        let child_flow = measure_tree_walker_node_without_layout_write(
            root_dom,
            tree_size,
            current_child_key,
            &next_child_context,
            child_style,
            child_inherited_style,
            resolver,
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
    )
}

fn measure_inline_node_without_layout_write<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: &LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
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
            resolved_node_style(root_dom, current_child_key, &child_context.inherited_style)?;
        let next_child_context = next_inline_child_context(child_context.clone(), flow);
        let child_flow = measure_tree_walker_node_without_layout_write(
            root_dom,
            tree_size,
            current_child_key,
            &next_child_context,
            child_style,
            child_inherited_style,
            resolver,
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
    let auto_content_width_px = measured_inline_content_width(&entered, flow);
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
    )
}

fn measure_exited_node_flow_size(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    auto_content_width_px: f32,
    auto_content_width_contribution_px: (f32, f32),
    auto_content_height_px: f32,
) -> Result<FlowSize, DomError> {
    let (auto_min_content_width_px, auto_max_content_width_px) = auto_content_width_contribution_px;
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
    Ok(layout_flow_size(
        entered.style,
        measured_box_model(entered.box_model, entered.style, content_size),
        min_content_width_px,
        max_content_width_px,
    ))
}

fn measured_auto_content_height(
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
        wrapped_text_content_height_px(node, &entered.inherited_style, content_width_px)
            .unwrap_or(fallback_content_height_px.max(0.0));
    Ok(non_negative_auto_content_height(content_height_px))
}

fn measured_box_model(
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

fn measured_block_child_context(
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

fn update_measured_block_flow(
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

fn measured_block_content_height(
    intrinsic_content_height_px: Option<f32>,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return non_negative_auto_content_height(flow.block_top_px + flow.line_height_px);
    }
    intrinsic_content_height_px.unwrap_or(0.0)
}

fn measured_block_content_width(
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

fn measured_block_min_content_width(
    entered: &EnteredNodeLayout,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

fn measured_block_max_content_width(
    entered: &EnteredNodeLayout,
    flow: MeasuredBlockFlowState,
) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

fn measured_inline_content_width(entered: &EnteredNodeLayout, flow: InlineFlowState) -> f32 {
    if !flow.has_children && entered.is_text_node {
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

fn measured_inline_min_content_width(entered: &EnteredNodeLayout, flow: InlineFlowState) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

fn measured_inline_max_content_width(entered: &EnteredNodeLayout, flow: InlineFlowState) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

fn staged_flex_container_style(node: &DomNode) -> FlexContainerStyle {
    let flex = node.computed_flex();
    let (main_axis, is_main_reversed) = flex_direction_from_kw(flex.direction);
    FlexContainerStyle {
        main_axis,
        is_main_reversed,
        text_direction: effective_flex_container_text_direction(node),
        wrap_mode: flex_wrap_mode_from_kw(flex.wrap),
        justify_content: justify_content_from_kw(flex.justify_content),
        align_items: align_items_from_kw(flex.align_items),
        align_content: align_content_from_kw(flex.align_content),
        gap_sizes: flex_gap_sizes(flex.gap, main_axis),
    }
}

fn flex_direction_from_kw(kw: FlexDirectionKw) -> (FlexAxis, bool) {
    match kw {
        FlexDirectionKw::Row => (FlexAxis::Horizontal, false),
        FlexDirectionKw::RowReverse => (FlexAxis::Horizontal, true),
        FlexDirectionKw::Column => (FlexAxis::Vertical, false),
        FlexDirectionKw::ColumnReverse => (FlexAxis::Vertical, true),
    }
}

fn flex_wrap_mode_from_kw(kw: FlexWrapKw) -> FlexWrapMode {
    match kw {
        FlexWrapKw::NoWrap => FlexWrapMode::NoWrap,
        FlexWrapKw::Wrap => FlexWrapMode::Wrap,
        FlexWrapKw::WrapReverse => FlexWrapMode::WrapReverse,
    }
}

fn justify_content_from_kw(kw: JustifyContentKw) -> FlexMainAxisAlignment {
    match kw {
        JustifyContentKw::Start => FlexMainAxisAlignment::Start,
        JustifyContentKw::End => FlexMainAxisAlignment::End,
        JustifyContentKw::FlexStart => FlexMainAxisAlignment::FlexStart,
        JustifyContentKw::FlexEnd => FlexMainAxisAlignment::FlexEnd,
        JustifyContentKw::Center => FlexMainAxisAlignment::Center,
        JustifyContentKw::SpaceBetween => FlexMainAxisAlignment::SpaceBetween,
        JustifyContentKw::SpaceAround => FlexMainAxisAlignment::SpaceAround,
        JustifyContentKw::SpaceEvenly => FlexMainAxisAlignment::SpaceEvenly,
    }
}

fn align_items_from_kw(kw: AlignItemsKw) -> FlexCrossAxisAlignment {
    match kw {
        AlignItemsKw::Start => FlexCrossAxisAlignment::Start,
        AlignItemsKw::End => FlexCrossAxisAlignment::End,
        AlignItemsKw::FlexStart => FlexCrossAxisAlignment::FlexStart,
        AlignItemsKw::FlexEnd => FlexCrossAxisAlignment::FlexEnd,
        AlignItemsKw::Center => FlexCrossAxisAlignment::Center,
        AlignItemsKw::Stretch => FlexCrossAxisAlignment::Stretch,
    }
}

fn align_content_from_kw(kw: AlignContentKw) -> FlexLineCrossAxisAlignment {
    match kw {
        AlignContentKw::Start => FlexLineCrossAxisAlignment::Start,
        AlignContentKw::End => FlexLineCrossAxisAlignment::End,
        AlignContentKw::FlexStart => FlexLineCrossAxisAlignment::FlexStart,
        AlignContentKw::FlexEnd => FlexLineCrossAxisAlignment::FlexEnd,
        AlignContentKw::Center => FlexLineCrossAxisAlignment::Center,
        AlignContentKw::Stretch => FlexLineCrossAxisAlignment::Stretch,
        AlignContentKw::SpaceBetween => FlexLineCrossAxisAlignment::SpaceBetween,
        AlignContentKw::SpaceAround => FlexLineCrossAxisAlignment::SpaceAround,
        AlignContentKw::SpaceEvenly => FlexLineCrossAxisAlignment::SpaceEvenly,
    }
}

fn flex_basis_source_from_kw(kw: FlexBasisKw) -> FlexBasisSource {
    match kw {
        FlexBasisKw::Auto => FlexBasisSource::Auto,
        FlexBasisKw::Content => FlexBasisSource::Content,
        FlexBasisKw::Length(0.0) => FlexBasisSource::Zero,
        FlexBasisKw::Length(px) => FlexBasisSource::Length(px),
    }
}

fn effective_flex_container_text_direction(node: &DomNode) -> DomTextDirection {
    node.computed_text().direction
}

fn item_align_self(node: &DomNode) -> Option<FlexCrossAxisAlignment> {
    node.computed_flex().align_self.map(align_items_from_kw)
}

fn flex_gap_sizes(gap: [f32; 2], main_axis: FlexAxis) -> FlexGapSizes {
    let [row_gap_px, column_gap_px] = gap;
    match main_axis {
        FlexAxis::Horizontal => FlexGapSizes {
            between_items_px: column_gap_px,
            between_lines_px: row_gap_px,
        },
        FlexAxis::Vertical => FlexGapSizes {
            between_items_px: row_gap_px,
            between_lines_px: column_gap_px,
        },
    }
}

fn inline_start_is_reversed(text_direction: DomTextDirection) -> bool {
    text_direction == DomTextDirection::Rtl
}

fn logical_main_axis_start_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => inline_start_is_reversed(container_style.text_direction),
        FlexAxis::Vertical => false,
    }
}

fn logical_cross_axis_start_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => false,
        FlexAxis::Vertical => inline_start_is_reversed(container_style.text_direction),
    }
}

fn resolved_main_axis_is_reversed(container_style: FlexContainerStyle) -> bool {
    match container_style.main_axis {
        FlexAxis::Horizontal => {
            container_style.is_main_reversed
                ^ inline_start_is_reversed(container_style.text_direction)
        }
        FlexAxis::Vertical => container_style.is_main_reversed,
    }
}

fn resolved_cross_axis_is_reversed(container_style: FlexContainerStyle) -> bool {
    logical_cross_axis_start_is_reversed(container_style)
        ^ container_style.wrap_mode.is_cross_reversed()
}

fn main_axis_logical_start_matches_flex_start(container_style: FlexContainerStyle) -> bool {
    logical_main_axis_start_is_reversed(container_style)
        == resolved_main_axis_is_reversed(container_style)
}

fn cross_axis_logical_start_matches_flex_start(container_style: FlexContainerStyle) -> bool {
    logical_cross_axis_start_is_reversed(container_style)
        == resolved_cross_axis_is_reversed(container_style)
}

fn resolved_main_axis_alignment(
    justify_content: FlexMainAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexMainAxisAlignment {
    match justify_content {
        FlexMainAxisAlignment::Start => {
            if main_axis_logical_start_matches_flex_start(container_style) {
                FlexMainAxisAlignment::FlexStart
            } else {
                FlexMainAxisAlignment::FlexEnd
            }
        }
        FlexMainAxisAlignment::End => {
            if main_axis_logical_start_matches_flex_start(container_style) {
                FlexMainAxisAlignment::FlexEnd
            } else {
                FlexMainAxisAlignment::FlexStart
            }
        }
        _ => justify_content,
    }
}

fn resolved_cross_axis_alignment(
    alignment: FlexCrossAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexCrossAxisAlignment {
    match alignment {
        FlexCrossAxisAlignment::Start => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexCrossAxisAlignment::FlexStart
            } else {
                FlexCrossAxisAlignment::FlexEnd
            }
        }
        FlexCrossAxisAlignment::End => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexCrossAxisAlignment::FlexEnd
            } else {
                FlexCrossAxisAlignment::FlexStart
            }
        }
        _ => alignment,
    }
}

fn resolved_line_cross_axis_alignment(
    align_content: FlexLineCrossAxisAlignment,
    container_style: FlexContainerStyle,
) -> FlexLineCrossAxisAlignment {
    match align_content {
        FlexLineCrossAxisAlignment::Start => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexLineCrossAxisAlignment::FlexStart
            } else {
                FlexLineCrossAxisAlignment::FlexEnd
            }
        }
        FlexLineCrossAxisAlignment::End => {
            if cross_axis_logical_start_matches_flex_start(container_style) {
                FlexLineCrossAxisAlignment::FlexEnd
            } else {
                FlexLineCrossAxisAlignment::FlexStart
            }
        }
        _ => align_content,
    }
}

fn item_order_from_node(node: &DomNode) -> i32 {
    node.computed_flex().order
}

fn item_flex_grow_factor_from_node(node: &DomNode) -> f32 {
    node.computed_flex().grow
}

fn item_flex_shrink_factor_from_node(node: &DomNode) -> f32 {
    node.computed_flex().shrink
}

fn flex_basis_source_from_node(node: &DomNode) -> FlexBasisSource {
    flex_basis_source_from_kw(node.computed_flex().basis)
}

fn preferred_flex_basis_px(node: &DomNode) -> Option<f32> {
    match flex_basis_source_from_node(node) {
        FlexBasisSource::Auto | FlexBasisSource::Content => None,
        FlexBasisSource::Zero => Some(0.0),
        FlexBasisSource::Length(length_px) => Some(length_px),
    }
}

fn measured_flex_base_size_px(
    node: &DomNode,
    style: NodeStyle,
    main_axis: FlexAxis,
    measured_border_box_main_size_px: f32,
) -> f32 {
    preferred_flex_basis_px(node).map_or(measured_border_box_main_size_px, |basis_px| {
        basis_px + main_axis_border_padding_size_px(style.sides, main_axis)
    })
}

fn supported_automatic_min_main_size_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    node: &DomNode,
    style: NodeStyle,
    inherited_style: &InheritedStyleContext,
    main_axis: FlexAxis,
    measured_main_size_px: f32,
) -> Result<f32, DomError> {
    if supported_auto_min_resolves_to_zero(node) {
        return Ok(0.0);
    }
    if style_has_definite_main_axis_size(style, main_axis) {
        return Ok(measured_main_size_px);
    }
    content_based_auto_min_main_size_px(
        root_dom,
        key,
        style,
        inherited_style,
        main_axis,
        measured_main_size_px,
    )
}

fn supported_auto_min_resolves_to_zero(node: &DomNode) -> bool {
    node.computed_box().overflow != OverflowKw::Visible
}

fn content_based_auto_min_main_size_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    style: NodeStyle,
    inherited_style: &InheritedStyleContext,
    main_axis: FlexAxis,
    measured_main_size_px: f32,
) -> Result<f32, DomError> {
    if main_axis == FlexAxis::Vertical {
        return Ok(measured_main_size_px);
    }
    let content_width_px =
        horizontal_content_based_auto_min_content_width_px(root_dom, key, style, inherited_style)?;
    Ok(border_box_width(style.sides, content_width_px))
}

fn horizontal_content_based_auto_min_content_width_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    style: NodeStyle,
    inherited_style: &InheritedStyleContext,
) -> Result<f32, DomError> {
    let node = node_for_key(root_dom, key)?;
    if let Some(intrinsic_content_width_px) =
        node_intrinsic_horizontal_content_width_px(node, inherited_style)
    {
        return Ok(intrinsic_content_width_px);
    }
    if style.display == LayoutDisplay::Flex {
        return horizontal_flex_container_content_based_auto_min_content_width_px(
            root_dom,
            key,
            inherited_style,
        );
    }
    let mut content_width_px: f32 = 0.0;
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let (child_style, child_inherited_style) =
            resolved_node_style(root_dom, current_child_key, inherited_style)?;
        if child_style.display == LayoutDisplay::None
            || matches!(
                child_style.position,
                PositionMode::Absolute | PositionMode::Fixed
            )
        {
            child_key = next_child_key;
            continue;
        }
        content_width_px =
            content_width_px.max(horizontal_content_based_auto_min_content_width_px(
                root_dom,
                current_child_key,
                child_style,
                &child_inherited_style,
            )?);
        child_key = next_child_key;
    }
    Ok(content_width_px)
}

fn horizontal_flex_container_content_based_auto_min_content_width_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    inherited_style: &InheritedStyleContext,
) -> Result<f32, DomError> {
    let container_node = node_for_key(root_dom, key)?;
    // The active-solver gate used to reject a flex-flow `wrap-reverse` token at
    // the stringly direction gate (`supported_flex_direction_from_flow_value`
    // returned `None`). The typed flex group cannot distinguish `flex-flow:
    // wrap-reverse` from the `flex-wrap: wrap-reverse` longhand, so it maps both
    // to `FlexWrapKw::WrapReverse`; the `flex_container_content_based_auto_min_is_supported`
    // assert below (which requires `NoWrap`) preserves the reject verdict.
    let container_style = staged_flex_container_style(container_node);
    assert!(
        flex_container_content_based_auto_min_is_supported(container_style),
        "visible-overflow flex auto-min should stay inside the supported intrinsic subset",
    );
    let mut content_width_px = 0.0;
    let mut has_previous_item = false;
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let (child_style, child_inherited_style) =
            resolved_node_style(root_dom, current_child_key, inherited_style)?;
        if child_style.display == LayoutDisplay::None
            || matches!(
                child_style.position,
                PositionMode::Absolute | PositionMode::Fixed
            )
        {
            child_key = next_child_key;
            continue;
        }
        if has_previous_item {
            content_width_px += container_style.gap_sizes.between_items_px;
        }
        let child_content_width_px = horizontal_content_based_auto_min_content_width_px(
            root_dom,
            current_child_key,
            child_style,
            &child_inherited_style,
        )?;
        content_width_px += horizontal_flex_container_auto_min_child_border_box_contribution_px(
            node_for_key(root_dom, current_child_key)?,
            child_style,
            border_box_width(child_style.sides, child_content_width_px),
        ) + child_style.sides.margin.left_px
            + child_style.sides.margin.right_px;
        has_previous_item = true;
        child_key = next_child_key;
    }
    Ok(content_width_px)
}

fn horizontal_flex_container_auto_min_child_border_box_contribution_px(
    child_node: &DomNode,
    child_style: NodeStyle,
    child_border_box_min_width_px: f32,
) -> f32 {
    assert!(
        item_flex_grow_factor_from_node(child_node) == 0.0,
        "visible-overflow flex container auto-min should only measure non-growing children",
    );
    if item_flex_shrink_factor_from_node(child_node) == 0.0 {
        return child_border_box_min_width_px.max(
            horizontal_flex_item_definite_base_border_box_width_px(child_node, child_style)
                .unwrap_or(child_border_box_min_width_px),
        );
    }
    child_border_box_min_width_px
}

fn horizontal_flex_item_definite_base_border_box_width_px(
    child_node: &DomNode,
    child_style: NodeStyle,
) -> Option<f32> {
    let base_content_width_px = preferred_flex_basis_px(child_node).or(child_style.width_px)?;
    Some(
        base_content_width_px
            + main_axis_border_padding_size_px(child_style.sides, FlexAxis::Horizontal),
    )
}

fn flex_container_content_based_auto_min_is_supported(container_style: FlexContainerStyle) -> bool {
    container_style.main_axis == FlexAxis::Horizontal
        && container_style.wrap_mode == FlexWrapMode::NoWrap
        && !resolved_main_axis_is_reversed(container_style)
}

fn node_intrinsic_horizontal_content_width_px(
    node: &DomNode,
    inherited_style: &InheritedStyleContext,
) -> Option<f32> {
    match node {
        DomNode::Text(text) => Some(text_min_content_width_px(
            text.data(),
            inherited_style.effective_font_size_px(),
        )),
        _ => match node.intrinsic() {
            DomIntrinsic::Img(intrinsic) => Some(intrinsic.width_px),
            DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => Some(intrinsic.width_px),
            DomIntrinsic::None | DomIntrinsic::NotMeasured | DomIntrinsic::Text(_) => None,
        },
    }
}

fn text_min_content_width_px(text: &str, font_size_px: f32) -> f32 {
    let mut widest_segment_width_px: f32 = 0.0;
    for segment in text.split_whitespace() {
        widest_segment_width_px = widest_segment_width_px.max(
            measure_text_intrinsic_from_content_and_font_size(segment, font_size_px)
                .max_content_width_px,
        );
    }
    if widest_segment_width_px > 0.0 {
        return widest_segment_width_px;
    }
    measure_text_intrinsic_from_content_and_font_size(text, font_size_px).max_content_width_px
}

fn clamp_main_axis_size(style: NodeStyle, main_axis: FlexAxis, value_px: f32) -> f32 {
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

fn main_axis_limits(style: NodeStyle, main_axis: FlexAxis) -> (Option<f32>, Option<f32>) {
    match main_axis {
        FlexAxis::Horizontal => (style.min_width_px, style.max_width_px),
        FlexAxis::Vertical => (style.min_height_px, style.max_height_px),
    }
}

fn style_has_definite_main_axis_size(style: NodeStyle, main_axis: FlexAxis) -> bool {
    match main_axis {
        FlexAxis::Horizontal => style.width_px.is_some(),
        FlexAxis::Vertical => style.height_px.is_some(),
    }
}

fn style_has_definite_cross_axis_size(style: NodeStyle, main_axis: FlexAxis) -> bool {
    match main_axis {
        FlexAxis::Horizontal => style.height_px.is_some(),
        FlexAxis::Vertical => style.width_px.is_some(),
    }
}

fn entered_container_main_size(entered: &EnteredNodeLayout, main_axis: FlexAxis) -> Option<f32> {
    match main_axis {
        FlexAxis::Horizontal => Some(entered.content_width_px),
        FlexAxis::Vertical => entered.style.height_px,
    }
}

fn entered_container_cross_size(entered: &EnteredNodeLayout, main_axis: FlexAxis) -> Option<f32> {
    match main_axis {
        FlexAxis::Horizontal => entered.style.height_px,
        FlexAxis::Vertical => Some(entered.content_width_px),
    }
}

fn order_modified_measured_items(items: &[MeasuredFlexItem]) -> Vec<MeasuredFlexItem> {
    let mut ordered_items = items.to_vec();
    ordered_items.sort_by_key(|item| item.order);
    ordered_items
}

fn build_measured_flex_lines(
    items: &[MeasuredFlexItem],
    wrap_mode: FlexWrapMode,
    container_main_size_px: Option<f32>,
    gap_between_items_px: f32,
) -> Vec<FlexLine> {
    // CSS Flexbox 9.2 (#algo-line-breaking): collect items into lines by
    // adding each item's outer hypothetical main size plus the inter-item
    // gap and breaking when the line would exceed the container's inner
    // main size. This is the same greedy line break as Blink's
    // `flex_line_breaker.cc` and Gecko's line-building loop in
    // nsFlexContainerFrame.cpp (`GenerateFlexLines`).
    if items.is_empty() {
        return Vec::new();
    }
    if !wrap_mode.is_wrapping() || container_main_size_px.is_none() {
        return vec![build_measured_flex_line(items, gap_between_items_px)];
    }
    let line_break_length_px = container_main_size_px.expect("line break length should exist");
    let mut lines = Vec::new();
    let mut line_start_index = 0usize;
    let mut line_outer_main_size_px = 0.0;
    for (index, item) in items.iter().enumerate() {
        let gap_before_item_px = if line_outer_main_size_px == 0.0 {
            0.0
        } else {
            gap_between_items_px
        };
        let next_outer_main_size_px =
            line_outer_main_size_px + gap_before_item_px + item.outer_hypothetical_main_size_px;
        if line_outer_main_size_px > 0.0 && next_outer_main_size_px > line_break_length_px {
            lines.push(build_measured_flex_line(
                &items[line_start_index..index],
                gap_between_items_px,
            ));
            line_start_index = index;
            line_outer_main_size_px = item.outer_hypothetical_main_size_px;
            continue;
        }
        line_outer_main_size_px += gap_before_item_px + item.outer_hypothetical_main_size_px;
    }
    lines.push(build_measured_flex_line(
        &items[line_start_index..],
        gap_between_items_px,
    ));
    if wrap_mode.is_cross_reversed() {
        lines.reverse();
    }
    lines
}

fn build_measured_flex_line(items: &[MeasuredFlexItem], gap_between_items_px: f32) -> FlexLine {
    let mut line = FlexLine {
        item_sources: Vec::with_capacity(items.len()),
        outer_hypothetical_main_size_px: 0.0,
        cross_size_px: 0.0,
    };
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            line.outer_hypothetical_main_size_px += gap_between_items_px;
        }
        line.item_sources.push(item.source);
        line.outer_hypothetical_main_size_px += item.outer_hypothetical_main_size_px;
        line.cross_size_px = line.cross_size_px.max(
            item.measured_cross_size_input_px
                + item.margin_cross_start_px
                + item.margin_cross_end_px,
        );
    }
    line
}

fn resolve_measured_flex_container(
    measured_items_in_dom_order: Vec<MeasuredFlexItem>,
    line_collection_items: Vec<MeasuredFlexItem>,
    lines: Vec<FlexLine>,
) -> FlexContainerSolve {
    let mut main_size_px: f32 = 0.0;
    let mut cross_size_px: f32 = 0.0;
    for line in &lines {
        main_size_px = main_size_px.max(line.outer_hypothetical_main_size_px);
        cross_size_px += line.cross_size_px;
    }
    FlexContainerSolve {
        measured_items_in_dom_order,
        line_collection_items,
        lines,
        line_cross_sizes_px: Vec::new(),
        resolved_items: Vec::new(),
        main_size_px,
        cross_size_px,
    }
}

fn resolve_active_flex_container(
    mut measured_solve: FlexContainerSolve,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    container_cross_size_px: Option<f32>,
) -> FlexContainerSolve {
    assert_eq!(
        flex_line_item_count(&measured_solve.lines),
        measured_solve.line_collection_items.len(),
        "active flex lines should cover every order-modified measured item exactly once",
    );
    let line_cross_sizes_px = resolved_line_cross_sizes(
        &measured_solve.lines,
        container_style,
        container_cross_size_px,
    );
    let line_cross_offsets_px = line_cross_offsets_px(
        &line_cross_sizes_px,
        container_style.gap_sizes.between_lines_px,
        container_style,
        container_cross_size_px,
    );
    let measured_items_by_child_key =
        measured_items_by_child_key(&measured_solve.line_collection_items);
    let mut resolved_items = Vec::with_capacity(measured_solve.line_collection_items.len());
    for (line_index, line) in measured_solve.lines.iter().enumerate() {
        let line_items = measured_items_for_line(line, &measured_items_by_child_key);
        let mut resolved_line_items = resolve_flex_line_items(
            &line_items,
            container_main_size_px,
            container_style,
            container_style.gap_sizes.between_items_px,
        );
        for resolved_item in &mut resolved_line_items {
            resolved_item.cross_offset_px = line_cross_offsets_px[line_index];
        }
        resolved_items.extend(resolved_line_items);
    }
    measured_solve.line_cross_sizes_px = line_cross_sizes_px;
    measured_solve.resolved_items = resolved_items;
    measured_solve.main_size_px = container_main_size_px;
    measured_solve.cross_size_px = total_line_cross_size_px(
        &measured_solve.line_cross_sizes_px,
        container_style.gap_sizes.between_lines_px,
    );
    measured_solve
}

fn resolved_line_cross_sizes(
    lines: &[FlexLine],
    container_style: FlexContainerStyle,
    container_cross_size_px: Option<f32>,
) -> Vec<f32> {
    // CSS Flexbox 9.4: single-line containers always give their one line the
    // container's inner cross size (#algo-cross-line); multi-line containers
    // with `align-content: stretch` distribute the leftover free space
    // equally into each line's cross size (#algo-line-stretch). The leftover
    // subtracts the inter-line gaps: Gecko's `CrossAxisPositionTracker`
    // constructor in nsFlexContainerFrame.cpp and Blink's
    // flex_layout_algorithm.cc both compute
    // `content_size - sum(line cross sizes) - gap * (line count - 1)`
    // before stretching.
    if !container_style.wrap_mode.is_wrapping()
        && let Some(container_cross_size_px) = container_cross_size_px
    {
        return lines.iter().map(|_| container_cross_size_px).collect();
    }
    let mut line_cross_sizes_px = lines
        .iter()
        .map(|line| line.cross_size_px)
        .collect::<Vec<_>>();
    if !container_style.wrap_mode.is_wrapping()
        || resolved_line_cross_axis_alignment(container_style.align_content, container_style)
            != FlexLineCrossAxisAlignment::Stretch
    {
        return line_cross_sizes_px;
    }
    let Some(container_cross_size_px) = container_cross_size_px else {
        return line_cross_sizes_px;
    };
    if line_cross_sizes_px.is_empty() {
        return line_cross_sizes_px;
    }
    let remaining_cross_space_px = container_cross_size_px
        - total_line_cross_size_px(
            &line_cross_sizes_px,
            container_style.gap_sizes.between_lines_px,
        );
    if remaining_cross_space_px <= 0.0 {
        return line_cross_sizes_px;
    }
    let extra_cross_size_per_line_px =
        remaining_cross_space_px / non_negative_f32_from_usize(line_cross_sizes_px.len());
    for line_cross_size_px in &mut line_cross_sizes_px {
        *line_cross_size_px += extra_cross_size_per_line_px;
    }
    line_cross_sizes_px
}

fn line_cross_offsets_px(
    line_cross_sizes_px: &[f32],
    gap_between_lines_px: f32,
    container_style: FlexContainerStyle,
    container_cross_size_px: Option<f32>,
) -> Vec<f32> {
    let remaining_cross_space_px = remaining_cross_axis_line_pack_space_px(
        line_cross_sizes_px,
        gap_between_lines_px,
        container_cross_size_px,
        container_style.wrap_mode,
    );
    let mut next_line_cross_offset_px = initial_cross_axis_line_pack_offset_px(
        container_style,
        remaining_cross_space_px,
        line_cross_sizes_px.len(),
    );
    let extra_space_between_lines_px = extra_cross_axis_space_between_lines_px(
        resolved_line_cross_axis_alignment(container_style.align_content, container_style),
        remaining_cross_space_px,
        line_cross_sizes_px.len(),
    );
    let mut offsets_px = Vec::with_capacity(line_cross_sizes_px.len());
    for line_cross_size_px in line_cross_sizes_px {
        offsets_px.push(next_line_cross_offset_px);
        next_line_cross_offset_px +=
            line_cross_size_px + gap_between_lines_px + extra_space_between_lines_px;
    }
    offsets_px
}

fn remaining_cross_axis_line_pack_space_px(
    line_cross_sizes_px: &[f32],
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
    (container_cross_size_px - total_line_cross_size_px(line_cross_sizes_px, gap_between_lines_px))
        .max(0.0)
}

fn initial_cross_axis_line_pack_offset_px(
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

fn extra_cross_axis_space_between_lines_px(
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

fn total_line_cross_size_px(line_cross_sizes_px: &[f32], gap_between_lines_px: f32) -> f32 {
    let gap_count = flex_line_gap_count(line_cross_sizes_px.len());
    let gap_total_px = gap_between_lines_px * non_negative_f32_from_usize(gap_count);
    non_negative_flex_axis_size_px(line_cross_sizes_px.iter().sum::<f32>() + gap_total_px)
}

fn measured_items_by_child_key(
    measured_items: &[MeasuredFlexItem],
) -> HashMap<DomNodeSlotKey, MeasuredFlexItem> {
    let item_map = measured_items
        .iter()
        .copied()
        .map(|measured_item| (measured_item.source.key(), measured_item))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        item_map.len(),
        measured_items.len(),
        "active flex solve should keep one unique measured item per DOM child",
    );
    item_map
}

fn measured_items_for_line(
    line: &FlexLine,
    measured_items_by_child_key: &HashMap<DomNodeSlotKey, MeasuredFlexItem>,
) -> Vec<MeasuredFlexItem> {
    line.item_sources
        .iter()
        .map(|source| {
            measured_items_by_child_key
                .get(&source.key())
                .copied()
                .expect("active flex line should only reference measured child items")
        })
        .collect()
}

fn resolve_flex_line_items(
    items: &[MeasuredFlexItem],
    container_main_size_px: f32,
    container_style: FlexContainerStyle,
    gap_between_items_px: f32,
) -> Vec<ResolvedFlexItem> {
    if items.is_empty() {
        return Vec::new();
    }
    let resolution_states = resolve_flex_line_item_resolution_states(
        items,
        container_main_size_px,
        gap_between_items_px,
    );
    let line_main_size_px =
        resolved_flex_line_main_size_px(&resolution_states, gap_between_items_px);
    let remaining_main_space_px = container_main_size_px - line_main_size_px;
    let mut main_offset_px = initial_main_axis_alignment_offset_px(
        container_style,
        remaining_main_space_px,
        items.len(),
    );
    let extra_space_between_items_px = extra_main_axis_space_between_items_px(
        resolved_main_axis_alignment(container_style.justify_content, container_style),
        remaining_main_space_px,
        items.len(),
    );
    let mut resolved_items = Vec::with_capacity(resolution_states.len());
    for (index, resolution_state) in resolution_states.into_iter().enumerate() {
        resolved_items.push(ResolvedFlexItem {
            source: resolution_state.measured_item.source,
            align_self: resolution_state.measured_item.align_self,
            resolved_main_size_px: resolution_state.resolved_main_size_px,
            main_axis_border_padding_px: resolution_state.measured_item.main_axis_border_padding_px,
            resolved_cross_size_px: resolution_state.measured_item.measured_cross_size_input_px,
            main_offset_px,
            cross_offset_px: 0.0,
        });
        main_offset_px += resolution_state.measured_item.margin_main_start_px
            + resolution_state.resolved_main_size_px
            + resolution_state.measured_item.margin_main_end_px;
        if index + 1 < items.len() {
            main_offset_px += gap_between_items_px + extra_space_between_items_px;
        }
    }
    resolved_items
}

fn resolved_flex_line_main_size_px(
    resolution_states: &[FlexLineItemResolutionState],
    gap_between_items_px: f32,
) -> f32 {
    let gap_count = flex_line_gap_count(resolution_states.len());
    let resolved_item_main_size_px = resolution_states
        .iter()
        .map(flex_line_state_outer_resolved_main_size)
        .sum::<f32>();
    resolved_item_main_size_px + gap_between_items_px * non_negative_f32_from_usize(gap_count)
}

fn flex_line_state_outer_resolved_main_size(state: &FlexLineItemResolutionState) -> f32 {
    state.measured_item.margin_main_start_px
        + state.resolved_main_size_px
        + state.measured_item.margin_main_end_px
}

fn initial_main_axis_alignment_offset_px(
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
        FlexMainAxisAlignment::Center => remaining_main_space_px / 2.0,
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
            if remaining_main_space_px <= 0.0 {
                return remaining_main_space_px / 2.0;
            }
            remaining_main_space_px / (2.0 * non_negative_f32_from_usize(item_count))
        }
        FlexMainAxisAlignment::SpaceEvenly => {
            if item_count == 0 {
                return 0.0;
            }
            remaining_main_space_px / non_negative_f32_from_usize(item_count.saturating_add(1))
        }
    }
}

fn extra_main_axis_space_between_items_px(
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

fn resolve_flex_line_item_resolution_states(
    items: &[MeasuredFlexItem],
    container_main_size_px: f32,
    gap_between_items_px: f32,
) -> Vec<FlexLineItemResolutionState> {
    // CSS Flexbox 9.7 (#resolve-flexible-lengths): freeze inflexible items,
    // then loop the free-space distribution (scaled by grow factors, or by
    // shrink factors times base size) with min/max/auto-min violation
    // clamping and refreezing until every item is frozen. Blink mirrors this
    // in `LineFlexer::ResolveFlexibleLengths`/`FreezeItems` (line_flexer.cc)
    // and Gecko in `FlexLine::ResolveFlexibleLengths`
    // (nsFlexContainerFrame.cpp).
    let is_using_flex_grow =
        flex_line_uses_flex_grow(items, container_main_size_px, gap_between_items_px);
    let mut resolution_states = initial_flex_line_item_resolution_states(items);
    freeze_flex_line_inflexible_items(&mut resolution_states, is_using_flex_grow);
    let initial_free_space_px = remaining_flex_line_free_space(
        &resolution_states,
        container_main_size_px,
        gap_between_items_px,
    );
    loop {
        freeze_flex_line_inflexible_items(&mut resolution_states, is_using_flex_grow);
        if resolution_states.iter().all(|state| state.is_frozen) {
            return resolution_states;
        }
        let remaining_free_space_px = remaining_flex_line_free_space(
            &resolution_states,
            container_main_size_px,
            gap_between_items_px,
        );
        let remaining_free_space_px = remaining_flex_line_free_space_limited_by_factor_sum(
            &resolution_states,
            initial_free_space_px,
            remaining_free_space_px,
            is_using_flex_grow,
        );
        if !distribute_flex_line_free_space(
            &mut resolution_states,
            remaining_free_space_px,
            is_using_flex_grow,
        ) {
            return resolution_states;
        }
        if freeze_flex_line_main_size_violations(&mut resolution_states) {
            continue;
        }
        freeze_flex_line_unfrozen_items(&mut resolution_states);
    }
}

fn flex_line_uses_flex_grow(
    items: &[MeasuredFlexItem],
    container_main_size_px: f32,
    gap_between_items_px: f32,
) -> bool {
    let total_outer_hypothetical_main_size_px: f32 = items
        .iter()
        .map(|item| item.outer_hypothetical_main_size_px)
        .sum();
    total_outer_hypothetical_main_size_px
        + flex_line_gap_total_px(items.len(), gap_between_items_px)
        < container_main_size_px
}

fn initial_flex_line_item_resolution_states(
    items: &[MeasuredFlexItem],
) -> Vec<FlexLineItemResolutionState> {
    items
        .iter()
        .copied()
        .map(|measured_item| FlexLineItemResolutionState {
            measured_item,
            resolved_main_size_px: measured_item.flex_base_size_px,
            is_frozen: false,
        })
        .collect()
}

fn freeze_flex_line_inflexible_items(
    resolution_states: &mut [FlexLineItemResolutionState],
    is_using_flex_grow: bool,
) {
    for resolution_state in resolution_states {
        if flex_line_item_is_inflexible(resolution_state.measured_item, is_using_flex_grow) {
            resolution_state.is_frozen = true;
        }
    }
}

fn flex_line_item_is_inflexible(item: MeasuredFlexItem, is_using_flex_grow: bool) -> bool {
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

fn remaining_flex_line_free_space(
    resolution_states: &[FlexLineItemResolutionState],
    container_main_size_px: f32,
    gap_between_items_px: f32,
) -> f32 {
    let total_resolved_main_size_px: f32 = resolution_states
        .iter()
        .map(flex_line_state_main_size_for_free_space)
        .sum();
    container_main_size_px
        - total_resolved_main_size_px
        - flex_line_gap_total_px(resolution_states.len(), gap_between_items_px)
}

fn flex_line_state_main_size_for_free_space(state: &FlexLineItemResolutionState) -> f32 {
    let main_size_px = if state.is_frozen {
        state.resolved_main_size_px
    } else {
        state.measured_item.flex_base_size_px
    };
    main_size_px + state.measured_item.margin_main_start_px + state.measured_item.margin_main_end_px
}

fn flex_line_gap_total_px(item_count: usize, gap_between_items_px: f32) -> f32 {
    gap_between_items_px * non_negative_f32_from_usize(flex_line_gap_count(item_count))
}

fn distribute_flex_line_free_space(
    resolution_states: &mut [FlexLineItemResolutionState],
    remaining_free_space_px: f32,
    is_using_flex_grow: bool,
) -> bool {
    let flex_factor_sum = unfrozen_flex_line_factor_sum(resolution_states, is_using_flex_grow);
    if flex_factor_sum <= 0.0 {
        return false;
    }
    for resolution_state in resolution_states {
        if resolution_state.is_frozen {
            continue;
        }
        let flex_factor = flex_line_flex_factor(resolution_state.measured_item, is_using_flex_grow);
        let size_delta_px = remaining_free_space_px * (flex_factor / flex_factor_sum);
        resolution_state.resolved_main_size_px =
            resolution_state.measured_item.flex_base_size_px + size_delta_px;
    }
    true
}

fn remaining_flex_line_free_space_limited_by_factor_sum(
    resolution_states: &[FlexLineItemResolutionState],
    initial_free_space_px: f32,
    remaining_free_space_px: f32,
    is_using_flex_grow: bool,
) -> f32 {
    let flex_factor_sum = unfrozen_raw_flex_line_factor_sum(resolution_states, is_using_flex_grow);
    if !(0.0..1.0).contains(&flex_factor_sum) {
        return remaining_free_space_px;
    }
    let fractional_free_space_px = initial_free_space_px * flex_factor_sum;
    if fractional_free_space_px.abs() < remaining_free_space_px.abs() {
        return fractional_free_space_px;
    }
    remaining_free_space_px
}

fn freeze_flex_line_main_size_violations(
    resolution_states: &mut [FlexLineItemResolutionState],
) -> bool {
    let mut froze_violation = false;
    for resolution_state in resolution_states {
        if resolution_state.is_frozen {
            continue;
        }
        let clamped_main_size_px = clamped_main_size_after_flex_constraints(
            resolution_state.measured_item,
            resolution_state.resolved_main_size_px,
        );
        if flex_axis_sizes_match(clamped_main_size_px, resolution_state.resolved_main_size_px) {
            continue;
        }
        resolution_state.resolved_main_size_px = clamped_main_size_px;
        resolution_state.is_frozen = true;
        froze_violation = true;
    }
    froze_violation
}

fn clamped_main_size_after_flex_constraints(
    item: MeasuredFlexItem,
    resolved_main_size_px: f32,
) -> f32 {
    let mut clamped_main_size_px = resolved_main_size_px
        .max(item.automatic_min_main_size_px)
        .max(item.main_axis_border_padding_px);
    if let Some(min_main_size_px) = item.min_main_size_px {
        clamped_main_size_px = clamped_main_size_px.max(min_main_size_px);
    }
    if let Some(max_main_size_px) = item.max_main_size_px {
        clamped_main_size_px = clamped_main_size_px.min(max_main_size_px);
    }
    clamped_main_size_px
}

fn unfrozen_flex_line_factor_sum(
    resolution_states: &[FlexLineItemResolutionState],
    is_using_flex_grow: bool,
) -> f32 {
    resolution_states
        .iter()
        .filter(|state| !state.is_frozen)
        .map(|state| flex_line_flex_factor(state.measured_item, is_using_flex_grow))
        .sum()
}

fn unfrozen_raw_flex_line_factor_sum(
    resolution_states: &[FlexLineItemResolutionState],
    is_using_flex_grow: bool,
) -> f32 {
    resolution_states
        .iter()
        .filter(|state| !state.is_frozen)
        .map(|state| raw_flex_line_flex_factor(state.measured_item, is_using_flex_grow))
        .sum()
}

fn raw_flex_line_flex_factor(item: MeasuredFlexItem, is_using_flex_grow: bool) -> f32 {
    if is_using_flex_grow {
        return item.flex_grow_factor;
    }
    item.flex_shrink_factor
}

fn flex_line_flex_factor(item: MeasuredFlexItem, is_using_flex_grow: bool) -> f32 {
    if is_using_flex_grow {
        return item.flex_grow_factor;
    }
    item.flex_shrink_factor * item.flex_base_size_px
}

fn freeze_flex_line_unfrozen_items(resolution_states: &mut [FlexLineItemResolutionState]) {
    for resolution_state in resolution_states {
        resolution_state.is_frozen = true;
    }
}

fn exit_active_flex_container<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: ActiveFlexExitRequest,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let placed_children = place_active_flex_resolved_children(
        root_dom,
        tree_size,
        &ActiveFlexPlacementRequest {
            key: request.key,
            child_context: &request.child_context,
            container_style: request.container_style,
            lines: &request.solve.lines,
            line_cross_sizes_px: &request.solve.line_cross_sizes_px,
            container_main_size_px: request.solve.main_size_px,
            resolved_items: &request.solve.resolved_items,
        },
        resolver,
    )?;
    let solve =
        flex_container_solve_with_placed_active_flex_children(request.solve, placed_children);
    let content_width_px = active_flex_content_width_px(
        request.container_style.main_axis,
        solve.main_size_px,
        solve.cross_size_px,
    );
    let auto_content_width_px = auto_content_width_for_exit(
        &request.entered,
        request.context.available_width_px,
        content_width_px,
        content_width_px,
        content_width_px,
    );
    exit_node(
        root_dom,
        request.key,
        request.context,
        request.entered,
        auto_content_width_px,
        (auto_content_width_px, auto_content_width_px),
        active_flex_content_height_px(
            request.container_style.main_axis,
            solve.main_size_px,
            solve.cross_size_px,
        ),
    )
}

fn flex_container_solve_with_placed_active_flex_children(
    mut solve: FlexContainerSolve,
    placed_children: PlacedFlexChildren,
) -> FlexContainerSolve {
    solve.resolved_items = placed_children.resolved_items;
    solve.cross_size_px = placed_children.container_cross_size_px;
    solve
}

fn place_active_flex_resolved_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexPlacementRequest<'_>,
    resolver: &mut R,
) -> Result<PlacedFlexChildren, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let resolved_items_by_child_key = resolved_items_by_child_key(request.resolved_items);
    let line_index_by_child_key = line_index_by_child_key(request.lines);
    let mut child_key = first_child_key(root_dom, request.key)?;
    let mut placed_line_cross_sizes_px: Vec<f32> = vec![0.0; request.lines.len()];
    let mut placed_item_count = 0usize;
    let mut placed_resolved_items = Vec::with_capacity(request.resolved_items.len());
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let resolved_item = resolved_items_by_child_key
            .get(&current_child_key)
            .copied()
            .expect("active flex placement should keep one resolved item per DOM child");
        let line_index = line_index_by_child_key
            .get(&current_child_key)
            .copied()
            .expect("active flex placement should keep one line index per DOM child");
        let aligned_resolved_item = aligned_resolved_flex_item(
            resolved_item,
            request.container_style,
            request.line_cross_sizes_px[line_index],
        );
        let placed_resolved_item = relayout_active_flex_child(
            root_dom,
            tree_size,
            &ActiveFlexChildRelayoutRequest {
                source: FlexItemSource::BlockItem(current_child_key),
                child_context: request.child_context,
                container_style: request.container_style,
                container_main_size_px: request.container_main_size_px,
                line_cross_size_px: request.line_cross_sizes_px[line_index],
                resolved_item: aligned_resolved_item,
            },
            resolver,
        )?;
        placed_line_cross_sizes_px[line_index] =
            placed_line_cross_sizes_px[line_index].max(placed_resolved_item.resolved_cross_size_px);
        placed_resolved_items.push(placed_resolved_item);
        placed_item_count += 1;
        child_key = next_child_key;
    }
    assert_eq!(
        placed_item_count,
        request.resolved_items.len(),
        "active flex placement should write each resolved child exactly once",
    );
    assert_eq!(
        line_index_by_child_key.len(),
        request.resolved_items.len(),
        "active flex placement should keep one line membership per resolved DOM child",
    );
    assert_wrapped_line_cross_sizes_match_measurement(
        request.container_style.wrap_mode,
        request.lines,
        &placed_line_cross_sizes_px,
    );
    Ok(PlacedFlexChildren {
        resolved_items: placed_resolved_items,
        container_cross_size_px: total_line_cross_size_px(
            request.line_cross_sizes_px,
            request.container_style.gap_sizes.between_lines_px,
        ),
    })
}

fn aligned_resolved_flex_item(
    resolved_item: ResolvedFlexItem,
    container_style: FlexContainerStyle,
    line_cross_size_px: f32,
) -> ResolvedFlexItem {
    ResolvedFlexItem {
        cross_offset_px: resolved_item.cross_offset_px
            + cross_axis_alignment_offset_px(resolved_item, container_style, line_cross_size_px),
        ..resolved_item
    }
}

fn relayout_active_flex_child<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexChildRelayoutRequest<'_>,
    resolver: &mut R,
) -> Result<ResolvedFlexItem, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let (child_style, _) = resolved_node_style(
        root_dom,
        request.source.key(),
        &request.child_context.inherited_style,
    )?;
    let placed_context = active_flex_resolved_child_context(
        request.child_context.clone(),
        request.container_style,
        request.container_main_size_px,
        request.resolved_item,
        child_style,
        request.line_cross_size_px,
    );
    let child_flow = relayout_tree_walker_node(
        root_dom,
        tree_size,
        request.source.key(),
        placed_context,
        resolver,
    )?;
    Ok(ResolvedFlexItem {
        resolved_cross_size_px: cross_axis_flow_size(request.container_style.main_axis, child_flow),
        ..request.resolved_item
    })
}

fn resolved_items_by_child_key(
    resolved_items: &[ResolvedFlexItem],
) -> HashMap<DomNodeSlotKey, ResolvedFlexItem> {
    let item_map = resolved_items
        .iter()
        .copied()
        .map(|resolved_item| (resolved_item.source.key(), resolved_item))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        item_map.len(),
        resolved_items.len(),
        "active flex placement should keep one unique resolved item per DOM child",
    );
    item_map
}

fn line_index_by_child_key(lines: &[FlexLine]) -> HashMap<DomNodeSlotKey, usize> {
    let mut item_map = HashMap::new();
    for (line_index, line) in lines.iter().enumerate() {
        for source in &line.item_sources {
            let previous = item_map.insert(source.key(), line_index);
            assert!(
                previous.is_none(),
                "active flex lines should not place the same child in more than one line",
            );
        }
    }
    item_map
}

fn flex_line_item_count(lines: &[FlexLine]) -> usize {
    lines.iter().map(|line| line.item_sources.len()).sum()
}

fn flex_line_gap_count(item_count: usize) -> usize {
    item_count.saturating_sub(1)
}

fn cross_axis_alignment_offset_px(
    resolved_item: ResolvedFlexItem,
    container_style: FlexContainerStyle,
    line_cross_size_px: f32,
) -> f32 {
    let alignment = resolved_flex_item_cross_axis_alignment(resolved_item, container_style);
    let remaining_cross_space_px = line_cross_size_px - resolved_item.resolved_cross_size_px;
    let is_cross_reversed = resolved_cross_axis_is_reversed(container_style);
    match alignment {
        FlexCrossAxisAlignment::Start | FlexCrossAxisAlignment::End => {
            panic!("cross-axis alignment should resolve logical start/end before placement")
        }
        FlexCrossAxisAlignment::FlexStart | FlexCrossAxisAlignment::Stretch => {
            if is_cross_reversed {
                remaining_cross_space_px
            } else {
                0.0
            }
        }
        FlexCrossAxisAlignment::FlexEnd => {
            if is_cross_reversed {
                0.0
            } else {
                remaining_cross_space_px
            }
        }
        FlexCrossAxisAlignment::Center => remaining_cross_space_px / 2.0,
    }
}

fn resolved_flex_item_cross_axis_alignment(
    resolved_item: ResolvedFlexItem,
    container_style: FlexContainerStyle,
) -> FlexCrossAxisAlignment {
    resolved_cross_axis_alignment(
        resolved_item
            .align_self
            .unwrap_or(container_style.align_items),
        container_style,
    )
}

fn assert_wrapped_line_cross_sizes_match_measurement(
    wrap_mode: FlexWrapMode,
    lines: &[FlexLine],
    line_cross_sizes_px: &[f32],
) {
    if !wrap_mode.is_wrapping() {
        return;
    }
    assert_eq!(
        lines.len(),
        line_cross_sizes_px.len(),
        "wrapped active flex placement should track one cross size per line",
    );
    for (line, line_cross_size_px) in lines.iter().zip(line_cross_sizes_px.iter().copied()) {
        assert!(
            flex_axis_sizes_match(line.cross_size_px, line_cross_size_px),
            "wrapped active flex placement should preserve measured line cross sizes",
        );
    }
}

fn flex_axis_sizes_match(left_px: f32, right_px: f32) -> bool {
    (left_px - right_px).abs() <= f32::EPSILON
}

fn non_negative_f32_from_usize(value: usize) -> f32 {
    value
        .to_string()
        .parse::<f32>()
        .expect("usize-backed flex count should parse as f32")
}

fn active_flex_resolved_child_context(
    child_context: LayoutContext,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    resolved_item: ResolvedFlexItem,
    child_style: NodeStyle,
    line_cross_size_px: f32,
) -> LayoutContext {
    let cross_axis_alignment =
        resolved_flex_item_cross_axis_alignment(resolved_item, container_style);
    let main_start_px = resolved_flex_item_main_start_px(
        resolved_item.main_offset_px,
        resolved_item.resolved_main_size_px,
        container_main_size_px,
        resolved_main_axis_is_reversed(container_style),
    );
    let cross_start_px = resolved_item.cross_offset_px;
    let resolved_content_main_size_px =
        resolved_flex_item_content_main_size_px(resolved_item).max(0.0);
    let stretched_content_cross_size_px = stretched_flex_item_content_cross_size_px(
        child_style,
        container_style,
        cross_axis_alignment,
        line_cross_size_px,
    );
    match container_style.main_axis {
        FlexAxis::Horizontal => LayoutContext {
            flow_left_px: child_context.flow_left_px + main_start_px,
            flow_top_px: child_context.flow_top_px + cross_start_px,
            available_width_px: Some(resolved_content_main_size_px),
            node_content_width_override_px: Some(resolved_content_main_size_px),
            node_content_height_override_px: stretched_content_cross_size_px,
            ..child_context
        },
        FlexAxis::Vertical => LayoutContext {
            flow_left_px: child_context.flow_left_px + cross_start_px,
            flow_top_px: child_context.flow_top_px + main_start_px,
            available_height_px: Some(resolved_content_main_size_px),
            node_content_width_override_px: stretched_content_cross_size_px,
            node_content_height_override_px: Some(resolved_content_main_size_px),
            ..child_context
        },
    }
}

fn stretched_flex_item_content_cross_size_px(
    child_style: NodeStyle,
    container_style: FlexContainerStyle,
    cross_axis_alignment: FlexCrossAxisAlignment,
    line_cross_size_px: f32,
) -> Option<f32> {
    if cross_axis_alignment != FlexCrossAxisAlignment::Stretch
        || style_has_definite_cross_axis_size(child_style, container_style.main_axis)
    {
        return None;
    }
    Some(non_negative_flex_axis_size_px(
        line_cross_size_px
            - cross_axis_border_padding_size_px(child_style.sides, container_style.main_axis),
    ))
}

fn resolved_flex_item_content_main_size_px(resolved_item: ResolvedFlexItem) -> f32 {
    assert!(
        resolved_item.resolved_main_size_px + f32::EPSILON
            >= resolved_item.main_axis_border_padding_px,
        "resolved flex item border-box main size should not be smaller than border and padding",
    );
    resolved_item.resolved_main_size_px - resolved_item.main_axis_border_padding_px
}

fn resolved_flex_item_main_start_px(
    main_offset_px: f32,
    resolved_main_size_px: f32,
    container_main_size_px: f32,
    is_main_reversed: bool,
) -> f32 {
    if !is_main_reversed {
        return main_offset_px;
    }
    container_main_size_px - main_offset_px - resolved_main_size_px
}

fn active_flex_content_width_px(main_axis: FlexAxis, main_size_px: f32, cross_size_px: f32) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => main_size_px,
        FlexAxis::Vertical => cross_size_px,
    }
}

fn active_flex_content_height_px(
    main_axis: FlexAxis,
    main_size_px: f32,
    cross_size_px: f32,
) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => cross_size_px,
        FlexAxis::Vertical => main_size_px,
    }
}

fn non_negative_flex_axis_size_px(size_px: f32) -> f32 {
    size_px.max(0.0)
}

fn main_axis_flow_size(main_axis: FlexAxis, measured_flow: FlowSize) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => measured_flow.width_px,
        FlexAxis::Vertical => measured_flow.height_px,
    }
}

fn main_axis_border_box_size_from_flow(
    main_axis: FlexAxis,
    measured_flow: FlowSize,
    style: NodeStyle,
) -> f32 {
    (main_axis_flow_size(main_axis, measured_flow)
        - main_axis_margin_size_px(style.sides, main_axis))
    .max(0.0)
}

fn cross_axis_flow_size(main_axis: FlexAxis, measured_flow: FlowSize) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => measured_flow.height_px,
        FlexAxis::Vertical => measured_flow.width_px,
    }
}

fn cross_axis_border_box_size_from_flow(
    main_axis: FlexAxis,
    measured_flow: FlowSize,
    style: NodeStyle,
) -> f32 {
    (cross_axis_flow_size(main_axis, measured_flow)
        - cross_axis_margin_size_px(style.sides, main_axis))
    .max(0.0)
}

fn cross_axis(main_axis: FlexAxis) -> FlexAxis {
    match main_axis {
        FlexAxis::Horizontal => FlexAxis::Vertical,
        FlexAxis::Vertical => FlexAxis::Horizontal,
    }
}

fn main_axis_border_padding_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => {
            sides.border.left_px
                + sides.border.right_px
                + sides.padding.left_px
                + sides.padding.right_px
        }
        FlexAxis::Vertical => {
            sides.border.top_px
                + sides.border.bottom_px
                + sides.padding.top_px
                + sides.padding.bottom_px
        }
    }
}

fn cross_axis_border_padding_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => {
            sides.border.top_px
                + sides.border.bottom_px
                + sides.padding.top_px
                + sides.padding.bottom_px
        }
        FlexAxis::Vertical => {
            sides.border.left_px
                + sides.border.right_px
                + sides.padding.left_px
                + sides.padding.right_px
        }
    }
}

fn main_axis_margin_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    main_axis_start_margin_from_sides(sides, main_axis)
        + main_axis_end_margin_from_sides(sides, main_axis)
}

fn cross_axis_margin_size_px(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    cross_axis_start_margin_from_sides(sides, main_axis)
        + cross_axis_end_margin_from_sides(sides, main_axis)
}

fn main_axis_start_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    main_axis_start_margin_from_sides(style.sides, main_axis)
}

fn main_axis_start_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.left_px,
        FlexAxis::Vertical => sides.margin.top_px,
    }
}

fn main_axis_end_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    main_axis_end_margin_from_sides(style.sides, main_axis)
}

fn main_axis_end_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.right_px,
        FlexAxis::Vertical => sides.margin.bottom_px,
    }
}

fn cross_axis_start_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    cross_axis_start_margin_from_sides(style.sides, main_axis)
}

fn cross_axis_start_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.top_px,
        FlexAxis::Vertical => sides.margin.left_px,
    }
}

fn cross_axis_end_margin(style: NodeStyle, main_axis: FlexAxis) -> f32 {
    cross_axis_end_margin_from_sides(style.sides, main_axis)
}

fn cross_axis_end_margin_from_sides(sides: BoxSides, main_axis: FlexAxis) -> f32 {
    match main_axis {
        FlexAxis::Horizontal => sides.margin.bottom_px,
        FlexAxis::Vertical => sides.margin.right_px,
    }
}

fn node_for_key(root_dom: &RootDom, key: DomNodeSlotKey) -> Result<&DomNode, DomError> {
    root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn test_keys(count: usize) -> Vec<DomNodeSlotKey> {
        let mut keys = slotmap::SlotMap::with_key();
        (0..count).map(|_| keys.insert(())).collect()
    }
    fn source(key: DomNodeSlotKey) -> FlexItemSource {
        FlexItemSource::BlockItem(key)
    }

    fn measured_row_item(
        source: FlexItemSource,
        flex_grow_factor: f32,
        flex_shrink_factor: f32,
        flex_base_size_px: f32,
        measured_cross_size_input_px: f32,
    ) -> MeasuredFlexItem {
        measured_row_item_with_automatic_min(
            source,
            flex_grow_factor,
            flex_shrink_factor,
            flex_base_size_px,
            measured_cross_size_input_px,
            0.0,
        )
    }

    fn measured_row_item_with_automatic_min(
        source: FlexItemSource,
        flex_grow_factor: f32,
        flex_shrink_factor: f32,
        flex_base_size_px: f32,
        measured_cross_size_input_px: f32,
        automatic_min_main_size_px: f32,
    ) -> MeasuredFlexItem {
        MeasuredFlexItem {
            source,
            order: 0,
            main_axis: FlexAxis::Horizontal,
            cross_axis: FlexAxis::Vertical,
            align_self: None,
            flex_grow_factor,
            flex_shrink_factor,
            flex_base_size_px,
            hypothetical_main_size_px: flex_base_size_px,
            automatic_min_main_size_px,
            min_main_size_px: None,
            max_main_size_px: None,
            main_axis_border_padding_px: 0.0,
            outer_hypothetical_main_size_px: flex_base_size_px,
            measured_cross_size_input_px,
            margin_main_start_px: 0.0,
            margin_main_end_px: 0.0,
            margin_cross_start_px: 0.0,
            margin_cross_end_px: 0.0,
        }
    }

    fn measured_row_item_with_limits(
        source: FlexItemSource,
        flex_grow_factor: f32,
        flex_shrink_factor: f32,
        flex_base_size_px: f32,
        measured_cross_size_input_px: f32,
        min_main_size_px: Option<f32>,
        max_main_size_px: Option<f32>,
    ) -> MeasuredFlexItem {
        let mut measured_item = measured_row_item(
            source,
            flex_grow_factor,
            flex_shrink_factor,
            flex_base_size_px,
            measured_cross_size_input_px,
        );
        measured_item.min_main_size_px = min_main_size_px;
        measured_item.max_main_size_px = max_main_size_px;
        measured_item.hypothetical_main_size_px = measured_item
            .hypothetical_main_size_px
            .max(min_main_size_px.unwrap_or(0.0));
        if let Some(max_main_size_px) = max_main_size_px {
            measured_item.hypothetical_main_size_px = measured_item
                .hypothetical_main_size_px
                .min(max_main_size_px);
        }
        measured_item.outer_hypothetical_main_size_px = measured_item.hypothetical_main_size_px;
        measured_item
    }

    #[derive(Clone, Copy)]
    struct MeasuredRowItemBoxEdges {
        main_size: f32,
        cross_size: f32,
        main_border_padding: f32,
        main_start_margin: f32,
        main_end_margin: f32,
        cross_start_margin: f32,
        cross_end_margin: f32,
    }

    fn measured_row_item_with_box_edges(
        source: FlexItemSource,
        box_edges: MeasuredRowItemBoxEdges,
    ) -> MeasuredFlexItem {
        MeasuredFlexItem {
            source,
            order: 0,
            main_axis: FlexAxis::Horizontal,
            cross_axis: FlexAxis::Vertical,
            align_self: None,
            flex_grow_factor: 0.0,
            flex_shrink_factor: 0.0,
            flex_base_size_px: box_edges.main_size,
            hypothetical_main_size_px: box_edges.main_size,
            automatic_min_main_size_px: box_edges.main_size,
            min_main_size_px: None,
            max_main_size_px: None,
            main_axis_border_padding_px: box_edges.main_border_padding,
            outer_hypothetical_main_size_px: box_edges.main_size
                + box_edges.main_start_margin
                + box_edges.main_end_margin,
            measured_cross_size_input_px: box_edges.cross_size,
            margin_main_start_px: box_edges.main_start_margin,
            margin_main_end_px: box_edges.main_end_margin,
            margin_cross_start_px: box_edges.cross_start_margin,
            margin_cross_end_px: box_edges.cross_end_margin,
        }
    }

    fn test_gap_sizes() -> super::FlexGapSizes {
        super::FlexGapSizes {
            between_items_px: 0.0,
            between_lines_px: 0.0,
        }
    }

    fn row_container_style() -> FlexContainerStyle {
        FlexContainerStyle {
            main_axis: FlexAxis::Horizontal,
            is_main_reversed: false,
            text_direction: DomTextDirection::Ltr,
            wrap_mode: FlexWrapMode::NoWrap,
            justify_content: super::FlexMainAxisAlignment::FlexStart,
            align_items: super::FlexCrossAxisAlignment::FlexStart,
            align_content: super::FlexLineCrossAxisAlignment::FlexStart,
            gap_sizes: test_gap_sizes(),
        }
    }

    fn wrap_row_container_style() -> FlexContainerStyle {
        FlexContainerStyle {
            wrap_mode: FlexWrapMode::Wrap,
            ..row_container_style()
        }
    }

    fn base_context() -> LayoutContext {
        LayoutContext {
            inherited_style: InheritedStyleContext::default(),
            flow_left_px: 0.0,
            flow_top_px: 0.0,
            root_local_left_px: None,
            root_local_top_px: None,
            available_width_px: Some(320.0),
            available_height_px: Some(320.0),
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            parent_absolute_left_px: 0.0,
            parent_absolute_top_px: 0.0,
            containing_layer_absolute_left_px: 0.0,
            containing_layer_absolute_top_px: 0.0,
            containing_layer_width_px: 320.0,
            containing_layer_height_px: 320.0,
            viewport_width_px: 320.0,
            viewport_height_px: 320.0,
            parent_box: None,
            allows_parent_child_block_margin_collapse: false,
        }
    }

    fn append_div(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
        let key = root.create_element(DomTag::Div).expect("div should insert");
        root.append_child(parent_key, key)
            .expect("div should append");
        key
    }

    fn append_span(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
        let key = root
            .create_element(DomTag::Span)
            .expect("span should insert");
        root.append_child(parent_key, key)
            .expect("span should append");
        key
    }

    fn append_text(root: &mut RootDom, parent_key: DomNodeSlotKey, text: &str) -> DomNodeSlotKey {
        let key = root.create_text_node(text).expect("text should insert");
        root.append_child(parent_key, key)
            .expect("text should append");
        key
    }

    fn flex_container_with_context() -> (RootDom, DomNodeSlotKey, LayoutContext) {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document_key = root.document_node().0;
        let container = append_div(&mut root, document_key);
        root.style_mut(container)
            .set("display", "flex")
            .expect("display should set");
        (root, container, base_context())
    }

    fn collect_items_for_container(
        root: &mut RootDom,
        container: DomNodeSlotKey,
        context: LayoutContext,
    ) -> Vec<MeasuredFlexItem> {
        let document_key = root.document_node().0;
        recompute_dirty_style_subtree_for_layer_root(root, document_key).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let (style, inherited_style) =
            resolved_node_style(root, container, &context.inherited_style)
                .expect("container style");
        let container_style =
            staged_flex_container_style(root.nodes.get(container).expect("container node"));
        let entered = enter_node(
            root,
            (320.0, 320.0),
            container,
            context.clone(),
            style,
            inherited_style,
            &mut resolver,
        )
        .expect("container should enter");
        let child_context = child_layout_context(context, entered);
        collect_measured_flex_items_in_dom_order(
            root,
            (320.0, 320.0),
            container,
            &child_context,
            container_style,
            &mut resolver,
        )
        .expect("items should measure")
    }

    fn build_solve_for_container(
        root: &mut RootDom,
        container: DomNodeSlotKey,
        context: LayoutContext,
    ) -> super::FlexContainerSolve {
        let document_key = root.document_node().0;
        recompute_dirty_style_subtree_for_layer_root(root, document_key).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let (style, inherited_style) =
            resolved_node_style(root, container, &context.inherited_style)
                .expect("container style");
        build_staged_measurement_solve(
            root,
            (320.0, 320.0),
            container,
            context,
            style,
            inherited_style,
            &mut resolver,
        )
        .expect("staged solve should build")
    }

    fn node_layout(root: &RootDom, key: DomNodeSlotKey) -> DomLayoutState {
        match root.node(key).expect("node should exist") {
            DomNode::Document(node) => node.base.layout.clone(),
            DomNode::DocumentFragment(node) => node.base.layout.clone(),
            DomNode::Element(node) => node.base.layout.clone(),
            DomNode::Text(node) => node.base.layout.clone(),
        }
    }

    fn node_box(root: &RootDom, key: DomNodeSlotKey) -> DomBoxModel {
        node_box_from_ref(root.node(key).expect("node should exist"))
    }

    fn node_layout_dirty(root: &RootDom, key: DomNodeSlotKey) -> bool {
        layout_dirty_from_ref(root.node(key).expect("node should exist"))
    }

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
        assert_eq!(item.main_axis, FlexAxis::Horizontal);
        assert_eq!(item.cross_axis, FlexAxis::Vertical);
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
        let (text_style, _) = resolved_node_style(&root, text, &InheritedStyleContext::default())
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
    fn measured_item_record_uses_measured_height_for_vertical_visible_overflow_auto_min() {
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

        assert_eq!(item.automatic_min_main_size_px, 30.0);
        assert_eq!(item.flex_base_size_px, 10.0);
    }

    #[test]
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

        assert_eq!(solve.measured_items_in_dom_order.len(), 1);
        assert_eq!(solve.measured_items_in_dom_order[0].flex_base_size_px, 20.0);
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

        assert_eq!(solve.measured_items_in_dom_order.len(), 1);
        assert_eq!(solve.line_collection_items.len(), 1);
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

        let items = collect_items_for_container(&mut root, container, context);
        let ordered_items = order_modified_measured_items(&items);

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

    #[test]
    fn line_membership_uses_outer_hypothetical_main_sizes() {
        let (mut root, container, context) = flex_container_with_context();
        root.style_mut(container)
            .set("flex-wrap", "wrap")
            .expect("wrap should set");
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

        let items = collect_items_for_container(&mut root, container, context);
        let ordered_items = order_modified_measured_items(&items);
        let lines = build_measured_flex_lines(&ordered_items, FlexWrapMode::Wrap, Some(50.0), 0.0);

        assert_eq!(
            lines,
            vec![
                FlexLine {
                    item_sources: vec![source(first)],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
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

        let lines = build_measured_flex_lines(&items, FlexWrapMode::Wrap, Some(25.0), 5.0);

        assert_eq!(
            lines,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
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

        let lines = build_measured_flex_lines(&items, FlexWrapMode::NoWrap, Some(50.0), 0.0);

        assert_eq!(
            lines,
            vec![FlexLine {
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

        let first_lines = build_measured_flex_lines(&items, FlexWrapMode::Wrap, Some(50.0), 0.0);
        let second_lines = build_measured_flex_lines(&items, FlexWrapMode::Wrap, Some(50.0), 0.0);

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

        let lines = build_measured_flex_lines(&items, FlexWrapMode::WrapReverse, Some(25.0), 5.0);

        assert_eq!(
            lines,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[2])],
                    outer_hypothetical_main_size_px: 10.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 10.0,
                },
            ],
        );
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
        assert_eq!(
            solve
                .measured_items_in_dom_order
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![first, second, third],
        );
        assert_eq!(
            solve
                .line_collection_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![second, third, first],
        );
        assert_eq!(
            solve.lines,
            vec![
                FlexLine {
                    item_sources: vec![source(second), source(third)],
                    outer_hypothetical_main_size_px: 35.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(first)],
                    outer_hypothetical_main_size_px: 20.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        assert_eq!(solve.main_size_px, 35.0);
        assert_eq!(solve.cross_size_px, 20.0);
        assert!(solve.resolved_items.is_empty());
    }

    #[test]
    fn resolve_active_flex_container_assigns_cross_offsets_per_line() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[1]), 0.0, 0.0, 30.0, 10.0),
            measured_row_item(source(keys[2]), 0.0, 0.0, 20.0, 10.0),
        ];
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );

        let solve = resolve_active_flex_container(
            measured_solve,
            wrap_row_container_style(),
            50.0,
            Some(10.0),
        );

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            keys,
        );
        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 0.0, 30.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::Center;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::FlexEnd;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceBetween;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
                item_sources: vec![source(keys[0]), source(keys[1])],
                outer_hypothetical_main_size_px: 20.0,
                cross_size_px: 10.0,
            }],
        );
        let mut container_style = row_container_style();
        container_style.justify_content = super::FlexMainAxisAlignment::FlexEnd;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 60.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 60.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 100.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceAround;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 100.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::SpaceEvenly;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        let resolved_cross_offsets = solve
            .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 30.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[1]), source(keys[2])],
                    outer_hypothetical_main_size_px: 50.0,
                    cross_size_px: 10.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.align_content = super::FlexLineCrossAxisAlignment::Stretch;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 50.0, Some(40.0));

        assert_eq!(solve.line_cross_sizes_px, vec![20.0, 20.0]);
        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.cross_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 20.0, 20.0],
        );
        assert_eq!(solve.cross_size_px, 40.0);
    }

    #[test]
    fn cross_axis_alignment_offset_packs_item_to_cross_end() {
        let resolved_item = super::ResolvedFlexItem {
            source: FlexItemSource::BlockItem(DomNodeSlotKey::default()),
            align_self: Some(super::FlexCrossAxisAlignment::FlexEnd),
            resolved_main_size_px: 10.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 10.0,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexStart;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0),
            40.0,
        );
    }

    #[test]
    fn resolve_active_flex_container_distributes_positive_free_space_by_grow_factor() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 1.0, 10.0, 10.0),
            measured_row_item(source(keys[1]), 1.0, 1.0, 10.0, 10.0),
            measured_row_item(source(keys[2]), 2.0, 1.0, 10.0, 10.0),
        ];
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 90.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![10.0, 30.0, 50.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 100.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![50.0, 20.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 100.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![20.0, 80.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 20.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![15.0, 5.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 120.0, Some(18.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![16.0, 84.0, 16.0],
        );
        assert_eq!(
            solve
                .resolved_items
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

        let lines = build_measured_flex_lines(&items, FlexWrapMode::Wrap, Some(45.0), 5.0);

        assert_eq!(
            lines,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0])],
                    outer_hypothetical_main_size_px: 23.0,
                    cross_size_px: 10.0,
                },
                FlexLine {
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 80.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 28.0],
        );
        assert_eq!(
            solve
                .resolved_items
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

        let line = super::build_measured_flex_line(&items, 5.0);

        assert_eq!(
            line,
            FlexLine {
                item_sources: keys
                    .iter()
                    .copied()
                    .map(FlexItemSource::BlockItem)
                    .collect::<Vec<_>>(),
                outer_hypothetical_main_size_px: 40.0,
                cross_size_px: 15.0,
            },
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![
                FlexLine {
                    item_sources: vec![source(keys[0]), source(keys[1])],
                    outer_hypothetical_main_size_px: 25.0,
                    cross_size_px: 15.0,
                },
                FlexLine {
                    item_sources: vec![source(keys[2])],
                    outer_hypothetical_main_size_px: 10.0,
                    cross_size_px: 7.0,
                },
            ],
        );
        let mut container_style = wrap_row_container_style();
        container_style.gap_sizes.between_lines_px = 5.0;

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 25.0, Some(40.0));

        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 50.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![28.0, 22.0],
        );
        assert_eq!(
            solve
                .resolved_items
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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 20.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.resolved_main_size_px)
                .collect::<Vec<_>>(),
            vec![40.0, 0.0],
        );
        assert_eq!(
            solve
                .resolved_items
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

        let resolved = super::resolve_flex_line_items(&[item], 4.0, row_container_style(), 0.0);

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
        let measured_solve = resolve_measured_flex_container(
            items.clone(),
            items,
            vec![FlexLine {
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

        let solve =
            resolve_active_flex_container(measured_solve, container_style, 100.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![40.0, 50.0],
        );
    }

    #[test]
    fn align_self_center_overrides_container_align_items_for_cross_axis_alignment() {
        let resolved_item = super::ResolvedFlexItem {
            source: FlexItemSource::BlockItem(test_keys(1)[0]),
            align_self: Some(super::FlexCrossAxisAlignment::Center),
            resolved_main_size_px: 10.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 10.0,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexStart;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0),
            20.0,
        );
    }

    #[test]
    fn stretch_alignment_uses_line_start_offset() {
        let resolved_item = super::ResolvedFlexItem {
            source: FlexItemSource::BlockItem(test_keys(1)[0]),
            align_self: Some(super::FlexCrossAxisAlignment::Stretch),
            resolved_main_size_px: 10.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 10.0,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexEnd;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0),
            0.0,
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
        let solve =
            resolve_active_flex_container(measured_solve, row_container_style(), 30.0, Some(10.0));

        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.source.key())
                .collect::<Vec<_>>(),
            vec![second, third, first],
        );
        assert_eq!(
            solve
                .resolved_items
                .iter()
                .map(|item| item.main_offset_px)
                .collect::<Vec<_>>(),
            vec![0.0, 10.0, 20.0],
        );
    }

    #[test]
    fn resolved_flex_item_main_start_px_flips_reversed_axis_without_reordering_items() {
        assert_eq!(
            resolved_flex_item_main_start_px(20.0, 10.0, 90.0, false),
            20.0,
        );
        assert_eq!(
            resolved_flex_item_main_start_px(20.0, 10.0, 90.0, true),
            60.0,
        );
    }

    #[test]
    fn single_line_flex_resolved_child_context_maps_vertical_main_axis_to_top_and_height() {
        let child_context = base_context();
        let resolved_item = super::ResolvedFlexItem {
            source: FlexItemSource::BlockItem(test_keys(1)[0]),
            align_self: None,
            resolved_main_size_px: 30.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 12.0,
            main_offset_px: 18.0,
            cross_offset_px: 7.0,
        };

        let placed_context = active_flex_resolved_child_context(
            child_context.clone(),
            FlexContainerStyle {
                main_axis: FlexAxis::Vertical,
                is_main_reversed: true,
                text_direction: DomTextDirection::Ltr,
                wrap_mode: FlexWrapMode::NoWrap,
                justify_content: super::FlexMainAxisAlignment::FlexStart,
                align_items: super::FlexCrossAxisAlignment::FlexStart,
                align_content: super::FlexLineCrossAxisAlignment::FlexStart,
                gap_sizes: test_gap_sizes(),
            },
            90.0,
            resolved_item,
            NodeStyle {
                display: LayoutDisplay::Block,
                position: PositionMode::Static,
                layer_kind: DomLayerKind::Normal,
                font_size_px: 16.0,
                sides: BoxSides {
                    margin: BoxEdgeInsets::default(),
                    border: BoxEdgeInsets::default(),
                    padding: BoxEdgeInsets::default(),
                },
                box_sizing: BoxSizingKw::ContentBox,
                width_px: None,
                height_px: Some(12.0),
                min_width_px: None,
                min_height_px: None,
                max_width_px: None,
                max_height_px: None,
                top_px: None,
                left_px: None,
                right_px: None,
                bottom_px: None,
            },
            12.0,
        );

        assert_eq!(
            placed_context.flow_left_px,
            child_context.flow_left_px + 7.0
        );
        assert_eq!(placed_context.flow_top_px, child_context.flow_top_px + 42.0);
        assert_eq!(
            placed_context.available_width_px,
            child_context.available_width_px
        );
        assert_eq!(placed_context.available_height_px, Some(30.0));
    }

    #[test]
    fn single_line_flex_content_sizes_map_vertical_axis_into_width_and_height() {
        assert_eq!(
            active_flex_content_width_px(FlexAxis::Horizontal, 90.0, 18.0),
            90.0,
        );
        assert_eq!(
            active_flex_content_height_px(FlexAxis::Horizontal, 90.0, 18.0),
            18.0,
        );
        assert_eq!(
            active_flex_content_width_px(FlexAxis::Vertical, 90.0, 18.0),
            18.0,
        );
        assert_eq!(
            active_flex_content_height_px(FlexAxis::Vertical, 90.0, 18.0),
            90.0,
        );
    }

    #[test]
    fn resolve_measured_flex_container_uses_widest_line_and_total_cross_size() {
        let keys = test_keys(3);
        let items = vec![
            measured_row_item(source(keys[0]), 0.0, 1.0, 10.0, 12.0),
            measured_row_item(source(keys[1]), 0.0, 1.0, 16.0, 18.0),
        ];
        let lines = vec![
            FlexLine {
                item_sources: vec![source(keys[0])],
                outer_hypothetical_main_size_px: 10.0,
                cross_size_px: 12.0,
            },
            FlexLine {
                item_sources: vec![source(keys[1])],
                outer_hypothetical_main_size_px: 16.0,
                cross_size_px: 18.0,
            },
        ];

        let solve = resolve_measured_flex_container(items.clone(), items.clone(), lines.clone());

        assert_eq!(solve.measured_items_in_dom_order.len(), items.len());
        assert_eq!(solve.line_collection_items, items);
        assert_eq!(solve.lines, lines);
        assert_eq!(solve.main_size_px, 16.0);
        assert_eq!(solve.cross_size_px, 30.0);
        assert!(solve.resolved_items.is_empty());
    }
}
