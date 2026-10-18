#![allow(dead_code)]

//! Private DOM-style layout property interpretation for the shared `TreeWalker`.
//!
//! What this file contains:
//!
//! - DOM-themed display and position interpretation
//! - extraction of box-edge and size properties from the typed computed groups
//! - layer-kind derivation from current node state and computed groups
//! - layer-order lookup from internal node layout state
//! - node-owned dirty and paint state accessors used by layout and composite
//! - a transform translate helper read from the computed transform group
//!
//! Invariants and rules:
//!
//! - this file interprets computed group values but does not walk the tree
//! - this file does not solve layout; it only derives property-level answers
//! - every answer here must be derivable from the current node and its current
//!   computed groups (read visit-locally via `DomNode::computed_*`)
//! - this file must stay strict with supported DOM value meanings and fail
//!   closed through defaults rather than inventing new CSS-like behavior
//! - layer decisions here describe the node to later phases; they must not act
//!   as solver scratch or hidden traversal state
//!
//! Advice and code philosophy:
//!
//! - keep vocabulary DOM-themed: display, position, overflow, transform
//! - prefer direct derivation over storing extra state for later convenience
//! - keep the `*Kw` → solver-enum mapping local and unsurprising so solver code
//!   can trust the answers
//! - if a helper is only needed by one solver branch, keep it there instead of
//!   teaching `style.rs` a broader concept than the DOM actually exposes
//!
use crate::domains::style::computed::{BoxLength, BoxSizingKw, DisplayKw, PositionKw};
use crate::types::primitives::PointPx;
use crate::types::{
    BoxEdgeInsets, DomBoxModel, DomError, DomIntrinsic, DomLayerKind, DomNode, DomRect,
};

#[cfg(test)]
use crate::domains::style::computed::{ComputedGroups, OverflowKw};
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{DomNodeSlotKey, DomTag, RootDom, RootDomContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LayoutDisplay {
    None,
    Block,
    Inline,
    InlineBlock,
    Flex,
    InlineFlex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PositionMode {
    Static,
    Relative,
    Absolute,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct BoxSides {
    pub margin: BoxEdgeInsets,
    pub border: BoxEdgeInsets,
    pub padding: BoxEdgeInsets,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NodeStyle {
    pub display: LayoutDisplay,
    pub position: PositionMode,
    pub layer_kind: DomLayerKind,
    pub font_size_px: f32,
    pub sides: BoxSides,
    /// `[top, right, bottom, left]` — `true` marks an authored `auto` margin
    /// edge (CSS `margin-*: auto`), resolved at layout time per CSS 2.1
    /// §10.3.3. Group array order, matching `ComputedBoxGroup::margin`. The
    /// resolved `sides.margin` edge for an auto margin is a 0.0 placeholder
    /// that the block auto-margin solver overwrites.
    pub margin_auto: [bool; 4],
    /// CSS UI 4 §3.1: whether the specified `width`/`height` names the content
    /// box (`ContentBox`) or the border box (`BorderBox`). The sizing
    /// projection (`flow.rs`) reads this to subtract insets under `BorderBox`.
    pub box_sizing: BoxSizingKw,
    pub width_px: Option<f32>,
    pub height_px: Option<f32>,
    pub min_width_px: Option<f32>,
    pub min_height_px: Option<f32>,
    pub max_width_px: Option<f32>,
    pub max_height_px: Option<f32>,
    pub top_px: Option<f32>,
    pub left_px: Option<f32>,
    pub right_px: Option<f32>,
    pub bottom_px: Option<f32>,
}

/// Returns the node-owned box model currently stored on one DOM node. This
/// gives later layout and composite code one uniform `box_model` accessor while
/// keeping node-kind matching local to this file.
///
/// Example:
///
/// ```ignore
/// let border_box = node_box_from_ref(node).border_box;
/// assert!(border_box.width_px >= 0.0);
/// ```
pub(crate) fn node_box_from_ref(node: &DomNode) -> DomBoxModel {
    match node {
        DomNode::Document(document) => document.base.box_model,
        DomNode::DocumentFragment(fragment) => fragment.base.box_model,
        DomNode::Element(element) => element.base.box_model,
        DomNode::Text(text) => text.base.box_model,
    }
}

/// Returns whether one DOM node is still marked layout-dirty. This lets walker
/// and invalidation code read layout dirtiness without repeating per-node-kind
/// matching.
///
/// Example:
///
/// ```ignore
/// if layout_dirty_from_ref(node) {
///     // this subtree still needs a layout pass
/// }
/// ```
pub(crate) fn layout_dirty_from_ref(node: &DomNode) -> bool {
    match node {
        DomNode::Document(document) => document.base.layout.dirty,
        DomNode::DocumentFragment(fragment) => fragment.base.layout.dirty,
        DomNode::Element(element) => element.base.layout.dirty,
        DomNode::Text(text) => text.base.layout.dirty,
    }
}

/// Returns whether one DOM node is still marked paint-dirty. This gives
/// paint/composite scheduling one helper for stored paint dirtiness.
///
/// Example:
///
/// ```ignore
/// if paint_dirty_from_ref(node) {
///     // this node still needs paint work
/// }
/// ```
pub(crate) fn paint_dirty_from_ref(node: &DomNode) -> bool {
    !matches!(
        node.paint_repaint_state(),
        crate::types::DomPaintState::None
    )
}

/// Clears all stored layout dirty flags on one DOM node. This marks the node as
/// layout-clean after the current walker pass completes.
///
/// Example:
///
/// ```ignore
/// clear_layout_dirty(node);
/// assert!(!layout_dirty_from_ref(node));
/// ```
pub(crate) fn clear_layout_dirty(node: &mut DomNode) {
    match node {
        DomNode::Document(document) => document.base.layout.dirty = false,
        DomNode::DocumentFragment(fragment) => fragment.base.layout.dirty = false,
        DomNode::Element(element) => element.base.layout.dirty = false,
        DomNode::Text(text) => text.base.layout.dirty = false,
    }
}

/// Derives the current layer kind for one node from DOM-style properties. This
/// describes the node to later phases such as composite and paint without
/// caching solver-only decisions.
///
/// Example:
///
/// ```ignore
/// let layer_kind = layer_kind_for_node(node);
/// if layer_kind != DomLayerKind::None {
///     // this node participates as a layer
/// }
/// ```
pub(crate) fn layer_kind_for_node(node: &DomNode) -> DomLayerKind {
    let box_ = node.computed_box();
    if box_.display == DisplayKw::None {
        return DomLayerKind::None;
    }
    if box_.overflow.creates_clip() || box_.isolate {
        return DomLayerKind::Clipped;
    }
    if node.parent_key().is_none() || node_has_unclipped_layer(node) {
        return DomLayerKind::Normal;
    }
    DomLayerKind::None
}

/// Returns whether the node's layer (if any) should allow local size
/// updates from layout. This describes whether the node's layer can be
/// resized by layout changes or whether it should be treated as a
/// fixed-size paint target. This is currently true for positioned nodes
/// and nodes with overflow-based clipping.
///
/// Currently, any non-`visible` `overflow` value is treated as creating a
/// clipped layer, which allows local size updates.
///
pub(crate) fn layer_allows_local_size_update(node: &DomNode) -> bool {
    let box_ = node.computed_box();
    matches!(box_.position, PositionKw::Absolute | PositionKw::Fixed)
        || box_.overflow.creates_clip()
}

/// Derives the current layout-facing property view for one node. This gives
/// solver code one strict, DOM-themed bundle of the properties it actually
/// needs for layout decisions.
///
/// Example:
///
/// ```ignore
/// let style = node_style(node);
/// assert_eq!(style.position, PositionMode::Static);
/// ```
#[allow(clippy::unnecessary_wraps)]
pub(super) fn node_style(
    node: &DomNode,
    basis_width_px: f32,
    basis_height_px: f32,
) -> Result<NodeStyle, DomError> {
    let box_ = node.computed_box();
    let font = node.computed_font();
    let (margin_edges, margin_auto) = resolve_margin_edges(box_.margin, basis_width_px);
    let padding_edges = resolve_padding_edges(box_.padding, basis_width_px);
    Ok(NodeStyle {
        display: presented_display(node),
        position: presented_position(node),
        layer_kind: layer_kind_for_node(node),
        font_size_px: font.size_px,
        sides: BoxSides {
            margin: box_edge_insets_from_array(margin_edges),
            border: box_edge_insets_from_array(box_.border_width),
            padding: box_edge_insets_from_array(padding_edges),
        },
        margin_auto,
        box_sizing: box_.box_sizing,
        width_px: box_.width.resolve(basis_width_px),
        height_px: box_.height.resolve(basis_height_px),
        min_width_px: box_.min_width.resolve(basis_width_px),
        min_height_px: box_.min_height.resolve(basis_height_px),
        max_width_px: box_.max_width.resolve(basis_width_px),
        max_height_px: box_.max_height.resolve(basis_height_px),
        top_px: box_.top.resolve(basis_height_px),
        left_px: box_.left.resolve(basis_width_px),
        right_px: box_.right.resolve(basis_width_px),
        bottom_px: box_.bottom.resolve(basis_height_px),
    })
}

/// Resolves the four `[top, right, bottom, left]` margin `BoxLength`s against
/// the containing-block WIDTH (CSS Values §percentages: margin percentages
/// resolve against the containing block width, even vertically). Returns the
/// resolved px edges (auto edges are a 0.0 placeholder) and the parallel auto
/// flags in the same `[top, right, bottom, left]` order.
fn resolve_margin_edges(edges: [BoxLength; 4], basis_width_px: f32) -> ([f32; 4], [bool; 4]) {
    let mut resolved = [0.0; 4];
    let mut auto = [false; 4];
    for (index, edge) in edges.into_iter().enumerate() {
        match edge {
            BoxLength::Auto => auto[index] = true,
            BoxLength::Px(px) => resolved[index] = px,
            BoxLength::Percent(pct) => resolved[index] = basis_width_px * pct / 100.0,
        }
    }
    (resolved, auto)
}

/// Resolves the four padding `BoxLength`s against the containing-block WIDTH
/// (CSS Values §percentages, even vertically). `auto` never arrives (the
/// grammar rejects `padding: auto`), but maps to the absent 0.0 defensively.
fn resolve_padding_edges(edges: [BoxLength; 4], basis_width_px: f32) -> [f32; 4] {
    edges.map(|edge| edge.resolve_or(basis_width_px, 0.0))
}

/// Returns the intrinsic or previously-measured box for one node. This
/// preserves any nonzero stored box and otherwise seeds layout from the matched
/// paint target size.
///
/// Example:
///
/// ```ignore
/// let intrinsic = intrinsic_box_for_node(node, (200, 120));
/// assert!(intrinsic.border_box.width_px >= 0.0);
/// ```
pub(super) fn intrinsic_box_for_node(
    node: &DomNode,
    fallback_size_px: (f32, f32),
) -> Result<DomBoxModel, DomError> {
    let existing = node_box_from_ref(node);
    if matches!(node.intrinsic(), DomIntrinsic::None)
        && (existing.border_box.width_px > 0.0 || existing.border_box.height_px > 0.0)
    {
        return Ok(DomBoxModel {
            border_box: DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px: existing.border_box.width_px,
                height_px: existing.border_box.height_px,
            },
            ..existing
        });
    }
    let border_box = match node.intrinsic() {
        DomIntrinsic::None => intrinsic_border_box_for_non_intrinsic_node(node, fallback_size_px),
        DomIntrinsic::NotMeasured => return Err(DomError::UnmeasuredIntrinsic),
        DomIntrinsic::Text(text) => DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: text.max_content_width_px,
            height_px: text.line_height_px,
        },
        DomIntrinsic::Img(image) => DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: image.width_px,
            height_px: image.height_px,
        },
        DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => {
            return Ok(DomBoxModel {
                border_box: DomRect {
                    left_px: 0.0,
                    top_px: 0.0,
                    width_px: intrinsic.width_px,
                    height_px: intrinsic.height_px,
                },
                ..DomBoxModel::zero()
            });
        }
    };
    Ok(DomBoxModel {
        border_box: DomRect {
            left_px: border_box.left_px,
            top_px: border_box.top_px,
            width_px: border_box.width_px,
            height_px: border_box.height_px,
        },
        ..existing
    })
}

fn intrinsic_border_box_for_non_intrinsic_node(
    node: &DomNode,
    fallback_size_px: (f32, f32),
) -> DomRect {
    match node {
        DomNode::Document(_) | DomNode::DocumentFragment(_) => DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: fallback_size_px.0,
            height_px: fallback_size_px.1,
        },
        DomNode::Element(_) => DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: 0.0,
            height_px: 0.0,
        },
        DomNode::Text(_) => unreachable!("text nodes should not have DomIntrinsic::None"),
    }
}

/// Maps the computed `display` keyword onto the solver's `LayoutDisplay`
/// vocabulary. The variant sets are identical (`display_kw.rs` boundary note).
fn display_from_kw(kw: DisplayKw) -> LayoutDisplay {
    match kw {
        DisplayKw::None => LayoutDisplay::None,
        DisplayKw::Block => LayoutDisplay::Block,
        DisplayKw::Inline => LayoutDisplay::Inline,
        DisplayKw::InlineBlock => LayoutDisplay::InlineBlock,
        DisplayKw::Flex => LayoutDisplay::Flex,
        DisplayKw::InlineFlex => LayoutDisplay::InlineFlex,
    }
}

/// Maps the computed `position` keyword onto the solver's `PositionMode`
/// vocabulary. The variant sets are identical (`position_kw.rs` boundary note).
fn position_from_kw(kw: PositionKw) -> PositionMode {
    match kw {
        PositionKw::Static => PositionMode::Static,
        PositionKw::Relative => PositionMode::Relative,
        PositionKw::Absolute => PositionMode::Absolute,
        PositionKw::Fixed => PositionMode::Fixed,
    }
}

/// Reorders the box group's `[top, right, bottom, left]` array into the
/// `BoxEdgeInsets` field order (`top_px`, `right_px`, `bottom_px`, `left_px`).
fn box_edge_insets_from_array([top, right, bottom, left]: [f32; 4]) -> BoxEdgeInsets {
    BoxEdgeInsets {
        left_px: left,
        top_px: top,
        right_px: right,
        bottom_px: bottom,
    }
}

/// Returns the current DOM display meaning for one node. This normalizes
/// text-node behavior (always `Inline`) and maps the computed box group's
/// `DisplayKw` onto the solver's `LayoutDisplay`.
///
/// Example:
///
/// ```ignore
/// assert_eq!(presented_display(text_node), LayoutDisplay::Inline);
/// ```
pub(super) fn presented_display(node: &DomNode) -> LayoutDisplay {
    if matches!(node, DomNode::Text(_)) {
        return LayoutDisplay::Inline;
    }
    display_from_kw(node.computed_box().display)
}

/// Returns the current DOM position meaning for one node. This maps the
/// computed box group's `PositionKw` onto the solver's `PositionMode`.
///
/// Example:
///
/// ```ignore
/// let position = presented_position(node);
/// assert!(matches!(position, PositionMode::Static | PositionMode::Relative));
/// ```
pub(super) fn presented_position(node: &DomNode) -> PositionMode {
    position_from_kw(node.computed_box().position)
}

fn node_has_unclipped_layer(node: &DomNode) -> bool {
    if matches!(
        node.computed_box().position,
        PositionKw::Relative | PositionKw::Absolute | PositionKw::Fixed
    ) {
        return true;
    }
    node.computed_transform().has_transform
}

/// Returns the local translate offset encoded in the node's computed transform
/// group. This gives composite and hit testing one strict, derived translate
/// answer.
///
/// Example:
///
/// ```ignore
/// let offset = transform_translate_for_node(node);
/// assert_eq!(offset.x_px, 8.0);
/// ```
pub(super) fn transform_translate_for_node(node: &DomNode) -> PointPx {
    let [x_px, y_px] = node.computed_transform().translate_px;
    PointPx { x_px, y_px }
}

pub(crate) fn parse_translate(transform: &str) -> PointPx {
    if let Some(value) = translate_scalar(transform, "translateX(") {
        return PointPx {
            x_px: value,
            y_px: 0.0,
        };
    }
    if let Some(value) = translate_scalar(transform, "translateY(") {
        return PointPx {
            x_px: 0.0,
            y_px: value,
        };
    }
    let Some(inner) = strip_function_call(transform, "translate(") else {
        return PointPx::default();
    };
    let mut values = inner.split(',');
    PointPx {
        x_px: parse_px(values.next()),
        y_px: parse_px(values.next()),
    }
}

fn translate_scalar(transform: &str, function_name: &str) -> Option<f32> {
    Some(parse_px(Some(strip_function_call(
        transform,
        function_name,
    )?)))
}

fn strip_function_call<'a>(value: &'a str, function_name: &str) -> Option<&'a str> {
    let trimmed = value.trim();
    let inner = trimmed.strip_prefix(function_name)?;
    inner.strip_suffix(')')
}

fn parse_px(value: Option<&str>) -> f32 {
    let Some(value) = value else {
        return 0.0;
    };
    let trimmed = value.trim();
    let number = trimmed.strip_suffix("px").unwrap_or(trimmed);
    number.parse::<f32>().unwrap_or(0.0)
}

/// Feeds a detached element node's computed groups directly (spine 3.3g-2a):
/// inline tests write typed group values onto the node record, and the layout
/// readers (`node_style`/`presented_display`/`layer_kind_for_node`) read them
/// back visit-locally. There is no attr-projection channel anymore — the old
/// `project_detached_node_for_test` scaffold died with the stored-attr channel.
#[cfg(test)]
fn with_computed_groups<F: FnOnce(&mut ComputedGroups)>(mut node: DomNode, update: F) -> DomNode {
    match &mut node {
        DomNode::Element(element) => update(&mut element.base.presentation.computed),
        _ => unreachable!("layout style tests feed element nodes"),
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn div_node() -> DomNode {
        DomNode::element(test_owner_document_key(), DomTag::Div)
    }

    fn attached_div(root: &mut RootDom) -> DomNodeSlotKey {
        let key = root
            .create_element(DomTag::Div)
            .expect("element should insert");
        root.append_child(root.root, key)
            .expect("element should attach");
        key
    }

    fn node_style_of(node: &DomNode) -> super::NodeStyle {
        node_style(node, 0.0, 0.0).expect("style")
    }

    #[test]
    fn presented_display_uses_text_and_supported_display_values() {
        let text = DomNode::fixture_text(test_owner_document_key(), "hello");
        assert_eq!(presented_display(&text), LayoutDisplay::Inline);

        let flex = with_computed_groups(div_node(), |groups| {
            groups.box_.display = DisplayKw::Flex;
        });
        assert_eq!(presented_display(&flex), LayoutDisplay::Flex);

        let inline_flex = with_computed_groups(div_node(), |groups| {
            groups.box_.display = DisplayKw::InlineFlex;
        });
        assert_eq!(presented_display(&inline_flex), LayoutDisplay::InlineFlex);
    }

    #[test]
    fn presented_position_maps_supported_position_values() {
        let relative = with_computed_groups(div_node(), |groups| {
            groups.box_.position = PositionKw::Relative;
        });
        assert_eq!(presented_position(&relative), PositionMode::Relative);

        let absolute = with_computed_groups(div_node(), |groups| {
            groups.box_.position = PositionKw::Absolute;
        });
        assert_eq!(presented_position(&absolute), PositionMode::Absolute);
    }

    #[test]
    fn node_style_reads_pixel_edges_and_sizes() {
        let node = with_computed_groups(div_node(), |groups| {
            // `margin`/`padding`/`border_width` are [top, right, bottom, left].
            groups.box_.margin[3] = BoxLength::Px(6.0); // margin-left
            groups.box_.padding[0] = BoxLength::Px(4.0); // padding-top
            groups.box_.border_width[1] = 2.0; // border-right
            groups.box_.width = BoxLength::Px(40.0);
            groups.box_.bottom = BoxLength::Px(0.0);
        });

        let style = node_style_of(&node);

        assert_close(style.sides.margin.left_px, 6.0);
        assert_close(style.sides.padding.top_px, 4.0);
        assert_close(style.sides.border.right_px, 2.0);
        assert_eq!(style.width_px, Some(40.0));
        assert_eq!(style.bottom_px, Some(0.0));
    }

    #[test]
    fn node_style_reads_resolved_em_lengths_and_font_size() {
        // The style pass resolves `em` against the element's own font size and
        // writes resolved px into the groups; `node_style` reads those resolved
        // values (the `2em` font-size, `3em` width, `1em` padding-left, `0.5em`
        // margin-top all resolved against a 10px parent seed -> 20/60/20/10).
        let node = with_computed_groups(div_node(), |groups| {
            groups.font.size_px = 20.0;
            groups.box_.width = BoxLength::Px(60.0);
            groups.box_.padding[3] = BoxLength::Px(20.0); // padding-left
            groups.box_.margin[0] = BoxLength::Px(10.0); // margin-top
        });

        let style = node_style_of(&node);

        assert_close(style.font_size_px, 20.0);
        assert_eq!(style.width_px, Some(60.0));
        assert_close(style.sides.padding.left_px, 20.0);
        assert_close(style.sides.margin.top_px, 10.0);
    }

    #[test]
    fn layer_rules_mark_positioned_and_overflow_nodes_as_layers() {
        let relative = with_computed_groups(div_node(), |groups| {
            groups.box_.position = PositionKw::Relative;
        });
        assert_eq!(layer_kind_for_node(&relative), DomLayerKind::Normal);

        let transformed = with_computed_groups(div_node(), |groups| {
            groups.transform.translate_px = [6.0, 0.0];
            groups.transform.has_transform = true;
        });
        assert_eq!(layer_kind_for_node(&transformed), DomLayerKind::Normal);

        let hidden = with_computed_groups(div_node(), |groups| {
            groups.box_.overflow = OverflowKw::Hidden;
        });
        assert_eq!(layer_kind_for_node(&hidden), DomLayerKind::Clipped);

        let auto = with_computed_groups(div_node(), |groups| {
            groups.box_.overflow = OverflowKw::Auto;
        });
        assert_eq!(layer_kind_for_node(&auto), DomLayerKind::Clipped);

        let clip = with_computed_groups(div_node(), |groups| {
            groups.box_.overflow = OverflowKw::Clip;
        });
        assert_eq!(layer_kind_for_node(&clip), DomLayerKind::Clipped);
    }

    #[test]
    fn parentless_nodes_resolve_as_unclipped_layers() {
        let root = RootDom::new(RootDomContext::new_cpu());
        let layer_kind = layer_kind_for_node(root.nodes.get(root.root).expect("root should exist"));
        assert_eq!(layer_kind, DomLayerKind::Normal);
    }

    #[test]
    fn static_position_does_not_create_a_layer() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let key = attached_div(&mut root);
        root.style_mut(key)
            .set("position", "static")
            .expect("static");
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("style pass");
        assert_eq!(
            layer_kind_for_node(root.nodes.get(key).expect("node")),
            DomLayerKind::None
        );
    }

    #[test]
    fn display_none_does_not_create_a_layer() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let key = attached_div(&mut root);
        root.style_mut(key)
            .set("display", "none")
            .expect("display none");
        root.style_mut(key)
            .set("transform", "translateX(4px)")
            .expect("transform");
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("style pass");
        assert_eq!(
            layer_kind_for_node(root.nodes.get(key).expect("node")),
            DomLayerKind::None
        );
    }

    #[test]
    fn non_intrinsic_elements_do_not_use_viewport_fallback_as_intrinsic_size() {
        let node = div_node();

        let intrinsic_box = intrinsic_box_for_node(&node, (320.0, 240.0)).expect("intrinsic box");

        assert_close(intrinsic_box.border_box.width_px, 0.0);
        assert_close(intrinsic_box.border_box.height_px, 0.0);
    }

    #[test]
    fn document_nodes_keep_fallback_viewport_as_intrinsic_size() {
        let node = DomNode::document(test_owner_document_key());

        let intrinsic_box = intrinsic_box_for_node(&node, (320.0, 240.0)).expect("intrinsic box");

        assert_close(intrinsic_box.border_box.width_px, 320.0);
        assert_close(intrinsic_box.border_box.height_px, 240.0);
    }

    #[test]
    fn transform_translate_parses_supported_translate_functions() {
        assert_eq!(
            parse_translate("translate(8px, 12px)"),
            PointPx {
                x_px: 8.0,
                y_px: 12.0,
            }
        );
        assert_eq!(
            parse_translate("translateY(5px)"),
            PointPx {
                x_px: 0.0,
                y_px: 5.0,
            }
        );
    }
}
