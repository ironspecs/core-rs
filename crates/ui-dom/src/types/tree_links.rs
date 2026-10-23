//! Tree link machinery for the DOM slot stores.
//!
//! What this file represents:
//!
//! - the retained tree memory shape shared by `RootDom` and `RootDomFragment`:
//!   a `SlotMap<DomNodeSlotKey, DomNode>` whose entries carry `{parent,
//!   first_child, last_child, previous_sibling, next_sibling}` links
//! - the fast append/detach/validate/collect primitives over that shape
//! - the ancestor walks that compute the current layer root for a key
//!
//! Invariants and rules:
//!
//! - a node is either fully detached (no parent and no sibling links) or fully
//!   linked; a partial link (sibling edges without a parent) is corruption that
//!   append rejects with `ParentMismatch`
//! - a parented node's edges and its parent's `first_child`/`last_child` chain
//!   agree, and each parented node appears exactly once in that chain
//! - append is a no-op when the child is already the parent's last child
//! - cycles are forbidden: append rejects `parent == child` and any append that
//!   would make a node its own ancestor (`CycleCreationAttempt`)
//! - `collect_subtree_keys` returns every node reachable from a root key
//!   exactly once, in reverse-sibling depth-first order, root first
//!
//! Ownership boundary:
//!
//! - these are free functions over an explicit `&SlotMap`/`&mut SlotMap`; they
//!   never read `RootDom` fields beyond the node store, so fragment and live
//!   documents share one implementation
//!
//! Extraction note (PR 5, E1): `mark_layout_dirty_up_to_layer_root_for_store`
//! is dirty-channel (E4) material, not link machinery — it lives here only
//! because `append_child_to_parent`'s byte-identical body calls it. When E4
//! lands it is a re-home candidate.
use slotmap::SlotMap;

use crate::domains::layout::layer_kind_for_node;
use crate::types::DomError;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};

#[cfg(test)]
use crate::types::dom_tag::DomTag;

pub(crate) fn current_layer_root_key(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let node = nodes.get(current)?;
        if layer_kind_for_node(node) != crate::types::DomLayerKind::None {
            return Some(current);
        }
        current_key = node.parent_key();
    }
    None
}

fn mark_layout_dirty_up_to_layer_root_for_store(
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

pub(crate) fn push_unique_key(keys: &mut Vec<DomNodeSlotKey>, key: DomNodeSlotKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AppendChildPlan {
    current_parent_key: Option<DomNodeSlotKey>,
    current_next_sibling_key: Option<DomNodeSlotKey>,
    previous_last_child: Option<DomNodeSlotKey>,
}

pub(crate) fn append_child_to_parent(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> Result<DomNodeSlotKey, DomError> {
    let append = validate_append_child(nodes, parent_key, child_key)?;
    if append_is_noop(parent_key, append) {
        return Ok(child_key);
    }

    detach_child_for_append(nodes, child_key, append.current_parent_key)?;

    if let Some(last_child_key) = append.previous_last_child {
        let last_child = nodes
            .get_mut(last_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(last_child, Some(child_key));
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if append.previous_last_child.is_none() {
            set_first_child_key(parent, Some(child_key));
        }
        set_last_child_key(parent, Some(child_key));
    }

    {
        let child = nodes.get_mut(child_key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(child, Some(parent_key));
        set_previous_sibling_key(child, append.previous_last_child);
        set_next_sibling_key(child, None);
    }

    if let Some(previous_parent_key) = append.current_parent_key {
        mark_layout_dirty_up_to_layer_root_for_store(nodes, previous_parent_key);
    }
    mark_layout_dirty_up_to_layer_root_for_store(nodes, child_key);
    Ok(child_key)
}

fn validate_append_child(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> Result<AppendChildPlan, DomError> {
    if parent_key == child_key {
        return Err(DomError::CycleCreationAttempt);
    }

    let parent = nodes.get(parent_key).ok_or(DomError::InvalidNodeKey)?;
    let child = nodes.get(child_key).ok_or(DomError::InvalidNodeKey)?;
    if !parent.is_parent_node() {
        return Err(DomError::InvalidParentTarget);
    }
    if parent.owner_document_key() != child.owner_document_key() {
        return Err(DomError::OwnerDocumentMismatch);
    }
    if append_creates_cycle(nodes, parent_key, child_key) {
        return Err(DomError::CycleCreationAttempt);
    }
    if child_is_partially_linked(child) {
        return Err(DomError::ParentMismatch);
    }

    Ok(AppendChildPlan {
        current_parent_key: child.parent_key(),
        current_next_sibling_key: next_sibling_key(child),
        previous_last_child: append_target_last_child(parent, child, child_key),
    })
}

fn append_is_noop(parent_key: DomNodeSlotKey, append: AppendChildPlan) -> bool {
    append.current_parent_key == Some(parent_key) && append.current_next_sibling_key.is_none()
}

fn append_target_last_child(
    parent: &DomNode,
    child: &DomNode,
    child_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    match last_child_key(parent) {
        Some(last_child_key) if last_child_key == child_key => previous_sibling_key(child),
        last_child_key => last_child_key,
    }
}

fn child_is_partially_linked(node: &DomNode) -> bool {
    let linked_edges = usize::from(node.parent_key().is_some())
        + usize::from(previous_sibling_key(node).is_some())
        + usize::from(next_sibling_key(node).is_some());
    linked_edges > 0 && node.parent_key().is_none()
}

fn append_creates_cycle(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> bool {
    let mut current_key = Some(parent_key);
    while let Some(current) = current_key {
        if current == child_key {
            return true;
        }
        current_key = nodes.get(current).and_then(DomNode::parent_key);
    }
    false
}

pub(crate) fn node_is_connected_to_root(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    root_key: DomNodeSlotKey,
    key: DomNodeSlotKey,
) -> bool {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        if current == root_key {
            return true;
        }
        current_key = nodes.get(current).and_then(DomNode::parent_key);
    }
    false
}

fn detach_child_for_append(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    child_key: DomNodeSlotKey,
    current_parent_key: Option<DomNodeSlotKey>,
) -> Result<(), DomError> {
    let Some(parent_key) = current_parent_key else {
        return Ok(());
    };

    let (previous_key, next_key) = {
        let child = nodes.get(child_key).ok_or(DomError::InvalidNodeKey)?;
        (previous_sibling_key(child), next_sibling_key(child))
    };

    if let Some(previous_key) = previous_key {
        let previous = nodes
            .get_mut(previous_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(previous, next_key);
    }
    if let Some(next_key) = next_key {
        let next = nodes.get_mut(next_key).ok_or(DomError::InvalidNodeKey)?;
        set_previous_sibling_key(next, previous_key);
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if first_child_key(parent) == Some(child_key) {
            set_first_child_key(parent, next_key);
        }
        if last_child_key(parent) == Some(child_key) {
            set_last_child_key(parent, previous_key);
        }
    }

    {
        let child = nodes.get_mut(child_key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(child, None);
        set_previous_sibling_key(child, None);
        set_next_sibling_key(child, None);
    }

    Ok(())
}

pub(crate) fn detach_node_from_parent(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let (parent_key, previous_key, next_key) = {
        let node = nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        (
            node.parent_key(),
            previous_sibling_key(node),
            next_sibling_key(node),
        )
    };

    let Some(parent_key) = parent_key else {
        return Ok(None);
    };

    if let Some(previous_key) = previous_key {
        let previous = nodes
            .get_mut(previous_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(previous, next_key);
    }
    if let Some(next_key) = next_key {
        let next = nodes.get_mut(next_key).ok_or(DomError::InvalidNodeKey)?;
        set_previous_sibling_key(next, previous_key);
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if first_child_key(parent) == Some(key) {
            set_first_child_key(parent, next_key);
        }
        if last_child_key(parent) == Some(key) {
            set_last_child_key(parent, previous_key);
        }
    }

    {
        let node = nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(node, None);
        set_previous_sibling_key(node, None);
        set_next_sibling_key(node, None);
    }

    Ok(Some(parent_key))
}

fn first_child_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.first_child_key_for_store()
}

fn last_child_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.last_child_key_for_store()
}

fn previous_sibling_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.previous_sibling_key_for_store()
}

fn next_sibling_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.next_sibling_key_for_store()
}

fn set_parent_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_parent_key_for_store(key);
}

fn set_first_child_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_first_child_key_for_store(key);
}

fn set_last_child_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_last_child_key_for_store(key);
}

fn set_previous_sibling_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_previous_sibling_key_for_store(key);
}

fn set_next_sibling_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_next_sibling_key_for_store(key);
}

pub(crate) fn mark_node_born_dirty(node: &mut DomNode) {
    node.mark_style_dirty();
    node.mark_layout_dirty();
    node.mark_full_layer_repaint();
}

pub(crate) fn collect_subtree_keys(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    root_key: DomNodeSlotKey,
) -> Result<Vec<DomNodeSlotKey>, DomError> {
    let mut stack = vec![root_key];
    let mut removed_keys = Vec::new();

    while let Some(key) = stack.pop() {
        let node = nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let mut child_key = node
            .first_child(|child_key| nodes.get(child_key))
            .map(|(key, _)| key);
        while let Some(current_child_key) = child_key {
            let child = nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child
                .next_sibling(|sibling_key| nodes.get(sibling_key))
                .map(|(key, _)| key);
            stack.push(current_child_key);
        }
        removed_keys.push(key);
    }

    Ok(removed_keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (SlotMap<DomNodeSlotKey, DomNode>, DomNodeSlotKey) {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        (nodes, root)
    }

    fn element(
        nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
        root: DomNodeSlotKey,
    ) -> DomNodeSlotKey {
        nodes.insert(DomNode::element(root, DomTag::Div))
    }

    #[test]
    fn append_child_rejects_self_cycle() {
        let (mut nodes, root) = make_store();
        let key = element(&mut nodes, root);

        assert_eq!(
            append_child_to_parent(&mut nodes, key, key),
            Err(DomError::CycleCreationAttempt)
        );
    }

    #[test]
    fn append_child_rejects_ancestor_cycle() {
        let (mut nodes, root) = make_store();
        let a = element(&mut nodes, root);
        let b = element(&mut nodes, root);

        // root -> a -> b
        append_child_to_parent(&mut nodes, root, a).expect("root -> a");
        append_child_to_parent(&mut nodes, a, b).expect("a -> b");

        // appending the ancestor a under its descendant b must fail
        assert_eq!(
            append_child_to_parent(&mut nodes, b, a),
            Err(DomError::CycleCreationAttempt)
        );
    }

    #[test]
    fn append_child_is_noop_when_already_last_child() {
        let (mut nodes, root) = make_store();
        let a = element(&mut nodes, root);
        let b = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, root, a).expect("root -> a");
        append_child_to_parent(&mut nodes, root, b).expect("root -> b");

        let snapshot = collect_subtree_keys(&nodes, root).expect("collect");

        // appending b (already the last child) again is a no-op and succeeds
        assert_eq!(append_child_to_parent(&mut nodes, root, b), Ok(b));
        assert_eq!(
            collect_subtree_keys(&nodes, root).expect("collect"),
            snapshot
        );
        // a's next sibling is still b, b has no next sibling
        assert_eq!(next_sibling_key(nodes.get(a).expect("a")), Some(b));
        assert_eq!(next_sibling_key(nodes.get(b).expect("b")), None);
    }

    #[test]
    fn append_child_rejects_partially_linked_node() {
        let (mut nodes, root) = make_store();
        let parent = element(&mut nodes, root);
        let child = element(&mut nodes, root);
        let sibling = element(&mut nodes, root);

        // forge a partial link: child has a next sibling but no parent
        set_next_sibling_key(nodes.get_mut(child).expect("child"), Some(sibling));

        assert_eq!(
            append_child_to_parent(&mut nodes, parent, child),
            Err(DomError::ParentMismatch)
        );
    }

    #[test]
    fn detach_node_relinks_siblings_and_clears_links() {
        let (mut nodes, root) = make_store();
        let parent = element(&mut nodes, root);
        let a = element(&mut nodes, root);
        let b = element(&mut nodes, root);
        let c = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, parent, a).expect("a");
        append_child_to_parent(&mut nodes, parent, b).expect("b");
        append_child_to_parent(&mut nodes, parent, c).expect("c");

        // detach the middle child
        assert_eq!(detach_node_from_parent(&mut nodes, b), Ok(Some(parent)));

        // parent chain is now a -> c
        assert_eq!(first_child_key(nodes.get(parent).expect("parent")), Some(a));
        assert_eq!(last_child_key(nodes.get(parent).expect("parent")), Some(c));
        assert_eq!(next_sibling_key(nodes.get(a).expect("a")), Some(c));
        assert_eq!(previous_sibling_key(nodes.get(c).expect("c")), Some(a));
        assert_eq!(next_sibling_key(nodes.get(c).expect("c")), None);

        // the detached node is fully cleared
        let b_node = nodes.get(b).expect("b");
        assert_eq!(b_node.parent_key(), None);
        assert_eq!(previous_sibling_key(b_node), None);
        assert_eq!(next_sibling_key(b_node), None);
    }

    #[test]
    fn detach_node_returns_none_for_already_detached() {
        let (mut nodes, root) = make_store();
        let a = element(&mut nodes, root);
        assert_eq!(detach_node_from_parent(&mut nodes, a), Ok(None));
    }

    #[test]
    fn collect_subtree_visits_every_descendant_exactly_once_root_first() {
        let (mut nodes, root) = make_store();
        let a = element(&mut nodes, root);
        let b = element(&mut nodes, root);
        let a1 = element(&mut nodes, root);
        let a2 = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, root, a).expect("a");
        append_child_to_parent(&mut nodes, root, b).expect("b");
        append_child_to_parent(&mut nodes, a, a1).expect("a1");
        append_child_to_parent(&mut nodes, a, a2).expect("a2");

        let subtree = collect_subtree_keys(&nodes, a).expect("collect");
        assert_eq!(subtree.first(), Some(&a));
        assert_eq!(subtree.len(), 3);
        assert_eq!(subtree, vec![a, a2, a1]);

        let whole = collect_subtree_keys(&nodes, root).expect("collect");
        assert_eq!(whole.len(), 5);
        assert_eq!(whole.first(), Some(&root));
        for key in [root, a, b, a1, a2] {
            assert!(whole.contains(&key), "missing {key:?}");
        }
    }

    #[test]
    fn connectedness_follows_parent_chain_to_root() {
        let (mut nodes, root) = make_store();
        let a = element(&mut nodes, root);
        let b = element(&mut nodes, root);
        let detached = element(&mut nodes, root);
        append_child_to_parent(&mut nodes, root, a).expect("root -> a");
        append_child_to_parent(&mut nodes, a, b).expect("a -> b");

        assert!(node_is_connected_to_root(&nodes, root, a));
        assert!(node_is_connected_to_root(&nodes, root, b));
        assert!(node_is_connected_to_root(&nodes, root, root));
        assert!(!node_is_connected_to_root(&nodes, root, detached));
    }

    #[test]
    fn born_dirty_marks_style_layout_and_paint() {
        let (mut nodes, root) = make_store();
        let key = element(&mut nodes, root);

        mark_node_born_dirty(nodes.get_mut(key).expect("node"));

        // every mark reports it was already dirty, i.e. born-dirty set all three
        assert!(nodes.get_mut(key).expect("node").mark_style_dirty());
        assert!(nodes.get_mut(key).expect("node").mark_layout_dirty());
        assert!(nodes.get_mut(key).expect("node").mark_full_layer_repaint());
    }
}
