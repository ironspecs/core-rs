//! Shared inherited-style ancestor seed for retained visitors.
//!
//! This module owns the two group-derived inheritance derivations that style,
//! layout, paint, and composite visitors reuse:
//!
//! - `inherited_style_context_from_ancestors` seeds a subtree root by climbing
//!   to the nearest ancestor element and reading its three inherited computed
//!   groups (crossing layer-root boundaries exactly like the legacy ancestor
//!   climb it replaces).
//! - `inherited_context_from_node` derives a node's own carried context from
//!   its projected groups during a DFS; text nodes (never projected) inherit
//!   the carried context unchanged.
//!
//! Since spine step 3.3d the climb no longer does per-property first-set-wins
//! capture, pending `em`-scale accumulation, or an early-exit completeness
//! check: group-copy inheritance (A5) already carries every inherited value in
//! the element's groups, so the seed is a single group read.
//!
//! The resolved value set (`InheritedStyleContext`) lives in its own file and
//! is re-exported here.

use crate::types::dom_node::DomNode;
use crate::types::{DomError, DomNodeSlotKey, RootDom};

pub(crate) use super::inherited_style_context::InheritedStyleContext;

#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{DomTag, RootDomContext, RootDomContextConfig};

/// Seeds the inherited context for a subtree rooted at `start_key` by climbing
/// to the nearest ancestor ELEMENT and reading its OVERLAY-MERGED inherited
/// groups (`computed_*`) — CSS Cascade §6.1: the transition tier wins the
/// cascade, so a mid-flight interpolated value on the ancestor is the cascaded
/// value and inherits (spine 3.3e). When no ancestor element exists, the seed
/// is the no-parent `ComputedGroups::initial()`.
pub(crate) fn inherited_style_context_from_ancestors(
    root_dom: &RootDom,
    start_key: DomNodeSlotKey,
) -> Result<InheritedStyleContext, DomError> {
    let start = root_dom.node(start_key).ok_or(DomError::InvalidNodeKey)?;
    let mut cursor = start.parent_key();
    while let Some(key) = cursor {
        let ancestor = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
        if matches!(ancestor, DomNode::Element(_)) {
            return Ok(inherited_context_from_merged_groups(ancestor));
        }
        cursor = ancestor.parent_key();
    }
    Ok(InheritedStyleContext::default())
}

/// Derives one node's own carried context from its OVERLAY-MERGED groups.
///
/// Elements and document-like nodes read their own merged groups (projected
/// earlier in the pass, transition-overlay merged). Text nodes are never
/// projected — their groups stay `initial()` — so they inherit the carried
/// context unchanged, exactly as the legacy derivation returned the prior
/// context when a text node had no attrs.
#[must_use]
pub(crate) fn inherited_context_from_node(
    node: &DomNode,
    prior: &InheritedStyleContext,
) -> InheritedStyleContext {
    if matches!(node, DomNode::Text(_)) {
        return *prior;
    }
    inherited_context_from_merged_groups(node)
}

/// The three inherited groups read through the `computed_*` accessors (stored
/// group copy merged with the active-transition overlay).
#[must_use]
fn inherited_context_from_merged_groups(node: &DomNode) -> InheritedStyleContext {
    InheritedStyleContext {
        font: node.computed_font(),
        text: node.computed_text(),
        color: node.computed_color(),
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn ancestor_seed_reads_the_nearest_element_groups() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, start).expect("append start");
        root.style_mut(far)
            .set("font-size", "12px")
            .expect("far font-size");
        root.style_mut(near)
            .set("font-size", "18px")
            .expect("near font-size");

        // Resolve the ancestors' groups before the climb reads them.
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");
        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");
        // The nearest ancestor element (near, 18px) wins over far (12px).
        assert_eq!(inherited.effective_font_size_px(), 18.0);
    }

    #[test]
    fn ancestor_seed_defaults_when_no_element_ancestor_exists() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, start).expect("append start");

        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");
        assert_eq!(inherited.effective_font_size_px(), 14.0);
    }

    #[test]
    fn ancestor_seed_reads_the_inherited_color_group() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let start = root.create_element(DomTag::Div).expect("start");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, start).expect("append start");
        root.style_mut(parent)
            .set("color", "#abcdef")
            .expect("color");

        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");
        let inherited = inherited_style_context_from_ancestors(&root, start).expect("climb");
        assert_eq!(
            inherited.effective_color(),
            crate::types::parse_css_color_rgba("#abcdef").expect("proof color should parse")
        );
    }

    #[test]
    fn node_derivation_reads_element_groups_and_passes_text_through() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let text = root.create_text_node("inherit me").expect("text");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, text).expect("append text");
        root.style_mut(parent)
            .set("font-size", "22px")
            .expect("parent font-size");

        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");
        let seed = inherited_style_context_from_ancestors(&root, parent).expect("climb");
        let parent_node = root.node(parent).expect("parent node");
        let parent_context = inherited_context_from_node(parent_node, &seed);
        assert_eq!(parent_context.effective_font_size_px(), 22.0);

        // A text node is never projected: its derivation returns the prior
        // (inherited) context unchanged.
        let text_node = root.node(text).expect("text node");
        let text_context = inherited_context_from_node(text_node, &parent_context);
        assert_eq!(text_context.effective_font_size_px(), 22.0);
    }
}
