//! DOM-subtree hit resolution for one already-chosen layer root.
//!
//! This file represents the DOM-space portion of hit testing after composite
//! has already chosen the matching Bevy layer entity. Its purpose is to keep
//! DOM path resolution separate from Bevy raycast concerns while still sharing
//! one strict subtree-local hit algorithm across callers.
//!
//! Invariants and rules:
//!
//! - this file resolves DOM hits only inside one layer-owned subtree
//! - it must not perform Bevy-space layer selection itself
//! - nested layer roots must remain traversal boundaries here
//!
//! Advice and code philosophy:
//!
//! - keep this file DOM-shaped and local-coordinate based
//! - reuse DOM traversal state instead of inventing second hit graphs
use crate::domains::layer_hit::{LayerClusterHit, collect_layer_cluster_hits_from_root_point};
#[cfg(test)]
use crate::domains::layer_hit::{LayerPointHit, collect_layer_hits_from_root_point};
use crate::domains::layout::{layer_kind_for_node, node_box_from_ref, relative_offset_for_node};
use crate::types::primitives::PointPx;
use crate::types::{DomAttribute, DomAttributeValue, DomError, DomNodeSlotKey, DomRect, RootDom};

#[cfg(test)]
use crate::types::{DomNode, DomTag, RootDomContext};
#[cfg(test)]
use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DomSubtreeHit {
    pub path: Vec<DomNodeSlotKey>,
    pub target_key: DomNodeSlotKey,
}

#[derive(Clone, Copy)]
struct SearchContext {
    effective_rect: DomRect,
    point: PointPx,
    pointer_events_enabled: bool,
}

#[cfg(test)]
pub(crate) fn hit_layer_points_from_root_point(
    root_dom: &RootDom,
    point: PointPx,
) -> Result<Vec<LayerPointHit>, DomError> {
    let mut layer_hits = Vec::new();
    collect_layer_hits_from_root_point(root_dom, point, &mut layer_hits)?;
    layer_hits.reverse();
    Ok(layer_hits)
}

pub(crate) fn hit_layer_cluster_hits_from_root_point(
    root_dom: &RootDom,
    point: PointPx,
) -> Result<Vec<LayerClusterHit>, DomError> {
    let mut layer_hits = Vec::new();
    collect_layer_cluster_hits_from_root_point(root_dom, point, &mut layer_hits)?;
    layer_hits.reverse();
    Ok(layer_hits)
}

#[cfg(test)]
pub(crate) fn hit_test_from_dom_subtree_local(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    point: PointPx,
) -> Result<Option<DomSubtreeHit>, DomError> {
    hit_test_from_dom_subtree_cluster_local(
        root_dom,
        subtree_root,
        point,
        subtree_effective_rect(root_dom, subtree_root)?,
    )
}

pub(crate) fn hit_test_from_dom_subtree_cluster_local(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    point: PointPx,
    effective_rect: DomRect,
) -> Result<Option<DomSubtreeHit>, DomError> {
    let Some(path) = dom_hit_path_from_local_point(root_dom, subtree_root, point, effective_rect)?
    else {
        return Ok(None);
    };
    Ok(Some(DomSubtreeHit {
        target_key: *path.last().expect("dom hit path should keep one target"),
        path,
    }))
}

pub(crate) fn topmost_active_pseudo_target_from_dom_subtree_cluster_local(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    point: PointPx,
    effective_rect: DomRect,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let context = subtree_entry_context(point, effective_rect);
    visit_topmost_in_local_subtree(
        root_dom,
        subtree_root,
        PointPx::default(),
        context,
        &mut |current_key, node, absolute_offset, _current_path| {
            if point_hits_node(context.point, absolute_offset, node_box_from_ref(node))
                && node
                    .as_element()
                    .is_some_and(|element| element.behavior().active_pseudo)
            {
                return Ok(Some(current_key));
            }
            Ok(None)
        },
    )
}

fn dom_hit_path_from_local_point(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    point: PointPx,
    effective_rect: DomRect,
) -> Result<Option<Vec<DomNodeSlotKey>>, DomError> {
    let context = subtree_entry_context(point, effective_rect);
    visit_topmost_in_local_subtree(
        root_dom,
        subtree_root,
        PointPx::default(),
        context,
        &mut |current_key, node, absolute_offset, current_path| {
            if !point_hits_node(context.point, absolute_offset, node_box_from_ref(node)) {
                return Ok(None);
            }
            let _ = current_key;
            Ok(Some(current_path.to_vec()))
        },
    )
}

fn visit_topmost_in_local_subtree<T, F>(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    absolute_offset: PointPx,
    context: SearchContext,
    accept: &mut F,
) -> Result<Option<T>, DomError>
where
    F: FnMut(
        DomNodeSlotKey,
        &crate::types::DomNode,
        PointPx,
        &[DomNodeSlotKey],
    ) -> Result<Option<T>, DomError>,
{
    let mut current_path = Vec::new();
    visit_topmost_in_local_subtree_branch(
        root_dom,
        subtree_root,
        subtree_root,
        absolute_offset,
        context,
        &mut current_path,
        accept,
    )
}

fn visit_topmost_in_local_subtree_branch<T, F>(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
    absolute_offset: PointPx,
    context: SearchContext,
    current_path: &mut Vec<DomNodeSlotKey>,
    accept: &mut F,
) -> Result<Option<T>, DomError>
where
    F: FnMut(
        DomNodeSlotKey,
        &crate::types::DomNode,
        PointPx,
        &[DomNodeSlotKey],
    ) -> Result<Option<T>, DomError>,
{
    let node = root_dom
        .nodes
        .get(current_key)
        .ok_or(DomError::InvalidNodeKey)?;
    if current_key != subtree_root && layer_kind_for_node(node) != crate::types::DomLayerKind::None
    {
        return Ok(None);
    }

    let node_rect = rect_at_offset(absolute_offset, node_box_from_ref(node).border_box);
    let next_effective_rect = if current_key == subtree_root {
        context.effective_rect
    } else {
        next_effective_rect_for_node(node, context.effective_rect, node_rect)
    };
    if !rect_contains_point(next_effective_rect, context.point) {
        return Ok(None);
    }

    current_path.push(current_key);
    let next_context = descendant_context(
        root_dom,
        subtree_root,
        current_key,
        node,
        absolute_offset,
        next_effective_rect,
        context,
    );

    let mut child_key = node.last_child_key_for_store();
    while let Some(current_child_key) = child_key {
        let child = root_dom
            .node(current_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        child_key = child.previous_sibling_key_for_store();
        if let Some(found) = visit_topmost_in_local_subtree_branch(
            root_dom,
            subtree_root,
            current_child_key,
            relative_offset_for_node(root_dom, current_child_key),
            next_context,
            current_path,
            accept,
        )? {
            current_path.pop();
            return Ok(Some(found));
        }
    }

    if !next_context.pointer_events_enabled {
        current_path.pop();
        return Ok(None);
    }

    let accepted = accept(current_key, node, absolute_offset, current_path)?;
    current_path.pop();
    Ok(accepted)
}

fn subtree_entry_context(point: PointPx, effective_rect: DomRect) -> SearchContext {
    SearchContext {
        effective_rect,
        point,
        pointer_events_enabled: true,
    }
}

#[cfg(test)]
fn subtree_effective_rect(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
) -> Result<DomRect, DomError> {
    let subtree_root_node = root_dom
        .node(subtree_root)
        .ok_or(DomError::InvalidNodeKey)?;
    let root_box = node_box_from_ref(subtree_root_node).border_box;
    Ok(DomRect {
        left_px: 0.0,
        top_px: 0.0,
        width_px: root_box.width_px,
        height_px: root_box.height_px,
    })
}

fn next_effective_rect_for_node(
    node: &crate::types::DomNode,
    effective_rect: DomRect,
    node_rect: DomRect,
) -> DomRect {
    if layer_kind_for_node(node) == crate::types::DomLayerKind::Clipped {
        intersect_rects(effective_rect, node_rect)
    } else {
        effective_rect
    }
}

fn descendant_context(
    root_dom: &RootDom,
    subtree_root: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
    node: &crate::types::DomNode,
    absolute_offset: PointPx,
    next_effective_rect: DomRect,
    context: SearchContext,
) -> SearchContext {
    if current_key == subtree_root
        || layer_kind_for_node(node) != crate::types::DomLayerKind::Clipped
    {
        return SearchContext {
            effective_rect: next_effective_rect,
            point: context.point,
            pointer_events_enabled: next_pointer_events_enabled(
                node,
                context.pointer_events_enabled,
            ),
        };
    }
    let scroll_offset_y_px = root_dom.vertical_scroll_offset_y_px(current_key);
    SearchContext {
        effective_rect: translate_rect(
            translate_rect(
                next_effective_rect,
                -absolute_offset.x_px,
                -absolute_offset.y_px,
            ),
            0.0,
            scroll_offset_y_px,
        ),
        point: PointPx {
            x_px: context.point.x_px - absolute_offset.x_px,
            y_px: context.point.y_px - absolute_offset.y_px + scroll_offset_y_px,
        },
        pointer_events_enabled: next_pointer_events_enabled(node, context.pointer_events_enabled),
    }
}

fn next_pointer_events_enabled(node: &crate::types::DomNode, inherited_enabled: bool) -> bool {
    match node_pointer_events_override(node) {
        Some(enabled) => enabled,
        None => inherited_enabled,
    }
}

fn node_pointer_events_override(node: &crate::types::DomNode) -> Option<bool> {
    match node.presented_attr(&DomAttribute::Custom("pointer-events".to_owned())) {
        Some(DomAttributeValue::String(value)) if value == "none" => Some(false),
        Some(DomAttributeValue::String(value)) if value == "auto" => Some(true),
        _ => None,
    }
}

fn point_hits_node(
    point: PointPx,
    absolute_offset: PointPx,
    box_model: crate::types::DomBoxModel,
) -> bool {
    let rect = box_model.border_box;
    if rect.width_px <= 0.0 || rect.height_px <= 0.0 {
        return false;
    }

    let left = absolute_offset.x_px;
    let top = absolute_offset.y_px;
    let right = left + rect.width_px;
    let bottom = top + rect.height_px;
    point.x_px >= left && point.x_px <= right && point.y_px >= top && point.y_px <= bottom
}

fn rect_at_offset(offset: PointPx, mut rect: DomRect) -> DomRect {
    rect.left_px = offset.x_px;
    rect.top_px = offset.y_px;
    rect
}

fn translate_rect(mut rect: DomRect, horizontal_shift_px: f32, vertical_shift_px: f32) -> DomRect {
    rect.left_px += horizontal_shift_px;
    rect.top_px += vertical_shift_px;
    rect
}

fn intersect_rects(left: DomRect, right: DomRect) -> DomRect {
    let next_left_px = left.left_px.max(right.left_px);
    let next_top_px = left.top_px.max(right.top_px);
    let next_right_px = (left.left_px + left.width_px).min(right.left_px + right.width_px);
    let next_bottom_px = (left.top_px + left.height_px).min(right.top_px + right.height_px);
    DomRect {
        left_px: next_left_px,
        top_px: next_top_px,
        width_px: (next_right_px - next_left_px).max(0.0),
        height_px: (next_bottom_px - next_top_px).max(0.0),
    }
}

fn rect_contains_point(rect: DomRect, point: PointPx) -> bool {
    if rect.width_px <= 0.0 || rect.height_px <= 0.0 {
        return false;
    }
    let right_px = rect.left_px + rect.width_px;
    let bottom_px = rect.top_px + rect.height_px;
    point.x_px >= rect.left_px
        && point.x_px <= right_px
        && point.y_px >= rect.top_px
        && point.y_px <= bottom_px
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{image::Image, prelude::*};

    #[test]
    fn root_layer_hit_prefers_topmost_same_layer_overlay_path() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let underlay = root.create_element(DomTag::Button).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("absolute overlay");
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

        let layer_paths = detached_layer_paths_from_root_point(&root, point(5.0, 5.0))
            .expect("hit test should succeed");

        assert_eq!(layer_paths, vec![vec![overlay]]);
    }

    #[test]
    fn hit_layers_include_overlapping_absolute_layer_paths_before_root_path() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let underlay = root.create_element(DomTag::Button).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(underlay, "position", "absolute")
            .expect("absolute underlay");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("absolute overlay");
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

        let layer_paths = detached_layer_paths_from_root_point(&root, point(5.0, 5.0))
            .expect("hit test should succeed");

        assert_eq!(layer_paths, vec![vec![overlay], vec![underlay]]);
    }

    #[test]
    fn local_subtree_hit_does_not_reaccumulate_resolved_child_offsets() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let child = root.create_element(DomTag::Button).expect("child");
        let root_key = root.root;

        root.append_child(root_key, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, child).expect("append child");
        set_box(&mut root, root_key, 0.0, 0.0, 240.0, 180.0);
        set_box(&mut root, wrapper, 20.0, 30.0, 180.0, 120.0);
        set_box(&mut root, child, 40.0, 50.0, 80.0, 30.0);

        let subtree_hit =
            super::hit_test_from_dom_subtree_local(&root, root_key, point(45.0, 55.0))
                .expect("subtree hit should succeed")
                .expect("subtree hit should exist");

        assert_eq!(
            subtree_hit.target_key, child,
            "subtree hit should use the resolved child box position without adding the wrapper offset a second time",
        );
    }

    #[test]
    fn cluster_local_subtree_hit_keeps_resolved_offsets_for_non_root_subtrees() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let root_key = root.root;
        let subtree_root = root.create_element(DomTag::Div).expect("subtree root");
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let child = root.create_element(DomTag::Button).expect("child");

        root.append_child(root_key, subtree_root)
            .expect("append subtree root");
        root.append_child(subtree_root, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, child).expect("append child");
        set_box(&mut root, root_key, 0.0, 0.0, 1280.0, 720.0);
        set_box(&mut root, subtree_root, 960.0, 22.0, 320.0, 686.0);
        set_box(&mut root, wrapper, 16.0, 36.0, 288.0, 120.0);
        set_box(&mut root, child, 40.0, 50.0, 120.0, 30.0);

        let subtree_hit = super::hit_test_from_dom_subtree_cluster_local(
            &root,
            subtree_root,
            point(45.0, 55.0),
            DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px: 320.0,
                height_px: 686.0,
            },
        )
        .expect("cluster-local subtree hit should succeed")
        .expect("cluster-local subtree hit should exist");

        assert_eq!(
            subtree_hit.target_key, child,
            "cluster-local subtree hit should keep child offsets resolved within the chosen subtree root",
        );
    }

    #[test]
    fn attached_root_keeps_same_overlapping_absolute_layer_paths() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("root should attach");

        let underlay = root.create_element(DomTag::Button).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(underlay, "position", "absolute")
            .expect("absolute underlay");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("absolute overlay");
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

        let layer_paths = detached_layer_paths_from_root_point(&root, point(5.0, 5.0))
            .expect("hit test should succeed");

        assert_eq!(layer_paths, vec![vec![overlay], vec![underlay]]);
    }

    fn point(x_px: f32, y_px: f32) -> PointPx {
        PointPx { x_px, y_px }
    }

    fn detached_layer_paths_from_root_point(
        root_dom: &RootDom,
        point: PointPx,
    ) -> Result<Vec<Vec<DomNodeSlotKey>>, DomError> {
        let layer_hits = super::hit_layer_cluster_hits_from_root_point(root_dom, point)?;
        let mut layer_paths = Vec::with_capacity(layer_hits.len().saturating_add(1));
        for layer_hit in layer_hits {
            let Some(hit) = super::hit_test_from_dom_subtree_cluster_local(
                root_dom,
                layer_hit.node_key,
                layer_hit.point,
                layer_hit.effective_rect,
            )?
            else {
                continue;
            };
            layer_paths.push(hit.path);
        }
        if layer_paths.is_empty()
            && let Some(hit) =
                super::hit_test_from_dom_subtree_local(root_dom, root_dom.root, point)?
        {
            layer_paths.push(hit.path);
        }
        Ok(layer_paths)
    }

    fn set_box(
        root: &mut RootDom,
        key: DomNodeSlotKey,
        left_px: f32,
        top_px: f32,
        width_px: f32,
        height_px: f32,
    ) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.box_model.border_box.left_px = left_px;
                document.base.box_model.border_box.top_px = top_px;
                document.base.box_model.border_box.width_px = width_px;
                document.base.box_model.border_box.height_px = height_px;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.box_model.border_box.left_px = left_px;
                fragment.base.box_model.border_box.top_px = top_px;
                fragment.base.box_model.border_box.width_px = width_px;
                fragment.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Element(element) => {
                element.base.box_model.border_box.left_px = left_px;
                element.base.box_model.border_box.top_px = top_px;
                element.base.box_model.border_box.width_px = width_px;
                element.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Text(text) => {
                text.base.box_model.border_box.left_px = left_px;
                text.base.box_model.border_box.top_px = top_px;
                text.base.box_model.border_box.width_px = width_px;
                text.base.box_model.border_box.height_px = height_px;
            }
        }
    }
}
