//! Retained style recompute visitor for one dirty layer-root subtree.
//!
//! Exact algorithm:
//!
//! 1. Start from one dirty layer root.
//! 2. Build one ancestor seed by walking parents outside that layer root and
//!    collecting only the transmitting inherited properties.
//! 3. Walk the subtree in DFS, carrying only traversal-local inherited state.
//! 4. For each element, match stylesheet rules directly in this visitor pass.
//! 5. Build that element's effective stylesheet-owned map from:
//!    - the carried inherited transmitting state
//!    - then the matched local stylesheet declarations
//! 6. Apply that effective map onto retained node attrs.
//! 7. Derive the child traversal state from the element's effective retained
//!    result, including direct attrs and transition-presented values.
//! 8. For text nodes, remeasure retained intrinsic text metrics from the
//!    carried effective font size in this same visitor pass.
//! 9. Clear style dirtiness for each visited node.
//!
//! Hard rule:
//!
//! - this visitor must not call `RootDom::compute_style_map`
//!
//! Why this algorithm exists:
//!
//! - it does one ancestor-seed walk per dirty layer root instead of rebuilding
//!   parent style maps independently for every node
//! - it carries only the transmitting inherited state (`color`, `font-size`,
//!   `line-height`, `text-align`, `direction`, `white-space`,
//!   `overflow-wrap`, and `word-break`) instead
//!   of cloning full per-node style maps through the live recompute path
//! - it keeps retained node attrs and retained intrinsic state as the single
//!   source of truth for layout and paint
//! - it lets text intrinsic updates happen during the real style pass without
//!   a second style query path
//!
//! `compute_style_map` remains the separate one-off helper API for callers that
//! need a pure style query. The live retained recompute path in this file must
//! stay independent from that helper for both architecture and performance.

use crate::domains::style::{
    ComputedStyleMap, apply_pseudo_state_style_declarations, matched_style_rules_for_element,
};
#[cfg(test)]
use crate::domains::visitor::inherited_style::inherited_style_context_from_ancestors;
use crate::domains::visitor::inherited_style::{
    InheritedStyleContext, inherited_color_attribute, inherited_context_from_node,
};
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomOverflowWrap, DomWhiteSpace, DomWordBreak,
    RootDom, RootDomMetricName,
};

#[cfg(test)]
#[tracing::instrument(
    name = "style_updated",
    level = "debug",
    skip(root_dom),
    fields(
        dom_parent = ?root_dom.attached_bevy_parent_entity(),
        layer_root_key = ?layer_root_key
    )
)]
pub(crate) fn recompute_dirty_style_subtree_for_layer_root(
    root_dom: &mut RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let inherited_context = inherited_style_context_from_ancestors(root_dom, layer_root_key)?;
    recompute_dirty_style_subtree_for_layer_root_with_inherited(
        root_dom,
        layer_root_key,
        &inherited_context,
    )
}

pub(crate) fn recompute_dirty_style_subtree_for_layer_root_with_inherited(
    root_dom: &mut RootDom,
    layer_root_key: DomNodeSlotKey,
    inherited_context: &InheritedStyleContext,
) -> Result<(), DomError> {
    if !style_tree_needs_recompute(root_dom, layer_root_key)? {
        return Ok(());
    }
    root_dom.increment_metric(RootDomMetricName::StyleUpdated);
    recompute_style_subtree_dfs(root_dom, layer_root_key, inherited_context)?;
    Ok(())
}

pub(crate) fn style_tree_needs_recompute(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    Ok(match node {
        DomNode::Document(node) => node.base.style.dirty,
        DomNode::DocumentFragment(node) => node.base.style.dirty,
        DomNode::Element(node) => node.base.style.dirty,
        DomNode::Text(node) => node.base.style.dirty,
    })
}

fn recompute_style_subtree_dfs(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    inherited_context: &InheritedStyleContext,
) -> Result<(), DomError> {
    let child_context = recompute_style_for_node(root_dom, node_key, inherited_context)?;
    let mut child_key = root_dom
        .node(node_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    while let Some(current_child_key) = child_key {
        let next_child_key = root_dom
            .node(current_child_key)
            .ok_or(DomError::InvalidNodeKey)?
            .next_sibling_key_for_store();
        recompute_style_subtree_dfs(root_dom, current_child_key, &child_context)?;
        child_key = next_child_key;
    }
    Ok(())
}

fn recompute_style_for_node(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    inherited_context: &InheritedStyleContext,
) -> Result<InheritedStyleContext, DomError> {
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    match node {
        DomNode::Document(_) | DomNode::DocumentFragment(_) => {
            clear_node_style_dirty(root_dom, node_key)?;
            Ok(inherited_context.clone())
        }
        DomNode::Element(_) => recompute_element_style(root_dom, node_key, inherited_context),
        DomNode::Text(_) => recompute_text_style(root_dom, node_key, inherited_context),
    }
}

fn recompute_element_style(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    inherited_context: &InheritedStyleContext,
) -> Result<InheritedStyleContext, DomError> {
    let element = root_dom
        .node(node_key)
        .and_then(DomNode::as_element)
        .ok_or(DomError::InvalidAttributeTarget)?;
    let matched_rules = matched_style_rules_for_element(root_dom, node_key)?;
    let mut next_map = stylesheet_map_from_inherited_context(inherited_context);
    apply_pseudo_state_style_declarations(element, &mut next_map);
    for rule in matched_rules {
        for (attribute, value) in &rule.attr_map {
            next_map.insert(attribute.clone(), value.clone());
        }
    }
    root_dom.apply_styles(node_key, &next_map)?;
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    let child_context = inherited_context_from_node(node, inherited_context)?;
    recompute_text_metric_intrinsic_for_style_pass(
        root_dom,
        node_key,
        child_context.effective_font_size_px(),
        child_context.effective_line_height_px(),
        child_context.effective_white_space(),
        child_context.effective_overflow_wrap(),
        child_context.effective_word_break(),
    )?;
    clear_node_style_dirty(root_dom, node_key)?;
    Ok(child_context)
}

fn recompute_text_style(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    inherited_context: &InheritedStyleContext,
) -> Result<InheritedStyleContext, DomError> {
    recompute_text_metric_intrinsic_for_style_pass(
        root_dom,
        node_key,
        inherited_context.effective_font_size_px(),
        inherited_context.effective_line_height_px(),
        inherited_context.effective_white_space(),
        inherited_context.effective_overflow_wrap(),
        inherited_context.effective_word_break(),
    )?;
    clear_node_style_dirty(root_dom, node_key)?;
    Ok(inherited_context.clone())
}

fn clear_node_style_dirty(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let node = root_dom
        .nodes
        .get_mut(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    match node {
        DomNode::Document(document) => document.base.style.dirty = false,
        DomNode::DocumentFragment(fragment) => fragment.base.style.dirty = false,
        DomNode::Element(element) => element.base.style.dirty = false,
        DomNode::Text(text) => text.base.style.dirty = false,
    }
    Ok(())
}

fn stylesheet_map_from_inherited_context(
    inherited_context: &InheritedStyleContext,
) -> ComputedStyleMap {
    let mut map = ComputedStyleMap::new();
    if let Some(value) = &inherited_context.color {
        map.insert(inherited_color_attribute(), value.clone());
    }
    if let Some(font_size_px) = inherited_context.font_size_px {
        map.insert(
            DomAttribute::FontSize,
            font_size_attr_value_from_px(font_size_px),
        );
    }
    if let Some(text_align) = inherited_context.text_align {
        map.insert(
            DomAttribute::TextAlign,
            DomAttributeValue::String(
                match text_align {
                    crate::types::DomTextAlign::Start => "start",
                    crate::types::DomTextAlign::Left => "left",
                    crate::types::DomTextAlign::Center => "center",
                    crate::types::DomTextAlign::Right => "right",
                    crate::types::DomTextAlign::Justified => "justify",
                    crate::types::DomTextAlign::End => "end",
                }
                .to_string(),
            ),
        );
    }
    if let Some(text_direction) = inherited_context.text_direction {
        map.insert(
            DomAttribute::Direction,
            DomAttributeValue::String(
                match text_direction {
                    crate::types::DomTextDirection::Ltr => "ltr",
                    crate::types::DomTextDirection::Rtl => "rtl",
                }
                .to_string(),
            ),
        );
    }
    map
}

fn font_size_attr_value_from_px(font_size_px: f32) -> DomAttributeValue {
    if font_size_px == 0.0 {
        return DomAttributeValue::String("0".to_string());
    }
    if font_size_px.fract() == 0.0 {
        return DomAttributeValue::String(format!("{font_size_px:.0}px"));
    }
    DomAttributeValue::String(format!("{font_size_px}px"))
}

fn recompute_text_metric_intrinsic_for_style_pass(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    effective_font_size_px: f32,
    effective_line_height_px: Option<f32>,
    effective_white_space: DomWhiteSpace,
    effective_overflow_wrap: DomOverflowWrap,
    effective_word_break: DomWordBreak,
) -> Result<(), DomError> {
    let node = root_dom
        .nodes
        .get_mut(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let intrinsic_changed = node.after_style_recompute_intrinsic_update(
        effective_font_size_px,
        effective_line_height_px,
        effective_white_space,
        effective_overflow_wrap,
        effective_word_break,
    );
    if !intrinsic_changed {
        return Ok(());
    }
    root_dom.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
    root_dom.mark_layout_dirty_for_attribute_change(node_key);
    root_dom.mark_paint_dirty_for_attribute_change(node_key);
    Ok(())
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::sync::Arc;

    use crate::domains::style::StyleRuleOrigin;
    use crate::types::{
        DomAttribute, DomAttributeValue, DomIntrinsic, DomTag, RootDom, RootDomContext,
        RootDomContextConfig, parse_css_color_rgba,
    };

    use super::recompute_dirty_style_subtree_for_layer_root;

    fn assert_f32_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "expected {expected}, got {actual}",
        );
    }

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    #[test]
    fn style_recompute_applies_transition_longhands_for_scheduling() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let panel = root.create_element(DomTag::Div).expect("panel");
        root.append_child(root.root, panel).expect("append panel");
        root.set_attr_str(panel, "class", "drawer")
            .expect("class should set");
        root.set_attr_str(panel, "left", "0px")
            .expect("initial left should set");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".drawer { transition-property: left; transition-duration: 180ms; }",
            )
            .expect("stylesheet should load");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("recompute should apply transition longhands");
        root.set_attr_str(panel, "left", "100px")
            .expect("left should transition");

        assert_eq!(
            root.node(panel)
                .expect("panel should exist")
                .presented_attr(&DomAttribute::Left),
            Some(&DomAttributeValue::Float(0.0))
        );
    }

    #[test]
    fn style_recompute_carries_font_size_to_descendant_text_intrinsic() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let label = root.create_element(DomTag::Span).expect("label");
        let text = root.create_text_node("font size carry").expect("text");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, label).expect("append label");
        root.append_child(label, text).expect("append text");
        root.set_attr(
            container,
            DomAttribute::Class,
            Some(DomAttributeValue::String("large".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".large { font-size: 20px; color: #abcdef; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.font_size_px, 20.0);
        let label_node = root.node(label).expect("label node");
        assert_eq!(
            label_node.presented_attr(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("20px".to_string()))
        );
        assert_eq!(
            label_node.presented_attr(&DomAttribute::Custom("color".to_string())),
            Some(&color("#abcdef"))
        );
    }

    #[test]
    fn style_recompute_carries_line_height_to_descendant_text_intrinsic() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let text = root.create_text_node("line height carry").expect("text");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, text).expect("append text");
        root.set_attr(
            container,
            DomAttribute::Class,
            Some(DomAttributeValue::String("row".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".row { line-height: 18px; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.line_height_px, 18.0);
    }

    #[test]
    fn style_recompute_keeps_stylesheet_em_font_size_raw_and_remeasures_text_from_it() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let text = root.create_text_node("font size carry").expect("text");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, text).expect("append text");
        root.set_attr(
            container,
            DomAttribute::Class,
            Some(DomAttributeValue::String("scaled".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".scaled { font-size: 1.5em; color: #abcdef; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let container_node = root.node(container).expect("container node");
        assert_eq!(
            container_node.presented_attr(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("1.5em".to_string()))
        );
        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.font_size_px, 21.0);
    }

    #[test]
    fn style_recompute_resolves_stylesheet_em_font_size_against_inherited_base() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let ancestor = root.create_element(DomTag::Div).expect("ancestor");
        let container = root.create_element(DomTag::Div).expect("container");
        let text = root.create_text_node("font size carry").expect("text");
        root.append_child(root.root, ancestor)
            .expect("append ancestor");
        root.append_child(ancestor, container)
            .expect("append container");
        root.append_child(container, text).expect("append text");
        root.set_attr(
            ancestor,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("10px".to_string())),
        )
        .expect("ancestor font-size");
        root.set_attr(
            container,
            DomAttribute::Class,
            Some(DomAttributeValue::String("scaled".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".scaled { font-size: 1.5em; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.font_size_px, 15.0);
    }

    #[test]
    fn style_recompute_uses_default_font_size_when_no_attr_is_present() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let text = root.create_text_node("default size").expect("text");
        root.append_child(root.root, text).expect("append text");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.font_size_px, 14.0);
    }

    #[test]
    fn style_recompute_seeds_font_size_from_ancestor_outside_start_layer() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let ancestor = root.create_element(DomTag::Div).expect("ancestor");
        let start = root.create_element(DomTag::Div).expect("start");
        let text = root.create_text_node("seeded size").expect("text");
        root.append_child(root.root, ancestor)
            .expect("append ancestor");
        root.append_child(ancestor, start).expect("append start");
        root.append_child(start, text).expect("append text");
        root.set_attr(
            ancestor,
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("22px".to_string())),
        )
        .expect("ancestor font-size");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(4px)".to_string())),
        )
        .expect("start layer");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");

        let text_node = root.node(text).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        assert_f32_eq(intrinsic.font_size_px, 22.0);
    }

    #[test]
    fn style_recompute_source_does_not_call_compute_style_map() {
        let source = include_str!("style_recompute.rs");
        let forbidden_call_pattern = [".", "compute_style_map", "("].concat();
        assert!(
            !source.contains(&forbidden_call_pattern),
            "retained style recompute visitor must not call compute_style_map",
        );
    }

    #[test]
    fn style_recompute_updates_input_preferred_size_from_effective_font_size() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, input).expect("append input");
        root.set_attr(
            input,
            DomAttribute::Placeholder,
            Some(DomAttributeValue::String("Input placeholder".to_string())),
        )
        .expect("placeholder");
        root.set_attr(
            input,
            DomAttribute::Class,
            Some(DomAttributeValue::String("large".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".large { font-size: 20px; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let input_node = root.node(input).expect("input node");
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = input_node.intrinsic() else {
            panic!("input should keep default preferred size intrinsic");
        };
        assert_f32_eq(intrinsic.font_size_px, 20.0);
        assert!(intrinsic.width_px > 0.0);
    }

    #[test]
    fn style_recompute_updates_input_preferred_size_from_effective_line_height() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, input).expect("append input");
        root.set_attr(
            input,
            DomAttribute::Class,
            Some(DomAttributeValue::String("large".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".large { line-height: 2; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let input_node = root.node(input).expect("input node");
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = input_node.intrinsic() else {
            panic!("input should keep default preferred size intrinsic");
        };
        assert_f32_eq(intrinsic.line_height_px, 28.0);
        assert_f32_eq(intrinsic.height_px, 28.0);
    }

    #[test]
    fn style_recompute_updates_textarea_preferred_size_from_effective_font_size() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, textarea)
            .expect("append textarea");
        root.set_attr(
            textarea,
            DomAttribute::Placeholder,
            Some(DomAttributeValue::String("Line 1\nLine 2".to_string())),
        )
        .expect("placeholder");
        root.set_attr(
            container,
            DomAttribute::Class,
            Some(DomAttributeValue::String("large".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".large { font-size: 18px; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let textarea_node = root.node(textarea).expect("textarea node");
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = textarea_node.intrinsic() else {
            panic!("textarea should keep default preferred size intrinsic");
        };
        assert_f32_eq(intrinsic.font_size_px, 18.0);
        assert!(intrinsic.height_px > intrinsic.line_height_px);
    }

    #[test]
    fn style_recompute_updates_textarea_preferred_size_from_effective_line_height() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, textarea)
            .expect("append textarea");
        root.set_attr(
            textarea,
            DomAttribute::Placeholder,
            Some(DomAttributeValue::String("Line 1\nLine 2".to_string())),
        )
        .expect("placeholder");
        root.set_attr(
            textarea,
            DomAttribute::Class,
            Some(DomAttributeValue::String("tall".to_string())),
        )
        .expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".tall { line-height: 1.5; }",
            )
            .expect("stylesheet");
        let document_key = root.root;

        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        let textarea_node = root.node(textarea).expect("textarea node");
        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = textarea_node.intrinsic() else {
            panic!("textarea should keep default preferred size intrinsic");
        };
        assert_f32_eq(intrinsic.line_height_px, 21.0);
        assert_f32_eq(intrinsic.height_px, 42.0);
    }

    #[test]
    fn style_recompute_uses_upward_seed_from_ancestors_outside_start_layer() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let ancestor = root.create_element(DomTag::Div).expect("ancestor");
        let start = root.create_element(DomTag::Div).expect("start");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, ancestor)
            .expect("append ancestor");
        root.append_child(ancestor, start).expect("append start");
        root.append_child(start, leaf).expect("append leaf");
        root.set_attr(
            ancestor,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#101010".to_string())),
        )
        .expect("seed color");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(6px)".to_string())),
        )
        .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");
        assert_eq!(
            element_attr(&root, leaf, &DomAttribute::Custom("color".to_string())),
            Some(color("#101010"))
        );
    }

    #[test]
    fn style_recompute_upward_seed_prefers_nearest_ancestor_value() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far_ancestor = root.create_element(DomTag::Div).expect("far ancestor");
        let near_ancestor = root.create_element(DomTag::Div).expect("near ancestor");
        let start = root.create_element(DomTag::Div).expect("start");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, far_ancestor)
            .expect("append far ancestor");
        root.append_child(far_ancestor, near_ancestor)
            .expect("append near ancestor");
        root.append_child(near_ancestor, start)
            .expect("append start");
        root.append_child(start, leaf).expect("append leaf");
        root.set_attr(
            far_ancestor,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#111111".to_string())),
        )
        .expect("far ancestor color");
        root.set_attr(
            near_ancestor,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#222222".to_string())),
        )
        .expect("near ancestor color");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(6px)".to_string())),
        )
        .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");

        assert_eq!(
            element_attr(&root, leaf, &DomAttribute::Custom("color".to_string())),
            Some(color("#222222"))
        );
    }

    #[test]
    fn style_recompute_crosses_nested_layer_boundaries() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let start = root.create_element(DomTag::Div).expect("start");
        let nested_layer = root.create_element(DomTag::Div).expect("nested");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, start).expect("append start");
        root.append_child(start, nested_layer)
            .expect("append nested");
        root.append_child(nested_layer, leaf).expect("append leaf");
        root.set_attr(
            nested_layer,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(4px)".to_string())),
        )
        .expect("nested layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".start { color: #abcdef; }",
            )
            .expect("stylesheet");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");
        assert_eq!(
            element_attr(&root, leaf, &DomAttribute::Custom("color".to_string())),
            Some(color("#abcdef"))
        );
    }

    #[test]
    fn style_recompute_parent_stack_pop_isolates_siblings() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let start = root.create_element(DomTag::Div).expect("start");
        let left = root.create_element(DomTag::Div).expect("left");
        let right = root.create_element(DomTag::Div).expect("right");
        let left_leaf = root.create_element(DomTag::Span).expect("left leaf");
        let right_leaf = root.create_element(DomTag::Span).expect("right leaf");
        root.append_child(root.root, start).expect("append start");
        root.append_child(start, left).expect("append left");
        root.append_child(start, right).expect("append right");
        root.append_child(left, left_leaf)
            .expect("append left leaf");
        root.append_child(right, right_leaf)
            .expect("append right leaf");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");
        root.set_attr(
            left,
            DomAttribute::Class,
            Some(DomAttributeValue::String("left".to_string())),
        )
        .expect("left class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".left { color: #ee1111; }",
            )
            .expect("stylesheet");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");
        assert_eq!(
            element_attr(&root, left_leaf, &DomAttribute::Custom("color".to_string())),
            Some(color("#ee1111"))
        );
        assert_eq!(
            element_attr(
                &root,
                right_leaf,
                &DomAttribute::Custom("color".to_string())
            ),
            None
        );
    }

    #[test]
    fn style_recompute_clears_style_dirty_flags_for_visited_nodes() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let start = root.create_element(DomTag::Div).expect("start");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, start).expect("append start");
        root.append_child(start, child).expect("append child");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");
        assert!(!style_dirty(&root, start));
        assert!(!style_dirty(&root, child));
    }

    #[test]
    fn style_recompute_does_not_propagate_non_transmitting_attributes() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let ancestor = root.create_element(DomTag::Div).expect("ancestor");
        let start = root.create_element(DomTag::Div).expect("start");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, ancestor)
            .expect("append ancestor");
        root.append_child(ancestor, start).expect("append start");
        root.append_child(start, leaf).expect("append leaf");
        root.set_attr(
            ancestor,
            DomAttribute::Custom("background-color".to_string()),
            Some(DomAttributeValue::String("#222222".to_string())),
        )
        .expect("ancestor background-color");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(2px)".to_string())),
        )
        .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("start class");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");

        assert_eq!(
            element_attr(
                &root,
                leaf,
                &DomAttribute::Custom("background-color".to_string())
            ),
            None
        );
    }

    #[test]
    fn style_recompute_skips_clean_nested_layer_after_parent_pass() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let start = root.create_element(DomTag::Div).expect("start");
        let nested_layer = root.create_element(DomTag::Div).expect("nested");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, start).expect("append start");
        root.append_child(start, nested_layer)
            .expect("append nested");
        root.append_child(nested_layer, leaf).expect("append leaf");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(1px)".to_string())),
        )
        .expect("start layer");
        root.set_attr(
            nested_layer,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(2px)".to_string())),
        )
        .expect("nested layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("wide".to_string())),
        )
        .expect("start class");
        root.set_attr(
            nested_layer,
            DomAttribute::Class,
            Some(DomAttributeValue::String("nested".to_string())),
        )
        .expect("nested class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".wide .nested { width: 30px; }",
            )
            .expect("stylesheet");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("start pass");
        clear_layout_dirty_for_test(&mut root, nested_layer);
        clear_layout_dirty_for_test(&mut root, leaf);

        recompute_dirty_style_subtree_for_layer_root(&mut root, nested_layer).expect("nested pass");

        assert!(!layout_dirty_for_test(&root, nested_layer));
        assert!(!layout_dirty_for_test(&root, leaf));
    }

    #[test]
    fn style_recompute_clean_layer_is_strong_noop_even_when_ancestor_changes() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let ancestor = root.create_element(DomTag::Div).expect("ancestor");
        let start = root.create_element(DomTag::Div).expect("start");
        let nested_layer = root.create_element(DomTag::Div).expect("nested layer");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, ancestor)
            .expect("append ancestor");
        root.append_child(ancestor, start).expect("append start");
        root.append_child(start, nested_layer)
            .expect("append nested layer");
        root.append_child(nested_layer, leaf).expect("append leaf");
        root.set_attr(
            ancestor,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#111111".to_string())),
        )
        .expect("ancestor color");
        root.set_attr(
            start,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(1px)".to_string())),
        )
        .expect("start layer");
        root.set_attr(
            nested_layer,
            DomAttribute::Transform,
            Some(DomAttributeValue::String("translateX(2px)".to_string())),
        )
        .expect("nested layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("seed".to_string())),
        )
        .expect("style dirty");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("start pass");
        let before = element_attr(&root, leaf, &DomAttribute::Custom("color".to_string()));
        assert_eq!(before, Some(color("#111111")));
        assert!(!style_dirty(&root, nested_layer));

        root.set_attr(
            ancestor,
            DomAttribute::Custom("color".to_string()),
            Some(DomAttributeValue::String("#333333".to_string())),
        )
        .expect("ancestor color update");

        recompute_dirty_style_subtree_for_layer_root(&mut root, nested_layer).expect("nested pass");
        let after = element_attr(&root, leaf, &DomAttribute::Custom("color".to_string()));

        assert_eq!(after, before);
    }

    fn element_attr(
        root: &RootDom,
        key: crate::types::DomNodeSlotKey,
        attribute: &DomAttribute,
    ) -> Option<DomAttributeValue> {
        let node = root.node(key)?;
        let element = node.as_element()?;
        element.attrs.attributes.get(attribute).cloned()
    }

    fn style_dirty(root: &RootDom, key: crate::types::DomNodeSlotKey) -> bool {
        let node = root.node(key).expect("node should exist");
        match node {
            crate::types::DomNode::Document(document) => document.base.style.dirty,
            crate::types::DomNode::DocumentFragment(fragment) => fragment.base.style.dirty,
            crate::types::DomNode::Element(element) => element.base.style.dirty,
            crate::types::DomNode::Text(text) => text.base.style.dirty,
        }
    }

    fn clear_layout_dirty_for_test(root: &mut RootDom, key: crate::types::DomNodeSlotKey) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            crate::types::DomNode::Document(document) => {
                document.base.layout.dirty = false;
            }
            crate::types::DomNode::DocumentFragment(fragment) => {
                fragment.base.layout.dirty = false;
            }
            crate::types::DomNode::Element(element) => {
                element.base.layout.dirty = false;
            }
            crate::types::DomNode::Text(text) => {
                text.base.layout.dirty = false;
            }
        }
    }

    fn layout_dirty_for_test(root: &RootDom, key: crate::types::DomNodeSlotKey) -> bool {
        let node = root.node(key).expect("node should exist");
        match node {
            crate::types::DomNode::Document(document) => document.base.layout.dirty,
            crate::types::DomNode::DocumentFragment(fragment) => fragment.base.layout.dirty,
            crate::types::DomNode::Element(element) => element.base.layout.dirty,
            crate::types::DomNode::Text(text) => text.base.layout.dirty,
        }
    }
}
