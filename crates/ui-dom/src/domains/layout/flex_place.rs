//! Flex placement — final position and size writes.
//!
//! This file owns final placement: walking the resolved items in DOM order,
//! aligning each item on the cross axis, resolving the reversed main-axis
//! start, absorbing auto margins, laying out each child subtree, and placing
//! excluded abs-pos children at their static positions.
//!
//! Invariants and rules:
//!
//! - placement reads the resolve phase's packing results (main/cross offsets,
//!   resolved line cross) without re-solving
//! - auto margins are absorbed here (§8.1); the block solver is guarded to 0
//!   inside a flex item so absorption is never double-counted
//! - excluded abs-pos children are laid out after real items, at their 0×0
//!   static position, with the container's padding box as containing block
#![allow(clippy::needless_pass_by_value)] // LayoutContext is passed by value as a child's owned context, mirroring flow.rs

use crate::types::{DomError, DomNode, DomNodeSlotKey, LayoutScratch, RootDom, RootDomMetricName};

use super::DomLayoutResolver;
use super::flex::{
    ActiveFlexChildRelayoutRequest, ActiveFlexExitRequest, ActiveFlexPlacementRequest, FlexAxis,
    FlexContainerSolve, FlexContainerStyle, FlexCrossAxisAlignment, FlexItemView,
    PlacedFlexChildren, ResolvedFlexItem, active_flex_content_height_px,
    active_flex_content_width_px, cross_axis_border_padding_size_px, cross_axis_flow_size,
    item_align_self, item_order_from_node, node_for_key, non_negative_flex_axis_size_px,
    resolved_cross_axis_alignment, resolved_cross_axis_is_reversed, resolved_main_axis_is_reversed,
};
use super::flex_anonymous_run::node_is_excluded_flex_child;
use super::flex_item_source::FlexItemSource;
use super::flex_resolve::{
    initial_main_axis_alignment_offset_px, total_line_cross_size_px_from_lane,
};
use super::flow::{
    EnteredNodeLayout, FlowSize, InlineFlowState, LayoutContext, auto_content_width_for_exit,
    exit_node, first_child_key, next_inline_child_context, next_sibling_key,
    relayout_tree_walker_node, resolved_node_style,
};
use super::style::{NodeStyle, PositionMode, presented_position};

pub(crate) fn exit_active_flex_container<R>(
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
            child_context: &request.child_context,
            container_style: request.container_style,
            container_main_size_px: request.solve.main_size_px,
            solve: request.solve,
        },
        resolver,
    )?;
    let solve =
        flex_container_solve_with_placed_active_flex_children(request.solve, placed_children);
    place_excluded_abs_flex_children(
        root_dom,
        &ActiveFlexAbsPlacementRequest {
            tree_size,
            container_key: request.key,
            entered: &request.entered,
            child_context: &request.child_context,
            container_style: request.container_style,
            solve,
        },
        resolver,
    )?;
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

pub(crate) fn flex_container_solve_with_placed_active_flex_children(
    mut solve: FlexContainerSolve,
    placed_children: PlacedFlexChildren,
) -> FlexContainerSolve {
    solve.cross_size_px = placed_children.container_cross_size_px;
    solve
}

/// Lays out each EXCLUDED abs-pos/fixed child of the container at its D3
/// static position (flex arc step 3b). Runs after the real items are placed:
/// the static position is read from the solved lanes (item/order/line/
/// resolution) WITHOUT re-solving, and each child is laid out through the
/// existing abs-pos machinery (`relayout_tree_walker_node`) with the
/// container's PADDING box as its containing block.
pub(crate) struct ActiveFlexAbsPlacementRequest<'a> {
    tree_size: (f32, f32),
    container_key: DomNodeSlotKey,
    entered: &'a EnteredNodeLayout,
    child_context: &'a LayoutContext,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
}

pub(crate) fn place_excluded_abs_flex_children<R>(
    root_dom: &mut RootDom,
    request: &ActiveFlexAbsPlacementRequest<'_>,
    resolver: &mut R,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let mut child_key = root_dom
        .nodes
        .get(request.container_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    while let Some(current_child_key) = child_key {
        let (is_abs_fixed, child_order, child_align_self, next_child_key) = {
            let current_node = root_dom
                .nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            let is_abs_fixed = matches!(
                presented_position(current_node),
                PositionMode::Absolute | PositionMode::Fixed
            );
            let child_order = if is_abs_fixed {
                item_order_from_node(current_node)
            } else {
                0
            };
            let child_align_self = if is_abs_fixed {
                item_align_self(current_node)
            } else {
                None
            };
            (
                is_abs_fixed,
                child_order,
                child_align_self,
                current_node.next_sibling_key_for_store(),
            )
        };
        if is_abs_fixed {
            place_one_excluded_abs_flex_child(
                root_dom,
                request,
                current_child_key,
                child_order,
                child_align_self,
                resolver,
            )?;
        }
        child_key = next_child_key;
    }
    Ok(())
}

/// Lays out ONE excluded abs-pos/fixed child at its D3 static position.
pub(crate) fn place_one_excluded_abs_flex_child<R>(
    root_dom: &mut RootDom,
    request: &ActiveFlexAbsPlacementRequest<'_>,
    child_key: DomNodeSlotKey,
    child_order: i32,
    child_align_self: Option<FlexCrossAxisAlignment>,
    resolver: &mut R,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let (static_main_px, static_cross_px) = static_position_for_excluded_abs_child(
        root_dom,
        request.container_key,
        child_key,
        child_order,
        child_align_self,
        request.container_style,
        request.solve,
    )?;

    // Map (static main, static cross) to physical CONTENT-box offsets, applying
    // main-axis reversal exactly like `active_flex_resolved_child_context`
    // (a 0×0 item has zero resolved main size).
    let main_start_px = resolved_flex_item_main_start_px(
        static_main_px,
        0.0,
        request.solve.main_size_px,
        resolved_main_axis_is_reversed(request.container_style),
        // An excluded abs-pos/fixed child has no flex-item margins: its static
        // position is a 0×0 placeholder, so the reversed flip has no own
        // main-start/main-end margin to subtract.
        0.0,
        0.0,
    );
    let (content_left_px, content_top_px) = match request.container_style.main_axis {
        FlexAxis::Horizontal => (main_start_px, static_cross_px),
        FlexAxis::Vertical => (static_cross_px, main_start_px),
    };

    // The static position is CONTENT-box-relative; the containing block is the
    // container's PADDING box (CSS 2.1 §10.1, the reviewer's amendment). Keep
    // the two distinct: the abs-pos layout below receives the padding box as CB.
    let (padding_left_px, padding_top_px, padding_width_px, padding_height_px) =
        flex_container_padding_box_geometry(
            request.entered,
            request.container_style,
            request.solve,
        );

    let abs_context = LayoutContext {
        containing_layer_absolute_left_px: padding_left_px,
        containing_layer_absolute_top_px: padding_top_px,
        containing_layer_width_px: padding_width_px,
        containing_layer_height_px: padding_height_px,
        flow_left_px: 0.0,
        flow_top_px: 0.0,
        ..*request.child_context
    };

    // Real abs-pos layout (CSS 2.1 §10.3.7/§10.6.4): the child sizes per its
    // own rules and its subtree is laid out. With no explicit left/top offsets
    // the machinery resolves the border-box origin to the CB origin (0,0), so
    // the static position below is applied by writing the child's border-box
    // origin: content-box origin + static offset. Composite reads a positioned
    // layer's own border-box origin as its layer offset, so this lands the
    // whole subtree at the static position.
    relayout_tree_walker_node(
        root_dom,
        request.tree_size,
        child_key,
        abs_context,
        resolver,
    )?;
    let content_left_px =
        padding_left_px + request.entered.style.sides.padding.left_px + content_left_px;
    let content_top_px =
        padding_top_px + request.entered.style.sides.padding.top_px + content_top_px;
    write_node_border_box_position(root_dom, child_key, content_left_px, content_top_px)
}

/// The container's PADDING-box geometry as the abs-pos containing block:
/// `(absolute_left, absolute_top, width, height)`.
pub(crate) fn flex_container_padding_box_geometry(
    entered: &EnteredNodeLayout,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
) -> (f32, f32, f32, f32) {
    let content_height_px = active_flex_content_height_px(
        container_style.main_axis,
        solve.main_size_px,
        solve.cross_size_px,
    );
    let padding_box_width_px = entered.content_width_px
        + entered.style.sides.padding.left_px
        + entered.style.sides.padding.right_px;
    let padding_box_height_px = content_height_px
        + entered.style.sides.padding.top_px
        + entered.style.sides.padding.bottom_px;
    (
        entered.absolute_left_px + entered.style.sides.border.left_px,
        entered.absolute_top_px + entered.style.sides.border.top_px,
        padding_box_width_px,
        padding_box_height_px,
    )
}

/// The D3 static position of one excluded abs-pos child: `(static_main,
/// static_cross)` in the container's content box, computed from the solved
/// lanes WITHOUT re-solving (the 0×0 item changes nothing about line breaking,
/// so real items keep their solved line membership).
pub(crate) fn static_position_for_excluded_abs_child(
    root_dom: &RootDom,
    container_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
    child_order: i32,
    child_align_self: Option<FlexCrossAxisAlignment>,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
) -> Result<(f32, f32), DomError> {
    let scratch = &root_dom.layout_scratch;
    let slot = excluded_flex_child_slot(
        root_dom,
        scratch,
        container_key,
        child_key,
        child_order,
        solve,
    )?;
    let static_main_px = excluded_flex_child_static_main_px(scratch, container_style, solve, slot);
    let static_cross_px = excluded_flex_child_static_cross_px(
        scratch,
        container_style,
        solve,
        slot,
        child_align_self,
        child_key,
    );
    Ok((static_main_px, static_cross_px))
}

/// The order-modified insertion slot (0..=n) for an excluded child, keyed by
/// `(order, dom sequence)` among the REAL items (D3): the number of real items
/// that sort strictly before the excluded child. Stable: equal orders keep DOM
/// order.
pub(crate) fn excluded_flex_child_slot(
    root_dom: &RootDom,
    scratch: &LayoutScratch,
    container_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
    child_order: i32,
    solve: FlexContainerSolve,
) -> Result<usize, DomError> {
    let item_entries = scratch.items.slice(solve.item_range);
    // items_before_dom: real items whose DOM anchor precedes the child (the
    // item lane is DOM-ordered, so the first items_before_dom dom_indices are
    // exactly the items before the child in DOM order).
    let mut items_before_dom = 0usize;
    for entry in item_entries {
        if dom_child_precedes(root_dom, container_key, entry.view.source.key(), child_key)? {
            items_before_dom += 1;
        }
    }
    let first_dom_index_after_child = solve.item_range.start
        + u32::try_from(items_before_dom).expect("item count should fit into u32");
    let mut slot = 0usize;
    for entry in scratch.order.slice(solve.order_range) {
        let sorts_before_child = entry.order < child_order
            || (entry.order == child_order && entry.dom_index < first_dom_index_after_child);
        if sorts_before_child {
            slot += 1;
        }
    }
    Ok(slot)
}

/// Whether `first` precedes `second` in the container's child list (both are
/// direct children). `false` when neither appears first (internal corruption).
pub(crate) fn dom_child_precedes(
    root_dom: &RootDom,
    container_key: DomNodeSlotKey,
    first: DomNodeSlotKey,
    second: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let mut child_key = first_child_key(root_dom, container_key)?;
    while let Some(current_key) = child_key {
        if current_key == first {
            return Ok(true);
        }
        if current_key == second {
            return Ok(false);
        }
        child_key = next_sibling_key(root_dom, current_key)?;
    }
    Ok(false)
}

/// The static MAIN position of the 0×0 item at `slot` (D3): walk the line's
/// solved items — the position after the `k`th item is
/// `item[k].main_offset + item[k].outer_main + gap`; `k == 0` yields the
/// line's initial justify offset; a slot past the end yields the position after
/// the last item (no trailing gap).
pub(crate) fn excluded_flex_child_static_main_px(
    scratch: &LayoutScratch,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
    slot: usize,
) -> f32 {
    let order_entries = scratch.order.slice(solve.order_range);
    let item_count = order_entries.len();
    if item_count == 0 {
        // No real items: the 0×0 item forms its own single line, packed by
        // justify-content alone (its line main size is 0).
        return initial_main_axis_alignment_offset_px(container_style, solve.main_size_px, 1);
    }
    if slot == 0 {
        // Before the first item: the first line's justify offset.
        return scratch.items.get(order_entries[0].dom_index).main_offset_px;
    }
    let last_dom_index = order_entries[slot - 1].dom_index;
    let last_entry = scratch.items.get(last_dom_index);
    let last_resolved_main_px = scratch
        .resolution
        .get(last_entry.order_index)
        .resolved_main_size_px;
    let outer_main_px = last_entry.view.margin_main_start_px
        + last_resolved_main_px
        + last_entry.view.margin_main_end_px;
    if slot == item_count {
        // Past the end: after the last item, no trailing gap.
        return last_entry.main_offset_px + outer_main_px;
    }
    let next_entry = scratch.items.get(order_entries[slot].dom_index);
    if last_entry.line_index == next_entry.line_index {
        // Same line: one gap adjacency before the slot.
        last_entry.main_offset_px + outer_main_px + container_style.gap_sizes.between_items_px
    } else {
        // The slot sits at a line break: the end of item[slot-1]'s line, no gap.
        last_entry.main_offset_px + outer_main_px
    }
}

/// The static CROSS position of the 0-height item (D3): per the child's
/// `align_self` (falling back to `align_items`), resolved against the line's
/// cross-start edge (which already carries the `align_content` packing offset).
pub(crate) fn excluded_flex_child_static_cross_px(
    scratch: &LayoutScratch,
    container_style: FlexContainerStyle,
    solve: FlexContainerSolve,
    slot: usize,
    child_align_self: Option<FlexCrossAxisAlignment>,
    child_key: DomNodeSlotKey,
) -> f32 {
    let order_entries = scratch.order.slice(solve.order_range);
    if order_entries.is_empty() {
        // No real items: the empty line has zero cross size, so every alignment
        // resolves to the cross-start (0).
        return 0.0;
    }
    // The slot's line is the line containing item[slot-1] (or item 0 at slot 0).
    let anchor_dom_index = if slot == 0 {
        order_entries[0].dom_index
    } else {
        order_entries[slot - 1].dom_index
    };
    let line_index = scratch.items.get(anchor_dom_index).line_index;
    let line = scratch.lines.get(line_index);
    let dummy_item = ResolvedFlexItem {
        source: FlexItemSource::BlockItem(child_key),
        align_self: child_align_self,
        resolved_main_size_px: 0.0,
        main_axis_border_padding_px: 0.0,
        resolved_cross_size_px: 0.0,
        replaced_ratio: false,
        main_offset_px: 0.0,
        cross_offset_px: 0.0,
    };
    line.cross_offset_px
        + cross_axis_alignment_offset_px(
            dummy_item,
            container_style,
            line.resolved_cross_size_px,
            false,
        )
}

/// Writes a node's retained border-box origin (the positioned-layer offset
/// composite reads back via `relative_offset_for_node`). Mirrors `write_node_box`
/// in `flow.rs`; scoped to the abs-child static-position write.
pub(crate) fn write_node_border_box_position(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    left_px: f32,
    top_px: f32,
) -> Result<(), DomError> {
    let node = root_dom
        .nodes
        .get_mut(key)
        .ok_or(DomError::InvalidNodeKey)?;
    match node {
        DomNode::Document(document) => {
            document.base.box_model.border_box.left_px = left_px;
            document.base.box_model.border_box.top_px = top_px;
        }
        DomNode::DocumentFragment(fragment) => {
            fragment.base.box_model.border_box.left_px = left_px;
            fragment.base.box_model.border_box.top_px = top_px;
        }
        DomNode::Element(element) => {
            element.base.box_model.border_box.left_px = left_px;
            element.base.box_model.border_box.top_px = top_px;
        }
        DomNode::Text(text) => {
            text.base.box_model.border_box.left_px = left_px;
            text.base.box_model.border_box.top_px = top_px;
        }
    }
    Ok(())
}

/// The physical `(left, right, top, bottom)` absorbed auto margins for one item
/// view, gated by the view's per-side auto flags: a non-auto edge reports 0.0
/// so the write below leaves the block solver's already-resolved edge untouched
/// (and the block solver is guarded to 0 for the auto edges).
///
/// The edge mapping is FIXED and physical (not reversal-aware), and that is
/// correct for BOTH LTR and reversed containers: `main_start` maps to physical
/// left (Horizontal) / top (Vertical) and `main_end` to physical right / bottom
/// regardless of axis reversal. The reversal is handled downstream — the
/// reversed flip in `resolved_flex_item_main_start_px` subtracts the item's
/// full outer main size (both margins) to place its margin-box start, so a
/// reversed axis needs no reversal-aware mapping here. Do NOT "fix" this
/// mapping to follow the reversed axis.
pub(crate) fn physical_absorbed_auto_margins(
    view: FlexItemView,
    main_axis: FlexAxis,
) -> (f32, f32, f32, f32) {
    let main_start = if view.margin_main_start_auto {
        view.margin_main_start_px
    } else {
        0.0
    };
    let main_end = if view.margin_main_end_auto {
        view.margin_main_end_px
    } else {
        0.0
    };
    let cross_start = if view.margin_cross_start_auto {
        view.margin_cross_start_px
    } else {
        0.0
    };
    let cross_end = if view.margin_cross_end_auto {
        view.margin_cross_end_px
    } else {
        0.0
    };
    match main_axis {
        FlexAxis::Horizontal => (main_start, main_end, cross_start, cross_end),
        FlexAxis::Vertical => (cross_start, cross_end, main_start, main_end),
    }
}

/// Writes a flex item's absorbed auto margins into its retained box model's
/// MARGIN edges (CSS Flexbox §8.1) for paint/hit-test. The border-box origin is
/// NOT shifted here: the placement context places the item's content origin at
/// the absorbed start BEFORE the subtree relayout
/// (`active_flex_resolved_child_context` adds the absorbed start margins to the
/// flow origin), so the layout's own border-box write lands at the absorbed
/// origin (the block auto-margin solver resolves a flex item's auto margins to
/// 0 — the double-absorption guard — so nothing is added on top). Run items
/// have no node box (and no auto margins), so this is a no-op for them; a view
/// with no absorbed edges is likewise a no-op.
pub(crate) fn write_flex_item_absorbed_auto_margins(
    root_dom: &mut RootDom,
    source: FlexItemSource,
    main_axis: FlexAxis,
    view: FlexItemView,
) -> Result<(), DomError> {
    let FlexItemSource::BlockItem(key) = source else {
        return Ok(());
    };
    let (absorbed_left, absorbed_right, absorbed_top, absorbed_bottom) =
        physical_absorbed_auto_margins(view, main_axis);
    if absorbed_left == 0.0
        && absorbed_right == 0.0
        && absorbed_top == 0.0
        && absorbed_bottom == 0.0
    {
        return Ok(());
    }
    let node = root_dom
        .nodes
        .get_mut(key)
        .ok_or(DomError::InvalidNodeKey)?;
    match node {
        DomNode::Document(document) => {
            document.base.box_model.margin.left_px += absorbed_left;
            document.base.box_model.margin.right_px += absorbed_right;
            document.base.box_model.margin.top_px += absorbed_top;
            document.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::DocumentFragment(fragment) => {
            fragment.base.box_model.margin.left_px += absorbed_left;
            fragment.base.box_model.margin.right_px += absorbed_right;
            fragment.base.box_model.margin.top_px += absorbed_top;
            fragment.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::Element(element) => {
            element.base.box_model.margin.left_px += absorbed_left;
            element.base.box_model.margin.right_px += absorbed_right;
            element.base.box_model.margin.top_px += absorbed_top;
            element.base.box_model.margin.bottom_px += absorbed_bottom;
        }
        DomNode::Text(text) => {
            text.base.box_model.margin.left_px += absorbed_left;
            text.base.box_model.margin.right_px += absorbed_right;
            text.base.box_model.margin.top_px += absorbed_top;
            text.base.box_model.margin.bottom_px += absorbed_bottom;
        }
    }
    Ok(())
}

pub(crate) fn place_active_flex_resolved_children<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexPlacementRequest<'_>,
    resolver: &mut R,
) -> Result<PlacedFlexChildren, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let solve = request.solve;
    let mut placed_item_count = 0usize;
    for dom_index in solve.item_range.start..solve.item_range.start + solve.item_range.len {
        let item_entry = *root_dom.layout_scratch.items.get(dom_index);
        let view = item_entry.view;
        let resolved_main_size_px = root_dom
            .layout_scratch
            .resolution
            .get(item_entry.order_index)
            .resolved_main_size_px;
        let line = *root_dom.layout_scratch.lines.get(item_entry.line_index);
        let resolved_item = ResolvedFlexItem {
            source: view.source,
            align_self: view.align_self,
            resolved_main_size_px,
            main_axis_border_padding_px: view.main_axis_border_padding_px,
            resolved_cross_size_px: view.resolved_cross_size_px,
            replaced_ratio: view.replaced_ratio,
            main_offset_px: item_entry.main_offset_px,
            cross_offset_px: line.cross_offset_px,
        };
        // D3 (flex arc step 10): an item whose auto cross margins absorbed a
        // positive share has its align-items/align-self offset suppressed (the
        // margins own the free space). The share is positive iff an auto cross
        // margin was mutated away from its 0.0 placeholder.
        let cross_auto_margins_absorbed = (view.margin_cross_start_auto
            && view.margin_cross_start_px > 0.0)
            || (view.margin_cross_end_auto && view.margin_cross_end_px > 0.0);
        let aligned_resolved_item = aligned_resolved_flex_item(
            resolved_item,
            request.container_style,
            line.resolved_cross_size_px,
            cross_auto_margins_absorbed,
        );
        let placed_resolved_item = relayout_resolved_flex_child(
            root_dom,
            tree_size,
            &ActiveFlexChildRelayoutRequest {
                source: view.source,
                child_context: request.child_context,
                container_style: request.container_style,
                container_main_size_px: request.container_main_size_px,
                line_cross_size_px: line.resolved_cross_size_px,
                resolved_item: aligned_resolved_item,
                view,
            },
            resolver,
        )?;
        // The block auto-margin solver is guarded to 0 inside a flex item, so
        // the absorbed auto margins land here, in the item's retained box model
        // (all four margin edges written for paint/hit-test). The border-box
        // origin is NOT shifted here — the placement context applies the
        // absorbed start margins to the flow origin before the subtree relayout
        // (`active_flex_resolved_child_context`), so this call writes only the
        // margin edges.
        write_flex_item_absorbed_auto_margins(
            root_dom,
            view.source,
            request.container_style.main_axis,
            view,
        )?;
        let line_slot = usize::try_from(item_entry.line_index - solve.line_range.start)
            .expect("line index should fall within the container's line range");
        let line_entries = root_dom.layout_scratch.lines.slice_mut(solve.line_range);
        let line_entry = &mut line_entries[line_slot];
        line_entry.placed_cross_size_px = line_entry
            .placed_cross_size_px
            .max(placed_resolved_item.resolved_cross_size_px);
        placed_item_count += 1;
    }
    assert_eq!(
        placed_item_count,
        usize::try_from(solve.item_range.len).expect("item range length should fit into usize"),
        "active flex placement should write each resolved child exactly once",
    );
    Ok(PlacedFlexChildren {
        container_cross_size_px: total_line_cross_size_px_from_lane(
            &root_dom.layout_scratch,
            solve.line_range,
            request.container_style.gap_sizes.between_lines_px,
        ),
    })
}

pub(crate) fn aligned_resolved_flex_item(
    resolved_item: ResolvedFlexItem,
    container_style: FlexContainerStyle,
    line_cross_size_px: f32,
    cross_auto_margins_absorbed: bool,
) -> ResolvedFlexItem {
    ResolvedFlexItem {
        cross_offset_px: resolved_item.cross_offset_px
            + cross_axis_alignment_offset_px(
                resolved_item,
                container_style,
                line_cross_size_px,
                cross_auto_margins_absorbed,
            ),
        ..resolved_item
    }
}

pub(crate) fn relayout_active_flex_child<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexChildRelayoutRequest<'_>,
    resolver: &mut R,
) -> Result<ResolvedFlexItem, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    // One increment per child the placement walk relayouts (the double-layout
    // work counter — every resolved child is relaid out once per placement).
    root_dom.increment_metric(RootDomMetricName::FlexNodesPlaced);
    let (child_style, _) =
        resolved_node_style(root_dom, request.source.key(), request.child_context)?;
    let placed_context = active_flex_resolved_child_context(
        request.child_context.clone(),
        request.container_style,
        request.container_main_size_px,
        request.resolved_item,
        child_style,
        request.line_cross_size_px,
        request.view,
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

/// Placement dispatch: a block item relayouts its single child; an anonymous
/// inline run relayouts its run of children as one inline formatting context.
pub(crate) fn relayout_resolved_flex_child<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexChildRelayoutRequest<'_>,
    resolver: &mut R,
) -> Result<ResolvedFlexItem, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    match request.source {
        FlexItemSource::BlockItem(_) => {
            relayout_active_flex_child(root_dom, tree_size, request, resolver)
        }
        FlexItemSource::AnonymousInlineGroup { .. } => {
            relayout_anonymous_inline_run(root_dom, tree_size, request, resolver)
        }
    }
}

/// Lays out an anonymous inline run's children as ONE inline formatting
/// context inside the resolved item box (CSS Flexbox §4). The children flow
/// inline from the item's content origin, reusing the existing inline flow
/// machinery (`relayout_tree_walker_node` + `next_inline_child_context`) over
/// the parentless sibling range — the thin-entry-point path, not a reshaped
/// production walk.
pub(crate) fn relayout_anonymous_inline_run<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    request: &ActiveFlexChildRelayoutRequest<'_>,
    resolver: &mut R,
) -> Result<ResolvedFlexItem, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    // One increment per run the placement walk relayouts (a run is one virtual
    // item relaid out once per placement, matching the block-item counter).
    root_dom.increment_metric(RootDomMetricName::FlexNodesPlaced);
    let first_child_key = request.source.first_child_key();
    let child_count = request.source.child_count();
    let origin_context = anonymous_inline_run_child_context(
        request.child_context,
        request.container_style,
        request.container_main_size_px,
        request.resolved_item,
    );
    let mut flow = InlineFlowState::default();
    let mut child_key = Some(first_child_key);
    // Lay out EXACTLY `child_count` run MEMBERS (the run's direct children),
    // never past the run's end into the following item. Excluded children
    // (abs-pos/`display: none`) sit between run members without breaking the
    // run (the D1 amendment), so they are SKIPPED here and laid out by
    // `place_excluded_abs_flex_children` — they are not members and contribute
    // no inline width. A missing member (a `child_count` larger than the
    // surviving members) is internal corruption.
    let mut members_laid_out = 0usize;
    while members_laid_out
        < usize::try_from(child_count).expect("run child count should fit into usize")
    {
        let current_child_key = child_key.expect("run child should exist within child_count");
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        if node_is_excluded_flex_child(node_for_key(root_dom, current_child_key)?) {
            child_key = next_child_key;
            continue;
        }
        let child_context = next_inline_child_context(origin_context.clone(), flow);
        let child_flow = relayout_tree_walker_node(
            root_dom,
            tree_size,
            current_child_key,
            child_context,
            resolver,
        )?;
        flow.inline_left_px += child_flow.width_px;
        flow.line_height_px = flow.line_height_px.max(child_flow.height_px);
        members_laid_out += 1;
        child_key = next_child_key;
    }
    let measured_cross_size_px = cross_axis_flow_size(
        request.container_style.main_axis,
        FlowSize {
            width_px: flow.inline_left_px,
            height_px: flow.line_height_px,
            min_width_px: 0.0,
            max_width_px: flow.inline_left_px,
        },
    );
    Ok(ResolvedFlexItem {
        resolved_cross_size_px: anonymous_inline_run_resolved_cross_size_px(
            request.container_style,
            request.resolved_item,
            request.line_cross_size_px,
            measured_cross_size_px,
        ),
        ..request.resolved_item
    })
}

/// The run's item-box content origin, mirroring `active_flex_resolved_child_context`
/// without a child style (a run has no node style): `flow_left`/`flow_top` are
/// the item's main/cross start. On the horizontal-main axis the resolved
/// content main size (the run's resolved width minus its zero border/padding)
/// is passed down as `available_width_px`, so the run's children lay out
/// inside the resolved box and wrap at the resolved width (CSS Flexbox §4:
/// the run shrinks with the CSS initial `flex-shrink: 1`, and a shrunk run's
/// cross reflects the wrapping the shrink causes). The vertical-main axis keeps
/// `available_width_px: None` — the run's cross (its horizontal max-content
/// width) never shrinks, so there is no vertical-main wrapping width to impose.
pub(crate) fn anonymous_inline_run_child_context(
    child_context: &LayoutContext,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    resolved_item: ResolvedFlexItem,
) -> LayoutContext {
    let main_start_px = resolved_flex_item_main_start_px(
        resolved_item.main_offset_px,
        resolved_item.resolved_main_size_px,
        container_main_size_px,
        resolved_main_axis_is_reversed(container_style),
        // A run item has no node style and therefore no authored/auto margins
        // (CSS Flexbox §4: an anonymous run box carries no box model), so the
        // reversed flip has no own margins to subtract.
        0.0,
        0.0,
    );
    let cross_start_px = resolved_item.cross_offset_px;
    match container_style.main_axis {
        FlexAxis::Horizontal => LayoutContext {
            flow_left_px: child_context.flow_left_px + main_start_px,
            flow_top_px: child_context.flow_top_px + cross_start_px,
            available_width_px: Some(
                resolved_flex_item_content_main_size_px(resolved_item).max(0.0),
            ),
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            ..*child_context
        },
        FlexAxis::Vertical => LayoutContext {
            flow_left_px: child_context.flow_left_px + cross_start_px,
            flow_top_px: child_context.flow_top_px + main_start_px,
            available_width_px: None,
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            ..*child_context
        },
    }
}

pub(crate) fn cross_axis_alignment_offset_px(
    resolved_item: ResolvedFlexItem,
    container_style: FlexContainerStyle,
    line_cross_size_px: f32,
    cross_auto_margins_absorbed: bool,
) -> f32 {
    // CSS Flexbox §8.1 (D3): when the item's auto cross margins absorbed the
    // line's free cross space, align-items/align-self has no effect on that
    // item — the absorbed margins own the free space and the alignment offset
    // is 0.
    if cross_auto_margins_absorbed {
        return 0.0;
    }
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

pub(crate) fn resolved_flex_item_cross_axis_alignment(
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

/// Builds the placement item-context for one resolved flex item (flex arc
/// step 6). Two basis separations live here:
///
/// - `available_width_px` is the GENERAL percentage basis — the CONTAINER's
///   content-box width (carried from the container's entered record via
///   `child_context.available_width_px`), matching measure time and CSS Values
///   §percentages for the whole width family (`width`, `min-width`,
///   `max-width`, `margin×4`, `padding×4`, `left`, `right`). It is NOT the
///   item's resolved main size.
/// - `auto_margin_reference_width_px` is the DOUBLE-ABSORPTION GUARD (D4, flex
///   arc step 10): it gives the block §10.3.3 solver a reference width ≤ the
///   item's own resolved content size on the axis the block solver would
///   otherwise absorb, so `free = item_content − item_border_box ≤ 0` and the
///   block solver's `resolve_block_auto_margins` resolves flex-item auto
///   margins ONLY to 0 — never twice. This is the guard that keeps the flex and
///   block auto-margin solvers disjoint (§8.1 landed in the flex solver, so the
///   flex solver owns absorption: D2 main, D3 cross). `None` (every non-flex
///   context) preserves the block solver's normal fallback.
///
/// Verification enumeration (D2 obligation): the item context's
/// `available_width_px` is read by (1) the item's `NodeStyle` width-family
/// resolution (`resolved_node_style` → `node_style_basis`, `flow.rs`) — the
/// fix's target; (2) `resolve_node_content_width` for the item's own box —
/// driven by the `node_content_width_override_px` override, not this field
/// (`style_with_node_content_size_overrides`, `flow.rs`); (3) the children's
/// context derivation — from the entered record's `content_width_px`
/// (`child_available_width_px`, `flow.rs`), not this field; (4) the re-pointed
/// `resolve_block_auto_margins` — now the dedicated
/// `auto_margin_reference_width_px`, not this field; (5) no other reader
/// changes beyond the width family.
pub(crate) fn active_flex_resolved_child_context(
    child_context: LayoutContext,
    container_style: FlexContainerStyle,
    container_main_size_px: f32,
    resolved_item: ResolvedFlexItem,
    child_style: NodeStyle,
    line_cross_size_px: f32,
    view: FlexItemView,
) -> LayoutContext {
    let cross_axis_alignment =
        resolved_flex_item_cross_axis_alignment(resolved_item, container_style);
    let main_start_px = resolved_flex_item_main_start_px(
        resolved_item.main_offset_px,
        resolved_item.resolved_main_size_px,
        container_main_size_px,
        resolved_main_axis_is_reversed(container_style),
        view.margin_main_start_px,
        view.margin_main_end_px,
    );
    let cross_start_px = resolved_item.cross_offset_px;
    // The absorbed auto START margins (physical left/top) shift the item's
    // content origin BEFORE the subtree relayout, so the item's own border-box
    // write and its descendants both land at the absorbed origin. The block
    // auto-margin solver resolves a flex item's auto margins to 0 at relayout
    // (the double-absorption guard), so the layout's own margin application
    // adds nothing on top of this shift — no double count. For REVERSED
    // containers the main-axis flip above already subtracted the item's own
    // margins (so the absorbed start margin re-enters here without a double
    // count); the CROSS-start absorbed margin is unaffected by main-axis
    // reversal and always shifts.
    let (absorbed_left, _, absorbed_top, _) =
        physical_absorbed_auto_margins(view, container_style.main_axis);
    let resolved_content_main_size_px =
        resolved_flex_item_content_main_size_px(resolved_item).max(0.0);
    let resolved_content_cross_size_px = (resolved_item.resolved_cross_size_px
        - cross_axis_border_padding_size_px(child_style.sides, container_style.main_axis))
    .max(0.0);
    let stretched_content_cross_size_px = stretched_flex_item_content_cross_size_px(
        child_style,
        container_style,
        cross_axis_alignment,
        line_cross_size_px,
        resolved_item.replaced_ratio,
        view.margin_cross_start_auto || view.margin_cross_end_auto,
    );
    match container_style.main_axis {
        FlexAxis::Horizontal => LayoutContext {
            flow_left_px: child_context.flow_left_px + main_start_px + absorbed_left,
            flow_top_px: child_context.flow_top_px + cross_start_px + absorbed_top,
            available_width_px: child_context.available_width_px,
            auto_margin_reference_width_px: Some(resolved_content_main_size_px),
            node_content_width_override_px: Some(resolved_content_main_size_px),
            node_content_height_override_px: stretched_content_cross_size_px,
            ..child_context
        },
        FlexAxis::Vertical => LayoutContext {
            flow_left_px: child_context.flow_left_px + cross_start_px + absorbed_left,
            flow_top_px: child_context.flow_top_px + main_start_px + absorbed_top,
            available_height_px: Some(resolved_content_main_size_px),
            auto_margin_reference_width_px: Some(resolved_content_cross_size_px),
            node_content_width_override_px: stretched_content_cross_size_px,
            node_content_height_override_px: Some(resolved_content_main_size_px),
            ..child_context
        },
    }
}

pub(crate) fn stretched_flex_item_content_cross_size_px(
    child_style: NodeStyle,
    container_style: FlexContainerStyle,
    cross_axis_alignment: FlexCrossAxisAlignment,
    line_cross_size_px: f32,
    replaced_ratio: bool,
    cross_margins_auto: bool,
) -> Option<f32> {
    // CSS Flexbox §8.3 + CSS 2.1 §10.6.2: stretch applies only to a cross size
    // that is `auto`. A replaced element with an intrinsic ratio has a non-auto
    // cross size (its used height follows the ratio from its used width), so
    // stretch must NOT override it — returning `None` leaves the item's ratio-
    // derived cross intact (flex arc step 7 D1.2/D5). CSS Flexbox §9.4 (used
    // cross size) additionally requires that neither of the item's cross-axis
    // margins is auto: when either cross margin is auto, the used cross size is
    // the item's hypothetical cross size and the auto margin(s) absorb the free
    // space, so stretch must not override it either.
    if cross_axis_alignment != FlexCrossAxisAlignment::Stretch
        || replaced_ratio
        || style_has_definite_cross_axis_size(child_style, container_style.main_axis)
        || cross_margins_auto
    {
        return None;
    }
    Some(non_negative_flex_axis_size_px(
        line_cross_size_px
            - cross_axis_border_padding_size_px(child_style.sides, container_style.main_axis),
    ))
}

/// The run item's placed cross size (flex arc step 3c): a run's cross is always
/// auto and its border/padding is zero, so under `align-items`/`align-self:
/// stretch` the resolved cross equals the line cross — the stretched content
/// cross is `line cross − zero border/padding`. The run's children still render
/// at their own inline heights at the line's cross-start
/// (`relayout_anonymous_inline_run` / `anonymous_inline_run_child_context` lay
/// them out at `cross_start_px` with no cross-size override), matching the
/// node-item stretch behavior for their subtrees. Any other alignment keeps the
/// measured joint cross.
pub(crate) fn anonymous_inline_run_resolved_cross_size_px(
    container_style: FlexContainerStyle,
    resolved_item: ResolvedFlexItem,
    line_cross_size_px: f32,
    measured_cross_size_px: f32,
) -> f32 {
    if resolved_flex_item_cross_axis_alignment(resolved_item, container_style)
        == FlexCrossAxisAlignment::Stretch
    {
        non_negative_flex_axis_size_px(line_cross_size_px)
    } else {
        measured_cross_size_px
    }
}

pub(crate) fn resolved_flex_item_content_main_size_px(resolved_item: ResolvedFlexItem) -> f32 {
    assert!(
        resolved_item.resolved_main_size_px + f32::EPSILON
            >= resolved_item.main_axis_border_padding_px,
        "resolved flex item border-box main size should not be smaller than border and padding",
    );
    resolved_item.resolved_main_size_px - resolved_item.main_axis_border_padding_px
}

pub(crate) fn resolved_flex_item_main_start_px(
    main_offset_px: f32,
    resolved_main_size_px: f32,
    container_main_size_px: f32,
    is_main_reversed: bool,
    main_start_margin_px: f32,
    main_end_margin_px: f32,
) -> f32 {
    if !is_main_reversed {
        return main_offset_px;
    }
    // The reversed flip places the item's MARGIN-BOX start against the physical
    // main-start edge by subtracting the item's full outer main size — its own
    // main-start AND main-end margins plus the border-box main size (CSS
    // Flexbox §9.2: reversing the main axis swaps main-start/main-end, so the
    // item's margin at the spec main-start side is our `main_end_margin_px` —
    // under our fixed `main_axis_start/end` → left/right (Horizontal) / top/
    // bottom (Vertical) mapping, "end" is the bottom/right physical side, which
    // IS the reversed spec main-start). The two margins re-enter placement
    // downstream unchanged: the non-auto main-end margin via the block solver
    // (`local_left = flow_left + margin_left`), the auto one via the absorbed
    // start shift — so subtracting both here is what keeps the item's own
    // margins from drifting the border box (the old form dropped them entirely).
    container_main_size_px
        - main_offset_px
        - main_start_margin_px
        - main_end_margin_px
        - resolved_main_size_px
}

pub(crate) fn style_has_definite_cross_axis_size(style: NodeStyle, main_axis: FlexAxis) -> bool {
    match main_axis {
        FlexAxis::Horizontal => style.height_px.is_some(),
        FlexAxis::Vertical => style.width_px.is_some(),
    }
}
#[cfg(test)]
use super::flex_wrap_mode::FlexWrapMode;
#[cfg(test)]
use super::style::{BoxSides, LayoutDisplay};
#[cfg(test)]
use crate::domains::layout::flex::{
    FlexLineCrossAxisAlignment, FlexMainAxisAlignment, base_context, measured_row_item,
    row_container_style, source, test_gap_sizes, test_keys,
};
#[cfg(test)]
use crate::domains::style::computed::BoxSizingKw;
#[cfg(test)]
use crate::types::{BoxEdgeInsets, DomLayerKind, DomTextDirection};

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn cross_axis_alignment_offset_packs_item_to_cross_end() {
        let resolved_item = super::ResolvedFlexItem {
            source: FlexItemSource::BlockItem(DomNodeSlotKey::default()),
            align_self: Some(super::FlexCrossAxisAlignment::FlexEnd),
            resolved_main_size_px: 10.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 10.0,
            replaced_ratio: false,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexStart;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0, false),
            40.0,
        );
    }

    #[test]
    fn anonymous_inline_run_resolves_cross_to_line_cross_under_stretch() {
        // A run's cross is always auto and its border/padding is zero, so under
        // `align-items: stretch` its placed cross resolves to the line cross
        // (the stretched content cross = line cross − zero border/padding);
        // under any other alignment it keeps the measured joint cross. This is
        // the flex arc step 3c re-adjudication: the anonymous wrapper stretches
        // while its inline content renders at its own inline height at the
        // line's cross-start.
        let run_item = super::ResolvedFlexItem {
            source: FlexItemSource::AnonymousInlineGroup {
                first_child_key: DomNodeSlotKey::default(),
                child_count: 1,
            },
            align_self: None,
            resolved_main_size_px: 14.0,
            main_axis_border_padding_px: 0.0,
            resolved_cross_size_px: 14.0,
            replaced_ratio: false,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::Stretch;

        assert_eq!(
            anonymous_inline_run_resolved_cross_size_px(container_style, run_item, 20.0, 14.0),
            20.0,
            "a stretch-aligned run resolves its placed cross to the line cross",
        );

        let mut centered_style = container_style;
        centered_style.align_items = super::FlexCrossAxisAlignment::Center;
        assert_eq!(
            anonymous_inline_run_resolved_cross_size_px(centered_style, run_item, 20.0, 14.0),
            14.0,
            "a non-stretch-aligned run keeps its measured joint cross",
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
            replaced_ratio: false,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexStart;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0, false),
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
            replaced_ratio: false,
            main_offset_px: 0.0,
            cross_offset_px: 0.0,
        };
        let mut container_style = row_container_style();
        container_style.align_items = super::FlexCrossAxisAlignment::FlexEnd;

        assert_eq!(
            cross_axis_alignment_offset_px(resolved_item, container_style, 50.0, false),
            0.0,
        );
    }

    #[test]
    fn resolved_flex_item_main_start_px_flips_reversed_axis_without_reordering_items() {
        assert_eq!(
            resolved_flex_item_main_start_px(20.0, 10.0, 90.0, false, 0.0, 0.0),
            20.0,
        );
        assert_eq!(
            resolved_flex_item_main_start_px(20.0, 10.0, 90.0, true, 0.0, 0.0),
            60.0,
        );
        // The reversed flip subtracts the item's full outer main size — both
        // main margins plus the border-box size — to place its margin-box start.
        assert_eq!(
            resolved_flex_item_main_start_px(20.0, 10.0, 90.0, true, 15.0, 5.0),
            40.0,
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
            replaced_ratio: false,
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
                margin_auto: [false; 4],
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
            measured_row_item(source(test_keys(1)[0]), 0.0, 0.0, 0.0, 0.0),
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
}
