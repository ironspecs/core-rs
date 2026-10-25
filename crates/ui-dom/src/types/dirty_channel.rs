//! The dirty channel: retained-dirty ancestor walks over the DOM link shape.
//!
//! What this file represents:
//!
//! - the ancestor walk that sets a retained dirty mark (layout or style) on
//!   every node from a starting key up to and including its layer root, and
//!   stops there — the hot path every attribute change flows through
//! - the whole-subtree born-dirty walk used when a freshly linked subtree is
//!   attached into a live document
//! - the classification policy table that projects the style pass's group-diff
//!   classification back into `AttributeInvalidation` flags
//!
//! Ownership boundary:
//!
//! - these are free functions over the node store; they never read `RootDom`
//!   fields beyond the node store
//!
//! Invariants and rules:
//!
//! - mutation dirtiness never crosses a layer boundary: the walk stops at and
//!   includes the layer root (a later layout pass may still propagate solved
//!   size changes to parent layers, but that is a separate pipeline and must
//!   not be inferred from retained dirty bits)
//! - the layout mark is monotonic: if the walk reaches an already-layout-dirty
//!   ancestor it stops, because an earlier mark already propagated layout to
//!   the layer root
//! - the style mark is not monotonic in the same way: the style walk always
//!   reaches the layer root, because the enclosing layer root's style gate
//!   must be armed regardless of intermediate dirty state
//!
//! Extraction note (PR 5, E4) — the two-walk divergence:
//!
//! - `mark_dirty_up_to_layer_root` (live document) and
//!   `mark_layout_dirty_up_to_layer_root_for_store` (fragment/link machinery)
//!   are the SAME walk shape, but they deliberately do NOT merge into one
//!   primitive. The live walk operates on `RootDomNodes` — the metric proxy —
//!   so its node-store `get_mut` traffic stays on the pinned observability
//!   contract (`perf_metric.rs`). The store walk operates on the raw
//!   `SlotMap`, because the fragment store and the link machinery hold no
//!   metric proxy. Back-off doctrine verdict (c): genuine divergence — the
//!   observability boundary is part of the live walk's contract.
//! - `mark_previous_layer_remainder_dirty` / `mark_previous_layer_node_dirty`
//!   STAY in `root_dom.rs`: they are the layer-membership flip walk (start at
//!   parent, skip until crossing the current layer root, stop at the previous
//!   layer root), not the plain dirty channel.
//! - the paint flag deliberately does NOT flow through this walk: paint marks
//!   use the paint-domain damage-rect walk (`domains/paint::mark_dirty`),
//!   which stops at a layout root and carries rect state — genuinely divergent
//!   (back-off doctrine verdict (c), see the `root_dom` trio methods).
//! - this module and `tree_links` reference each other (`current_layer_root_key`
//!   stays in `tree_links`; `append_child_to_parent` calls back into
//!   `mark_layout_dirty_up_to_layer_root_for_store`). The two-module cycle is
//!   benign and reflects the true dependency: append marks dirty, and the
//!   dirty walk reads the layer root.
use slotmap::SlotMap;

use crate::domains::style::computed::GroupDirtyClassification;
use crate::types::dom_node::{AttributeInvalidation, DomNode, DomNodeSlotKey};
use crate::types::root_dom_nodes::RootDomNodes;
use crate::types::tree_links::current_layer_root_key;

#[cfg(test)]
use crate::domains::layout::layer_kind_for_node;
#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use crate::types::tree_links::append_child_to_parent;
#[cfg(test)]
use crate::types::{DomLayerKind, DomPaintState};

/// The retained dirty marks the layer-root ancestor walk can set.
///
/// Layout and style share one walk shape; paint does not (it is a paint-domain
/// damage-rect walk), so it is absent here.
#[derive(Clone, Copy)]
pub(crate) enum DirtyFlag {
    Layout,
    Style,
}

/// Marks `key` and every ancestor up to and including its layer root with
/// `dirty_flag`, then stops, on the live document's metric-instrumented node
/// store.
///
/// The layout mark early-stops at an already-layout-dirty ancestor; the style
/// mark walks to the layer root regardless.
pub(crate) fn mark_dirty_up_to_layer_root(
    nodes: &mut RootDomNodes,
    key: DomNodeSlotKey,
    dirty_flag: DirtyFlag,
) {
    let Some(layer_root_key) = current_layer_root_key(nodes, key) else {
        return;
    };
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let (was_dirty, parent_key) = {
            let Some(node) = nodes.get_mut(current) else {
                return;
            };
            let was_dirty = match dirty_flag {
                DirtyFlag::Layout => node.mark_layout_dirty(),
                DirtyFlag::Style => node.mark_style_dirty(),
            };
            (was_dirty, node.parent_key())
        };
        if current == layer_root_key {
            return;
        }
        if was_dirty && matches!(dirty_flag, DirtyFlag::Layout) {
            return;
        }
        current_key = parent_key;
    }
}

/// Marks `key` and every ancestor up to and including its layer root
/// layout-dirty, on a raw `SlotMap` store (fragment store, link machinery).
///
/// This is the uninstrumented sibling of `mark_dirty_up_to_layer_root` with
/// the layout flag fixed; the live document routes through the metric proxy,
/// the fragment store does not (see the file header's two-walk divergence).
pub(crate) fn mark_layout_dirty_up_to_layer_root_for_store(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) {
    let Some(layer_root_key) = current_layer_root_key(nodes, key) else {
        return;
    };
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let (was_dirty, parent_key) = {
            let Some(node) = nodes.get_mut(current) else {
                return;
            };
            (node.mark_layout_dirty(), node.parent_key())
        };
        if was_dirty || current == layer_root_key {
            return;
        }
        current_key = parent_key;
    }
}

/// Marks every node in the subtree rooted at `subtree_root_key` born dirty
/// (style + layout + full-layer repaint) in reverse-sibling depth-first order,
/// on the live document's metric-instrumented node store.
pub(crate) fn mark_newly_attached_subtree_dirty(
    nodes: &mut RootDomNodes,
    subtree_root_key: DomNodeSlotKey,
) {
    let mut stack = vec![subtree_root_key];
    while let Some(current_key) = stack.pop() {
        let child_keys = {
            let Some(node) = nodes.get_mut(current_key) else {
                continue;
            };
            node.mark_style_dirty();
            node.mark_layout_dirty();
            node.mark_full_layer_repaint();

            let mut child_keys = Vec::new();
            let mut next_child_key = node.first_child_key_for_store();
            while let Some(child_key) = next_child_key {
                child_keys.push(child_key);
                next_child_key = nodes
                    .get(child_key)
                    .and_then(DomNode::next_sibling_key_for_store);
            }
            child_keys
        };
        stack.extend(child_keys.into_iter().rev());
    }
}

/// Projects the A7 dirty classification back into the `AttributeInvalidation`
/// flags the shared upward-invalidation machinery consumes (`remeasure_text`
/// maps to the style flag, which the pass remeasures text under).
///
/// This is the classification policy table: the style pass's group-diff
/// outcome is projected onto the four invalidation flags the dirty channel and
/// the layer-membership machinery understand.
pub(crate) fn attribute_invalidation_from_classification(
    classification: GroupDirtyClassification,
) -> AttributeInvalidation {
    let mut invalidation = AttributeInvalidation::default();
    if classification.layout {
        invalidation = invalidation.with_layout();
    }
    if classification.paint {
        invalidation = invalidation.with_paint();
    }
    if classification.remeasure_text {
        invalidation = invalidation.with_style();
    }
    if classification.reevaluate_layer_membership {
        invalidation = invalidation.with_layer_membership();
    }
    invalidation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (RootDomNodes, DomNodeSlotKey) {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        (nodes, root)
    }

    fn element(nodes: &mut RootDomNodes, root: DomNodeSlotKey) -> DomNodeSlotKey {
        nodes.insert(DomNode::element(root, DomTag::Div))
    }

    fn make_layer_root(nodes: &mut RootDomNodes, key: DomNodeSlotKey) {
        match nodes.get_mut(key).expect("layer root should exist") {
            DomNode::Element(node) => node.base.presentation.computed.box_.isolate = true,
            _ => panic!("layer root fixture should be an element"),
        }
    }

    fn clear_marks(nodes: &mut RootDomNodes, key: DomNodeSlotKey) {
        let node = nodes.get_mut(key).expect("node should exist");
        let base = match node {
            DomNode::Document(node) => &mut node.base,
            DomNode::DocumentFragment(node) => &mut node.base,
            DomNode::Element(node) => &mut node.base,
            DomNode::Text(node) => &mut node.base,
        };
        base.layout.dirty = false;
        base.style.dirty = false;
        base.paint = DomPaintState::None;
    }

    fn layout_dirty(nodes: &RootDomNodes, key: DomNodeSlotKey) -> bool {
        nodes.get(key).expect("node should exist").layout().dirty
    }

    fn style_dirty(nodes: &RootDomNodes, key: DomNodeSlotKey) -> bool {
        nodes
            .get(key)
            .expect("node should exist")
            .style_object()
            .dirty
    }

    fn paint_dirty(nodes: &RootDomNodes, key: DomNodeSlotKey) -> bool {
        nodes
            .get(key)
            .expect("node should exist")
            .paint_repaint_state()
            .is_dirty()
    }

    fn chain(nodes: &mut RootDomNodes, root: DomNodeSlotKey, depth: usize) -> Vec<DomNodeSlotKey> {
        let mut keys = vec![root];
        let mut parent = root;
        for _ in 0..depth {
            let child = element(nodes, root);
            append_child_to_parent(nodes, parent, child).expect("append");
            keys.push(child);
            parent = child;
        }
        keys
    }

    #[test]
    fn marks_layout_chain_up_to_and_including_the_document_layer_root() {
        let (mut nodes, root) = make_store();
        let keys = chain(&mut nodes, root, 3);
        for &key in &keys {
            clear_marks(&mut nodes, key);
        }

        mark_dirty_up_to_layer_root(&mut nodes, *keys.last().expect("leaf"), DirtyFlag::Layout);

        // every ancestor up to and including the document (the layer root) is
        // marked; there is nothing above the document
        for &key in &keys {
            assert!(layout_dirty(&nodes, key), "expected {key:?} layout-dirty");
            assert!(!style_dirty(&nodes, key), "layout walk must not set style");
            assert!(!paint_dirty(&nodes, key), "layout walk must not set paint");
        }
    }

    #[test]
    fn stops_at_explicit_layer_root_without_marking_above_it() {
        let (mut nodes, root) = make_store();
        let outer = element(&mut nodes, root);
        let layer_root = element(&mut nodes, root);
        let child = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, root, outer).expect("outer");
        append_child_to_parent(&mut nodes, outer, layer_root).expect("layer root");
        append_child_to_parent(&mut nodes, layer_root, child).expect("child");
        make_layer_root(&mut nodes, layer_root);
        assert_ne!(
            layer_kind_for_node(nodes.get(layer_root).expect("layer root")),
            DomLayerKind::None
        );
        for key in [root, outer, layer_root, child] {
            clear_marks(&mut nodes, key);
        }

        mark_dirty_up_to_layer_root(&mut nodes, child, DirtyFlag::Layout);

        assert!(layout_dirty(&nodes, child));
        assert!(layout_dirty(&nodes, layer_root));
        assert!(
            !layout_dirty(&nodes, outer),
            "walk must stop at the layer root"
        );
        assert!(
            !layout_dirty(&nodes, root),
            "walk must stop at the layer root"
        );
    }

    #[test]
    fn layout_walk_stops_early_at_an_already_dirty_ancestor() {
        let (mut nodes, root) = make_store();
        let keys = chain(&mut nodes, root, 3);
        for &key in &keys {
            clear_marks(&mut nodes, key);
        }
        let leaf = *keys.last().expect("leaf");
        let parent = keys[keys.len() - 2];
        nodes.get_mut(parent).expect("parent").mark_layout_dirty();

        mark_dirty_up_to_layer_root(&mut nodes, leaf, DirtyFlag::Layout);

        assert!(layout_dirty(&nodes, leaf));
        assert!(layout_dirty(&nodes, parent));
        // the walk stopped at the already-dirty ancestor
        assert!(!layout_dirty(&nodes, root));
    }

    #[test]
    fn style_walk_reaches_the_layer_root_through_already_dirty_ancestors() {
        let (mut nodes, root) = make_store();
        let keys = chain(&mut nodes, root, 3);
        for &key in &keys {
            clear_marks(&mut nodes, key);
        }
        let leaf = *keys.last().expect("leaf");
        let parent = keys[keys.len() - 2];
        nodes.get_mut(parent).expect("parent").mark_style_dirty();

        mark_dirty_up_to_layer_root(&mut nodes, leaf, DirtyFlag::Style);

        for &key in &keys {
            assert!(style_dirty(&nodes, key), "expected {key:?} style-dirty");
            assert!(!layout_dirty(&nodes, key), "style walk must not set layout");
        }
    }

    #[test]
    fn store_walk_marks_layout_only_up_to_the_layer_root() {
        let (mut nodes, root) = make_store();
        let keys = chain(&mut nodes, root, 3);
        for &key in &keys {
            clear_marks(&mut nodes, key);
        }

        mark_layout_dirty_up_to_layer_root_for_store(&mut nodes, *keys.last().expect("leaf"));

        for &key in &keys {
            assert!(layout_dirty(&nodes, key), "expected {key:?} layout-dirty");
            assert!(!style_dirty(&nodes, key), "store walk must not set style");
            assert!(!paint_dirty(&nodes, key), "store walk must not set paint");
        }
    }

    #[test]
    fn marks_the_whole_subtree_born_dirty_and_nothing_else() {
        let (mut nodes, root) = make_store();
        let subtree = element(&mut nodes, root);
        let child = element(&mut nodes, root);
        let grandchild = element(&mut nodes, root);
        let outsider = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, root, subtree).expect("subtree");
        append_child_to_parent(&mut nodes, subtree, child).expect("child");
        append_child_to_parent(&mut nodes, child, grandchild).expect("grandchild");
        append_child_to_parent(&mut nodes, root, outsider).expect("outsider");
        for key in [root, subtree, child, grandchild, outsider] {
            clear_marks(&mut nodes, key);
        }

        mark_newly_attached_subtree_dirty(&mut nodes, subtree);

        for key in [subtree, child, grandchild] {
            assert!(style_dirty(&nodes, key), "expected {key:?} style-dirty");
            assert!(layout_dirty(&nodes, key), "expected {key:?} layout-dirty");
            assert!(paint_dirty(&nodes, key), "expected {key:?} paint-dirty");
        }
        assert!(!style_dirty(&nodes, root));
        assert!(!layout_dirty(&nodes, root));
        assert!(!paint_dirty(&nodes, root));
        assert!(!style_dirty(&nodes, outsider));
        assert!(!layout_dirty(&nodes, outsider));
        assert!(!paint_dirty(&nodes, outsider));
    }

    #[test]
    fn classification_projects_each_flag_into_its_invalidation_flag() {
        let layout = attribute_invalidation_from_classification(GroupDirtyClassification {
            layout: true,
            ..GroupDirtyClassification::NONE
        });
        assert!(layout.layout());
        assert!(!layout.paint());
        assert!(!layout.style());
        assert!(!layout.layer_membership());

        let paint = attribute_invalidation_from_classification(GroupDirtyClassification {
            paint: true,
            ..GroupDirtyClassification::NONE
        });
        assert!(paint.paint());
        assert!(!paint.layout());
        assert!(!paint.style());
        assert!(!paint.layer_membership());

        let remeasure_text = attribute_invalidation_from_classification(GroupDirtyClassification {
            remeasure_text: true,
            ..GroupDirtyClassification::NONE
        });
        assert!(remeasure_text.style(), "remeasure_text projects to style");
        assert!(!remeasure_text.layout());
        assert!(!remeasure_text.paint());
        assert!(!remeasure_text.layer_membership());

        let membership = attribute_invalidation_from_classification(GroupDirtyClassification {
            reevaluate_layer_membership: true,
            ..GroupDirtyClassification::NONE
        });
        assert!(membership.layer_membership());
        assert!(!membership.layout());
        assert!(!membership.paint());
        assert!(!membership.style());
    }

    #[test]
    fn classification_never_marks_an_unset_flag() {
        let invalidation = attribute_invalidation_from_classification(GroupDirtyClassification {
            layout: true,
            paint: true,
            remeasure_text: true,
            reevaluate_layer_membership: true,
        });
        assert!(invalidation.layout());
        assert!(invalidation.paint());
        assert!(invalidation.style());
        assert!(invalidation.layer_membership());

        let none = attribute_invalidation_from_classification(GroupDirtyClassification::NONE);
        assert!(!none.layout());
        assert!(!none.paint());
        assert!(!none.style());
        assert!(!none.layer_membership());
    }
}
