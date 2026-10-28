#![allow(dead_code)]

//! Shared `TreeWalker` orchestration for DOM-style layout traversal.
//!
//! What this file contains:
//!
//! - the single shared DFS entrypoints used by `ui-dom` layout
//! - dirty-tree queries for layout and paint scheduling
//! - small public inspection helpers for root keys, stored boxes, and dirty
//!   flags
//! - the current relative-offset bridge consumed by composite and hit testing
//!
//! Invariants and rules:
//!
//! - this file owns traversal orchestration, not style interpretation or box
//!   math
//! - there must be exactly one shared `TreeWalker` implementation in `ui-dom`
//! - this file must not grow a second implicit walker through special-case
//!   recursion or wrapper-specific subtree traversal
//! - the dirty-tree query (`tree_contains_dirty_node`) is an iterative visitor
//!   over a RootDom-owned reused work buffer (D1(a′)); it never recurses, so a
//!   deep tree must not grow the call stack or allocate per scan in steady
//!   state
//! - public helpers here expose stored node state; they must not re-solve DOM
//!   semantics beyond what the layout/composite seam already expects
//! - layer/overflow/position gates and the relative-offset bridge read the
//!   computed groups visit-locally (`DomNode::computed_box` /
//!   `computed_transform`), never the stringly presented attrs
//!
//! Advice and code philosophy:
//!
//! - keep the walker generic and move property meaning into `style.rs`
//! - keep solver math in `flow.rs`, not in traversal helpers
//! - if a helper is only needed inside one pass, keep it private to that pass
//! - prefer one honest seam per concept: tree walk here, property derivation in
//!   `style.rs`, flow solving in `flow.rs`
//!
use crate::domains::style::computed::PositionKw;
use crate::domains::visitor::inherited_style::{
    InheritedStyleContext, inherited_style_context_from_ancestors,
};
use crate::types::primitives::PointPx;
use crate::types::{
    DomBoxModel, DomError, DomLayerKind, DomNode, DomNodeSlotKey, RootDom, RootDomMetricName,
};

use super::DomLayoutResolver;
use super::flow::{LayoutContext, relayout_tree_walker_node};
use super::layout_pass_result::LayoutPassResult;
use super::style::{
    layer_kind_for_node, layout_dirty_from_ref, node_box_from_ref, paint_dirty_from_ref,
    transform_translate_for_node,
};

#[cfg(test)]
use super::style::clear_layout_dirty;
#[cfg(test)]
use crate::domains::layout::IdentityDomLayoutResolver;
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{
    DomAttribute, DomAttributeValue, DomTag, RootDomContext, RootDomNodesMetricName,
};

/// Returns whether the shared `TreeWalker` must run layout for this tree root.
/// This lets composite decide whether one matched DOM tree still needs layout
/// work.
///
/// Example:
///
/// ```ignore
/// if tree_needs_layout(root_dom, tree_root)? {
///     relayout_tree(root_dom, tree_root, (200.0, 120.0), resolver)?;
/// }
/// ```
pub(crate) fn tree_needs_layout(
    root_dom: &mut RootDom,
    tree_root: DomNodeSlotKey,
) -> Result<bool, DomError> {
    tree_contains_dirty_node(root_dom, tree_root, true)
}

/// Returns whether the shared `TreeWalker` must run paint for this tree root.
/// This lets composite skip paint work when a subtree is already paint-clean.
///
/// Example:
///
/// ```ignore
/// if tree_needs_paint(root_dom, tree_root)? {
///     // schedule paint for this tree
/// }
/// ```
#[allow(dead_code)]
pub(crate) fn tree_needs_paint(
    root_dom: &mut RootDom,
    tree_root: DomNodeSlotKey,
) -> Result<bool, DomError> {
    tree_contains_dirty_node(root_dom, tree_root, false)
}

/// Runs the shared `TreeWalker` layout pass for one matched hierarchy tree root.
/// This seeds the root layout context from the viewport-sized paint target and
/// hands traversal to the single shared walker implementation.
///
/// Example:
///
/// ```ignore
/// relayout_tree(root_dom, tree_root, (200.0, 120.0), &mut resolver)?;
/// ```
#[tracing::instrument(
    name = "layout_updated",
    level = "debug",
    skip(root_dom, resolver),
    fields(
        dom_parent = ?root_dom.attached_bevy_parent_entity(),
        tree_root = ?tree_root,
        tree_width_px = tree_size.0,
        tree_height_px = tree_size.1
    )
)]
pub(crate) fn relayout_tree<R>(
    root_dom: &mut RootDom,
    tree_root: DomNodeSlotKey,
    tree_size: (f32, f32),
    resolver: &mut R,
) -> Result<LayoutPassResult, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    let inherited_style = inherited_style_context_from_ancestors(root_dom, tree_root)?;
    relayout_tree_with_inherited(root_dom, tree_root, tree_size, &inherited_style, resolver)
}

pub(crate) fn relayout_tree_with_inherited<R>(
    root_dom: &mut RootDom,
    tree_root: DomNodeSlotKey,
    tree_size: (f32, f32),
    inherited_style: &InheritedStyleContext,
    resolver: &mut R,
) -> Result<LayoutPassResult, DomError>
where
    R: DomLayoutResolver + ?Sized,
{
    root_dom.increment_metric(RootDomMetricName::LayoutUpdated);
    // Layout is the foundational retained pipeline stage. Composite scheduling
    // may revisit a tree root every frame, but once a root is known to need
    // layout this stage must run before paint or final layer arrangement.
    let viewport_width_px = tree_size.0;
    let viewport_height_px = tree_size.1;
    let (available_width_px, available_height_px) =
        available_size_for_tree_root(root_dom, tree_root, viewport_width_px, viewport_height_px)?;
    let root_position = if tree_root == root_dom.root {
        None
    } else {
        let root_box = root_dom.node(tree_root).ok_or(DomError::InvalidNodeKey)?;
        let root_box = node_box_from_ref(root_box).border_box;
        Some(PointPx {
            x_px: root_box.left_px,
            y_px: root_box.top_px,
        })
    };
    let root_context = LayoutContext {
        inherited_style: *inherited_style,
        flow_left_px: 0.0,
        flow_top_px: 0.0,
        root_local_left_px: root_position.map(|offset| offset.x_px),
        root_local_top_px: root_position.map(|offset| offset.y_px),
        available_width_px: Some(available_width_px),
        available_height_px: Some(available_height_px),
        auto_margin_reference_width_px: None,
        node_content_width_override_px: None,
        node_content_height_override_px: None,
        parent_absolute_left_px: 0.0,
        parent_absolute_top_px: 0.0,
        containing_layer_absolute_left_px: 0.0,
        containing_layer_absolute_top_px: 0.0,
        containing_layer_width_px: viewport_width_px,
        containing_layer_height_px: viewport_height_px,
        viewport_width_px,
        viewport_height_px,
        parent_box: None,
        allows_parent_child_block_margin_collapse: false,
        parent_is_flex_container: false,
        parent_is_grid_container: false,
    };
    let node_before_layout = root_dom.node(tree_root).ok_or(DomError::InvalidNodeKey)?;
    let previous_box = node_box_from_ref(node_before_layout).border_box;
    relayout_tree_walker_node(root_dom, tree_size, tree_root, root_context, resolver)?;
    let node_after_layout = root_dom.node(tree_root).ok_or(DomError::InvalidNodeKey)?;
    Ok(propagate_parent_size_change_after_layout(
        node_after_layout,
        previous_box.width_px,
        previous_box.height_px,
        node_box_from_ref(node_after_layout).border_box.width_px,
        node_box_from_ref(node_after_layout).border_box.height_px,
    ))
}

fn available_size_for_tree_root(
    root_dom: &RootDom,
    tree_root: DomNodeSlotKey,
    viewport_width_px: f32,
    viewport_height_px: f32,
) -> Result<(f32, f32), DomError> {
    let node = root_dom.node(tree_root).ok_or(DomError::InvalidNodeKey)?;
    if tree_root_uses_viewport_sized_available_size(node) {
        return Ok((viewport_width_px, viewport_height_px));
    }
    let layout = node.layout();
    let available_width_px = if layout.available_width_px > 0.0 {
        layout.available_width_px
    } else {
        viewport_width_px
    };
    let available_height_px = if layout.available_height_px > 0.0 {
        layout.available_height_px
    } else {
        viewport_height_px
    };
    Ok((available_width_px, available_height_px))
}

fn tree_root_uses_viewport_sized_available_size(node: &DomNode) -> bool {
    node.computed_box().position == PositionKw::Fixed
}

/// Returns the root node key owned by this `RootDom`.
/// This gives callers one stable way to start DOM traversal or test setup.
///
/// Example:
///
/// ```ignore
/// let document_key = root_key(root_dom);
/// ```
#[must_use]
pub fn root_key(root_dom: &RootDom) -> DomNodeSlotKey {
    root_dom.root
}

/// Returns the node-owned minimal box model stored on one DOM node.
/// This exposes finalized `box_model` geometry for tests, paint, and composite.
///
/// Example:
///
/// ```ignore
/// let box_model = node_box(root_dom, key).expect("node should exist");
/// assert!(box_model.border_box.width_px >= 0.0);
/// ```
#[must_use]
pub fn node_box(root_dom: &RootDom, key: DomNodeSlotKey) -> Option<DomBoxModel> {
    Some(node_box_from_ref(root_dom.nodes.get(key)?))
}

/// Returns whether one node is still marked layout-dirty.
/// This exposes stored layout dirtiness without leaking node-kind matching.
///
/// Example:
///
/// ```ignore
/// assert_eq!(node_layout_dirty(root_dom, key), Some(false));
/// ```
#[must_use]
pub fn node_layout_dirty(root_dom: &RootDom, key: DomNodeSlotKey) -> Option<bool> {
    Some(layout_dirty_from_ref(root_dom.nodes.get(key)?))
}

/// Returns whether one node is still marked paint-dirty.
/// This exposes stored paint dirtiness for tests and composite scheduling.
///
/// Example:
///
/// ```ignore
/// if node_paint_dirty(root_dom, key) == Some(true) {
///     // repaint this node
/// }
/// ```
#[must_use]
pub fn node_paint_dirty(root_dom: &RootDom, key: DomNodeSlotKey) -> Option<bool> {
    Some(paint_dirty_from_ref(root_dom.nodes.get(key)?))
}

/// Returns the stored local offset plus transform translation for one node.
/// This bridges the current layout/composite contract without asking composite
/// to re-parse transform attrs.
///
/// Example:
///
/// ```ignore
/// let offset = relative_offset_for_node(root_dom, key);
/// ```
pub(crate) fn relative_offset_for_node(root_dom: &RootDom, key: DomNodeSlotKey) -> PointPx {
    let Some(node) = root_dom.nodes.get(key) else {
        return PointPx::default();
    };
    let border_box = node_box_from_ref(node).border_box;
    let translate = transform_translate_for_node(node);
    PointPx {
        x_px: border_box.left_px + translate.x_px,
        y_px: border_box.top_px + translate.y_px,
    }
}

/// Returns one layer root's offset inside its nearest ancestor layer.
///
/// Non-layer DOM ancestors are intentionally not accumulated here because
/// layout stores the layer root's solved position in the ancestor layer's
/// coordinate space. Composite presentation and layer hit testing both depend
/// on this same contract.
pub(crate) fn layer_offset_from_dom_climb(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<PointPx, DomError> {
    if node_key == root_key(root_dom) {
        return Ok(PointPx::default());
    }
    root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    let mut layer_offset = relative_offset_for_node(root_dom, node_key);
    let mut current_key = root_dom.node(node_key).and_then(DomNode::parent_key);
    while let Some(current_key_value) = current_key {
        let current_node = root_dom
            .node(current_key_value)
            .ok_or(DomError::InvalidNodeKey)?;
        if is_layer_root_key(root_dom, current_key_value) {
            layer_offset.y_px -= root_dom.vertical_scroll_offset_y_px(current_key_value);
            return Ok(layer_offset);
        }
        current_key = current_node.parent_key();
    }
    Ok(layer_offset)
}

fn is_layer_root_key(root_dom: &RootDom, node_key: DomNodeSlotKey) -> bool {
    if node_key == root_key(root_dom) {
        return true;
    }
    root_dom
        .node(node_key)
        .is_some_and(|node| layer_kind_for_node(node) != DomLayerKind::None)
}

/// Returns whether any node in the subtree rooted at `key` is still dirty for
/// the requested pass (layout when `check_layout`, paint otherwise), so
/// composite can decide whether that tree still needs work.
///
/// The walk is an iterative pre-order visitor over the RootDom-owned
/// `dirty_scan_stack` work buffer (D1(a′)), never recursion. Children are
/// pushed in reverse sibling order (last child first, via the
/// `last_child`/`previous_sibling` links) so they pop back in DOM order,
/// preserving the previous recursive walk's depth-first coverage exactly. The
/// predicate is any-dirty — order-insensitive — and the walk still stops at the
/// first dirty node, so the only thing that changed is which dirty node is
/// reported first; the returned `bool` is unchanged.
///
/// The buffer is cleared here and its capacity is kept at the high-water mark,
/// so a warmed document's gate scan allocates nothing (the crate's
/// zero-allocation steady-state rule).
///
/// Layer-root children (and their subtrees) are skipped exactly as before:
/// dirtiness inside a layer subtree is reached by a separate walk rooted at
/// that layer, never from here.
fn tree_contains_dirty_node(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    check_layout: bool,
) -> Result<bool, DomError> {
    // Iterative pre-order visitor over the reused work buffer. Every node is
    // fetched once when popped (dirty check) and once in its parent's
    // reverse-sibling loop (link/layer lookahead), so the node-store access
    // count matches the recursive walk's two-reads-per-child shape.
    let stack = &mut root_dom.dirty_scan_stack;
    stack.clear();
    stack.push(key);
    while let Some(current_key) = stack.pop() {
        let node = root_dom
            .nodes
            .get(current_key)
            .ok_or(DomError::InvalidNodeKey)?;
        if check_layout && layout_dirty_from_ref(node) {
            return Ok(true);
        }
        if !check_layout && paint_dirty_from_ref(node) {
            return Ok(true);
        }

        // Push children in reverse sibling order so they pop back in DOM
        // order, matching the recursive walk's first-to-last descent. Layer
        // roots are skipped before they are pushed, so their subtrees are
        // never walked from here — exactly as the recursion skipped them.
        let mut child_key = node.last_child_key_for_store();
        while let Some(sibling_key) = child_key {
            let child_node = root_dom
                .nodes
                .get(sibling_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child_node.previous_sibling_key_for_store();
            if layer_kind_for_node(child_node) != DomLayerKind::None {
                continue;
            }
            stack.push(sibling_key);
        }
    }

    Ok(false)
}

fn propagate_parent_size_change_after_layout(
    node: &DomNode,
    previous_width_px: f32,
    previous_height_px: f32,
    width_px: f32,
    height_px: f32,
) -> LayoutPassResult {
    let has_parent_layer = node.parent_key().is_some();
    let is_layer = layer_kind_for_node(node) != crate::types::DomLayerKind::None;
    let box_ = node.computed_box();
    let blocks_parent_propagation_from_position =
        matches!(box_.position, PositionKw::Absolute | PositionKw::Fixed);
    let blocks_parent_propagation_from_overflow = box_.overflow.creates_clip();
    let parent_layer_propagation_allowed = has_parent_layer
        && is_layer
        && !blocks_parent_propagation_from_position
        && !blocks_parent_propagation_from_overflow;
    if !parent_layer_propagation_allowed {
        return LayoutPassResult::default();
    }
    LayoutPassResult {
        propagate_width_to_parent_layer: size_change_exceeds_epsilon(previous_width_px, width_px),
        propagate_height_to_parent_layer: size_change_exceeds_epsilon(
            previous_height_px,
            height_px,
        ),
    }
}

fn size_change_exceeds_epsilon(previous_px: f32, next_px: f32) -> bool {
    (previous_px - next_px).abs() > layout_length_epsilon_px()
}

const fn layout_length_epsilon_px() -> f32 {
    0.01
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

    /// Runs the style recompute pass for one layer-root subtree so freshly
    /// written inline declarations resolve into computed groups before layout
    /// reads them. The layer root must match the subtree the following layout
    /// call walks — a node that is itself a layer root (positioned/transformed/
    /// clipped) does not propagate style dirtiness up to the document, so the
    /// recompute must be keyed to that node's own layer root.
    fn apply_style_pass(root: &mut RootDom, layer_root: DomNodeSlotKey) {
        recompute_dirty_style_subtree_for_layer_root(root, layer_root).expect("style pass");
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct NodeStoreAccessSummary {
        get_count: u64,
        get_mut_count: u64,
        history_len: usize,
    }

    fn node_store_history_names(root: &RootDom) -> Vec<RootDomNodesMetricName> {
        root.node_history().map(|entry| entry.name).collect()
    }

    fn assert_solver_only_reads_and_writes_nodes(root: &RootDom) {
        let history_names = node_store_history_names(root);

        assert!(
            !history_names.is_empty(),
            "solver should touch the node store during relayout"
        );
        assert!(
            history_names.iter().all(|name| matches!(
                name,
                RootDomNodesMetricName::Get | RootDomNodesMetricName::GetMut
            )),
            "solver should only traverse via direct get/get_mut node-store access"
        );
        assert_eq!(root.node_count(RootDomNodesMetricName::ContainsKey), 0);
        assert_eq!(root.node_count(RootDomNodesMetricName::Insert), 0);
        assert_eq!(root.node_count(RootDomNodesMetricName::InsertWithKey), 0);
        assert_eq!(root.node_count(RootDomNodesMetricName::IsEmpty), 0);
        assert_eq!(root.node_count(RootDomNodesMetricName::Remove), 0);
        assert_eq!(
            u64::try_from(history_names.len()).expect("history length should fit into u64"),
            root.node_count(RootDomNodesMetricName::Get)
                + root.node_count(RootDomNodesMetricName::GetMut)
        );
    }

    fn node_store_access_summary_after_sibling_layout(
        child_count: usize,
    ) -> NodeStoreAccessSummary {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        root.append_child(document, parent).expect("append parent");
        root.style_mut(parent)
            .set("width", "100px")
            .expect("parent width");

        for _ in 0..child_count {
            let child = root.create_element(DomTag::Div).expect("child");
            root.append_child(parent, child).expect("append child");
            root.style_mut(child)
                .set("height", "10px")
                .expect("child height");
        }

        apply_style_pass(&mut root, document);
        root.reset_node_metrics();

        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        assert_solver_only_reads_and_writes_nodes(&root);
        NodeStoreAccessSummary {
            get_count: root.node_count(RootDomNodesMetricName::Get),
            get_mut_count: root.node_count(RootDomNodesMetricName::GetMut),
            history_len: root.node_history().len(),
        }
    }

    fn node_store_access_summary_after_nested_layout(depth: usize) -> NodeStoreAccessSummary {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let mut parent = document;
        for _ in 0..depth {
            let child = root.create_element(DomTag::Div).expect("child");
            root.append_child(parent, child).expect("append child");
            root.style_mut(child)
                .set("width", "100px")
                .expect("child width");
            parent = child;
        }
        root.style_mut(parent)
            .set("height", "10px")
            .expect("leaf height");

        apply_style_pass(&mut root, document);
        root.reset_node_metrics();

        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        assert_solver_only_reads_and_writes_nodes(&root);
        NodeStoreAccessSummary {
            get_count: root.node_count(RootDomNodesMetricName::Get),
            get_mut_count: root.node_count(RootDomNodesMetricName::GetMut),
            history_len: root.node_history().len(),
        }
    }

    #[test]
    fn relayout_tree_only_uses_direct_node_store_reads_and_writes() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(parent)
            .set("width", "100px")
            .expect("parent width");
        root.style_mut(child)
            .set("height", "20px")
            .expect("child height");

        apply_style_pass(&mut root, document);
        root.reset_node_metrics();

        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        assert_solver_only_reads_and_writes_nodes(&root);
    }

    #[test]
    fn relayout_tree_additional_siblings_increase_node_store_work_by_a_constant_step() {
        let one_child = node_store_access_summary_after_sibling_layout(1);
        let two_children = node_store_access_summary_after_sibling_layout(2);
        let three_children = node_store_access_summary_after_sibling_layout(3);

        assert_eq!(
            two_children.get_count - one_child.get_count,
            three_children.get_count - two_children.get_count
        );
        assert_eq!(
            two_children.get_mut_count - one_child.get_mut_count,
            three_children.get_mut_count - two_children.get_mut_count
        );
        assert_eq!(
            two_children.history_len - one_child.history_len,
            three_children.history_len - two_children.history_len
        );
    }

    #[test]
    fn relayout_tree_additional_depth_increases_node_store_work_by_a_constant_step() {
        let one_level = node_store_access_summary_after_nested_layout(1);
        let two_levels = node_store_access_summary_after_nested_layout(2);
        let three_levels = node_store_access_summary_after_nested_layout(3);

        // The leading-margin-chain computation (flex arc step 9) reads one
        // computed style per positioned container plus one more per empty
        // intermediary level below it — style-read-only, bounded by the
        // empty-run length, never layout (see FLEX_STEP9_DESIGN.md's chain-walk
        // cost model). This all-empty nested chain therefore grows its Get step
        // (and the history it records) by a constant 2 per added level — a
        // named, bounded O(depth²) shape; the mutation step stays constant.
        assert_eq!(
            two_levels.get_count - one_level.get_count,
            7,
            "Get step for the first added level"
        );
        assert_eq!(
            three_levels.get_count - two_levels.get_count,
            9,
            "Get step for the second added level (constant +2 growth)"
        );
        assert_eq!(
            two_levels.get_mut_count - one_level.get_mut_count,
            three_levels.get_mut_count - two_levels.get_mut_count,
            "mutation step stays constant"
        );
        assert_eq!(
            two_levels.history_len - one_level.history_len,
            8,
            "history step for the first added level"
        );
        assert_eq!(
            three_levels.history_len - two_levels.history_len,
            10,
            "history step for the second added level (constant +2 growth)"
        );
    }

    #[test]
    fn tree_walker_block_layout_resolves_width_on_entry_and_height_from_children() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(parent)
            .set("width", "100px")
            .expect("parent width");
        root.style_mut(child)
            .set("height", "20px")
            .expect("child height");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let parent_box = node_box(&root, parent).expect("parent box");
        let child_box = node_box(&root, child).expect("child box");
        assert_close(parent_box.border_box.width_px, 100.0);
        assert_close(parent_box.border_box.height_px, 20.0);
        assert_close(child_box.border_box.left_px, 0.0);
        assert_close(child_box.border_box.top_px, 0.0);
        assert_close(child_box.border_box.width_px, 100.0);
        assert_close(child_box.border_box.height_px, 20.0);
    }

    #[test]
    fn relayout_tree_with_inherited_resolves_em_width_from_ancestor_seed() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(parent)
            .set("font-size", "12px")
            .expect("parent font-size");
        root.style_mut(child)
            .set("width", "2em")
            .expect("child width");
        root.style_mut(child)
            .set("height", "1em")
            .expect("child height");

        let document = root.root;
        apply_style_pass(&mut root, document);
        let inherited = inherited_style_context_from_ancestors(&root, child).expect("seed");
        let mut resolver = IdentityDomLayoutResolver;
        let _ = relayout_tree_with_inherited(
            &mut root,
            child,
            (200.0, 120.0),
            &inherited,
            &mut resolver,
        )
        .expect("layout");

        let child_box = node_box(&root, child).expect("child box");
        assert_close(child_box.border_box.width_px, 24.0);
        assert_close(child_box.border_box.height_px, 12.0);
    }

    #[test]
    fn tree_walker_flex_layout_places_children_inline() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let a = root.create_element(DomTag::Div).expect("a");
        let b = root.create_element(DomTag::Div).expect("b");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, a).expect("append a");
        root.append_child(parent, b).expect("append b");
        root.style_mut(parent).set("display", "flex").expect("flex");
        root.style_mut(a).set("width", "10px").expect("width a");
        root.style_mut(b).set("width", "12px").expect("width b");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let b_box = node_box(&root, b).expect("b box");
        assert_close(b_box.border_box.left_px, 10.0);
    }

    #[test]
    fn relative_children_stay_in_normal_flow() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("position", "relative")
            .expect("relative");
        root.style_mut(child).set("height", "12px").expect("height");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let parent_box = node_box(&root, parent).expect("parent box");
        assert_close(parent_box.border_box.height_px, 12.0);
    }

    #[test]
    fn transformed_children_stay_in_normal_flow() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("transform", "translateX(6px)")
            .expect("transform");
        root.style_mut(child).set("height", "14px").expect("height");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let parent_box = node_box(&root, parent).expect("parent box");
        assert_close(parent_box.border_box.height_px, 14.0);
    }

    #[test]
    fn overflow_layer_children_stay_in_normal_flow() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("overflow", "hidden")
            .expect("overflow");
        root.style_mut(child).set("height", "16px").expect("height");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("layout");

        let parent_box = node_box(&root, parent).expect("parent box");
        assert_close(parent_box.border_box.height_px, 16.0);
    }

    #[test]
    fn separate_layout_tree_size_changes_return_parent_layer_propagation_axes() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("transform", "translateX(6px)")
            .expect("transform");
        root.style_mut(child).set("width", "20px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver)
            .expect("warm child layout");

        root.style_mut(child).set("width", "30px").expect("width");

        apply_style_pass(&mut root, child);
        let result =
            relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver).expect("child layout");

        assert!(result.propagate_width_to_parent_layer, "result={result:?}");
        assert!(!result.propagate_height_to_parent_layer);
        assert_eq!(node_layout_dirty(&root, parent), Some(false));
        assert_eq!(node_layout_dirty(&root, document), Some(false));
    }

    #[test]
    fn separate_layout_tree_same_size_rerun_does_not_propagate_to_parent_layer() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("transform", "translateX(6px)")
            .expect("transform");
        root.style_mut(child).set("width", "20px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver)
            .expect("warm child layout");

        root.style_mut(child)
            .set("width", "20.005px")
            .expect("width");
        apply_style_pass(&mut root, child);
        let result =
            relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver).expect("child layout");

        assert_eq!(result, LayoutPassResult::default());
    }

    #[test]
    fn absolute_layout_trees_do_not_propagate_size_changes_to_parent_layers() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("position", "absolute")
            .expect("absolute");
        root.style_mut(child).set("width", "20px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver)
            .expect("warm child layout");

        root.style_mut(child).set("width", "30px").expect("width");
        apply_style_pass(&mut root, child);
        let result =
            relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver).expect("child layout");

        assert_eq!(result, LayoutPassResult::default());
    }

    #[test]
    fn fixed_layout_trees_do_not_propagate_size_changes_to_parent_layers() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("position", "fixed")
            .expect("fixed");
        root.style_mut(child).set("width", "20px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (200.0, 120.0), &mut resolver)
            .expect("warm child layout");

        root.style_mut(child).set("width", "30px").expect("width");
        apply_style_pass(&mut root, child);
        let result =
            relayout_tree(&mut root, child, (200.0, 120.0), &mut resolver).expect("child layout");

        assert_eq!(result, LayoutPassResult::default());
    }

    #[test]
    fn fixed_layout_tree_root_keeps_right_anchored_explicit_width_pinned_to_viewport() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, child).expect("append child");
        root.style_mut(wrapper)
            .set("width", "140px")
            .expect("wrapper width");
        root.style_mut(wrapper)
            .set("margin-left", "80px")
            .expect("wrapper margin-left");
        root.style_mut(child)
            .set("position", "fixed")
            .expect("fixed");
        root.style_mut(child).set("top", "132px").expect("top");
        root.style_mut(child).set("right", "25px").expect("right");
        root.style_mut(child).set("width", "196px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (320.0, 320.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, child, (320.0, 320.0), &mut resolver).expect("child layout");

        let child_box = node_box(&root, child).expect("child box");
        assert_close(child_box.border_box.left_px, 99.0);
        assert_close(child_box.border_box.top_px, 132.0);
        assert_close(child_box.border_box.width_px, 196.0);
    }

    #[test]
    fn fixed_layout_tree_root_uses_current_viewport_width_after_resize() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, child).expect("append child");
        root.style_mut(child)
            .set("position", "fixed")
            .expect("fixed");
        root.style_mut(child).set("top", "132px").expect("top");
        root.style_mut(child).set("right", "25px").expect("right");
        root.style_mut(child).set("width", "196px").expect("width");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, document, (1280.0, 720.0), &mut resolver)
            .expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, child, (1280.0, 720.0), &mut resolver).expect("child layout");

        root.set_attr(
            child,
            DomAttribute::Custom("data-runtime-sync".to_string()),
            Some(DomAttributeValue::String("second-pass".to_string())),
        )
        .expect("mark second update");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (900.0, 700.0), &mut resolver)
            .expect("resized child layout");

        let child_node = root.node(child).expect("child node");
        let child_box = node_box(&root, child).expect("child box");

        assert_close(child_node.layout().available_width_px, 900.0);
        assert_close(child_box.border_box.left_px, 679.0);
        assert_close(child_box.border_box.top_px, 132.0);
        assert_close(child_box.border_box.width_px, 196.0);
    }

    #[test]
    fn overflow_layout_trees_do_not_propagate_size_changes_to_parent_layers() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("overflow", "scroll")
            .expect("scroll overflow");
        root.style_mut(child).set("width", "20px").expect("width");
        root.style_mut(child).set("height", "10px").expect("height");

        let mut resolver = IdentityDomLayoutResolver;
        apply_style_pass(&mut root, document);
        let _ =
            relayout_tree(&mut root, document, (200.0, 120.0), &mut resolver).expect("warm layout");
        apply_style_pass(&mut root, document);
        let _ = relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver)
            .expect("warm child layout");

        root.style_mut(child).set("width", "30px").expect("width");
        root.style_mut(child).set("height", "22px").expect("height");
        apply_style_pass(&mut root, child);
        let result =
            relayout_tree(&mut root, child, (20.0, 16.0), &mut resolver).expect("child layout");

        assert_eq!(result, LayoutPassResult::default());
    }

    /// Clears one node's layout-dirty bit so a dirty-query contract test can
    /// isolate exactly which node still carries the mark.
    fn clear_layout_dirty_for(root: &mut RootDom, key: DomNodeSlotKey) {
        clear_layout_dirty(root.nodes.get_mut(key).expect("node should exist"));
    }

    #[test]
    fn tree_contains_dirty_node_finds_a_dirty_deep_descendant() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        let leaf = root.create_element(DomTag::Div).expect("leaf");
        root.append_child(document, parent)
            .expect("document -> parent");
        root.append_child(parent, child).expect("parent -> child");
        root.append_child(child, leaf).expect("child -> leaf");

        // Every freshly-appended node is born layout-dirty. Clear the
        // ancestors so only the deepest leaf carries the mark; the query must
        // descend document -> parent -> child to reach it.
        clear_layout_dirty_for(&mut root, document);
        clear_layout_dirty_for(&mut root, parent);
        clear_layout_dirty_for(&mut root, child);
        assert_eq!(node_layout_dirty(&root, leaf), Some(true));

        assert!(tree_needs_layout(&mut root, document).expect("dirty query"));
    }

    #[test]
    fn tree_contains_dirty_node_returns_false_for_a_fully_clean_tree() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent)
            .expect("document -> parent");
        root.append_child(parent, child).expect("parent -> child");

        clear_layout_dirty_for(&mut root, document);
        clear_layout_dirty_for(&mut root, parent);
        clear_layout_dirty_for(&mut root, child);

        assert!(!tree_needs_layout(&mut root, document).expect("clean query"));
    }

    #[test]
    fn tree_contains_dirty_node_returns_true_immediately_for_a_dirty_root() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent)
            .expect("document -> parent");
        root.append_child(parent, child).expect("parent -> child");

        // Dirty root, clean descendants: the walk must stop at the root
        // without descending, so it records exactly one node-store Get.
        clear_layout_dirty_for(&mut root, parent);
        clear_layout_dirty_for(&mut root, child);
        assert_eq!(node_layout_dirty(&root, document), Some(true));
        root.reset_node_metrics();

        assert!(tree_needs_layout(&mut root, document).expect("dirty-root query"));
        assert_eq!(
            root.node_count(RootDomNodesMetricName::Get),
            1,
            "a dirty root must answer without walking its descendants"
        );
    }

    #[test]
    fn tree_contains_dirty_node_walks_a_pathologically_deep_tree_without_recursion() {
        const DEPTH: usize = 2_000;

        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let mut parent = document;
        let mut keys = Vec::with_capacity(DEPTH + 1);
        keys.push(document);
        for _ in 0..DEPTH {
            let child = root.create_element(DomTag::Div).expect("level node");
            root.append_child(parent, child).expect("level attach");
            keys.push(child);
            parent = child;
        }

        // Clear every ancestor's layout-dirty bit but keep the deepest leaf
        // dirty, so the query must descend the full DEPTH chain to answer. The
        // iterative work stack keeps the depth on the heap — a recursive walk
        // would grow the call stack one frame per level.
        for key in &keys[..DEPTH] {
            clear_layout_dirty_for(&mut root, *key);
        }
        assert_eq!(node_layout_dirty(&root, keys[DEPTH]), Some(true));

        assert!(tree_needs_layout(&mut root, document).expect("deep-tree query"));
    }

    #[test]
    fn tree_contains_dirty_node_uses_the_paint_flag_for_paint_queries() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root_key(&root);
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(document, parent)
            .expect("document -> parent");
        root.append_child(parent, child).expect("parent -> child");

        // Every freshly-appended node is born paint-dirty. Clear the
        // ancestors' repaint state so only the deepest child is paint-dirty;
        // the paint query must descend to it.
        root.nodes
            .get_mut(document)
            .expect("document")
            .clear_paint_repaint_state();
        root.nodes
            .get_mut(parent)
            .expect("parent")
            .clear_paint_repaint_state();
        assert_eq!(node_paint_dirty(&root, child), Some(true));

        assert!(tree_needs_paint(&mut root, document).expect("paint query"));
    }
}
