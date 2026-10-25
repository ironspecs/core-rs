//! The unified fragment-import splice walk (PR 5, E1 phase 2).
//!
//! What this file represents:
//!
//! - ONE splice algorithm that imports a detached `RootDomFragment`'s root
//!   children under a destination parent, in DOM order, into either a live
//!   document's node store or a fragment-owned node store
//! - the `SpliceJournal`: the caller-owned, fixed-capacity struct that carries
//!   the live-vs-fragment differences as data (born-dirty marks, per-node
//!   metric counts) so the walk itself stays one algorithm
//!
//! Ownership boundary:
//!
//! - the walk is generic over `NodeStore`; it never reads `RootDom` fields
//!   beyond the destination store and id lookup, so live and fragment
//!   destinations share one implementation
//! - the walk performs the structural work (build → intrinsic update →
//!   born-dirty-via-journal → insert → id-index → append) and fills the
//!   journal; the CALLER translates the journal into metric increments. The
//!   walk never emits metrics itself (see `root_dom_metrics.rs`: metrics are
//!   not passed down to lower-level functions)
//!
//! Invariants and rules:
//!
//! - the walk is an ITERATIVE VISITOR with an explicit work stack — no
//!   recursion, per the arc's visitor-not-batch / no-free-recursion doctrine
//!   (D1(a′)): a fragment nested arbitrarily deep imports with bounded state,
//!   never touching the call stack beyond one frame per walk
//! - duplicate element ids: the up-front validation pass catches a fragment
//!   id that collides with a DESTINATION id, before any insert; a duplicate
//!   INTERNAL to the fragment is not in the destination lookup yet, so it
//!   fails mid-import at the per-node assert (`insert_spliced_node`) —
//!   unreachable via the authoring API, which enforces fragment-local id
//!   uniqueness
//! - the fragment root itself is never imported; only its reachable children
//!   are, in document order
//! - unreachable fragment nodes are skipped (only nodes reachable from the
//!   fragment root's child chain are walked)
//! - `Document`/`DocumentFragment` nodes inside the fragment are rejected
//!   (`InvalidParentTarget`)
//! - the journal counters are plain `u64` fields — zero allocation, no
//!   per-event buffers; the walk only increments them
//! - metric emission is grouped by type at the boundary
//!   (`emit_splice_journal_metrics`), a deliberate ordering change from the
//!   pre-unification live walk's per-node interleave (counts are pinned; only
//!   the fill order changed)
//!
//! Extraction note (PR 5, E1 phase 2; corrected 2026-10-25): this replaces
//! two near-duplicate walks — `dom_ops.rs::append_fragment_children_into_owner`
//! (fragment → fragment, recursive) and
//! `root_dom.rs::import_fragment_children_into_parent` (fragment → live,
//! iterative with an `imported_key_by_fragment_key` map). The first
//! unification aligned on the recursive shape and dropped the map — the WRONG
//! direction (D1(a′)): recursion moved the live walk's explicit bounded state
//! onto an unbounded call stack. The walk now aligns toward the better memory
//! shape: iterative, with the destination parent carried on each work-stack
//! entry (so the map stays gone), never on the call stack.
use std::collections::HashMap;

use crate::types::DomError;
use crate::types::dom_node::{DomIntrinsic, DomNode, DomNodeSlotKey};
use crate::types::element_behaviors::DomElementIntrinsicKind;
use crate::types::id_index::assert_id_is_available;
use crate::types::node_store::NodeStore;
use crate::types::root_dom_fragment::RootDomFragment;
use crate::types::tree_links::mark_node_born_dirty;

#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use crate::types::raw_node_store::RawNodeStore;
#[cfg(test)]
use crate::types::root_dom_nodes::{RootDomNodes, RootDomNodesMetricName};
#[cfg(test)]
use crate::types::{DomAttribute, DomAttributeValue};
#[cfg(test)]
use slotmap::SlotMap;

/// The live-vs-fragment divergence journal for one splice.
///
/// The walk fills the counters as it imports nodes; the caller translates them
/// into metric increments. `born_dirty` marks the live document's per-node
/// born-dirty behavior (the fragment store never marks dirty — it never runs
/// layout/paint). All fields are fixed and zero-allocation.
#[derive(Default)]
pub(crate) struct SpliceJournal {
    /// Mark every imported node born-dirty before insert (live document only).
    pub(crate) born_dirty: bool,
    /// Imported node count (one `NodeSlotAllocation` per node).
    pub(crate) node_slot_allocations: u64,
    /// Imported text-node count (one `TextStorageAllocation` per text node).
    pub(crate) text_storage_allocations: u64,
    /// Nodes whose `after_create_intrinsic_update` measured text
    /// (`TextIntrinsicMeasurement`).
    pub(crate) text_intrinsic_measurements: u64,
    /// Nodes whose intrinsic recomputed an image (`ImgIntrinsicRecomputation`).
    pub(crate) img_intrinsic_recomputations: u64,
    /// Nodes whose intrinsic updated image state (`ImgIntrinsicStateUpdate`).
    pub(crate) img_intrinsic_state_updates: u64,
}

/// Splices one fragment's root children under `parent_key` in the destination
/// store, returning the imported top-level child keys in DOM order.
pub(crate) fn splice_fragment_children_into_store<S: NodeStore>(
    owner_root: DomNodeSlotKey,
    store: &mut S,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    parent_key: DomNodeSlotKey,
    fragment: &RootDomFragment,
    journal: &mut SpliceJournal,
) -> Result<Vec<DomNodeSlotKey>, DomError> {
    assert_splice_parent_is_valid(store, parent_key)?;
    let top_level_child_keys = fragment.top_level_child_keys();
    assert_fragment_ids_are_available(id_lookup, fragment, &top_level_child_keys)?;
    let mut imported_keys = Vec::new();
    for child_key in top_level_child_keys {
        let imported_key = splice_fragment_subtree_into_store(
            owner_root, store, id_lookup, parent_key, fragment, child_key, journal,
        )?;
        imported_keys.push(imported_key);
    }
    Ok(imported_keys)
}

fn assert_splice_parent_is_valid<S: NodeStore>(
    store: &S,
    parent_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let parent = store.get(parent_key).ok_or(DomError::InvalidNodeKey)?;
    if !parent.is_parent_node() {
        return Err(DomError::InvalidParentTarget);
    }
    Ok(())
}

/// Validates that every element id reachable from the fragment's top-level
/// children is available in the DESTINATION id lookup, failing the import
/// before any node is written. This pass catches destination conflicts only:
/// a duplicate id internal to the fragment is not in the destination lookup
/// yet, so it would instead fail mid-import at `insert_spliced_node`'s
/// per-node assert — unreachable via the authoring API, which enforces
/// fragment-local id uniqueness.
fn assert_fragment_ids_are_available(
    id_lookup: &HashMap<String, DomNodeSlotKey>,
    fragment: &RootDomFragment,
    top_level_child_keys: &[DomNodeSlotKey],
) -> Result<(), DomError> {
    for &child_key in top_level_child_keys {
        assert_subtree_ids_available(id_lookup, fragment, child_key)?;
    }
    Ok(())
}

fn assert_subtree_ids_available(
    id_lookup: &HashMap<String, DomNodeSlotKey>,
    fragment: &RootDomFragment,
    key: DomNodeSlotKey,
) -> Result<(), DomError> {
    // Iterative pre-order visitor over an explicit key stack: children are
    // pushed in reverse sibling order so they pop back in DOM order, matching
    // the recursive walk's depth-first visit exactly (so the first duplicate
    // id is reported from the same node as before).
    let mut stack: Vec<DomNodeSlotKey> = vec![key];
    while let Some(current_key) = stack.pop() {
        let node = fragment.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        assert_id_is_available(
            id_lookup,
            node.as_element()
                .and_then(|element| element.attrs.id.as_deref()),
        )?;
        visit_children_in_reverse_sibling_order(fragment, current_key, |child_key| {
            stack.push(child_key);
        });
    }
    Ok(())
}

fn splice_fragment_subtree_into_store<S: NodeStore>(
    owner_root: DomNodeSlotKey,
    store: &mut S,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    parent_key: DomNodeSlotKey,
    fragment: &RootDomFragment,
    fragment_key: DomNodeSlotKey,
    journal: &mut SpliceJournal,
) -> Result<DomNodeSlotKey, DomError> {
    // Iterative pre-order visitor over an explicit work stack. Each entry is
    // `(source_fragment_key, destination_parent_key)`: the destination parent
    // carries the source→imported correspondence on the stack, so no
    // `imported_key_by_fragment_key` map is needed. Children are pushed in
    // reverse sibling order so they pop back in DOM order, preserving the
    // pre-order import sequence the journal and insertion order observe.
    let mut stack: Vec<(DomNodeSlotKey, DomNodeSlotKey)> = Vec::new();
    stack.push((fragment_key, parent_key));
    let mut imported_root_key: Option<DomNodeSlotKey> = None;

    while let Some((source_key, destination_key)) = stack.pop() {
        let fragment_node = fragment.node(source_key).ok_or(DomError::InvalidNodeKey)?;
        let imported_key = match fragment_node {
            DomNode::Document(_) | DomNode::DocumentFragment(_) => {
                return Err(DomError::InvalidParentTarget);
            }
            DomNode::Element(_) => insert_spliced_node(
                store,
                id_lookup,
                fragment_node.clone_for_owner_document(owner_root),
                journal,
            )?,
            DomNode::Text(text_node) => insert_spliced_node(
                store,
                id_lookup,
                DomNode::text(owner_root, text_node.data().into()),
                journal,
            )?,
        };
        if imported_root_key.is_none() {
            imported_root_key = Some(imported_key);
        }
        store.append_child_to_parent(destination_key, imported_key)?;
        visit_children_in_reverse_sibling_order(fragment, source_key, |child_key| {
            stack.push((child_key, imported_key));
        });
    }

    // The walk is seeded with the subtree root, so it is always the first
    // entry popped and the first key created.
    Ok(imported_root_key.expect("the splice walk is seeded with the subtree root"))
}

/// Visits a fragment node's children in reverse sibling order (last child
/// first), so a caller can push them onto a work stack and pop them back in
/// DOM order. Walking the doubly-linked sibling chain backward avoids a
/// per-node buffer.
fn visit_children_in_reverse_sibling_order(
    fragment: &RootDomFragment,
    key: DomNodeSlotKey,
    mut visit: impl FnMut(DomNodeSlotKey),
) {
    let Some(node) = fragment.node(key) else {
        return;
    };
    let mut child_key = node
        .last_child(|child_key| fragment.node(child_key))
        .map(|(child_key, _)| child_key);
    while let Some(current_child_key) = child_key {
        visit(current_child_key);
        child_key = fragment
            .node(current_child_key)
            .and_then(|child| child.previous_sibling(|sibling_key| fragment.node(sibling_key)))
            .map(|(sibling_key, _)| sibling_key);
    }
}

fn insert_spliced_node<S: NodeStore>(
    store: &mut S,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    mut node: DomNode,
    journal: &mut SpliceJournal,
) -> Result<DomNodeSlotKey, DomError> {
    assert_id_is_available(
        id_lookup,
        node.as_element()
            .and_then(|element| element.attrs.id.as_deref()),
    )?;
    node.after_create_intrinsic_update()?;
    record_journal_intrinsic_metrics(&node, journal);
    let is_text_node = matches!(node, DomNode::Text(_));
    if journal.born_dirty {
        mark_node_born_dirty(&mut node);
    }
    let key = store.insert(node);
    journal.node_slot_allocations += 1;
    if is_text_node {
        journal.text_storage_allocations += 1;
    }
    index_spliced_node_id_if_present(store, id_lookup, key);
    Ok(key)
}

/// Records the per-node intrinsic work into the journal counters, mirroring
/// `RootDom::record_intrinsic_metrics_for_node` without emitting metrics
/// directly (metrics are the caller's translation of the journal).
fn record_journal_intrinsic_metrics(node: &DomNode, journal: &mut SpliceJournal) {
    match node.intrinsic() {
        DomIntrinsic::Text(_) => journal.text_intrinsic_measurements += 1,
        DomIntrinsic::Img(_) => {
            journal.img_intrinsic_recomputations += 1;
            journal.img_intrinsic_state_updates += 1;
        }
        DomIntrinsic::NotMeasured
            if matches!(
                node,
                DomNode::Element(element)
                    if element.behavior().intrinsic_kind == DomElementIntrinsicKind::Img
            ) =>
        {
            journal.img_intrinsic_state_updates += 1;
        }
        _ => {}
    }
}

fn index_spliced_node_id_if_present<S: NodeStore>(
    store: &S,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    key: DomNodeSlotKey,
) {
    let Some(id) = store
        .get(key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attrs.id.clone())
    else {
        return;
    };
    id_lookup.insert(id, key);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a source fragment: one `div#subject` element with one text child.
    fn source_fragment_with_id_and_text() -> RootDomFragment {
        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let parent = source.create_element(DomTag::Div).expect("parent");
        let child = source.create_text_node("hello").expect("child");
        source
            .set_attr(
                parent,
                DomAttribute::Id,
                Some(DomAttributeValue::String("subject".to_string())),
            )
            .expect("parent id");
        source
            .append_child(source_root, parent)
            .expect("parent attach");
        source.append_child(parent, child).expect("child attach");
        source
    }

    #[test]
    fn raw_splice_imports_structure_and_ids_without_born_dirty() {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let mut id_lookup = HashMap::new();
        let fragment = source_fragment_with_id_and_text();

        let mut store = RawNodeStore(&mut nodes);
        let mut journal = SpliceJournal::default();
        let imported = splice_fragment_children_into_store(
            root,
            &mut store,
            &mut id_lookup,
            root,
            &fragment,
            &mut journal,
        )
        .expect("splice should succeed");

        // Structure: one top-level element with one text child.
        assert_eq!(imported.len(), 1);
        let parent_key = imported[0];
        let child_key = nodes
            .get(parent_key)
            .and_then(DomNode::first_child_key_for_store)
            .expect("imported child");
        assert!(matches!(nodes.get(child_key), Some(DomNode::Text(_))));

        // The element id is indexed into the destination lookup.
        assert_eq!(id_lookup.get("subject"), Some(&parent_key));

        // A raw (fragment) splice records no born-dirty intent, but still
        // counts its per-node work in the journal for the caller to translate.
        assert!(!journal.born_dirty);
        assert_eq!(journal.node_slot_allocations, 2);
        assert_eq!(journal.text_storage_allocations, 1);
        assert_eq!(journal.text_intrinsic_measurements, 1);
    }

    #[test]
    fn live_splice_marks_born_dirty_indexes_ids_and_counts_node_store_metrics() {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let mut id_lookup = HashMap::new();
        let fragment = source_fragment_with_id_and_text();

        let mut journal = SpliceJournal {
            born_dirty: true,
            ..SpliceJournal::default()
        };
        let imported = splice_fragment_children_into_store(
            root,
            &mut nodes,
            &mut id_lookup,
            root,
            &fragment,
            &mut journal,
        )
        .expect("splice should succeed");

        assert_eq!(imported.len(), 1);
        let parent_key = imported[0];
        assert_eq!(id_lookup.get("subject"), Some(&parent_key));

        // The live splice routes node-store access through the instrumented
        // proxy: one Insert + one Get per imported node (2 nodes here), and
        // the journal records the born-dirty intent the live walk carries.
        assert_eq!(nodes.count(RootDomNodesMetricName::Insert), 2);
        assert!(nodes.count(RootDomNodesMetricName::Get) >= 2);
        assert!(journal.born_dirty);
        assert_eq!(journal.node_slot_allocations, 2);
        assert_eq!(journal.text_storage_allocations, 1);
    }

    #[test]
    fn splice_rejects_duplicate_ids_before_partial_import() {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let mut id_lookup = HashMap::new();
        // A destination element already owns "dup".
        let existing = nodes.insert(DomNode::element(root, DomTag::Span));
        id_lookup.insert("dup".to_string(), existing);

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let imported = source.create_element(DomTag::Button).expect("imported");
        source
            .set_attr(
                imported,
                DomAttribute::Id,
                Some(DomAttributeValue::String("dup".to_string())),
            )
            .expect("id");
        source.append_child(source_root, imported).expect("attach");

        let mut store = RawNodeStore(&mut nodes);
        let mut journal = SpliceJournal::default();
        let error = splice_fragment_children_into_store(
            root,
            &mut store,
            &mut id_lookup,
            root,
            &source,
            &mut journal,
        )
        .expect_err("duplicate id should fail");

        assert_eq!(error, DomError::DuplicateElementId);
        // No partial import: the destination store gained nothing.
        assert_eq!(nodes.len(), 2);
        assert_eq!(journal.node_slot_allocations, 0);
    }

    /// A fragment nested 10,000 elements deep imports through the live path
    /// without recursing: the iterative work-stack walk keeps the state on an
    /// explicit bounded stack, so no call-stack overflow occurs (the pre-fix
    /// recursive walk would overflow at this depth). The imported chain must be
    /// exactly as deep as authored, with no cycle and no truncation.
    #[test]
    fn splice_imports_a_pathologically_deep_fragment_without_recursion() {
        const DEPTH: u64 = 10_000;

        // Build a single chain `DEPTH` levels deep: level 0 under the fragment
        // root, level N under level N-1. Tiny plain divs, no styles.
        let mut fragment = RootDomFragment::new();
        let fragment_root = fragment.document_fragment_key();
        let top = fragment.create_element(DomTag::Div).expect("top level");
        fragment
            .append_child(fragment_root, top)
            .expect("top level attach");
        let mut parent = top;
        for _ in 1..DEPTH {
            let child = fragment.create_element(DomTag::Div).expect("level node");
            fragment.append_child(parent, child).expect("level attach");
            parent = child;
        }

        // Import through the live path: the instrumented node store with the
        // born-dirty journal flag set.
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let mut id_lookup = HashMap::new();
        let mut journal = SpliceJournal {
            born_dirty: true,
            ..SpliceJournal::default()
        };

        let imported = splice_fragment_children_into_store(
            root,
            &mut nodes,
            &mut id_lookup,
            root,
            &fragment,
            &mut journal,
        )
        .expect("deep fragment import must succeed without recursion");

        // One top-level chain of DEPTH element nodes.
        assert_eq!(imported.len(), 1);
        assert_eq!(journal.node_slot_allocations, DEPTH);

        // Walk the links down and spot-check the depth: exactly DEPTH, no
        // cycle, no truncation.
        let mut cursor = imported[0];
        let mut depth = 1_u64;
        while let Some(child) = nodes
            .get(cursor)
            .and_then(DomNode::first_child_key_for_store)
        {
            cursor = child;
            depth += 1;
            assert!(
                depth <= DEPTH,
                "imported chain grew past the authored depth"
            );
        }
        assert_eq!(
            depth, DEPTH,
            "the imported chain must be exactly DEPTH deep"
        );
    }
}
