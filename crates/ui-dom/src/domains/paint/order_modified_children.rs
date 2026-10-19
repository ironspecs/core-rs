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
//! - **Run awareness (flex arc step 3a-ii, D8).** `order` applies to flex
//!   ITEMS (§5.4), and a contiguous run of inline-level children is ONE
//!   anonymous item — so a run sequences as ONE unit at `order: 0` and its
//!   members' own `order` values are IGNORED. Block-level items keep their
//!   orders. A run is yielded DOM-order-internally.
//! - **Fast path is the default case.** One pass reads each child's unit order
//!   (a run is `0`; a block item is its computed `order`); when every unit
//!   shares one value (the default all-`0` case included), the order-modified
//!   sequence IS DOM order and is yielded directly.
//! - **Slow path is selection iteration, used only when unit orders differ.**
//!   Repeatedly yield the not-yet-yielded unit with the smallest
//!   `(order, dom-sequence)` pair by scanning ascending distinct unit orders
//!   and walking the DOM list in source order within each value. A run unit
//!   yields all of its members before the walk resumes at the run's breaker.
//!   Stable; no allocation; O(n²) worst case (n distinct orders), bounded to
//!   containers whose units actually differ in `order`.
//! - **Same child set as the paint walk.** The helper yields every child via
//!   `first_child_key_for_store`/`next_sibling_key_for_store`; it reorders but
//!   never filters. Display:none / abs-pos handling stays in the per-node paint
//!   logic downstream, unchanged by this step.

use crate::domains::style::computed::{DisplayKw, PositionKw};
use crate::types::{DomError, DomNode, DomNodeSlotKey, RootDom};

#[cfg(test)]
use crate::types::{DomTag, RootDomContext};

/// Visits the children of `parent_key` in order-modified document order
/// (CSS Flexbox §5.4.1), calling `visit` once per child with the child key and
/// a reborrow of `root`.
///
/// The fast path (all units share one `order`) yields DOM order after one
/// detection pass; the slow path (any unit order differs) uses stable
/// selection iteration. No allocation in either path.
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

    if all_units_share_order(root, first_child)? {
        visit_in_dom_order(root, first_child, &mut visit)?;
    } else {
        visit_in_order_modified_unit_order(root, first_child, &mut visit)?;
    }
    Ok(())
}

/// The D1 run-membership predicate, paint-side: a text node (always inline) or
/// an in-flow `display: inline` element. Mirrors `flex_anonymous_run.rs`
/// `node_is_inline_run_member` over the computed box group.
fn node_is_inline_run_member(node: &DomNode) -> bool {
    if matches!(node, DomNode::Text(_)) {
        return true;
    }
    let box_ = node.computed_box();
    box_.display == DisplayKw::Inline
        && matches!(box_.position, PositionKw::Static | PositionKw::Relative)
}

/// One child's UNIT order: a run member sequences as its run's `order: 0`
/// (its own `order` is ignored — §5.4 applies to flex items, and the anonymous
/// run wrapper is the item); a block-level child keeps its own computed order.
fn unit_order_for(node: &DomNode) -> i32 {
    if node_is_inline_run_member(node) {
        0
    } else {
        node.computed_flex().order
    }
}

/// One pass over the children: `true` when every UNIT carries the same order
/// (runs are `0`; block items their own order). An empty or single-child list
/// is trivially true. A missing child key is internal corruption and surfaces
/// as an error, matching the paint walk's own contract.
fn all_units_share_order(
    root: &RootDom,
    first_child: Option<DomNodeSlotKey>,
) -> Result<bool, DomError> {
    let mut reference_order: Option<i32> = None;
    let mut child_key = first_child;
    while let Some(key) = child_key {
        let (order, sibling) = {
            let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
            (unit_order_for(node), node.next_sibling_key_for_store())
        };
        child_key = sibling;
        match reference_order {
            None => reference_order = Some(order),
            Some(reference) if reference != order => return Ok(false),
            Some(_) => {}
        }
    }
    Ok(true)
}

/// Yields the children in their DOM (source) order. This is the order-modified
/// sequence whenever every unit shares one `order` value (runs stay contiguous
/// in DOM order).
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

/// Yields the units in order-modified document order: ascending distinct unit
/// `order` values, DOM order within each value. A run unit yields its members
/// contiguously (their own orders ignored). Selection iteration — O(n²) worst
/// case, no allocation, stable.
fn visit_in_order_modified_unit_order<F>(
    root: &mut RootDom,
    first_child: Option<DomNodeSlotKey>,
    visit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>,
{
    let Some(mut next_order) = minimum_unit_order(root, first_child)? else {
        return Ok(());
    };
    loop {
        let mut min_greater: Option<i32> = None;
        let mut child_key = first_child;
        while let Some(key) = child_key {
            let (order, is_run_member, sibling) = {
                let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
                (
                    unit_order_for(node),
                    node_is_inline_run_member(node),
                    node.next_sibling_key_for_store(),
                )
            };
            if order == next_order {
                if is_run_member {
                    // Yield the whole run contiguously, then resume at its
                    // breaker (a non-member, whose own order the outer loop
                    // classifies next).
                    child_key = visit_run_and_return_breaker(root, key, visit)?;
                } else {
                    visit(root, key)?;
                    child_key = sibling;
                }
            } else {
                if order > next_order {
                    min_greater = Some(min_greater.map_or(order, |minimum| minimum.min(order)));
                }
                child_key = sibling;
            }
        }
        match min_greater {
            Some(greater) => next_order = greater,
            None => return Ok(()),
        }
    }
}

/// Yields a run's members in DOM order (the run starts at `first_member`) and
/// returns the first non-member sibling after the run (`None` at the end).
fn visit_run_and_return_breaker<F>(
    root: &mut RootDom,
    first_member: DomNodeSlotKey,
    visit: &mut F,
) -> Result<Option<DomNodeSlotKey>, DomError>
where
    F: FnMut(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>,
{
    let mut child_key = Some(first_member);
    while let Some(key) = child_key {
        let sibling = {
            let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
            if !node_is_inline_run_member(node) {
                return Ok(Some(key));
            }
            node.next_sibling_key_for_store()
        };
        visit(root, key)?;
        child_key = sibling;
    }
    Ok(None)
}

/// The smallest unit `order` among the children; `None` for an empty list.
fn minimum_unit_order(
    root: &RootDom,
    first_child: Option<DomNodeSlotKey>,
) -> Result<Option<i32>, DomError> {
    let mut minimum: Option<i32> = None;
    let mut child_key = first_child;
    while let Some(key) = child_key {
        let node = root.node(key).ok_or(DomError::InvalidNodeKey)?;
        let order = unit_order_for(node);
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

    fn append_span(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
        let key = root
            .create_element(DomTag::Span)
            .expect("span should insert");
        root.append_child(parent_key, key)
            .expect("span should append");
        key
    }

    fn append_text(root: &mut RootDom, parent_key: DomNodeSlotKey, text: &str) -> DomNodeSlotKey {
        let key = root.create_text_node(text).expect("text should insert");
        root.append_child(parent_key, key)
            .expect("text should append");
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

    #[test]
    fn span_order_inside_a_run_is_ignored() {
        // D8 pin (i): a span with `order: 1` inside a run of inline children
        // does not reorder — its order applies to the anonymous run wrapper
        // (the item), not to the inline member, so the sequence stays DOM order.
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_div(&mut root, document_key);
        let text_before = append_text(&mut root, parent, "a");
        let span = append_span(&mut root, parent);
        set_order(&mut root, span, 1);
        let text_after = append_text(&mut root, parent, "b");

        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(
            visited,
            vec![text_before, span, text_after],
            "the run members keep DOM order despite the span's order: 1"
        );
    }

    #[test]
    fn run_sequences_before_an_order_one_block() {
        // D8 pin (ii): a run beside an `order: 1` block item sequences the
        // block last (the run is one unit at order 0).
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_div(&mut root, document_key);
        let run_text = append_text(&mut root, parent, "a");
        let block = append_div(&mut root, parent);
        set_order(&mut root, block, 1);

        let visited = collect_visit_order(&mut root, parent);
        assert_eq!(
            visited,
            vec![run_text, block],
            "the order-0 run leads the order-1 block"
        );
    }
}
