#![allow(dead_code)]
#![allow(clippy::needless_pass_by_value)]

//! DOM-style normal-flow solving for the shared `TreeWalker`.
//!
//! What this file contains:
//!
//! - the single shared walker node entrypoint used during layout recursion
//! - block and inline flow solving for one node at a time
//! - local box math for width, height, margins, padding, borders, and
//!   positioned offsets
//! - final writes into node `box_model`, `layout`, and `measurement` (the
//!   measurement record is a pass OUTPUT written once per node at `exit_node`,
//!   equality-gated through `replace_measurement_if_changed` and never a dirt
//!   source)
//!
//! Invariants and rules:
//!
//! - this file solves one node from current DOM properties and current walker
//!   context; it must not store solver scratch on the node for its own use
//! - all persistent writes here must be for other phases or other nodes:
//!   `box_model`, `layout`, and `measurement`
//! - this file must not create a second walker or a parallel layout engine
//! - layout decisions should be recomputed from current state instead of cached
//!   unless later profiling proves otherwise
//! - width and height are the parent-facing propagation dimensions; local x/y
//!   placement is a result, not an ancestor propagation signal
//!
//! Advice and code philosophy:
//!
//! - keep solver entrypoints honest: block and inline stay here, while flex
//!   keeps a sibling solver file without changing the walker model
//! - prefer small, unsurprising helpers for box math instead of large style
//!   warehouses or hidden policy structs
//! - keep DOM-themed names close to the authored properties being interpreted
//! - if a value is only useful during this call stack, keep it local and let it
//!   disappear when the node finishes
//!
use crate::domains::style::computed::BoxSizingKw;
use crate::domains::text_layout::{
    TextLayoutRequest, text_wrap_mode_for_css_layout, with_repo_text_layout_context,
};
use crate::domains::visitor::inherited_style::{
    InheritedStyleContext, inherited_context_from_node,
};
use crate::types::{
    DomBoxModel, DomElementIntrinsicKind, DomError, DomIntrinsic, DomLayerKind, DomNode,
    DomNodeSlotKey, DomRect, DomTag, RootDom,
};

use super::measurement_record::compute_node_measurement;
use super::style::{
    BoxSides, LayoutDisplay, NodeStyle, PositionMode, intrinsic_box_for_node, node_style,
    presented_position,
};
use super::{DomLayoutInput, DomLayoutResolver, MeasurementRecord};

#[cfg(test)]
use crate::domains::layout::walker::relayout_tree;
#[cfg(test)]
use crate::domains::layout::{IdentityDomLayoutResolver, MeasurementFlags, node_box};
#[cfg(test)]
use crate::domains::style::computed::{
    BoxLength, ComputedGroups, DisplayKw, OverflowKw, PositionKw,
};
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{BoxEdgeInsets, DomAttribute, DomAttributeValue, RootDomContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LayoutSolverKind {
    Block,
    Inline,
    Flex,
}

#[derive(Clone, Debug)]
pub(super) struct LayoutContext {
    pub inherited_style: InheritedStyleContext,
    pub flow_left_px: f32,
    pub flow_top_px: f32,
    pub root_local_left_px: Option<f32>,
    pub root_local_top_px: Option<f32>,
    pub available_width_px: Option<f32>,
    pub available_height_px: Option<f32>,
    /// The width reference a flex item's block-level auto margins resolve
    /// against, carried by the flex placement site (flex arc step 6). It is the
    /// ITEM's own resolved content main size, so the block §10.3.3 free-space
    /// solver sees `item_content − item_border_box ≤ 0` and the deferred §8.1
    /// main-axis auto-margin admission stays at 0. `None` (every non-flex
    /// context) falls back to the general width basis in `enter_node`,
    /// preserving block-world behavior exactly.
    pub auto_margin_reference_width_px: Option<f32>,
    pub node_content_width_override_px: Option<f32>,
    pub node_content_height_override_px: Option<f32>,
    pub parent_absolute_left_px: f32,
    pub parent_absolute_top_px: f32,
    pub containing_layer_absolute_left_px: f32,
    pub containing_layer_absolute_top_px: f32,
    pub containing_layer_width_px: f32,
    pub containing_layer_height_px: f32,
    pub viewport_width_px: f32,
    pub viewport_height_px: f32,
    pub parent_box: Option<DomBoxModel>,
    pub allows_parent_child_block_margin_collapse: bool,
}

#[derive(Clone, Copy, Debug, Default)]
#[allow(clippy::struct_field_names)]
pub(super) struct FlowSize {
    pub width_px: f32,
    pub height_px: f32,
    pub min_width_px: f32,
    pub max_width_px: f32,
}

/// Shared entered-node record used by block/inline here and by the sibling
/// flex solver during the same walker visit.
#[derive(Clone, Debug)]
pub(super) struct EnteredNodeLayout {
    pub(super) style: NodeStyle,
    pub(super) inherited_style: InheritedStyleContext,
    pub(super) box_model: DomBoxModel,
    pub(super) is_text_node: bool,
    pub(super) allows_parent_child_block_margin_collapse: bool,
    pub(super) intrinsic_content_height_px: Option<f32>,
    pub(super) img_intrinsic_aspect_ratio: Option<f32>,
    pub(super) content_width_px: f32,
    pub(super) min_content_width_px: f32,
    pub(super) max_content_width_px: f32,
    pub(super) local_left_px: f32,
    pub(super) local_top_px: f32,
    pub(super) absolute_left_px: f32,
    pub(super) absolute_top_px: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct BlockFlowState {
    block_top_px: f32,
    inline_left_px: f32,
    line_height_px: f32,
    first_collapsed_parent_top_margin_px: f32,
    previous_block_margin_bottom_px: f32,
    previous_block_allows_parent_child_margin_collapse: bool,
    max_width_px: f32,
    line_min_width_px: f32,
    line_max_width_px: f32,
    min_width_px: f32,
    max_content_width_px: f32,
    has_children: bool,
    has_block_children: bool,
}

#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_field_names)]
pub(super) struct ResolvedContentSizePx {
    pub(super) width_px: f32,
    pub(super) viewport_height_px: f32,
    pub(super) layout_height_px: f32,
}

/// Shared transient inline line state reused by inline solving and by the
/// extracted flex solver while flex still follows the current inline-style
/// placeholder behavior.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct InlineFlowState {
    pub(super) inline_left_px: f32,
    pub(super) line_height_px: f32,
    pub(super) min_width_px: f32,
    pub(super) max_content_width_px: f32,
    pub(super) has_children: bool,
}

/// Lays out one DOM node and its normal-flow descendants within the shared
/// `TreeWalker`. This chooses the active flow solver from current DOM display
/// semantics, dispatches to block/inline solvers here or the sibling flex
/// solver, and writes final layout-facing state for later phases.
///
/// Example:
///
/// ```ignore
/// let flow_size =
///     relayout_tree_walker_node(root_dom, (200.0, 120.0), root_key, context, &mut resolver)?;
/// assert!(flow_size.width_px >= 0.0);
/// ```
pub(super) fn relayout_tree_walker_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    resolver: &mut R,
) -> Result<FlowSize, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let (style, inherited_style) = resolved_node_style(root_dom, key, &context)?;
    let style = style_with_node_content_size_overrides(style, &context);
    let context = context_without_node_content_size_overrides(context);

    if style.display == LayoutDisplay::None {
        let node = root_dom
            .nodes
            .get_mut(key)
            .ok_or(DomError::InvalidNodeKey)?;
        write_layout_state(
            node,
            style.layer_kind,
            context.available_width_px.unwrap_or(0.0),
            available_height_px_for_node(style, &context).unwrap_or(0.0),
            0.0,
            0.0,
            0.0,
        );
        write_node_box(node, DomBoxModel::zero());
        super::style::clear_layout_dirty(node);
        return Ok(FlowSize::default());
    }

    match solver_kind_for_display(style.display) {
        LayoutSolverKind::Block => relayout_block_node(
            root_dom,
            tree_size,
            key,
            context,
            style,
            inherited_style,
            resolver,
        ),
        LayoutSolverKind::Inline => relayout_inline_node(
            root_dom,
            tree_size,
            key,
            context,
            style,
            inherited_style,
            resolver,
        ),
        LayoutSolverKind::Flex => super::flex::relayout_flex_node(
            root_dom,
            tree_size,
            key,
            context,
            style,
            inherited_style,
            resolver,
        ),
    }
}

fn relayout_block_node<R>(
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
    let mut flow = BlockFlowState::default();
    let mut child_key = first_child_key(root_dom, key)?;
    while let Some(current_child_key) = child_key {
        let next_child_key = next_sibling_key(root_dom, current_child_key)?;
        let child_style = {
            let child_node = root_dom
                .nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            let (basis_width_px, basis_height_px) =
                node_style_basis(presented_position(child_node), &child_context);
            node_style(child_node, basis_width_px, basis_height_px)?
        };
        let child_participates_in_flow = node_participates_in_normal_flow(child_style);
        let collapsed_parent_top_margin_px =
            if child_participates_in_flow && !display_is_inline_outer(child_style.display) {
                collapsed_parent_top_margin_px(&child_context, flow, child_style)
            } else {
                0.0
            };
        let next_child_context = if child_participates_in_flow {
            next_block_child_context(
                child_context.clone(),
                flow,
                child_style.display,
                child_style,
            )
        } else {
            child_context.clone()
        };
        let child_flow = relayout_tree_walker_node(
            root_dom,
            tree_size,
            current_child_key,
            next_child_context,
            resolver,
        )?;
        if child_participates_in_flow {
            flow.has_children = true;
            update_block_flow(
                &mut flow,
                child_flow,
                child_style,
                collapsed_parent_top_margin_px,
            );
        }
        child_key = next_child_key;
    }
    let min_content_width_px = block_flow_min_content_width(&entered, flow);
    let max_content_width_px = block_flow_max_content_width(&entered, flow);
    let auto_content_width_px = auto_content_width_for_exit(
        &entered,
        available_width_px_for_node(entered.style, &context),
        flow.max_width_px,
        min_content_width_px,
        max_content_width_px,
    );
    let auto_content_height_px = resolve_block_content_height(&entered, flow);
    let propagated_margin_height_px =
        propagated_empty_intermediary_block_margin_height_px(&entered, flow);
    let mut flow_size = exit_node(
        root_dom,
        key,
        context,
        entered,
        auto_content_width_px,
        (min_content_width_px, max_content_width_px),
        auto_content_height_px,
    )?;
    flow_size.height_px += propagated_margin_height_px;
    Ok(flow_size)
}

fn relayout_inline_node<R>(
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
        let child_style = {
            let child_node = root_dom
                .nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            let (basis_width_px, basis_height_px) =
                node_style_basis(presented_position(child_node), &child_context);
            node_style(child_node, basis_width_px, basis_height_px)?
        };
        let child_participates_in_flow = node_participates_in_normal_flow(child_style);
        let next_child_context = if child_participates_in_flow {
            next_inline_child_context(child_context.clone(), flow)
        } else {
            child_context.clone()
        };
        let child_flow = relayout_tree_walker_node(
            root_dom,
            tree_size,
            current_child_key,
            next_child_context,
            resolver,
        )?;
        if child_participates_in_flow {
            flow.has_children = true;
            flow.inline_left_px += child_flow.width_px;
            flow.line_height_px = flow.line_height_px.max(child_flow.height_px);
            flow.min_width_px = flow.min_width_px.max(child_flow.min_width_px);
            flow.max_content_width_px += child_flow.max_width_px;
        }
        child_key = next_child_key;
    }
    let content_height_px = if flow.has_children {
        flow.line_height_px
    } else {
        entered.intrinsic_content_height_px.unwrap_or(0.0)
    };
    let min_content_width_px = inline_flow_min_content_width(&entered, flow);
    let max_content_width_px = inline_flow_max_content_width(&entered, flow);
    let auto_content_width_px = auto_content_width_for_exit(
        &entered,
        available_width_px_for_node(entered.style, &context),
        flow.inline_left_px,
        min_content_width_px,
        max_content_width_px,
    );
    exit_node(
        root_dom,
        key,
        context,
        entered,
        auto_content_width_px,
        (min_content_width_px, max_content_width_px),
        non_negative_auto_content_height(content_height_px),
    )
}

pub(super) fn enter_node<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    style: NodeStyle,
    inherited_style: InheritedStyleContext,
    resolver: &mut R,
) -> Result<EnteredNodeLayout, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let mut style = style;
    let (
        intrinsic_box,
        allows_parent_child_block_margin_collapse,
        is_text_node,
        intrinsic_content_height_px,
        img_intrinsic_aspect_ratio,
        min_content_width_px,
        max_content_width_px,
    ) = {
        let node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let measured_box = resolver.measure_box(DomLayoutInput {
            node_key: key,
            node,
            parent_box: context.parent_box,
            image_size_px: intrinsic_size_px_for_node(node, tree_size),
        })?;
        let (min_content_width_px, max_content_width_px) =
            node_content_width_contribution(node, measured_box);
        (
            measured_box,
            matches!(node, DomNode::Element(_)) && style.position == PositionMode::Static,
            node.as_text().is_some(),
            intrinsic_content_height_px(node, measured_box),
            img_intrinsic_aspect_ratio(node),
            min_content_width_px,
            max_content_width_px,
        )
    };
    let mut box_model = intrinsic_box;
    box_model.margin = style.sides.margin;
    box_model.border = style.sides.border;
    box_model.padding = style.sides.padding;
    let content_size_style = content_size_property_style(
        style,
        img_intrinsic_aspect_ratio,
        intrinsic_content_height_px,
    );
    let content_width_px = if is_text_node {
        resolve_node_content_width(
            content_size_style,
            available_width_px_for_node(style, &context),
            box_model,
            true,
        )
    } else if let Some(positioned_auto_width_px) = resolve_positioned_auto_content_width(
        content_size_style,
        &context,
        min_content_width_px,
        max_content_width_px,
    ) {
        positioned_auto_width_px
    } else {
        resolve_node_content_width(
            content_size_style,
            available_width_px_for_node(style, &context),
            box_model,
            false,
        )
    };
    let border_width_px = border_box_width(style.sides, content_width_px);
    // CSS 2.1 §10.3.3: resolve `auto` horizontal margins for a block-level
    // non-replaced box in normal flow before local-left placement. The
    // resolved margins write back into both `style.sides.margin` (placement)
    // and `box_model.margin` (the used box model).
    // The dedicated auto-margin reference (set at the flex placement site to
    // the item's own resolved content main size) keeps a flex item's main-axis
    // `margin: auto` at 0 — `free = item_content − item_border_box ≤ 0` — the
    // deferred §8.1 admission preserved exactly (flex arc step 6). A `None`
    // reference falls back to the general width basis, so block-world and
    // column-cross-axis behavior is unchanged.
    let cb_width_px = context
        .auto_margin_reference_width_px
        .or_else(|| available_width_px_for_node(style, &context));
    resolve_block_auto_margins(
        &mut style,
        cb_width_px,
        border_width_px,
        img_intrinsic_aspect_ratio.is_some(),
    );
    box_model.margin = style.sides.margin;
    let local_left_px = resolve_local_left(style, context.clone(), box_model, border_width_px);
    let local_top_px = resolve_local_top_without_height(style, context.clone(), box_model);
    let absolute_left_px = absolute_left_px_for_node(style, &context, local_left_px);
    let absolute_top_px = absolute_top_px_for_node(style, &context, local_top_px);
    Ok(EnteredNodeLayout {
        style,
        inherited_style,
        box_model,
        is_text_node,
        allows_parent_child_block_margin_collapse,
        intrinsic_content_height_px,
        img_intrinsic_aspect_ratio,
        content_width_px,
        min_content_width_px,
        max_content_width_px,
        local_left_px,
        local_top_px,
        absolute_left_px,
        absolute_top_px,
    })
}

pub(super) fn exit_node(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    context: LayoutContext,
    entered: EnteredNodeLayout,
    auto_content_width_px: f32,
    auto_content_width_contribution_px: (f32, f32),
    auto_content_height_px: f32,
) -> Result<FlowSize, DomError> {
    let (auto_min_content_width_px, auto_max_content_width_px) = auto_content_width_contribution_px;
    let resolved_auto_content_width_px =
        resolve_content_width_for_exit(&entered, auto_content_width_px);
    let auto_content_height_px = resolved_auto_content_height_px(
        root_dom,
        key,
        &entered,
        resolved_auto_content_width_px,
        auto_content_height_px,
    )?;
    let content_size_style = entered_content_size_property_style(&entered);
    let content_size = resolved_content_size_px(
        content_size_style,
        entered.img_intrinsic_aspect_ratio,
        positioned_auto_height_reference_px(entered.style, &context),
        resolved_auto_content_width_px,
        auto_content_height_px,
    );
    let width_px = border_box_width(entered.style.sides, content_size.width_px);
    let height_px = border_box_height(entered.style.sides, content_size.viewport_height_px);
    let local_top_px = resolve_local_top(
        entered.style,
        context.clone(),
        entered.local_top_px,
        height_px,
    );
    let box_model = DomBoxModel {
        border_box: DomRect {
            left_px: entered.local_left_px,
            top_px: local_top_px,
            width_px,
            height_px,
        },
        ..entered.box_model
    };
    let (min_content_width_px, max_content_width_px) = content_width_contribution_for_exit(
        &entered,
        content_size.width_px,
        auto_min_content_width_px,
        auto_max_content_width_px,
    );
    let flow_size = layout_flow_size(
        entered.style,
        box_model,
        min_content_width_px,
        max_content_width_px,
    );
    {
        let node = root_dom
            .nodes
            .get_mut(key)
            .ok_or(DomError::InvalidNodeKey)?;
        // The record write is a pass output, never a dirt source: the equality
        // gate's bool result is deliberately unused in step 1 (record writes
        // never mark style/layout/paint dirty). Record assembly is unified
        // through `compute_node_measurement` (one computation, one comparison;
        // pass outputs, never a dirt source).
        node_measurement_mut(node).replace_measurement_if_changed(compute_node_measurement(
            &entered,
            (min_content_width_px, max_content_width_px),
            content_size,
        ));
        write_layout_state(
            node,
            entered.style.layer_kind,
            available_width_px_for_node(entered.style, &context).unwrap_or(width_px),
            available_height_px_for_node(entered.style, &context).unwrap_or(height_px),
            width_px,
            height_px,
            content_size.layout_height_px,
        );
        write_node_box(node, box_model);
        super::style::clear_layout_dirty(node);
    }
    Ok(flow_size)
}

fn seed_separate_layout_root_box<R>(
    root_dom: &mut RootDom,
    tree_size: (f32, f32),
    key: DomNodeSlotKey,
    context: LayoutContext,
    resolver: &mut R,
) -> Result<(), DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let (style, inherited_style) = resolved_node_style(root_dom, key, &context)?;
    let entered = enter_node(
        root_dom,
        tree_size,
        key,
        context.clone(),
        style,
        inherited_style,
        resolver,
    )?;
    let auto_content_width_px = resolve_content_width_for_exit(&entered, entered.content_width_px);
    let auto_content_height_px =
        resolved_auto_content_height_px(root_dom, key, &entered, auto_content_width_px, 0.0)?;
    let content_size_style = entered_content_size_property_style(&entered);
    let content_size = resolved_content_size_px(
        content_size_style,
        entered.img_intrinsic_aspect_ratio,
        positioned_auto_height_reference_px(entered.style, &context),
        auto_content_width_px,
        auto_content_height_px,
    );
    let width_px = border_box_width(entered.style.sides, content_size.width_px);
    let height_px = border_box_height(entered.style.sides, content_size.viewport_height_px);
    let local_top_px = resolve_local_top(
        entered.style,
        context.clone(),
        entered.local_top_px,
        height_px,
    );
    let box_model = DomBoxModel {
        border_box: DomRect {
            left_px: entered.local_left_px,
            top_px: local_top_px,
            width_px,
            height_px,
        },
        ..entered.box_model
    };
    let node = root_dom
        .nodes
        .get_mut(key)
        .ok_or(DomError::InvalidNodeKey)?;
    write_seeded_available_layout_state(
        node,
        entered.style.layer_kind,
        available_width_px_for_node(entered.style, &context).unwrap_or(width_px),
        available_height_px_for_node(entered.style, &context).unwrap_or(height_px),
        width_px,
        height_px,
    );
    write_node_box(node, box_model);
    Ok(())
}

pub(super) fn first_child_key(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    Ok(root_dom
        .nodes
        .get(key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store())
}

pub(super) fn next_sibling_key(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    Ok(root_dom
        .nodes
        .get(key)
        .ok_or(DomError::InvalidNodeKey)?
        .next_sibling_key_for_store())
}

fn intrinsic_size_px_for_node(node: &DomNode, fallback_size: (f32, f32)) -> (f32, f32) {
    let border_box = super::style::node_box_from_ref(node).border_box;
    let width_px = border_box.width_px.max(0.0);
    let height_px = border_box.height_px.max(0.0);
    if width_px > 0.0 || height_px > 0.0 {
        return (width_px, height_px);
    }
    (fallback_size.0.max(0.0), fallback_size.1.max(0.0))
}

fn next_block_child_context(
    parent: LayoutContext,
    flow: BlockFlowState,
    child_display: LayoutDisplay,
    child_style: NodeStyle,
) -> LayoutContext {
    if display_is_inline_outer(child_display) {
        LayoutContext {
            flow_left_px: parent.flow_left_px + flow.inline_left_px,
            flow_top_px: parent.flow_top_px + flow.block_top_px,
            available_width_px: parent
                .available_width_px
                .map(|width| (width - flow.inline_left_px).max(0.0)),
            auto_margin_reference_width_px: None,
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            ..parent
        }
    } else {
        let collapsed_margin_overlap_px = collapsed_sibling_margin_overlap_px(flow, child_style);
        let collapsed_parent_top_margin_px =
            collapsed_parent_top_margin_px(&parent, flow, child_style);
        let next_block_top_px = flow.block_top_px + flow.line_height_px
            - collapsed_margin_overlap_px
            - collapsed_parent_top_margin_px;
        LayoutContext {
            flow_left_px: parent.flow_left_px,
            flow_top_px: parent.flow_top_px + next_block_top_px,
            available_width_px: parent.available_width_px,
            available_height_px: parent
                .available_height_px
                .map(|height| (height - next_block_top_px.max(0.0)).max(0.0)),
            auto_margin_reference_width_px: None,
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            ..parent
        }
    }
}

pub(super) fn next_inline_child_context(
    parent: LayoutContext,
    flow: InlineFlowState,
) -> LayoutContext {
    LayoutContext {
        flow_left_px: parent.flow_left_px + flow.inline_left_px,
        flow_top_px: parent.flow_top_px,
        available_width_px: parent
            .available_width_px
            .map(|width| (width - flow.inline_left_px).max(0.0)),
        auto_margin_reference_width_px: None,
        node_content_width_override_px: None,
        node_content_height_override_px: None,
        ..parent
    }
}

pub(super) fn child_layout_context(
    parent: LayoutContext,
    entered: EnteredNodeLayout,
) -> LayoutContext {
    let content_left_px = entered.local_left_px
        + entered.style.sides.border.left_px
        + entered.style.sides.padding.left_px;
    let content_top_px = entered.local_top_px
        + entered.style.sides.border.top_px
        + entered.style.sides.padding.top_px;
    let (
        containing_layer_absolute_left_px,
        containing_layer_absolute_top_px,
        containing_layer_width_px,
        containing_layer_height_px,
        root_local_left_px,
        root_local_top_px,
        flow_left_px,
        flow_top_px,
        parent_box,
    ) = if entered.style.layer_kind == DomLayerKind::None {
        (
            parent.containing_layer_absolute_left_px,
            parent.containing_layer_absolute_top_px,
            parent.containing_layer_width_px,
            parent.containing_layer_height_px,
            None,
            None,
            content_left_px,
            content_top_px,
            Some(DomBoxModel {
                border_box: DomRect {
                    left_px: entered.local_left_px,
                    top_px: entered.local_top_px,
                    width_px: border_box_width(entered.style.sides, entered.content_width_px),
                    height_px: entered.box_model.border_box.height_px,
                },
                ..entered.box_model
            }),
        )
    } else {
        let local_content_left_px =
            entered.style.sides.border.left_px + entered.style.sides.padding.left_px;
        let local_content_top_px =
            entered.style.sides.border.top_px + entered.style.sides.padding.top_px;
        (
            entered.absolute_left_px,
            entered.absolute_top_px,
            border_box_width(entered.style.sides, entered.content_width_px),
            child_context_layer_height_px(&entered),
            Some(entered.local_left_px),
            Some(entered.local_top_px),
            local_content_left_px,
            local_content_top_px,
            Some(DomBoxModel {
                border_box: DomRect {
                    left_px: 0.0,
                    top_px: 0.0,
                    width_px: border_box_width(entered.style.sides, entered.content_width_px),
                    height_px: entered.box_model.border_box.height_px,
                },
                ..entered.box_model
            }),
        )
    };
    let available_width_px = child_available_width_px(&entered);
    let available_height_px = child_available_height_px(&entered);
    LayoutContext {
        inherited_style: entered.inherited_style,
        flow_left_px,
        flow_top_px,
        root_local_left_px,
        root_local_top_px,
        available_width_px,
        available_height_px,
        auto_margin_reference_width_px: None,
        node_content_width_override_px: None,
        node_content_height_override_px: None,
        parent_absolute_left_px: entered.absolute_left_px,
        parent_absolute_top_px: entered.absolute_top_px,
        containing_layer_absolute_left_px,
        containing_layer_absolute_top_px,
        containing_layer_width_px,
        containing_layer_height_px,
        viewport_width_px: parent.viewport_width_px,
        viewport_height_px: parent.viewport_height_px,
        parent_box,
        allows_parent_child_block_margin_collapse: entered
            .allows_parent_child_block_margin_collapse,
    }
}

fn style_with_node_content_size_overrides(
    mut style: NodeStyle,
    context: &LayoutContext,
) -> NodeStyle {
    if let Some(width_px) = context.node_content_width_override_px {
        style.width_px = Some(assert_node_content_size_override_px(width_px, "width"));
    }
    if let Some(height_px) = context.node_content_height_override_px {
        style.height_px = Some(assert_node_content_size_override_px(height_px, "height"));
    }
    // The flex solver's size overrides are already CONTENT-box sizes (the
    // solver strips border+padding before re-measuring the item). Feeding one
    // through `content_box_size_px` under `BorderBox` would subtract the insets
    // a second time, so the override reinterprets the specified size as the
    // content box (CSS UI 4 §3.1's `content-box` mode, verbatim).
    if context.node_content_width_override_px.is_some()
        || context.node_content_height_override_px.is_some()
    {
        style.box_sizing = BoxSizingKw::ContentBox;
    }
    style
}

fn assert_node_content_size_override_px(value_px: f32, axis_name: &str) -> f32 {
    assert!(
        value_px.is_finite() && value_px >= 0.0,
        "node content {axis_name} override should be finite and non-negative",
    );
    value_px
}

fn context_without_node_content_size_overrides(mut context: LayoutContext) -> LayoutContext {
    context.node_content_width_override_px = None;
    context.node_content_height_override_px = None;
    context
}

fn child_available_width_px(entered: &EnteredNodeLayout) -> Option<f32> {
    if node_defers_child_width_to_intrinsic_inline_measurement(
        entered.style,
        entered.img_intrinsic_aspect_ratio,
        entered.intrinsic_content_height_px,
    ) {
        return None;
    }
    Some(entered.content_width_px.max(0.0))
}

fn child_available_height_px(entered: &EnteredNodeLayout) -> Option<f32> {
    let content_size_style = entered_content_size_property_style(entered);
    content_size_style.height_px.map(|height_px| {
        clamp_length(
            height_px.max(0.0),
            content_size_style.min_height_px,
            content_size_style.max_height_px,
        )
    })
}

// Gecko's nsLineLayout path and Blink's LineBreaker path both derive inline
// text sizes from intrinsic text runs before final inline placement. A
// non-replaced inline or auto-width inline-block parent therefore must not
// impose its provisional zero content width on descendants.
fn node_defers_child_width_to_intrinsic_inline_measurement(
    style: NodeStyle,
    img_intrinsic_aspect_ratio: Option<f32>,
    intrinsic_content_height_px: Option<f32>,
) -> bool {
    non_replaced_inline_size_properties_do_not_apply(
        style,
        img_intrinsic_aspect_ratio,
        intrinsic_content_height_px,
    ) || (style.display == LayoutDisplay::InlineBlock && style.width_px.is_none())
}

pub(super) fn entered_content_size_property_style(entered: &EnteredNodeLayout) -> NodeStyle {
    content_size_property_style(
        entered.style,
        entered.img_intrinsic_aspect_ratio,
        entered.intrinsic_content_height_px,
    )
}

fn content_size_property_style(
    style: NodeStyle,
    img_intrinsic_aspect_ratio: Option<f32>,
    intrinsic_content_height_px: Option<f32>,
) -> NodeStyle {
    if non_replaced_inline_size_properties_do_not_apply(
        style,
        img_intrinsic_aspect_ratio,
        intrinsic_content_height_px,
    ) {
        return NodeStyle {
            width_px: None,
            height_px: None,
            min_width_px: None,
            min_height_px: None,
            max_width_px: None,
            max_height_px: None,
            ..style
        };
    }
    style
}

fn non_replaced_inline_size_properties_do_not_apply(
    style: NodeStyle,
    img_intrinsic_aspect_ratio: Option<f32>,
    intrinsic_content_height_px: Option<f32>,
) -> bool {
    style.display == LayoutDisplay::Inline
        && img_intrinsic_aspect_ratio.is_none()
        && intrinsic_content_height_px.is_none()
}

fn update_block_flow(
    flow: &mut BlockFlowState,
    child_flow: FlowSize,
    child_style: NodeStyle,
    collapsed_parent_top_margin_px: f32,
) {
    let child_display = child_style.display;
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
    flow.block_top_px += child_flow.height_px
        - collapsed_sibling_margin_overlap_px(*flow, child_style)
        - collapsed_parent_top_margin_px;
    if !flow.has_block_children {
        flow.first_collapsed_parent_top_margin_px = collapsed_parent_top_margin_px;
    }
    flow.previous_block_margin_bottom_px = child_style.sides.margin.bottom_px;
    flow.previous_block_allows_parent_child_margin_collapse =
        child_style.position == PositionMode::Static;
    flow.max_width_px = flow.max_width_px.max(child_flow.width_px);
    flow.min_width_px = flow.min_width_px.max(child_flow.min_width_px);
    flow.max_content_width_px = flow.max_content_width_px.max(child_flow.max_width_px);
    flow.has_block_children = true;
}

fn collapsed_sibling_margin_overlap_px(flow: BlockFlowState, child_style: NodeStyle) -> f32 {
    // CSS 2.1 8.3.1 (collapsing margins): the used margin between adjoining
    // vertical margins is the larger of two positive margins, the more
    // negative of two negative margins, and the sum of a positive and a
    // negative margin; the overlap saved is then a + b − used. Gecko and
    // Blink run the same three-branch rule in nsBlockFrame.cpp /
    // block_layout_algorithm.cc. Verification: (−10, −5) → used = −10 →
    // overlap = −5 → gap = −10 ✓; (10, 5) → used = 10 → overlap = 5 ✓
    // unchanged; (10, −5) → used = 5 → overlap = 0 ✓ unchanged.
    if !flow.has_block_children || flow.line_height_px > 0.0 {
        return 0.0;
    }
    let a = flow.previous_block_margin_bottom_px;
    let b = child_style.sides.margin.top_px;
    let used_margin_px = if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a < 0.0 && b < 0.0 {
        a.min(b)
    } else {
        a + b
    };
    a + b - used_margin_px
}

fn collapsed_parent_top_margin_px(
    parent: &LayoutContext,
    flow: BlockFlowState,
    child_style: NodeStyle,
) -> f32 {
    if !parent_top_boundary_collapses_with_first_block_child(parent, flow, child_style) {
        return 0.0;
    }
    child_style.sides.margin.top_px.max(0.0)
}

fn parent_top_boundary_collapses_with_first_block_child(
    parent: &LayoutContext,
    flow: BlockFlowState,
    child_style: NodeStyle,
) -> bool {
    let Some(parent_box) = parent.parent_box else {
        return false;
    };
    if !flow_is_at_first_block_child(flow) || child_style.position != PositionMode::Static {
        return false;
    }
    if !parent.allows_parent_child_block_margin_collapse {
        return false;
    }
    parent_box.border.top_px <= 0.0 && parent_box.padding.top_px <= 0.0
}

fn flow_is_at_first_block_child(flow: BlockFlowState) -> bool {
    !flow.has_block_children
        && flow.line_height_px <= 0.0
        && flow.block_top_px <= 0.0
        && flow.inline_left_px <= 0.0
}

fn child_context_layer_height_px(entered: &EnteredNodeLayout) -> f32 {
    entered.style.height_px.unwrap_or_else(|| {
        entered
            .intrinsic_content_height_px
            .unwrap_or(intrinsic_content_height(entered.box_model))
    }) + entered.style.sides.padding.top_px
        + entered.style.sides.padding.bottom_px
        + entered.style.sides.border.top_px
        + entered.style.sides.border.bottom_px
}

fn resolve_block_content_height(entered: &EnteredNodeLayout, flow: BlockFlowState) -> f32 {
    let line_total_px =
        flow.block_top_px + flow.line_height_px - collapsed_parent_bottom_margin_px(entered, flow);
    if flow.has_children {
        return non_negative_auto_content_height(line_total_px);
    }
    entered.intrinsic_content_height_px.unwrap_or(0.0)
}

fn collapsed_parent_bottom_margin_px(entered: &EnteredNodeLayout, flow: BlockFlowState) -> f32 {
    if !entered.allows_parent_child_block_margin_collapse
        || !flow.has_block_children
        || !flow.previous_block_allows_parent_child_margin_collapse
        || flow.line_height_px > 0.0
    {
        return 0.0;
    }
    if entered.box_model.border.bottom_px > 0.0 || entered.box_model.padding.bottom_px > 0.0 {
        return 0.0;
    }
    flow.previous_block_margin_bottom_px.max(0.0)
}

fn propagated_empty_intermediary_block_margin_height_px(
    entered: &EnteredNodeLayout,
    flow: BlockFlowState,
) -> f32 {
    if !empty_intermediary_block_can_propagate_child_boundary_margins(entered, flow) {
        return 0.0;
    }
    propagated_collapsed_margin_delta_px(
        entered.box_model.margin.top_px,
        flow.first_collapsed_parent_top_margin_px,
    ) + propagated_collapsed_margin_delta_px(
        entered.box_model.margin.bottom_px,
        flow.previous_block_margin_bottom_px.max(0.0),
    )
}

fn propagated_collapsed_margin_delta_px(own_margin_px: f32, child_margin_px: f32) -> f32 {
    child_margin_px.max(own_margin_px.max(0.0)) - own_margin_px.max(0.0)
}

fn empty_intermediary_block_can_propagate_child_boundary_margins(
    entered: &EnteredNodeLayout,
    flow: BlockFlowState,
) -> bool {
    entered.allows_parent_child_block_margin_collapse
        && flow.has_block_children
        && flow.line_height_px <= 0.0
        && entered.box_model.border.top_px <= 0.0
        && entered.box_model.border.bottom_px <= 0.0
        && entered.box_model.padding.top_px <= 0.0
        && entered.box_model.padding.bottom_px <= 0.0
        && entered.style.height_px.is_none()
        // `min-height: auto` computes to 0 for a block (CSS Sizing 3), so the
        // resolver carries it as `Some(0.0)`; an empty intermediary still
        // collapses its margins unless a positive minimum forces it taller.
        && entered.style.min_height_px.is_none_or(|px| px <= 0.0)
}

fn block_flow_min_content_width(entered: &EnteredNodeLayout, flow: BlockFlowState) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

fn block_flow_max_content_width(entered: &EnteredNodeLayout, flow: BlockFlowState) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

fn inline_flow_min_content_width(entered: &EnteredNodeLayout, flow: InlineFlowState) -> f32 {
    if flow.has_children {
        return flow.min_width_px;
    }
    entered.min_content_width_px
}

fn inline_flow_max_content_width(entered: &EnteredNodeLayout, flow: InlineFlowState) -> f32 {
    if flow.has_children {
        return flow.max_content_width_px;
    }
    entered.max_content_width_px
}

pub(super) fn auto_content_width_for_exit(
    entered: &EnteredNodeLayout,
    available_width_px: Option<f32>,
    measured_content_width_px: f32,
    min_content_width_px: f32,
    max_content_width_px: f32,
) -> f32 {
    let content_size_style = entered_content_size_property_style(entered);
    if content_size_style.width_px.is_some() {
        return entered.content_width_px;
    }
    if matches!(
        entered.style.position,
        PositionMode::Absolute | PositionMode::Fixed
    ) {
        let available_content_width_px = available_width_px.map_or(max_content_width_px, |width| {
            (width
                - entered.style.left_px.unwrap_or(0.0)
                - entered.style.right_px.unwrap_or(0.0)
                - horizontal_non_content_px(entered.style.sides))
            .max(0.0)
        });
        if entered.style.left_px.is_some() && entered.style.right_px.is_some() {
            return clamp_length(
                available_content_width_px,
                content_size_style.min_width_px,
                content_size_style.max_width_px,
            );
        }
        if entered.style.left_px.is_some() || entered.style.right_px.is_some() {
            return clamp_length(
                shrink_to_fit_content_width(
                    min_content_width_px,
                    max_content_width_px,
                    available_content_width_px,
                ),
                content_size_style.min_width_px,
                content_size_style.max_width_px,
            );
        }
    }
    if !display_is_inline_outer(entered.style.display) {
        return entered.content_width_px;
    }
    if entered.style.display == LayoutDisplay::InlineBlock {
        let available_content_width_px = available_width_px.map_or(max_content_width_px, |width| {
            (width - horizontal_non_content_px(entered.style.sides)).max(0.0)
        });
        return shrink_to_fit_content_width(
            min_content_width_px,
            max_content_width_px,
            available_content_width_px,
        );
    }
    if measured_content_width_px <= 0.0 {
        return entered.content_width_px;
    }
    measured_content_width_px
}

fn shrink_to_fit_content_width(min_px: f32, max_px: f32, available_px: f32) -> f32 {
    // CSS 2.1 10.3.5/10.3.7 (shrink-to-fit width): the used width is
    // min(max(preferred minimum width, available width), preferred width).
    // Gecko applies the same three-way clamp in ReflowInput.cpp's
    // shrink-wrap path and Blink in its fit-content width computation.
    max_px.min(min_px.max(available_px))
}

fn resolve_content_width_for_exit(entered: &EnteredNodeLayout, auto_content_width_px: f32) -> f32 {
    let content_size_style = entered_content_size_property_style(entered);
    clamp_length(
        content_size_style
            .width_px
            .unwrap_or(auto_content_width_px.max(0.0)),
        content_size_style.min_width_px,
        content_size_style.max_width_px,
    )
}

pub(super) fn non_negative_auto_content_height(content_height_px: f32) -> f32 {
    content_height_px.max(0.0)
}

fn resolved_auto_content_height_px(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    entered: &EnteredNodeLayout,
    content_width_px: f32,
    fallback_content_height_px: f32,
) -> Result<f32, DomError> {
    if !entered.is_text_node {
        return Ok(non_negative_auto_content_height(
            fallback_content_height_px.max(0.0),
        ));
    }
    let node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
    let content_height_px =
        wrapped_text_content_height_px(node, &entered.inherited_style, content_width_px)
            .unwrap_or(fallback_content_height_px.max(0.0));
    Ok(non_negative_auto_content_height(content_height_px))
}

pub(super) fn wrapped_text_content_height_px(
    node: &DomNode,
    inherited_style: &InheritedStyleContext,
    content_width_px: f32,
) -> Option<f32> {
    let text_node = node.as_text()?;
    let font_size_px = inherited_style.effective_font_size_px();
    let mut request = TextLayoutRequest::for_text_with_white_space(
        text_node.data(),
        font_size_px,
        inherited_style.effective_white_space(),
    );
    request.word_break = inherited_style.effective_word_break();
    request.line_height_px = inherited_style.effective_line_height_px();
    request.wrap_mode = text_wrap_mode_for_css_layout(
        inherited_style.effective_white_space(),
        inherited_style.effective_overflow_wrap(),
        inherited_style.effective_word_break(),
    );
    let wrapped_text_height_px = with_repo_text_layout_context(|context| {
        context.measure_wrapped_text_height(&TextLayoutRequest {
            width_px: Some(content_width_px),
            ..request
        })
    });
    Some(wrapped_text_height_px)
}

pub(super) fn layout_flow_size(
    style: NodeStyle,
    box_model: DomBoxModel,
    min_content_width_px: f32,
    max_content_width_px: f32,
) -> FlowSize {
    if matches!(style.position, PositionMode::Absolute | PositionMode::Fixed) {
        return FlowSize::default();
    }
    let horizontal_edges_px = horizontal_non_content_px(style.sides);
    let flow_height_px = if empty_block_self_collapses_vertical_margins(style, box_model) {
        box_model.margin.top_px.max(box_model.margin.bottom_px)
    } else {
        box_model.border_box.height_px + box_model.margin.top_px + box_model.margin.bottom_px
    };
    FlowSize {
        width_px: box_model.border_box.width_px
            + box_model.margin.left_px
            + box_model.margin.right_px,
        height_px: flow_height_px,
        min_width_px: (min_content_width_px + horizontal_edges_px).max(0.0),
        max_width_px: (max_content_width_px + horizontal_edges_px).max(0.0),
    }
}

fn empty_block_self_collapses_vertical_margins(style: NodeStyle, box_model: DomBoxModel) -> bool {
    style.position == PositionMode::Static
        && !display_is_inline_outer(style.display)
        && box_model.border_box.height_px <= 0.0
        && box_model.border.top_px <= 0.0
        && box_model.border.bottom_px <= 0.0
        && box_model.padding.top_px <= 0.0
        && box_model.padding.bottom_px <= 0.0
}

fn separate_tree_flow_size(
    node: &DomNode,
    inherited_style: &InheritedStyleContext,
    image_size_px: (f32, f32),
) -> Result<FlowSize, DomError> {
    let node_inherited_style = inherited_context_from_node(node, inherited_style);
    let style = node_style(node, 0.0, None)?;
    if style.display == LayoutDisplay::None
        || matches!(style.position, PositionMode::Absolute | PositionMode::Fixed)
    {
        return Ok(FlowSize::default());
    }
    let intrinsic_box = intrinsic_box_for_node(node, image_size_px)?;
    let img_intrinsic_aspect_ratio = img_intrinsic_aspect_ratio(node);
    let intrinsic_element_content_height_px = intrinsic_content_height_px(node, intrinsic_box);
    let content_size_style = content_size_property_style(
        style,
        img_intrinsic_aspect_ratio,
        intrinsic_element_content_height_px,
    );
    let auto_content_width_px = intrinsic_content_width(intrinsic_box);
    let auto_content_height_px =
        wrapped_text_content_height_px(node, &node_inherited_style, auto_content_width_px)
            .unwrap_or_else(|| intrinsic_content_height(intrinsic_box));
    let content_size = resolved_content_size_px(
        content_size_style,
        img_intrinsic_aspect_ratio,
        None,
        auto_content_width_px,
        auto_content_height_px,
    );
    Ok(layout_flow_size(
        style,
        DomBoxModel {
            border_box: DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px: border_box_width(style.sides, content_size.width_px),
                height_px: border_box_height(style.sides, content_size.viewport_height_px),
            },
            margin: style.sides.margin,
            border: style.sides.border,
            padding: style.sides.padding,
        },
        content_size.width_px,
        content_size.width_px,
    ))
}

fn node_content_width_contribution(node: &DomNode, box_model: DomBoxModel) -> (f32, f32) {
    match node.intrinsic() {
        DomIntrinsic::Text(text) => (text.min_content_width_px, text.max_content_width_px),
        DomIntrinsic::Img(image) => (image.width_px, image.width_px),
        DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => {
            (intrinsic.width_px, intrinsic.width_px)
        }
        DomIntrinsic::None => match node {
            DomNode::Document(_) | DomNode::DocumentFragment(_) => {
                let content_width_px = intrinsic_content_width(box_model);
                (content_width_px, content_width_px)
            }
            DomNode::Element(_) => (0.0, 0.0),
            DomNode::Text(_) => unreachable!("text nodes should not have DomIntrinsic::None"),
        },
        DomIntrinsic::NotMeasured => {
            unreachable!("unmeasured intrinsic should be rejected before width contribution")
        }
    }
}

pub(super) fn content_width_contribution_for_exit(
    entered: &EnteredNodeLayout,
    resolved_content_width_px: f32,
    auto_min_content_width_px: f32,
    auto_max_content_width_px: f32,
) -> (f32, f32) {
    let content_size_style = entered_content_size_property_style(entered);
    if content_size_style.width_px.is_some() {
        return (resolved_content_width_px, resolved_content_width_px);
    }
    let min_content_width_px = if auto_min_content_width_px > 0.0 {
        auto_min_content_width_px
    } else {
        entered.min_content_width_px
    };
    let max_content_width_px = if auto_max_content_width_px > 0.0 {
        auto_max_content_width_px
    } else {
        entered.max_content_width_px
    };
    assert!(
        min_content_width_px <= max_content_width_px,
        "min-content width should not exceed max-content width"
    );
    (
        clamp_length(
            min_content_width_px,
            content_size_style.min_width_px,
            content_size_style.max_width_px,
        ),
        clamp_length(
            max_content_width_px,
            content_size_style.min_width_px,
            content_size_style.max_width_px,
        ),
    )
}

pub(super) fn resolved_node_style(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    context: &LayoutContext,
) -> Result<(NodeStyle, InheritedStyleContext), DomError> {
    let node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
    let node_inherited_style = inherited_context_from_node(node, &context.inherited_style);
    let (basis_width_px, basis_height_px) = node_style_basis(presented_position(node), context);
    let style = node_style(node, basis_width_px, basis_height_px)?;
    Ok((style, node_inherited_style))
}

/// Resolves a node's layout style + inherited context with a caller-supplied
/// basis. Measurement paths and separate-tree intrinsic measurement have no
/// containing block, so they pass `(0.0, None)`: the width basis stays a
/// degenerate definite 0.0 (percentages resolve to 0 — the measurement subset
/// is gated by the active-solver support boundaries anyway), while the height
/// basis is `None` (indefinite) so percentage heights fall back to `auto`
/// rather than collapsing to 0.
pub(super) fn resolved_node_style_with_basis(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
    inherited_style: &InheritedStyleContext,
    basis_width_px: f32,
    basis_height_px: Option<f32>,
) -> Result<(NodeStyle, InheritedStyleContext), DomError> {
    let node = root_dom.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
    let node_inherited_style = inherited_context_from_node(node, inherited_style);
    let style = node_style(node, basis_width_px, basis_height_px)?;
    Ok((style, node_inherited_style))
}

/// The containing-block width/height a node's percentage lengths resolve
/// against (CSS 2.1 §10.1, CSS Values §percentages): the containing block's
/// content box. In-flow boxes use the available width/height; absolute/fixed
/// boxes use their reference space (containing layer / viewport).
///
/// The width basis is a plain `f32` because the containing-block width is
/// always definite (CSS 2.1 §10.1: it propagates through the ancestor chain to
/// the viewport), so a missing reference/available width falls back to a
/// degenerate definite 0.0.
///
/// The height basis is `Option<f32>`: the containing-block height of an
/// in-flow box is indefinite when the containing block is `height: auto`
/// (CSS 2.1 §10.5), so `None` propagates "indefinite" to the height-axis
/// resolvers instead of manufacturing a 0.0 basis that would collapse
/// percentage heights to zero.
fn node_style_basis(position: PositionMode, context: &LayoutContext) -> (f32, Option<f32>) {
    (
        positioned_width_reference_px_for(position, context)
            .or(context.available_width_px)
            .unwrap_or(0.0),
        positioned_height_reference_px_for(position, context).or(context.available_height_px),
    )
}

fn img_intrinsic_aspect_ratio(node: &DomNode) -> Option<f32> {
    let DomNode::Element(element) = node else {
        return None;
    };
    if element.tag != DomTag::Img {
        return None;
    }
    let DomIntrinsic::Img(image) = node.intrinsic() else {
        return None;
    };
    Some(image.aspect_ratio)
}

fn intrinsic_content_height_px(node: &DomNode, intrinsic_box: DomBoxModel) -> Option<f32> {
    let element = node.as_element()?;
    if element.behavior().intrinsic_kind != DomElementIntrinsicKind::ElementDefaultPreferredSize {
        return None;
    }
    Some(intrinsic_content_height(intrinsic_box))
}

pub(super) fn resolved_content_size_px(
    style: NodeStyle,
    img_intrinsic_aspect_ratio: Option<f32>,
    positioned_auto_height_reference_px: Option<f32>,
    auto_content_width_px: f32,
    auto_content_height_px: f32,
) -> ResolvedContentSizePx {
    if let Some((content_width_px, content_height_px)) =
        img_ratio_content_size_px(style, img_intrinsic_aspect_ratio)
    {
        return ResolvedContentSizePx {
            width_px: content_width_px,
            viewport_height_px: content_height_px,
            layout_height_px: content_height_px,
        };
    }
    let content_width_px = clamp_length(
        content_box_size_px(
            style.width_px,
            style.box_sizing,
            horizontal_border_padding_px(style.sides),
        )
        .unwrap_or(auto_content_width_px.max(0.0)),
        style.min_width_px,
        style.max_width_px,
    );
    let auto_content_height_px = resolve_positioned_auto_content_height(
        style,
        positioned_auto_height_reference_px,
        auto_content_height_px,
    );
    let content_height_px = clamp_length(
        content_box_size_px(
            style.height_px,
            style.box_sizing,
            vertical_border_padding_px(style.sides),
        )
        .unwrap_or(auto_content_height_px),
        style.min_height_px,
        style.max_height_px,
    );
    ResolvedContentSizePx {
        width_px: content_width_px,
        viewport_height_px: content_height_px,
        layout_height_px: auto_content_height_px,
    }
}

fn img_ratio_content_size_px(
    style: NodeStyle,
    img_intrinsic_aspect_ratio: Option<f32>,
) -> Option<(f32, f32)> {
    let aspect_ratio = img_intrinsic_aspect_ratio?;
    match (style.width_px, style.height_px) {
        (Some(width_px), None) => {
            let content_width_px = clamp_length(
                content_box_size_px(
                    Some(width_px),
                    style.box_sizing,
                    horizontal_border_padding_px(style.sides),
                )
                .unwrap_or(width_px),
                style.min_width_px,
                style.max_width_px,
            );
            let content_height_px = clamp_length(
                content_width_px / aspect_ratio,
                style.min_height_px,
                style.max_height_px,
            );
            Some((content_width_px, content_height_px))
        }
        (None, Some(height_px)) => {
            let content_height_px = clamp_length(
                content_box_size_px(
                    Some(height_px),
                    style.box_sizing,
                    vertical_border_padding_px(style.sides),
                )
                .unwrap_or(height_px),
                style.min_height_px,
                style.max_height_px,
            );
            let content_width_px = clamp_length(
                content_height_px * aspect_ratio,
                style.min_width_px,
                style.max_width_px,
            );
            Some((content_width_px, content_height_px))
        }
        (None, None) | (Some(_), Some(_)) => None,
    }
}

fn resolve_positioned_auto_content_height(
    style: NodeStyle,
    positioned_auto_height_reference_px: Option<f32>,
    auto_content_height_px: f32,
) -> f32 {
    if style.height_px.is_some() || style.top_px.is_none() || style.bottom_px.is_none() {
        return auto_content_height_px;
    }
    let Some(available_height_px) = positioned_auto_height_reference_px else {
        return auto_content_height_px;
    };
    (available_height_px
        - style.top_px.unwrap_or(0.0)
        - style.bottom_px.unwrap_or(0.0)
        - vertical_non_content_px(style.sides))
    .max(0.0)
}

fn positioned_auto_height_reference_px(style: NodeStyle, context: &LayoutContext) -> Option<f32> {
    positioned_height_reference_px_for(style.position, context)
}

fn positioned_width_reference_px(style: NodeStyle, context: &LayoutContext) -> Option<f32> {
    positioned_width_reference_px_for(style.position, context)
}

fn positioned_height_reference_px_for(
    position: PositionMode,
    context: &LayoutContext,
) -> Option<f32> {
    match position {
        PositionMode::Absolute => Some(context.containing_layer_height_px),
        PositionMode::Fixed => Some(context.viewport_height_px),
        PositionMode::Static | PositionMode::Relative => None,
    }
}

fn positioned_width_reference_px_for(
    position: PositionMode,
    context: &LayoutContext,
) -> Option<f32> {
    match position {
        PositionMode::Absolute => Some(context.containing_layer_width_px),
        PositionMode::Fixed => Some(context.viewport_width_px),
        PositionMode::Static | PositionMode::Relative => None,
    }
}

fn available_width_px_for_node(style: NodeStyle, context: &LayoutContext) -> Option<f32> {
    positioned_width_reference_px(style, context).or(context.available_width_px)
}

fn available_height_px_for_node(style: NodeStyle, context: &LayoutContext) -> Option<f32> {
    positioned_auto_height_reference_px(style, context).or(context.available_height_px)
}

fn resolve_positioned_auto_content_width(
    style: NodeStyle,
    context: &LayoutContext,
    min_content_width_px: f32,
    max_content_width_px: f32,
) -> Option<f32> {
    if !matches!(style.position, PositionMode::Absolute | PositionMode::Fixed)
        || style.width_px.is_some()
    {
        return None;
    }
    let reference_width_px = positioned_width_reference_px(style, context)?;
    let available_content_width_px = (reference_width_px
        - style.left_px.unwrap_or(0.0)
        - style.right_px.unwrap_or(0.0)
        - horizontal_non_content_px(style.sides))
    .max(0.0);

    if style.left_px.is_some() && style.right_px.is_some() {
        return Some(clamp_length(
            available_content_width_px,
            style.min_width_px,
            style.max_width_px,
        ));
    }
    if style.left_px.is_some() || style.right_px.is_some() {
        return Some(clamp_length(
            shrink_to_fit_content_width(
                min_content_width_px,
                max_content_width_px,
                available_content_width_px,
            ),
            style.min_width_px,
            style.max_width_px,
        ));
    }
    None
}

pub(super) fn display_is_inline_outer(display: LayoutDisplay) -> bool {
    matches!(
        display,
        LayoutDisplay::Inline | LayoutDisplay::InlineBlock | LayoutDisplay::InlineFlex
    )
}

fn node_participates_in_normal_flow(style: NodeStyle) -> bool {
    style.display != LayoutDisplay::None
        && !matches!(style.position, PositionMode::Absolute | PositionMode::Fixed)
}

fn solver_kind_for_display(display: LayoutDisplay) -> LayoutSolverKind {
    match display {
        LayoutDisplay::Block | LayoutDisplay::InlineBlock => LayoutSolverKind::Block,
        LayoutDisplay::Inline => LayoutSolverKind::Inline,
        LayoutDisplay::Flex | LayoutDisplay::InlineFlex => LayoutSolverKind::Flex,
        LayoutDisplay::None => unreachable!("display none is handled before solver dispatch"),
    }
}

fn resolve_content_width(
    style: NodeStyle,
    available_width_px: Option<f32>,
    intrinsic_box: DomBoxModel,
) -> f32 {
    let intrinsic_width_px = intrinsic_content_width(intrinsic_box);
    let auto_width_px = if matches!(style.display, LayoutDisplay::Block | LayoutDisplay::Flex) {
        available_width_px.map_or(intrinsic_width_px, |available| {
            (available - horizontal_non_content_px(style.sides)).max(0.0)
        })
    } else {
        intrinsic_width_px
    };
    clamp_length(
        content_box_size_px(
            style.width_px,
            style.box_sizing,
            horizontal_border_padding_px(style.sides),
        )
        .unwrap_or(auto_width_px),
        style.min_width_px,
        style.max_width_px,
    )
}

fn resolve_node_content_width(
    style: NodeStyle,
    available_width_px: Option<f32>,
    intrinsic_box: DomBoxModel,
    is_text_node: bool,
) -> f32 {
    if !is_text_node {
        return resolve_content_width(style, available_width_px, intrinsic_box);
    }
    let intrinsic_width_px = intrinsic_content_width(intrinsic_box);
    let auto_width_px = available_width_px.unwrap_or(intrinsic_width_px);
    clamp_length(
        content_box_size_px(
            style.width_px,
            style.box_sizing,
            horizontal_border_padding_px(style.sides),
        )
        .unwrap_or(auto_width_px.max(0.0)),
        style.min_width_px,
        style.max_width_px,
    )
}

/// CSS 2.1 §10.3.3 (block-level, non-replaced boxes in normal flow): with a
/// non-auto width, an auto horizontal margin absorbs the free space between the
/// containing-block width and the used border-box width — two auto margins
/// split it equally, one absorbs it all, both floored at zero. Auto vertical
/// margins resolve to zero (their placeholder). When the width is `auto`, auto
/// margins compute to zero per the same section.
///
/// Flex-item auto margins are a DIFFERENT algorithm (Flexbox §8.1) and are
/// deferred for the MAIN axis (PR 1 item 7 scope boundary): a flex item's
/// main-axis `margin: auto` computes to 0 for now. The flex placement context
/// gives the item's own resolved content size as `available_width_px`, so the
/// main-axis free space here is `item_content − item_border_box ≤ 0` and this
/// solver naturally leaves the 0.0 placeholder — see
/// `flex_item_auto_margin_computes_to_zero_for_now`. The registered cross-axis
/// exception is a block-level item in a COLUMN container: it reaches this
/// solver through the flex placed-child context's Vertical branch (which leaves
/// `available_width_px` as the container's content width), so its cross-axis
/// auto margins DO absorb free space — accidental but §8.1-aligned, pinned by
/// `column_flex_block_item_cross_auto_margins_absorb_free_space`. Full §8.1
/// semantics (auto margins vs align-self, main-axis absorption) await the named
/// D6 admission.
fn resolve_block_auto_margins(
    style: &mut NodeStyle,
    cb_width_px: Option<f32>,
    border_box_width_px: f32,
    replaced: bool,
) {
    if replaced
        || style.display != LayoutDisplay::Block
        || !matches!(
            style.position,
            PositionMode::Static | PositionMode::Relative
        )
        || style.width_px.is_none()
    {
        return;
    }
    let Some(cb_width_px) = cb_width_px else {
        return;
    };
    // `margin_auto` is `[top, right, bottom, left]` (group array order).
    let left_auto = style.margin_auto[3];
    let right_auto = style.margin_auto[1];
    if !left_auto && !right_auto {
        return;
    }
    let free_px = (cb_width_px - border_box_width_px).max(0.0);
    if left_auto && right_auto {
        style.sides.margin.left_px = free_px / 2.0;
        style.sides.margin.right_px = free_px / 2.0;
    } else if left_auto {
        style.sides.margin.left_px = free_px;
    } else {
        style.sides.margin.right_px = free_px;
    }
}

// Resolves the node's local left position after combining normal-flow
// placement with any position-mode-specific horizontal offsets.
fn resolve_local_left(
    style: NodeStyle,
    context: LayoutContext,
    box_model: DomBoxModel,
    width_px: f32,
) -> f32 {
    if style.position == PositionMode::Static
        && context.parent_box.is_none()
        && let Some(root_left_px) = context.root_local_left_px
    {
        return root_left_px;
    }
    let flow_left_px =
        context.flow_left_px + box_model.border_box.left_px + style.sides.margin.left_px;
    match style.position {
        PositionMode::Static => flow_left_px,
        PositionMode::Relative => {
            flow_left_px + style.left_px.unwrap_or(0.0) - style.right_px.unwrap_or(0.0)
        }
        PositionMode::Absolute => {
            let absolute_left_px = positioned_axis_absolute_offset(
                context.containing_layer_absolute_left_px,
                context.containing_layer_width_px,
                width_px,
                style.left_px,
                style.right_px,
            );
            absolute_left_px - context.containing_layer_absolute_left_px
        }
        PositionMode::Fixed => {
            let absolute_left_px = positioned_axis_absolute_offset(
                0.0,
                context.viewport_width_px,
                width_px,
                style.left_px,
                style.right_px,
            );
            absolute_left_px - context.containing_layer_absolute_left_px
        }
    }
}

// Resolves the node's provisional local top position before any
// height-dependent bottom anchoring adjustment is possible.
fn resolve_local_top_without_height(
    style: NodeStyle,
    context: LayoutContext,
    box_model: DomBoxModel,
) -> f32 {
    if style.position == PositionMode::Static
        && context.parent_box.is_none()
        && let Some(root_top_px) = context.root_local_top_px
    {
        return root_top_px;
    }
    let flow_top_px = context.flow_top_px + box_model.border_box.top_px + style.sides.margin.top_px;
    match style.position {
        PositionMode::Static => flow_top_px,
        PositionMode::Relative => {
            flow_top_px + style.top_px.unwrap_or(0.0) - style.bottom_px.unwrap_or(0.0)
        }
        PositionMode::Absolute => {
            style.top_px.unwrap_or(0.0) + context.containing_layer_absolute_top_px
                - context.containing_layer_absolute_top_px
        }
        PositionMode::Fixed => {
            style.top_px.unwrap_or(0.0) - context.containing_layer_absolute_top_px
        }
    }
}

fn resolve_local_top(
    style: NodeStyle,
    context: LayoutContext,
    provisional_local_top_px: f32,
    height_px: f32,
) -> f32 {
    match style.position {
        PositionMode::Absolute => {
            if style.top_px.is_some() || style.bottom_px.is_none() {
                return provisional_local_top_px;
            }
            let absolute_top_px = positioned_axis_absolute_offset(
                context.containing_layer_absolute_top_px,
                context.containing_layer_height_px,
                height_px,
                style.top_px,
                style.bottom_px,
            );
            absolute_top_px - context.containing_layer_absolute_top_px
        }
        PositionMode::Fixed => {
            if style.top_px.is_some() || style.bottom_px.is_none() {
                return provisional_local_top_px;
            }
            let absolute_top_px = positioned_axis_absolute_offset(
                0.0,
                context.viewport_height_px,
                height_px,
                style.top_px,
                style.bottom_px,
            );
            absolute_top_px - context.containing_layer_absolute_top_px
        }
        PositionMode::Static | PositionMode::Relative => provisional_local_top_px,
    }
}

fn absolute_left_px_for_node(style: NodeStyle, context: &LayoutContext, local_left_px: f32) -> f32 {
    match style.position {
        PositionMode::Static | PositionMode::Relative => {
            context.parent_absolute_left_px + local_left_px
        }
        PositionMode::Absolute | PositionMode::Fixed => {
            context.containing_layer_absolute_left_px + local_left_px
        }
    }
}

fn absolute_top_px_for_node(style: NodeStyle, context: &LayoutContext, local_top_px: f32) -> f32 {
    match style.position {
        PositionMode::Static | PositionMode::Relative => {
            context.parent_absolute_top_px + local_top_px
        }
        PositionMode::Absolute | PositionMode::Fixed => {
            context.containing_layer_absolute_top_px + local_top_px
        }
    }
}

fn positioned_axis_absolute_offset(
    absolute_origin_px: f32,
    available_px: f32,
    size_px: f32,
    start_px: Option<f32>,
    end_px: Option<f32>,
) -> f32 {
    if let Some(start_px) = start_px {
        return absolute_origin_px + start_px;
    }
    if let Some(end_px) = end_px {
        return absolute_origin_px + (available_px - size_px - end_px);
    }
    absolute_origin_px
}

pub(super) fn border_box_width(sides: BoxSides, content_width_px: f32) -> f32 {
    (content_width_px
        + sides.padding.left_px
        + sides.padding.right_px
        + sides.border.left_px
        + sides.border.right_px)
        .max(0.0)
}

pub(super) fn border_box_height(sides: BoxSides, content_height_px: f32) -> f32 {
    (content_height_px
        + sides.padding.top_px
        + sides.padding.bottom_px
        + sides.border.top_px
        + sides.border.bottom_px)
        .max(0.0)
}

/// The single box-sizing projection (CSS UI 4 §3.1): maps a specified
/// `width`/`height` (the computed `Option<f32>`) to the content-box size for
/// one axis. `content-box` (the initial) passes the specified size through
/// unchanged — it IS the content box, and `border_box_width`/`border_box_height`
/// add padding+border outside it (today's math verbatim). `border-box` treats
/// the specified size as the border box and subtracts the per-axis
/// padding+border insets to reach the content box, floored at 0 (CSS never
/// produces negative content sizes). `None` (auto) has no specified size to
/// reinterpret, so it passes through on both modes.
pub(super) fn content_box_size_px(
    specified_px: Option<f32>,
    box_sizing: BoxSizingKw,
    insets_px: f32,
) -> Option<f32> {
    match box_sizing {
        BoxSizingKw::ContentBox => specified_px,
        BoxSizingKw::BorderBox => specified_px.map(|px| (px - insets_px).max(0.0)),
    }
}

/// Horizontal padding+border insets (no margin — box-sizing subtracts only
/// padding and border).
pub(super) fn horizontal_border_padding_px(sides: BoxSides) -> f32 {
    sides.border.left_px + sides.border.right_px + sides.padding.left_px + sides.padding.right_px
}

/// Vertical padding+border insets (no margin — box-sizing subtracts only
/// padding and border).
pub(super) fn vertical_border_padding_px(sides: BoxSides) -> f32 {
    sides.border.top_px + sides.border.bottom_px + sides.padding.top_px + sides.padding.bottom_px
}

pub(super) fn intrinsic_content_width(box_model: DomBoxModel) -> f32 {
    (box_model.border_box.width_px
        - box_model.border.left_px
        - box_model.border.right_px
        - box_model.padding.left_px
        - box_model.padding.right_px)
        .max(0.0)
}

fn intrinsic_content_height(box_model: DomBoxModel) -> f32 {
    (box_model.border_box.height_px
        - box_model.border.top_px
        - box_model.border.bottom_px
        - box_model.padding.top_px
        - box_model.padding.bottom_px)
        .max(0.0)
}

fn horizontal_non_content_px(sides: BoxSides) -> f32 {
    sides.margin.left_px
        + sides.margin.right_px
        + sides.border.left_px
        + sides.border.right_px
        + sides.padding.left_px
        + sides.padding.right_px
}

fn vertical_non_content_px(sides: BoxSides) -> f32 {
    sides.margin.top_px
        + sides.margin.bottom_px
        + sides.border.top_px
        + sides.border.bottom_px
        + sides.padding.top_px
        + sides.padding.bottom_px
}

pub(super) fn clamp_length(value: f32, min_px: Option<f32>, max_px: Option<f32>) -> f32 {
    let mut clamped = value;
    if let Some(min_px) = min_px {
        clamped = clamped.max(min_px);
    }
    if let Some(max_px) = max_px {
        clamped = clamped.min(max_px);
    }
    clamped
}

fn write_layout_state(
    node: &mut DomNode,
    layer_kind: DomLayerKind,
    available_width_px: f32,
    available_height_px: f32,
    width_px: f32,
    height_px: f32,
    content_height_px: f32,
) {
    let layout = node_layout_state_mut(node);
    layout.layer_kind = layer_kind;
    layout.available_width_px = available_width_px;
    layout.available_height_px = available_height_px;
    layout.width_px = width_px;
    layout.height_px = height_px;
    layout.content_height_px = content_height_px;
}

fn write_seeded_available_layout_state(
    node: &mut DomNode,
    layer_kind: DomLayerKind,
    available_width_px: f32,
    available_height_px: f32,
    width_px: f32,
    height_px: f32,
) {
    let layout = node_layout_state_mut(node);
    layout.layer_kind = layer_kind;
    layout.available_width_px = available_width_px;
    layout.available_height_px = available_height_px;
    // The parent can seed the viewport box for a separate layout root, but it
    // does not own subtree-derived used size or scroll content extent.
    layout.width_px = width_px;
    layout.height_px = height_px;
}

fn node_layout_state_mut(node: &mut DomNode) -> &mut crate::types::DomLayoutState {
    match node {
        DomNode::Document(document) => &mut document.base.layout,
        DomNode::DocumentFragment(fragment) => &mut fragment.base.layout,
        DomNode::Element(element) => &mut element.base.layout,
        DomNode::Text(text) => &mut text.base.layout,
    }
}

fn write_node_box(node: &mut DomNode, next_box: DomBoxModel) {
    match node {
        DomNode::Document(document) => document.base.box_model = next_box,
        DomNode::DocumentFragment(fragment) => fragment.base.box_model = next_box,
        DomNode::Element(element) => element.base.box_model = next_box,
        DomNode::Text(text) => text.base.box_model = next_box,
    }
}

/// Returns the node's retained measurement record slot for the exit-time write.
/// The write itself is equality-gated by the caller through
/// `replace_measurement_if_changed`; this helper only routes to the right
/// node-kind base, mirroring `write_node_box`.
fn node_measurement_mut(node: &mut DomNode) -> &mut MeasurementRecord {
    match node {
        DomNode::Document(document) => &mut document.base.measurement,
        DomNode::DocumentFragment(fragment) => &mut fragment.base.measurement,
        DomNode::Element(element) => &mut element.base.measurement,
        DomNode::Text(text) => &mut text.base.measurement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn div_node() -> DomNode {
        DomNode::element(test_owner_document_key(), DomTag::Div)
    }

    fn text_node(text: &str) -> DomNode {
        let mut node = DomNode::fixture_text(test_owner_document_key(), text);
        node.after_create_intrinsic_update()
            .expect("text fixture should measure intrinsic size");
        node
    }

    fn img_root_with_attrs(
        width: Option<&str>,
        height: Option<&str>,
    ) -> (RootDom, DomNodeSlotKey, DomNodeSlotKey) {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let img_key = root.create_element(DomTag::Img).expect("img should insert");
        root.append_child(document_key, img_key)
            .expect("append img");
        root.set_attr(
            img_key,
            DomAttribute::Src,
            Some(DomAttributeValue::String("memory://a.png".to_string())),
        )
        .expect("src should set");
        if let Some(width) = width {
            root.style_mut(img_key)
                .set("width", width)
                .expect("width should set");
        }
        if let Some(height) = height {
            root.style_mut(img_key)
                .set("height", height)
                .expect("height should set");
        }
        (root, document_key, img_key)
    }

    fn intrinsic_box(width_px: f32, height_px: f32) -> DomBoxModel {
        DomBoxModel {
            border_box: DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px,
                height_px,
            },
            margin: BoxEdgeInsets::default(),
            border: BoxEdgeInsets::default(),
            padding: BoxEdgeInsets::default(),
        }
    }

    fn node_with_groups<F: FnOnce(&mut ComputedGroups)>(update: F) -> DomNode {
        let mut node = div_node();
        match &mut node {
            DomNode::Element(element) => update(&mut element.base.presentation.computed),
            _ => unreachable!("flow tests feed element nodes"),
        }
        node
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn base_context() -> LayoutContext {
        LayoutContext {
            inherited_style: InheritedStyleContext::default(),
            flow_left_px: 0.0,
            flow_top_px: 0.0,
            root_local_left_px: None,
            root_local_top_px: None,
            available_width_px: Some(100.0),
            available_height_px: Some(80.0),
            auto_margin_reference_width_px: None,
            node_content_width_override_px: None,
            node_content_height_override_px: None,
            parent_absolute_left_px: 0.0,
            parent_absolute_top_px: 0.0,
            containing_layer_absolute_left_px: 10.0,
            containing_layer_absolute_top_px: 20.0,
            containing_layer_width_px: 100.0,
            containing_layer_height_px: 80.0,
            viewport_width_px: 300.0,
            viewport_height_px: 200.0,
            parent_box: None,
            allows_parent_child_block_margin_collapse: false,
        }
    }

    fn default_inherited_style() -> InheritedStyleContext {
        InheritedStyleContext::default()
    }

    fn resolved_node_style(node: &DomNode) -> NodeStyle {
        node_style(node, 0.0, None).expect("node style")
    }

    #[test]
    fn display_solver_mapping_stays_consistent() {
        assert_eq!(
            solver_kind_for_display(LayoutDisplay::Block),
            LayoutSolverKind::Block
        );
        assert_eq!(
            solver_kind_for_display(LayoutDisplay::InlineBlock),
            LayoutSolverKind::Block
        );
        assert_eq!(
            solver_kind_for_display(LayoutDisplay::Inline),
            LayoutSolverKind::Inline
        );
        assert_eq!(
            solver_kind_for_display(LayoutDisplay::Flex),
            LayoutSolverKind::Flex
        );
        assert!(display_is_inline_outer(LayoutDisplay::InlineFlex));
        assert!(!display_is_inline_outer(LayoutDisplay::Flex));
    }

    #[test]
    fn content_width_uses_available_width_for_block_and_intrinsic_for_inline() {
        let block = node_with_groups(|groups| {
            // `margin`/`padding` are [top, right, bottom, left].
            groups.box_.margin[3] = BoxLength::Px(6.0); // margin-left
            groups.box_.padding[1] = BoxLength::Px(4.0); // padding-right
        });
        let block_style = resolved_node_style(&block);
        let block_width =
            resolve_content_width(block_style, Some(100.0), intrinsic_box(40.0, 12.0));
        assert_close(block_width, 90.0);

        let inline = node_with_groups(|groups| {
            groups.box_.display = DisplayKw::Inline;
        });
        let inline_style = resolved_node_style(&inline);
        let inline_width =
            resolve_content_width(inline_style, Some(100.0), intrinsic_box(40.0, 12.0));
        assert_close(inline_width, 40.0);
    }

    #[test]
    fn text_node_content_width_uses_available_width_for_wrapped_text_layout() {
        let inline_style = resolved_node_style(&text_node("Wrapped text"));
        let text_width =
            resolve_node_content_width(inline_style, Some(100.0), intrinsic_box(40.0, 12.0), true);

        assert_close(text_width, 100.0);
    }

    #[test]
    fn non_replaced_inline_size_properties_do_not_constrain_children_or_content_box() {
        let inline = node_with_groups(|groups| {
            groups.box_.display = DisplayKw::Inline;
            groups.box_.width = BoxLength::Px(20.0);
            groups.box_.height = BoxLength::Px(40.0);
            groups.box_.min_width = BoxLength::Px(10.0);
            groups.box_.min_height = BoxLength::Px(12.0);
            groups.box_.max_width = BoxLength::Px(14.0);
            groups.box_.max_height = BoxLength::Px(16.0);
        });
        let inline_style = resolved_node_style(&inline);

        assert!(node_defers_child_width_to_intrinsic_inline_measurement(
            inline_style,
            None,
            None
        ));
        let applicable_inline_style = content_size_property_style(inline_style, None, None);
        assert_eq!(applicable_inline_style.width_px, None);
        assert_eq!(applicable_inline_style.height_px, None);
        assert_eq!(applicable_inline_style.min_width_px, None);
        assert_eq!(applicable_inline_style.min_height_px, None);
        assert_eq!(applicable_inline_style.max_width_px, None);
        assert_eq!(applicable_inline_style.max_height_px, None);

        let inline_block = node_with_groups(|groups| {
            groups.box_.display = DisplayKw::InlineBlock;
        });
        assert!(node_defers_child_width_to_intrinsic_inline_measurement(
            resolved_node_style(&inline_block),
            None,
            None
        ));

        let explicit_inline_block = node_with_groups(|groups| {
            groups.box_.display = DisplayKw::InlineBlock;
            groups.box_.width = BoxLength::Px(20.0);
        });
        assert!(!node_defers_child_width_to_intrinsic_inline_measurement(
            resolved_node_style(&explicit_inline_block),
            None,
            None
        ));

        let block = div_node();
        assert!(!node_defers_child_width_to_intrinsic_inline_measurement(
            resolved_node_style(&block),
            None,
            None
        ));
    }

    #[test]
    fn block_auto_width_subtracts_all_non_content_edges() {
        let block = node_with_groups(|groups| {
            groups.box_.margin[3] = BoxLength::Px(4.0); // margin-left
            groups.box_.margin[1] = BoxLength::Px(6.0); // margin-right
            groups.box_.padding[3] = BoxLength::Px(3.0); // padding-left
            groups.box_.padding[1] = BoxLength::Px(2.0); // padding-right
            groups.box_.border_width[3] = 1.0; // border-left
            groups.box_.border_width[1] = 5.0; // border-right
        });

        let content_width_px = resolve_content_width(
            resolved_node_style(&block),
            Some(100.0),
            intrinsic_box(40.0, 12.0),
        );

        assert_close(content_width_px, 79.0);
    }

    #[test]
    fn explicit_width_and_clamps_stay_consistent() {
        let node = node_with_groups(|groups| {
            groups.box_.width = BoxLength::Px(10.0);
            groups.box_.min_width = BoxLength::Px(20.0);
            groups.box_.max_width = BoxLength::Px(30.0);
        });

        let content_width_px = resolve_content_width(
            resolved_node_style(&node),
            Some(100.0),
            intrinsic_box(40.0, 12.0),
        );

        assert_close(content_width_px, 20.0);
        assert_close(clamp_length(50.0, Some(10.0), Some(30.0)), 30.0);
    }

    #[test]
    fn separate_tree_flow_size_skips_out_of_flow_and_display_none_nodes() {
        let absolute = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Absolute;
        });
        let absolute_flow =
            separate_tree_flow_size(&absolute, &default_inherited_style(), (20.0, 10.0))
                .expect("absolute flow");
        assert_close(absolute_flow.width_px, 0.0);
        assert_close(absolute_flow.height_px, 0.0);

        let hidden = node_with_groups(|groups| {
            groups.box_.display = DisplayKw::None;
        });
        let hidden_flow =
            separate_tree_flow_size(&hidden, &default_inherited_style(), (20.0, 10.0))
                .expect("hidden flow");
        assert_close(hidden_flow.width_px, 0.0);
        assert_close(hidden_flow.height_px, 0.0);
    }

    #[test]
    fn separate_tree_flow_size_keeps_in_flow_hidden_overflow_nodes() {
        let hidden = node_with_groups(|groups| {
            groups.box_.overflow = OverflowKw::Hidden;
            groups.box_.width = BoxLength::Px(24.0);
            groups.box_.height = BoxLength::Px(12.0);
        });
        let flow = separate_tree_flow_size(&hidden, &default_inherited_style(), (20.0, 10.0))
            .expect("hidden flow");
        assert_close(flow.width_px, 24.0);
        assert_close(flow.height_px, 12.0);
    }

    #[test]
    fn separate_tree_flow_size_keeps_relative_nodes_in_normal_flow() {
        let relative = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Relative;
            groups.box_.width = BoxLength::Px(10.0);
            groups.box_.height = BoxLength::Px(12.0);
            groups.box_.margin[3] = BoxLength::Px(2.0); // margin-left
            groups.box_.margin[1] = BoxLength::Px(3.0); // margin-right
            groups.box_.margin[0] = BoxLength::Px(4.0); // margin-top
            groups.box_.margin[2] = BoxLength::Px(5.0); // margin-bottom
        });

        let flow = separate_tree_flow_size(&relative, &default_inherited_style(), (20.0, 10.0))
            .expect("relative flow");
        assert_close(flow.width_px, 15.0);
        assert_close(flow.height_px, 21.0);
    }

    #[test]
    fn explicit_height_node_retains_child_content_height_for_scroll_bounds() {
        let mut root_dom = RootDom::new(RootDomContext::new_cpu());
        let scroll_root = root_dom.create_element(DomTag::Div).expect("scroll root");
        let first_item = root_dom.create_element(DomTag::Div).expect("first item");
        let second_item = root_dom.create_element(DomTag::Div).expect("second item");
        root_dom
            .append_child(root_dom.root, scroll_root)
            .expect("append scroll root");
        root_dom
            .append_child(scroll_root, first_item)
            .expect("append first item");
        root_dom
            .append_child(scroll_root, second_item)
            .expect("append second item");
        root_dom
            .style_mut(scroll_root)
            .set("width", "80px")
            .expect("scroll width");
        root_dom
            .style_mut(scroll_root)
            .set("height", "40px")
            .expect("scroll height");
        root_dom
            .style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root_dom
            .style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root_dom
            .style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root_dom
            .style_mut(second_item)
            .set("height", "40px")
            .expect("second height");
        let document_key = root_dom.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root_dom, document_key)
            .expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        relayout_tree_walker_node(
            &mut root_dom,
            (300.0, 200.0),
            scroll_root,
            base_context(),
            &mut resolver,
        )
        .expect("layout should succeed");

        let scroll_layout = root_dom
            .node(scroll_root)
            .expect("scroll root should exist")
            .layout();
        assert_close(scroll_layout.height_px, 40.0);
        assert_close(scroll_layout.content_height_px, 80.0);
    }

    #[test]
    fn positioned_axis_offset_supports_start_and_end_anchors() {
        assert_close(
            positioned_axis_absolute_offset(10.0, 100.0, 20.0, Some(6.0), None),
            16.0,
        );
        assert_close(
            positioned_axis_absolute_offset(10.0, 100.0, 20.0, None, Some(8.0)),
            82.0,
        );
        assert_close(
            positioned_axis_absolute_offset(10.0, 100.0, 20.0, None, None),
            10.0,
        );
    }

    #[test]
    fn absolute_and_fixed_offsets_use_their_expected_reference_spaces() {
        let absolute = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Absolute;
        });
        let fixed = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Fixed;
        });

        let absolute_left_px = resolve_local_left(
            resolved_node_style(&absolute),
            base_context(),
            intrinsic_box(0.0, 0.0),
            40.0,
        );
        let fixed_left_px = resolve_local_left(
            resolved_node_style(&fixed),
            base_context(),
            intrinsic_box(0.0, 0.0),
            40.0,
        );

        assert_close(absolute_left_px, 0.0);
        assert_close(fixed_left_px, -10.0);
    }

    #[test]
    fn nested_out_of_flow_layer_offsets_ignore_non_layer_parent_absolute_position() {
        let absolute = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Absolute;
        });
        let fixed = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Fixed;
        });
        let mut context = base_context();
        context.parent_absolute_left_px = 80.0;
        context.parent_absolute_top_px = 80.0;
        context.containing_layer_absolute_left_px = 0.0;
        context.containing_layer_absolute_top_px = 0.0;

        let absolute_left_px = resolve_local_left(
            resolved_node_style(&absolute),
            context.clone(),
            intrinsic_box(0.0, 0.0),
            40.0,
        );
        let fixed_left_px = resolve_local_left(
            resolved_node_style(&fixed),
            context.clone(),
            intrinsic_box(0.0, 0.0),
            40.0,
        );
        let absolute_top_px = resolve_local_top_without_height(
            resolved_node_style(&absolute),
            context.clone(),
            intrinsic_box(0.0, 0.0),
        );
        let fixed_top_px = resolve_local_top_without_height(
            resolved_node_style(&fixed),
            context,
            intrinsic_box(0.0, 0.0),
        );

        assert_close(absolute_left_px, 0.0);
        assert_close(fixed_left_px, 0.0);
        assert_close(absolute_top_px, 0.0);
        assert_close(fixed_top_px, 0.0);
    }

    #[test]
    fn right_and_bottom_offsets_pin_against_available_space() {
        let absolute = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Absolute;
            groups.box_.right = BoxLength::Px(12.0);
            groups.box_.bottom = BoxLength::Px(14.0);
        });

        let style = resolved_node_style(&absolute);
        let context = base_context();
        let provisional_top_px =
            resolve_local_top_without_height(style, context.clone(), intrinsic_box(0.0, 0.0));
        let local_left_px =
            resolve_local_left(style, context.clone(), intrinsic_box(0.0, 0.0), 40.0);
        let local_top_px = resolve_local_top(style, context, provisional_top_px, 30.0);

        assert_close(local_left_px, 48.0);
        assert_close(local_top_px, 36.0);
        assert_close(
            positioned_axis_absolute_offset(10.0, 100.0, 40.0, None, Some(12.0)),
            58.0,
        );
    }

    #[test]
    fn box_math_and_clamps_are_consistent() {
        let sides = BoxSides {
            margin: BoxEdgeInsets {
                left_px: 3.0,
                top_px: 4.0,
                right_px: 5.0,
                bottom_px: 6.0,
            },
            border: BoxEdgeInsets {
                left_px: 1.0,
                top_px: 2.0,
                right_px: 3.0,
                bottom_px: 4.0,
            },
            padding: BoxEdgeInsets {
                left_px: 7.0,
                top_px: 8.0,
                right_px: 9.0,
                bottom_px: 10.0,
            },
        };
        assert_close(border_box_width(sides, 20.0), 40.0);
        assert_close(border_box_height(sides, 12.0), 36.0);
        assert_close(clamp_length(20.0, Some(24.0), Some(30.0)), 24.0);
        assert_close(clamp_length(40.0, Some(24.0), Some(30.0)), 30.0);

        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.position = PositionMode::Static;
        let flow = layout_flow_size(
            style,
            DomBoxModel {
                border_box: DomRect {
                    left_px: 0.0,
                    top_px: 0.0,
                    width_px: 20.0,
                    height_px: 12.0,
                },
                margin: sides.margin,
                border: sides.border,
                padding: sides.padding,
            },
            20.0,
            20.0,
        );
        assert_close(flow.width_px, 28.0);
        assert_close(flow.height_px, 22.0);
    }

    #[test]
    fn layout_flow_size_ignores_absolute_and_fixed_nodes() {
        let absolute = node_with_groups(|groups| {
            groups.box_.position = PositionKw::Absolute;
            groups.box_.margin[3] = BoxLength::Px(5.0); // margin-left
        });

        let style = resolved_node_style(&absolute);
        let box_model = DomBoxModel {
            border_box: DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px: border_box_width(style.sides, 30.0),
                height_px: border_box_height(style.sides, 10.0),
            },
            margin: style.sides.margin,
            border: style.sides.border,
            padding: style.sides.padding,
        };

        assert_close(layout_flow_size(style, box_model, 30.0, 30.0).width_px, 0.0);
        assert_close(
            layout_flow_size(style, box_model, 30.0, 30.0).height_px,
            0.0,
        );
    }

    #[test]
    fn resolved_content_size_uses_img_ratio_when_only_width_is_authored() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.width_px = Some(320.0);
        let content_size = resolved_content_size_px(style, Some(160.0 / 90.0), None, 160.0, 90.0);

        assert_close(content_size.width_px, 320.0);
        assert_close(content_size.viewport_height_px, 180.0);
        assert_close(content_size.layout_height_px, 180.0);
    }

    #[test]
    fn resolved_content_size_uses_img_ratio_when_only_height_is_authored() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.height_px = Some(45.0);
        let content_size = resolved_content_size_px(style, Some(160.0 / 90.0), None, 160.0, 90.0);

        assert_close(content_size.width_px, 80.0);
        assert_close(content_size.viewport_height_px, 45.0);
        assert_close(content_size.layout_height_px, 45.0);
    }

    #[test]
    fn resolved_content_size_keeps_both_explicit_img_dimensions() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.width_px = Some(320.0);
        style.height_px = Some(200.0);
        let content_size = resolved_content_size_px(style, Some(160.0 / 90.0), None, 160.0, 90.0);

        assert_close(content_size.width_px, 320.0);
        assert_close(content_size.viewport_height_px, 200.0);
        assert_close(content_size.layout_height_px, 90.0);
    }

    #[test]
    fn resolved_content_size_keeps_auto_layout_height_for_non_ratio_sizing() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.width_px = Some(320.0);
        style.height_px = Some(200.0);
        let content_size = resolved_content_size_px(style, None, None, 160.0, 90.0);

        assert_close(content_size.width_px, 320.0);
        assert_close(content_size.viewport_height_px, 200.0);
        assert_close(content_size.layout_height_px, 90.0);
    }

    #[test]
    fn resolved_content_size_uses_fixed_top_bottom_anchors_for_auto_height() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.position = PositionMode::Fixed;
        style.top_px = Some(0.0);
        style.bottom_px = Some(0.0);

        let content_size = resolved_content_size_px(style, None, Some(120.0), 80.0, 0.0);

        assert_close(content_size.viewport_height_px, 120.0);
        assert_close(content_size.layout_height_px, 120.0);
    }

    #[test]
    fn resolved_content_size_subtracts_vertical_non_content_from_anchored_auto_height() {
        let mut style: NodeStyle = resolved_node_style(&div_node());
        style.position = PositionMode::Fixed;
        style.top_px = Some(0.0);
        style.bottom_px = Some(0.0);
        style.sides.margin.top_px = 5.0;
        style.sides.margin.bottom_px = 7.0;
        style.sides.border.top_px = 2.0;
        style.sides.border.bottom_px = 3.0;
        style.sides.padding.top_px = 11.0;
        style.sides.padding.bottom_px = 13.0;

        let content_size = resolved_content_size_px(style, None, Some(120.0), 80.0, 0.0);

        assert_close(content_size.viewport_height_px, 79.0);
        assert_close(content_size.layout_height_px, 79.0);
    }

    #[test]
    fn separate_tree_and_normal_layout_both_use_img_ratio_for_one_sided_sizes() {
        let (mut width_root, width_document_key, width_img_key) =
            img_root_with_attrs(Some("320px"), None);
        recompute_dirty_style_subtree_for_layer_root(&mut width_root, width_document_key)
            .expect("width style pass");
        let width_node = width_root
            .node(width_img_key)
            .expect("img should exist")
            .clone();
        let separate_width_flow =
            separate_tree_flow_size(&width_node, &default_inherited_style(), (256.0, 96.0))
                .expect("separate width flow");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(
            &mut width_root,
            width_document_key,
            (256.0, 96.0),
            &mut resolver,
        )
        .expect("normal width relayout");
        let normal_width_box = node_box(&width_root, width_img_key).expect("normal width box");
        assert_close(separate_width_flow.width_px, 320.0);
        assert_close(separate_width_flow.height_px, 180.0);
        assert_close(normal_width_box.border_box.width_px, 320.0);
        assert_close(normal_width_box.border_box.height_px, 180.0);

        let (mut height_root, height_document_key, height_img_key) =
            img_root_with_attrs(None, Some("45px"));
        recompute_dirty_style_subtree_for_layer_root(&mut height_root, height_document_key)
            .expect("height style pass");
        let height_node = height_root
            .node(height_img_key)
            .expect("img should exist")
            .clone();
        let separate_height_flow =
            separate_tree_flow_size(&height_node, &default_inherited_style(), (256.0, 96.0))
                .expect("separate height flow");
        let _ = relayout_tree(
            &mut height_root,
            height_document_key,
            (256.0, 96.0),
            &mut resolver,
        )
        .expect("normal height relayout");
        let normal_height_box = node_box(&height_root, height_img_key).expect("normal height box");
        assert_close(separate_height_flow.width_px, 80.0);
        assert_close(separate_height_flow.height_px, 45.0);
        assert_close(normal_height_box.border_box.width_px, 80.0);
        assert_close(normal_height_box.border_box.height_px, 45.0);
    }

    // --- block-world measurement record identity (flex step 1, T2) ---

    fn node_measurement(root: &RootDom, key: DomNodeSlotKey) -> MeasurementRecord {
        match root.node(key).expect("node should exist") {
            DomNode::Document(node) => node.base.measurement,
            DomNode::DocumentFragment(node) => node.base.measurement,
            DomNode::Element(node) => node.base.measurement,
            DomNode::Text(node) => node.base.measurement,
        }
    }

    fn assert_record_close(actual: MeasurementRecord, expected: MeasurementRecord) {
        assert_close(actual.h_min_content_px, expected.h_min_content_px);
        assert_close(actual.h_max_content_px, expected.h_max_content_px);
        assert_close(
            actual.content_height_at_width_px,
            expected.content_height_at_width_px,
        );
        assert_eq!(actual.v_min_content_px, expected.v_min_content_px);
        assert_eq!(actual.v_max_content_px, expected.v_max_content_px);
        assert_eq!(actual.flags, expected.flags);
    }

    fn text_intrinsic_widths(root: &RootDom, key: DomNodeSlotKey) -> (f32, f32) {
        let DomIntrinsic::Text(text_intrinsic) = root.node(key).expect("text node").intrinsic()
        else {
            panic!("text node should carry a measured text intrinsic");
        };
        (
            text_intrinsic.min_content_width_px,
            text_intrinsic.max_content_width_px,
        )
    }

    #[test]
    fn block_world_exit_writes_text_node_measurement_identity() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let container = root.create_element(DomTag::Div).expect("container");
        root.append_child(document, container)
            .expect("append container");
        let text_key = root
            .create_text_node("Wrapped text sample")
            .expect("text node");
        root.append_child(container, text_key).expect("append text");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let text_node = root.node(text_key).expect("text node");
        let (min_width_px, max_width_px) = text_intrinsic_widths(&root, text_key);

        assert_record_close(
            node_measurement(&root, text_key),
            MeasurementRecord {
                h_min_content_px: min_width_px,
                h_max_content_px: max_width_px,
                content_height_at_width_px: text_node.layout().content_height_px,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty(),
            },
        );
    }

    #[test]
    fn block_world_exit_writes_block_with_children_measurement_identity() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let container = root.create_element(DomTag::Div).expect("container");
        root.append_child(document, container)
            .expect("append container");
        let first = root.create_element(DomTag::Div).expect("first child");
        let second = root.create_element(DomTag::Div).expect("second child");
        root.append_child(container, first).expect("append first");
        root.append_child(container, second).expect("append second");
        root.style_mut(first)
            .set("width", "50px")
            .expect("first width");
        root.style_mut(first)
            .set("height", "20px")
            .expect("first height");
        root.style_mut(second)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second)
            .set("height", "30px")
            .expect("second height");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        // The block's min/max content contribution is the widest child (80px),
        // and its content height is the stacked child heights (20px + 30px).
        assert_record_close(
            node_measurement(&root, container),
            MeasurementRecord {
                h_min_content_px: 80.0,
                h_max_content_px: 80.0,
                content_height_at_width_px: 50.0,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty(),
            },
        );
    }

    #[test]
    fn block_world_exit_writes_inline_children_measurement_identity() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let container = root.create_element(DomTag::Div).expect("container");
        root.append_child(document, container)
            .expect("append container");
        let span = root.create_element(DomTag::Span).expect("span");
        root.append_child(container, span).expect("append span");
        let first = root.create_text_node("one two").expect("first text");
        let second = root.create_text_node("three four").expect("second text");
        root.append_child(span, first).expect("append first");
        root.append_child(span, second).expect("append second");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (300.0, 120.0), &mut resolver).expect("layout");

        let (first_min, first_max) = text_intrinsic_widths(&root, first);
        let (second_min, second_max) = text_intrinsic_widths(&root, second);
        let span_node = root.node(span).expect("span node");

        // Inline min-content is the widest child's min; max-content is the sum
        // of the children's max (they lay out on one line).
        assert_record_close(
            node_measurement(&root, span),
            MeasurementRecord {
                h_min_content_px: first_min.max(second_min),
                h_max_content_px: first_max + second_max,
                content_height_at_width_px: span_node.layout().content_height_px,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty(),
            },
        );
    }

    #[test]
    fn block_world_exit_writes_img_measurement_identity() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let img_key = root.create_element(DomTag::Img).expect("img");
        root.append_child(document, img_key).expect("append img");
        root.set_attr(
            img_key,
            DomAttribute::Src,
            Some(DomAttributeValue::String("memory://a.png".to_string())),
        )
        .expect("src should set");
        root.style_mut(img_key)
            .set("width", "320px")
            .expect("width");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        // A specified width substitutes the intrinsic width for both content
        // contributions, and the intrinsic ratio (160/90) derives the height.
        assert_record_close(
            node_measurement(&root, img_key),
            MeasurementRecord {
                h_min_content_px: 320.0,
                h_max_content_px: 320.0,
                content_height_at_width_px: 180.0,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty().with_replaced_ratio(),
            },
        );
    }

    #[test]
    fn block_world_exit_writes_element_control_measurement_identity() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let input_key = root.create_element(DomTag::Input).expect("input");
        root.append_child(document, input_key)
            .expect("append input");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (300.0, 120.0), &mut resolver).expect("layout");

        let input_node = root.node(input_key).expect("input node");
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = input_node.intrinsic() else {
            panic!("input should carry a preferred-size intrinsic");
        };

        assert_record_close(
            node_measurement(&root, input_key),
            MeasurementRecord {
                h_min_content_px: intrinsic.width_px,
                h_max_content_px: intrinsic.width_px,
                content_height_at_width_px: intrinsic.height_px,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty(),
            },
        );
    }

    #[test]
    fn block_world_exit_second_identical_update_keeps_record_equal() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let container = root.create_element(DomTag::Div).expect("container");
        root.append_child(document, container)
            .expect("append container");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(container, child).expect("append child");
        root.style_mut(child)
            .set("width", "60px")
            .expect("child width");
        root.style_mut(child)
            .set("height", "10px")
            .expect("child height");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver)
            .expect("first layout");
        let first = node_measurement(&root, container);

        // A second, identical walk must produce the identical record: the
        // equality gate rejects the re-write, so the retained value never
        // thrashes.
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver)
            .expect("second layout");
        let second = node_measurement(&root, container);

        assert_record_close(first, second);
        assert_record_close(
            second,
            MeasurementRecord {
                h_min_content_px: 60.0,
                h_max_content_px: 60.0,
                content_height_at_width_px: 10.0,
                v_min_content_px: None,
                v_max_content_px: None,
                flags: MeasurementFlags::empty(),
            },
        );
    }

    #[test]
    fn block_world_exit_sets_preserve_mode_flag_for_pre_white_space() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.root;
        let container = root.create_element(DomTag::Div).expect("container");
        root.append_child(document, container)
            .expect("append container");
        root.style_mut(container)
            .set("white-space", "pre")
            .expect("white-space");
        let text_key = root.create_text_node("two  spaces").expect("text node");
        root.append_child(container, text_key).expect("append text");

        recompute_dirty_style_subtree_for_layer_root(&mut root, document).expect("style pass");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (300.0, 120.0), &mut resolver).expect("layout");

        let record = node_measurement(&root, text_key);
        assert!(record.flags.preserve_mode());
        assert!(!record.flags.replaced_ratio());
    }
}
