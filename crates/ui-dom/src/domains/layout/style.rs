#![allow(dead_code)]

//! Private DOM-style layout property interpretation for the shared `TreeWalker`.
//!
//! What this file contains:
//!
//! - DOM-themed display and position interpretation
//! - strict extraction of authored box-edge and size properties
//! - layer-kind derivation from current node state and authored properties
//! - layer-order lookup from internal node layout state
//! - node-owned dirty and paint state accessors used by layout and composite
//! - small parsing helpers for transform translate values and pixel attrs
//!
//! Invariants and rules:
//!
//! - this file interprets authored DOM properties but does not walk the tree
//! - this file does not solve layout; it only derives property-level answers
//! - every answer here must be derivable from the current node and its current
//!   authored/presented properties
//! - this file must stay strict with supported DOM value meanings and fail
//!   closed through defaults rather than inventing new CSS-like behavior
//! - layer decisions here describe the node to later phases; they must not act
//!   as solver scratch or hidden traversal state
//!
//! Advice and code philosophy:
//!
//! - keep vocabulary DOM-themed: display, position, overflow, transform
//! - prefer direct derivation over storing extra state for later convenience
//! - keep parsing local and unsurprising so solver code can trust the answers
//! - if a helper is only needed by one solver branch, keep it there instead of
//!   teaching `style.rs` a broader concept than the DOM actually exposes
//!
use crate::domains::visitor::inherited_style::InheritedStyleContext;
use crate::numeric::i64_to_f32;
use crate::types::primitives::PointPx;
use crate::types::{
    AuthoredLength, BoxEdgeInsets, DomAttribute, DomAttributeValue, DomBoxModel, DomError,
    DomIntrinsic, DomLayerKind, DomNode, DomRect, parse_authored_length,
};

#[cfg(test)]
use crate::domains::visitor::inherited_style::inherited_context_from_node;
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
    if presented_display(node) == LayoutDisplay::None {
        return DomLayerKind::None;
    }
    if node_has_clipped_layer(node) {
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
    matches!(
        presented_position(node),
        PositionMode::Absolute | PositionMode::Fixed
    ) || overflow_creates_clipped_layer(node)
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
    current_inherited_context: &InheritedStyleContext,
) -> Result<NodeStyle, DomError> {
    let font_size_px = current_inherited_context.effective_font_size_px();
    Ok(NodeStyle {
        display: presented_display(node),
        position: presented_position(node),
        layer_kind: layer_kind_for_node(node),
        font_size_px,
        sides: BoxSides {
            margin: box_edge_insets_for(node, EdgeKind::Margin, font_size_px),
            border: box_edge_insets_for(node, EdgeKind::Border, font_size_px),
            padding: box_edge_insets_for(node, EdgeKind::Padding, font_size_px),
        },
        width_px: layout_length_attr_px(node, &DomAttribute::Width, font_size_px),
        height_px: layout_length_attr_px(node, &DomAttribute::Height, font_size_px),
        min_width_px: layout_length_attr_px(node, &DomAttribute::MinWidth, font_size_px),
        min_height_px: layout_length_attr_px(node, &DomAttribute::MinHeight, font_size_px),
        max_width_px: layout_length_attr_px(node, &DomAttribute::MaxWidth, font_size_px),
        max_height_px: layout_length_attr_px(node, &DomAttribute::MaxHeight, font_size_px),
        top_px: layout_length_attr_px(node, &DomAttribute::Top, font_size_px),
        left_px: layout_length_attr_px(node, &DomAttribute::Left, font_size_px),
        right_px: layout_length_attr_px(node, &DomAttribute::Right, font_size_px),
        bottom_px: layout_length_attr_px(node, &DomAttribute::Bottom, font_size_px),
    })
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

/// Returns whether the node should currently produce paint output. This gives
/// paint/composite one derived visibility answer from current DOM props.
///
/// Returns the current DOM display meaning for one node. This normalizes
/// text-node and authored display behavior into one enum.
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
    match string_attr_for_attribute(node, &DomAttribute::Display) {
        Some("none") => LayoutDisplay::None,
        Some("inline") => LayoutDisplay::Inline,
        Some("inline-block") => LayoutDisplay::InlineBlock,
        Some("flex") => LayoutDisplay::Flex,
        Some("inline-flex") => LayoutDisplay::InlineFlex,
        _ => LayoutDisplay::Block,
    }
}

/// Returns the current DOM position meaning for one node. This normalizes
/// authored `position` values into one strict layout enum.
///
/// Example:
///
/// ```ignore
/// let position = presented_position(node);
/// assert!(matches!(position, PositionMode::Static | PositionMode::Relative));
/// ```
pub(super) fn presented_position(node: &DomNode) -> PositionMode {
    match string_attr_for_attribute(node, &DomAttribute::Position) {
        Some("relative") => PositionMode::Relative,
        Some("absolute") => PositionMode::Absolute,
        Some("fixed") => PositionMode::Fixed,
        _ => PositionMode::Static,
    }
}

#[derive(Clone, Copy)]
enum EdgeKind {
    Margin,
    Border,
    Padding,
}

fn box_edge_insets_for(node: &DomNode, edge_kind: EdgeKind, font_size_px: f32) -> BoxEdgeInsets {
    let (top, left, right, bottom) = match edge_kind {
        EdgeKind::Margin => (
            DomAttribute::MarginTop,
            DomAttribute::MarginLeft,
            DomAttribute::MarginRight,
            DomAttribute::MarginBottom,
        ),
        EdgeKind::Border => (
            DomAttribute::BorderTop,
            DomAttribute::BorderLeft,
            DomAttribute::BorderRight,
            DomAttribute::BorderBottom,
        ),
        EdgeKind::Padding => (
            DomAttribute::PaddingTop,
            DomAttribute::PaddingLeft,
            DomAttribute::PaddingRight,
            DomAttribute::PaddingBottom,
        ),
    };
    BoxEdgeInsets {
        left_px: layout_length_attr_px(node, &left, font_size_px).unwrap_or(0.0),
        top_px: layout_length_attr_px(node, &top, font_size_px).unwrap_or(0.0),
        right_px: layout_length_attr_px(node, &right, font_size_px).unwrap_or(0.0),
        bottom_px: layout_length_attr_px(node, &bottom, font_size_px).unwrap_or(0.0),
    }
}

fn layout_length_attr_px(
    node: &DomNode,
    attribute: &DomAttribute,
    font_size_px: f32,
) -> Option<f32> {
    match node.presented_attr(attribute) {
        Some(DomAttributeValue::String(value)) => match parse_authored_length(value.as_str())? {
            AuthoredLength::Unset => None,
            AuthoredLength::Px(px) => Some(px),
            AuthoredLength::Em(scale) => Some(font_size_px * scale),
        },
        Some(DomAttributeValue::Float(value)) => Some(*value),
        Some(DomAttributeValue::Integer(value)) => Some(i64_to_f32(*value)),
        Some(DomAttributeValue::Bool(value)) => {
            panic!("layout length attrs should not use boolean payloads: {attribute:?}={value}")
        }
        _ => None,
    }
}

fn node_has_clipped_layer(node: &DomNode) -> bool {
    overflow_creates_clipped_layer(node) || isolate_creates_clipped_layer(node)
}

fn node_has_unclipped_layer(node: &DomNode) -> bool {
    if positioned_node_creates_unclipped_layer(node) {
        return true;
    }
    let Some(transform) = string_attr_for_attribute(node, &DomAttribute::Transform) else {
        return false;
    };
    transform != "none"
}

fn positioned_node_creates_unclipped_layer(node: &DomNode) -> bool {
    matches!(
        string_attr_for_attribute(node, &DomAttribute::Position),
        Some(value) if matches!(value, "relative" | "absolute" | "fixed")
    )
}

fn overflow_creates_clipped_layer(node: &DomNode) -> bool {
    let Some(overflow) = string_attr_for_attribute(node, &DomAttribute::Overflow) else {
        return false;
    };
    overflow != "visible"
}

fn isolate_creates_clipped_layer(node: &DomNode) -> bool {
    match node.presented_attr(&DomAttribute::Isolate) {
        Some(DomAttributeValue::Bool(value)) => *value,
        Some(DomAttributeValue::String(value)) => value == "true" || value == "isolate",
        _ => false,
    }
}

fn string_attr_for_attribute<'a>(node: &'a DomNode, attribute: &DomAttribute) -> Option<&'a str> {
    match node.presented_attr(attribute) {
        Some(DomAttributeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

/// Returns the local translate offset encoded in the node's transform attr.
/// This gives composite and hit testing one strict, derived translate answer.
///
/// Example:
///
/// ```ignore
/// let offset = transform_translate_for_node(node);
/// assert_eq!(offset.x_px, 8.0);
/// ```
pub(super) fn transform_translate_for_node(node: &DomNode) -> PointPx {
    let Some(transform) = presented_transform_attr(node) else {
        return PointPx::default();
    };
    parse_translate(transform)
}

fn presented_transform_attr(node: &DomNode) -> Option<&str> {
    match node.presented_attr(&DomAttribute::Transform) {
        Some(DomAttributeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn parse_translate(transform: &str) -> PointPx {
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

    fn resolved_node_style(node: &DomNode) -> super::NodeStyle {
        let inherited =
            inherited_context_from_node(node, &InheritedStyleContext::default()).expect("context");
        node_style(node, &inherited).expect("style")
    }

    fn resolved_node_style_with_parent_font_size(
        node: &DomNode,
        parent_font_size_px: f32,
    ) -> super::NodeStyle {
        let inherited = inherited_context_from_node(
            node,
            &InheritedStyleContext {
                color: None,
                font_size_px: Some(parent_font_size_px),
                line_height_px: None,
                text_align: None,
                text_direction: None,
                white_space: None,
                overflow_wrap: None,
                word_break: None,
            },
        )
        .expect("context");
        node_style(node, &inherited).expect("style")
    }

    #[test]
    fn presented_display_uses_text_and_supported_display_values() {
        let text = DomNode::fixture_text(test_owner_document_key(), "hello");
        assert_eq!(presented_display(&text), LayoutDisplay::Inline);

        let mut flex = div_node();
        flex.set_attr(
            DomAttribute::Display,
            Some(DomAttributeValue::String("flex".to_string())),
        )
        .expect("flex display");
        assert_eq!(presented_display(&flex), LayoutDisplay::Flex);

        let mut inline_flex = div_node();
        inline_flex
            .set_attr(
                DomAttribute::Display,
                Some(DomAttributeValue::String("inline-flex".to_string())),
            )
            .expect("inline-flex display");
        assert_eq!(presented_display(&inline_flex), LayoutDisplay::InlineFlex);
    }

    #[test]
    fn presented_position_maps_supported_position_values() {
        let mut relative = div_node();
        relative
            .set_attr(
                DomAttribute::Position,
                Some(DomAttributeValue::String("relative".to_string())),
            )
            .expect("relative position");
        assert_eq!(presented_position(&relative), PositionMode::Relative);

        let mut absolute = div_node();
        absolute
            .set_attr(
                DomAttribute::Position,
                Some(DomAttributeValue::String("absolute".to_string())),
            )
            .expect("absolute position");
        assert_eq!(presented_position(&absolute), PositionMode::Absolute);
    }

    #[test]
    fn node_style_reads_pixel_edges_and_sizes() {
        let mut node = div_node();
        node.set_attr(
            DomAttribute::MarginLeft,
            Some(DomAttributeValue::String("6px".to_string())),
        )
        .expect("margin-left");
        node.set_attr(
            DomAttribute::PaddingTop,
            Some(DomAttributeValue::String("4px".to_string())),
        )
        .expect("padding-top");
        node.set_attr(
            DomAttribute::BorderRight,
            Some(DomAttributeValue::String("2px".to_string())),
        )
        .expect("border-right");
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("40px".to_string())),
        )
        .expect("width");
        node.set_attr(
            DomAttribute::Bottom,
            Some(DomAttributeValue::String("0".to_string())),
        )
        .expect("bottom");

        let style = resolved_node_style(&node);

        assert_close(style.sides.margin.left_px, 6.0);
        assert_close(style.sides.padding.top_px, 4.0);
        assert_close(style.sides.border.right_px, 2.0);
        assert_eq!(style.width_px, Some(40.0));
        assert_eq!(style.bottom_px, Some(0.0));
    }

    #[test]
    fn node_style_resolves_em_lengths_from_current_font_size() {
        let mut node = div_node();
        node.set_attr(
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("2em".to_string())),
        )
        .expect("font-size");
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("3em".to_string())),
        )
        .expect("width");
        node.set_attr(
            DomAttribute::PaddingLeft,
            Some(DomAttributeValue::String("1em".to_string())),
        )
        .expect("padding-left");
        node.set_attr(
            DomAttribute::MarginTop,
            Some(DomAttributeValue::String("0.5em".to_string())),
        )
        .expect("margin-top");

        let style = resolved_node_style_with_parent_font_size(&node, 10.0);

        assert_close(style.font_size_px, 20.0);
        assert_eq!(style.width_px, Some(60.0));
        assert_close(style.sides.padding.left_px, 20.0);
        assert_close(style.sides.margin.top_px, 10.0);
    }

    #[test]
    fn layer_rules_mark_positioned_and_overflow_nodes_as_layers() {
        let mut relative = div_node();
        relative
            .set_attr(
                DomAttribute::Position,
                Some(DomAttributeValue::String("relative".to_string())),
            )
            .expect("relative position");
        assert_eq!(layer_kind_for_node(&relative), DomLayerKind::Normal);

        let mut transformed = div_node();
        transformed
            .set_attr(
                DomAttribute::Transform,
                Some(DomAttributeValue::String("translateX(6px)".to_string())),
            )
            .expect("transform");
        assert_eq!(layer_kind_for_node(&transformed), DomLayerKind::Normal);

        let mut hidden = div_node();
        hidden
            .set_attr(
                DomAttribute::Overflow,
                Some(DomAttributeValue::String("hidden".to_string())),
            )
            .expect("overflow hidden");
        assert_eq!(layer_kind_for_node(&hidden), DomLayerKind::Clipped);

        let mut auto = div_node();
        auto.set_attr(
            DomAttribute::Overflow,
            Some(DomAttributeValue::String("auto".to_string())),
        )
        .expect("overflow auto");
        assert_eq!(layer_kind_for_node(&auto), DomLayerKind::Clipped);

        let mut clip = div_node();
        clip.set_attr(
            DomAttribute::Overflow,
            Some(DomAttributeValue::String("clip".to_string())),
        )
        .expect("overflow clip");
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
        root.set_attr(
            key,
            DomAttribute::Position,
            Some(DomAttributeValue::String("static".to_string())),
        )
        .expect("static");
        assert_eq!(
            layer_kind_for_node(root.nodes.get(key).expect("node")),
            DomLayerKind::None
        );
    }

    #[test]
    fn display_none_does_not_create_a_layer() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let key = attached_div(&mut root);
        root.set_attr(
            key,
            DomAttribute::Display,
            Some(DomAttributeValue::String("none".to_string())),
        )
        .expect("display none");
        root.set_attr(
            key,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(4px)".to_string())),
        )
        .expect("transform");
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
        let mut node = div_node();
        node.set_attr(
            DomAttribute::Transform,
            Some(DomAttributeValue::String(
                "translate(8px, 12px)".to_string(),
            )),
        )
        .expect("translate");
        assert_eq!(
            transform_translate_for_node(&node),
            PointPx {
                x_px: 8.0,
                y_px: 12.0,
            }
        );

        node.set_attr(
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateY(5px)".to_string())),
        )
        .expect("translateY");
        assert_eq!(
            transform_translate_for_node(&node),
            PointPx {
                x_px: 0.0,
                y_px: 5.0,
            }
        );
    }
}
