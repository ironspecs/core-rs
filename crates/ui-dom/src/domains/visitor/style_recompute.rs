//! Retained style recompute visitor for one dirty layer-root subtree.
//!
//! Exact algorithm:
//!
//! 1. Start from one dirty layer root.
//! 2. Build one ancestor seed by walking parents outside that layer root and
//!    collecting only the transmitting inherited properties.
//! 3. Walk the subtree in DFS, carrying only traversal-local inherited state.
//! 4. For each element, match stylesheet rules directly in this visitor pass.
//! 5. Fold the element's TYPED winners (base behavior sheet → active
//!    pseudo-state behavior sheet → matched rules in ascending precedence →
//!    inline block) and resolve + write the seven computed groups + the
//!    transition-longhand slot directly on the node record (spine 3.3g-2a,
//!    `winners_fold.rs`). There is no legacy map and no `apply_styles`.
//! 6. Diff the freshly resolved groups against the last completed pass's
//!    groups (`NodePresentation::previous_computed`, captured before the fold):
//!    schedule pass-driven transitions for the moved fields (spine 3.3g-1b,
//!    D2 one-timing — the change is detected at the style update, not the
//!    write) and apply A7's group-diff dirty classification. The first pass
//!    establishes the baseline without diffing, so initial style application
//!    never schedules a transition.
//! 7. Derive the child traversal state from the element's freshly projected
//!    groups, including transition-presented values.
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

#[cfg(test)]
use crate::domains::style::BoxLength;
#[cfg(test)]
use crate::domains::style::StyleRuleOrigin;
use crate::domains::style::computed::GroupDirtyClassification;
#[cfg(test)]
use crate::domains::visitor::inherited_style::inherited_style_context_from_ancestors;
use crate::domains::visitor::inherited_style::{
    InheritedStyleContext, inherited_context_from_node,
};
use crate::domains::visitor::winners_fold::fold_element_winners;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
#[cfg(test)]
use crate::types::{
    DomAttribute, DomAttributeValue, DomIntrinsic, DomTag, RootDomContext, RootDomContextConfig,
    parse_css_color_rgba,
};
use crate::types::{
    DomError, DomOverflowWrap, DomWhiteSpace, DomWordBreak, RootDom, RootDomMetricName,
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
            Ok(*inherited_context)
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
    // Capture the last-pass groups BEFORE the fold (D2's one-timing model: the
    // diff is against the last style update, not the write). `None` = the
    // node's first pass — the baseline establishes without diffing, so the
    // initial style application never schedules a transition (matching CSS).
    // The write path (style_mut / the style-attribute route) only upserts the
    // inline block and marks style-dirty, leaving `previous_computed` untouched,
    // so a direct write's pre-change value survives to here.
    let previous_computed = root_dom
        .nodes
        .get(node_key)
        .and_then(DomNode::previous_computed);

    // Element resolution cost unit (proposal J's visited counter). Only
    // elements reach this function — Document/DocumentFragment/Text nodes are
    // routed to their own `recompute_*` arms — because the winners fold +
    // group resolution work this counter prices is paid per element, not per
    // document container or text node.
    root_dom.increment_metric(RootDomMetricName::StylePassNodeVisited);

    // Typed winners fold (spine 3.3g-2a): fold base behavior sheet → active
    // pseudo-state behavior sheet → matched rules (ascending precedence) →
    // inline block, then resolve and write the seven groups + transition slot
    // directly on the node record (seeded from the parent's inherited groups).
    // No legacy-map rebuild, no `apply_styles`, no attr projection.
    fold_element_winners(root_dom, node_key)?;

    // Read the freshly resolved groups + slot, then diff/schedule/classify.
    // Order: fold → new slot available → diff/schedule with the NEW slot
    // (browsers resolve the whole cascade then schedule, so a same-pass
    // transition-longhand change applies to changes scheduled this pass).
    let (new_groups, slot) = {
        let node = root_dom
            .nodes
            .get(node_key)
            .ok_or(DomError::InvalidNodeKey)?;
        (
            node.stored_computed_groups(),
            node.resolved_transition_longhands().clone(),
        )
    };
    if let Some(old_groups) = previous_computed {
        root_dom.schedule_pass_transitions(node_key, &old_groups, &new_groups, &slot)?;
        let classification =
            root_dom.apply_group_diff_classification(node_key, &old_groups, &new_groups);
        // Consequence volume (proposal J's changed counter): the group diff
        // classified above is the same diff this counter prices — the C1/C3
        // coarseness signal is visited vs changed, so the changed count is
        // driven off the classification the pass already computed.
        if classification != GroupDirtyClassification::NONE {
            root_dom.increment_metric(RootDomMetricName::StylePassNodeChanged);
        }
    } else {
        // Baseline (first pass): no transition scheduling (D2 one-timing, CSS),
        // but the layer membership must still be re-evaluated — a freshly
        // styled node that became a layer root (e.g. `position: fixed`) needs
        // its layer materialized by the pass now that the write path no longer
        // queues it synchronously (spine 3.3g-2a).
        root_dom.reevaluate_layer_membership_on_baseline(node_key);
    }
    // Record the freshly resolved groups as the new baseline for the next pass.
    let node = root_dom
        .nodes
        .get_mut(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    node.set_previous_computed(new_groups);

    // Derive the child traversal state from the element's freshly projected
    // groups, then remeasure text intrinsics.
    let node = root_dom
        .nodes
        .get(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let child_context = inherited_context_from_node(node, inherited_context);
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
    Ok(*inherited_context)
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
    use super::*;
    use std::sync::Arc;

    fn assert_f32_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "expected {expected}, got {actual}",
        );
    }

    #[test]
    fn style_recompute_applies_transition_longhands_for_scheduling() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let panel = root.create_element(DomTag::Div).expect("panel");
        root.append_child(root.root, panel).expect("append panel");
        root.set_attr_str(panel, "class", "drawer")
            .expect("class should set");
        root.style_mut(panel)
            .set("left", "0px")
            .expect("initial left should set");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".drawer { transition-property: left; transition-duration: 180ms; }",
            )
            .expect("stylesheet should load");
        let document_key = root.root;

        // Baseline pass: applies the transition longhands and establishes the
        // node's computed groups (left=0) as the pre-change value.
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute should apply transition longhands");
        root.style_mut(panel)
            .set("left", "100px")
            .expect("left should transition");
        // Pass-driven scheduling (D2 one-timing): the pass detects the change
        // and schedules the transition, presented at `from`.
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("recompute should schedule the transition");

        assert_eq!(
            root.node(panel)
                .expect("panel should exist")
                .computed_box()
                .left,
            BoxLength::Px(0.0)
        );
    }

    #[test]
    fn style_pass_work_counters_price_resolve_and_change_volume() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let container = root.create_element(DomTag::Div).expect("container");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, container)
            .expect("append container");
        root.append_child(container, leaf).expect("append leaf");
        root.style_mut(container)
            .set("background-color", "#111111")
            .expect("container background");
        let document_key = root.root;

        // First pass establishes the baseline: both elements are resolved
        // (visited == 2), but the baseline establishes without diffing, so no
        // element reports a change.
        root.reset_metrics();
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("baseline");
        assert_eq!(
            root.metrics()
                .count(RootDomMetricName::StylePassNodeVisited),
            2
        );
        assert_eq!(
            root.metrics()
                .count(RootDomMetricName::StylePassNodeChanged),
            0
        );

        // A real group change on a non-inherited property: background-color
        // lives in the visual group, which the leaf does not inherit. The
        // container's visual group moves, the leaf's stays initial, so changed
        // prices exactly one element — and changed <= visited always.
        root.style_mut(container)
            .set("background-color", "#222222")
            .expect("recolor");
        root.reset_metrics();
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");
        assert_eq!(
            root.metrics()
                .count(RootDomMetricName::StylePassNodeVisited),
            2
        );
        assert_eq!(
            root.metrics()
                .count(RootDomMetricName::StylePassNodeChanged),
            1
        );
        assert!(
            root.metrics()
                .count(RootDomMetricName::StylePassNodeChanged)
                <= root
                    .metrics()
                    .count(RootDomMetricName::StylePassNodeVisited)
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
        assert_eq!(label_node.computed_font().size_px, 20.0);
        assert_eq!(
            label_node.computed_color().color_rgba,
            parse_css_color_rgba("#abcdef").expect("#abcdef")
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
    fn style_recompute_resolves_stylesheet_em_font_size_and_remeasures_text_from_it() {
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

        // The computed font group stores the resolved px (1.5em against the 14px
        // default base -> 21px), not the authored `em` string.
        let container_node = root.node(container).expect("container node");
        assert_eq!(container_node.computed_font().size_px, 21.0);
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
        root.style_mut(ancestor)
            .set("font-size", "10px")
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
        root.style_mut(ancestor)
            .set("font-size", "22px")
            .expect("ancestor font-size");
        root.style_mut(start)
            .set("transform", "translateX(4px)")
            .expect("start layer");

        // Resolve the ancestor's font-size first (the style pass folds from the
        // document; `start` is a layer root whose ancestors are outside it).
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute");
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
        root.style_mut(ancestor)
            .set("color", "#101010")
            .expect("seed color");
        root.style_mut(start)
            .set("transform", "translateX(6px)")
            .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");

        // Resolve the ancestor's color first (the seed climbs to ancestors
        // outside the start layer root).
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute");
        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");
        assert_eq!(
            computed_color_rgba(&root, leaf),
            parse_css_color_rgba("#101010").expect("#101010")
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
        root.style_mut(far_ancestor)
            .set("color", "#111111")
            .expect("far ancestor color");
        root.style_mut(near_ancestor)
            .set("color", "#222222")
            .expect("near ancestor color");
        root.style_mut(start)
            .set("transform", "translateX(6px)")
            .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("style dirty");

        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute");
        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");

        assert_eq!(
            computed_color_rgba(&root, leaf),
            parse_css_color_rgba("#222222").expect("#222222")
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
        root.style_mut(nested_layer)
            .set("transform", "translateX(4px)")
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
            computed_color_rgba(&root, leaf),
            parse_css_color_rgba("#abcdef").expect("#abcdef")
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
            computed_color_rgba(&root, left_leaf),
            parse_css_color_rgba("#ee1111").expect("#ee1111")
        );
        assert_eq!(
            computed_color_rgba(&root, right_leaf),
            // The sibling with no matching color rule keeps the initial text
            // color (spine 3.3g-2a: computed groups carry the resolved value).
            [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0]
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
        root.style_mut(ancestor)
            .set("background-color", "#222222")
            .expect("ancestor background-color");
        root.style_mut(start)
            .set("transform", "translateX(2px)")
            .expect("start layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("start".to_string())),
        )
        .expect("start class");

        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("recompute");

        assert_eq!(
            computed_background_rgba(&root, leaf),
            // background-color is non-inherited: the leaf keeps the transparent
            // initial even though the ancestor authored a background.
            [0.0, 0.0, 0.0, 0.0]
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
        root.style_mut(start)
            .set("transform", "translateX(1px)")
            .expect("start layer");
        root.style_mut(nested_layer)
            .set("transform", "translateX(2px)")
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
        root.style_mut(ancestor)
            .set("color", "#111111")
            .expect("ancestor color");
        root.style_mut(start)
            .set("transform", "translateX(1px)")
            .expect("start layer");
        root.style_mut(nested_layer)
            .set("transform", "translateX(2px)")
            .expect("nested layer");
        root.set_attr(
            start,
            DomAttribute::Class,
            Some(DomAttributeValue::String("seed".to_string())),
        )
        .expect("style dirty");

        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute");
        recompute_dirty_style_subtree_for_layer_root(&mut root, start).expect("start pass");
        let before = computed_color_rgba(&root, leaf);
        assert_eq!(before, parse_css_color_rgba("#111111").expect("#111111"));
        assert!(!style_dirty(&root, nested_layer));

        root.style_mut(ancestor)
            .set("color", "#333333")
            .expect("ancestor color update");

        recompute_dirty_style_subtree_for_layer_root(&mut root, nested_layer).expect("nested pass");
        let after = computed_color_rgba(&root, leaf);

        assert_eq!(after, before);
    }

    /// The leaf's resolved text color, read through the computed color group
    /// (spine 3.3g-2a: presentation values live in the computed groups, not
    /// the attrs map).
    fn computed_color_rgba(root: &RootDom, key: DomNodeSlotKey) -> [f32; 4] {
        root.node(key)
            .expect("node should exist")
            .computed_color()
            .color_rgba
    }

    fn computed_background_rgba(root: &RootDom, key: DomNodeSlotKey) -> [f32; 4] {
        root.node(key)
            .expect("node should exist")
            .computed_visual()
            .background_rgba
    }

    fn style_dirty(root: &RootDom, key: DomNodeSlotKey) -> bool {
        let node = root.node(key).expect("node should exist");
        match node {
            DomNode::Document(document) => document.base.style.dirty,
            DomNode::DocumentFragment(fragment) => fragment.base.style.dirty,
            DomNode::Element(element) => element.base.style.dirty,
            DomNode::Text(text) => text.base.style.dirty,
        }
    }

    fn clear_layout_dirty_for_test(root: &mut RootDom, key: DomNodeSlotKey) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.layout.dirty = false;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.layout.dirty = false;
            }
            DomNode::Element(element) => {
                element.base.layout.dirty = false;
            }
            DomNode::Text(text) => {
                text.base.layout.dirty = false;
            }
        }
    }

    fn layout_dirty_for_test(root: &RootDom, key: DomNodeSlotKey) -> bool {
        let node = root.node(key).expect("node should exist");
        match node {
            DomNode::Document(document) => document.base.layout.dirty,
            DomNode::DocumentFragment(fragment) => fragment.base.layout.dirty,
            DomNode::Element(element) => element.base.layout.dirty,
            DomNode::Text(text) => text.base.layout.dirty,
        }
    }
}
