//! Order-modified child iteration for the paint walk (flex arc step 2).
//!
//! This file owns `visit_order_modified_children`: the paint-side derivation of
//! CSS Flexbox §5.4.1 order-modified document order for one node's children —
//! ascending computed `order`, then DOM (source) order for ties.
//!
//! Invariants and rules:
//!
//! - **Paint derives the sequence itself, allocation-free.** The order lane
//!   (`flex_order_lane`, flex step 1) is update-scoped and reset at
//!   `RootDom::update`; a paint-dirty-only update (e.g. a color change) has no
//!   layout pass, so the lane is EMPTY and paint cannot read it. Both layout
//!   and paint therefore derive from the same `(order, dom-sequence)` rule
//!   independently; this file is paint's side of that rule.
//! - **Fast path is the default case.** One pass reads each child's
//!   `computed_flex().order`; when every child shares one order value (the
//!   default 0 case included), the order-modified sequence IS DOM order and is
//!   yielded directly — the only extra store reads are the per-child order
//!   reads on nodes the walk already fetches.
//! - **Slow path is selection iteration, used only when orders differ.**
//!   Repeatedly yield the not-yet-yielded child with the smallest
//!   `(order, dom-sequence)` pair by scanning ascending distinct order values
//!   and walking the DOM list in source order within each value. Stable; no
//!   allocation; O(n²) worst case (n distinct orders), bounded to containers
//!   whose children actually differ in `order`.
//! - **Same child set as the paint walk.** The helper yields every child via
//!   `first_child_key_for_store`/`next_sibling_key_for_store`; it reorders but
//!   never filters. Display:none / abs-pos handling stays in the per-node paint
//!   logic downstream, unchanged by this step.

use crate::types::{DomError, DomNodeSlotKey, RootDom};

#[cfg(test)]
use crate::types::{DomNode, DomTag, RootDomContext};

/// Visits the children of `parent_key` in order-modified document order
/// (CSS Flexbox §5.4.1), calling `visit` once per child with the child key and
/// a reborrow of `root`.
///
/// The fast path (all children share one `order`) yields DOM order after one
/// detection pass; the slow path (any order differs) uses stable selection
/// iteration. No allocation in either path.
pub(crate) fn visit_order_modified_children<F>(
    root: &mut RootDom,
    parent_key: DomNodeSlotKey,
    mut visit: F,
) -> Result<(), DomError>
where
    F: FnMut(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>,
{
    let first_child = root
        .node(parent_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();

    if all_children_share_order(root, first_child)? {
        visit_in_dom_order(root, first_child, &mut visit)?;
    } else {
        visit_in_order_modified_order(root, first_child, &mut visit)?;
    }
    Ok(())
}

/// One pass over the children: `true` when every child carries the same
/// computed `order` (an empty or single-child list is trivially true). A
/// missing child key is internal corruption and surfaces as an error, matching
/// the paint walk's own contract.
fn all_children_share_order(
    root: &RootDom,
    first_child: Option<DomNodeSlotKey>,
) -> Result<bool, DomError> {
    let mut reference_order: Option<i32> = None;
    let mut child_key = first_child;
    while let Some(key) = child_key {
        let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
        let order = node.computed_flex().order;
        child_key = node.next_sibling_key_for_store();
        match reference_order {
            None => reference_order = Some(order),
            Some(reference) if reference != order => return Ok(false),
            Some(_) => {}
        }
    }
    Ok(true)
}

/// Yields the children in their DOM (source) order. This is the order-modified
/// sequence whenever every child shares one `order` value.
fn visit_in_dom_order<F>(
    root: &mut RootDom,
    first_child: Option<DomNodeSlotKey>,
    visit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>,
{
    let mut child_key = first_child;
    while let Some(key) = child_key {
        let sibling = {
            let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
            node.next_sibling_key_for_store()
        };
        visit(root, key)?;
        child_key = sibling;
    }
    Ok(())
}

/// Yields the children in order-modified document order: ascending distinct
/// `order` values, DOM order within each value. Selection iteration — O(n²)
/// worst case, no allocation, stable.
fn visit_in_order_modified_order<F>(
    root: &mut RootDom,
    first_child: Option<DomNodeSlotKey>,
    visit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>,
{
    let Some(mut next_order) = minimum_order(root, first_child)? else {
        return Ok(());
    };
    loop {
        let mut min_greater: Option<i32> = None;
        let mut child_key = first_child;
        while let Some(key) = child_key {
            let (order, sibling) = {
                let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
                (
                    node.computed_flex().order,
                    node.next_sibling_key_for_store(),
                )
            };
            if order == next_order {
                visit(root, key)?;
            } else if order > next_order {
                min_greater = Some(min_greater.map_or(order, |minimum| minimum.min(order)));
            }
            child_key = sibling;
        }
        match min_greater {
            Some(greater) => next_order = greater,
            None => return Ok(()),
        }
    }
}

/// The smallest computed `order` among the children; `None` for an empty list.
fn minimum_order(
    root: &RootDom,
    first_child: Option<DomNodeSlotKey>,
) -> Result<Option<i32>, DomError> {
    let mut minimum: Option<i32> = None;
    let mut child_key = first_child;
    while let Some(key) = child_key {
        let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
        let order = node.computed_flex().order;
        child_key = node.next_sibling_key_for_store();
        minimum = Some(minimum.map_or(order, |current| current.min(order)));
    }
    Ok(minimum)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the computed `order` field directly, bypassing the style pass:
    /// the helper reads the computed flex group, and a unit test owns the
    /// computed projection (mirrors the `computed_style_ref` direct-write test).
    fn set_order(root: &mut RootDom, key: DomNodeSlotKey, order: i32) {
        let node = root.nodes.get_mut(key).expect("test node should exist");
        match node {
            DomNode::Element(element) => {
                element.base.presentation.computed.flex.order = order;
            }
            _ => panic!("order test helper expects element nodes"),
        }
    }

    fn append_div(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
        let key = root.create_element(DomTag::Div).expect("div should insert");
        root.append_child(parent_key, key)
            .expect("div should append");
        key
    }

    /// Builds a fresh root with `child_count` divs under `parent`; returns
    /// `(root, parent, children)` in DOM order.
    fn parent_with_children(child_count: usize) -> (RootDom, DomNodeSlotKey, Vec<DomNodeSlotKey>) {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_div(&mut root, document_key);
        let mut children = Vec::with_capacity(child_count);
        for _ in 0..child_count {
            children.push(append_div(&mut root, parent));
        }
        (root, parent, children)
    }

    fn collect_visit_order(root: &mut RootDom, parent_key: DomNodeSlotKey) -> Vec<DomNodeSlotKey> {
        let mut keys = Vec::new();
        visit_order_modified_children(root, parent_key, |_root, key| {
            keys.push(key);
            Ok(())
        })
        .expect("visit should succeed");
        keys
    }

    #[test]
    fn equal_orders_yield_dom_order() {
        let (mut root, parent, children) = parent_with_children(3);
        // All children keep the default order 0.
        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(visited, children, "equal orders should keep DOM order");
    }

    #[test]
    fn mixed_orders_yield_order_modified_order() {
        let (mut root, parent, children) = parent_with_children(3);
        set_order(&mut root, children[0], 2);
        set_order(&mut root, children[1], 0);
        set_order(&mut root, children[2], 1);

        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(
            visited,
            vec![children[1], children[2], children[0]],
            "order 0, then 1, then 2"
        );
    }

    #[test]
    fn equal_order_groups_keep_dom_stability() {
        let (mut root, parent, children) = parent_with_children(4);
        set_order(&mut root, children[0], 1);
        set_order(&mut root, children[1], 0);
        set_order(&mut root, children[2], 0);
        set_order(&mut root, children[3], 2);

        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(
            visited,
            vec![children[1], children[2], children[0], children[3]],
            "equal orders keep DOM order within their group"
        );
    }

    #[test]
    fn single_child_yields_that_child() {
        let (mut root, parent, children) = parent_with_children(1);
        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(visited, children, "a single child yields itself");
    }

    #[test]
    fn empty_parent_yields_nothing() {
        let (mut root, parent, _) = parent_with_children(0);
        let visited = collect_visit_order(&mut root, parent);
        assert!(visited.is_empty(), "an empty parent yields no children");
    }
}
