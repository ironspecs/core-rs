//! Layer-first hit collection for presented DOM layer roots.
//!
//! Intended algorithm:
//!
//! - treat layers as the traversal unit
//! - implement the walk as one streaming visitor or iterator with local
//!   cluster state, not as one mandatory collect-then-filter pipeline
//! - consult directly connected `DomNode`s only for the layer facts the walk
//!   needs: layer kind, box, relative offset, and scroll-adjusted local point
//! - do not fall back to plain DOM parent-hit-gated descent because composite
//!   child layers may live entirely outside an ancestor box when authored with
//!   `position: absolute` or `position: fixed`
//!
//! Cluster model:
//!
//! 1. start with one viewport-sized `DomRect` plus one root-space point
//! 2. walk the current cluster online and evaluate candidate layer roots as
//!    they are encountered
//! 3. for every candidate layer root, translate the point into that layer's
//!    local coordinate system and apply the final layer-local hit test at that
//!    moment unless the caller explicitly needs retained results
//! 4. when one layer root clips, intersect the current cluster rect with that
//!    clip rect, translate the point into the clipped layer's local coordinate
//!    system, and continue inside one new nested cluster under that narrower
//!    rect
//! 5. stop as soon as the active query mode allows; only the `all hit layers`
//!    mode should retain multiple hits, ideally by appending into
//!    caller-provided storage
//!
//! Why the walk is shaped this way:
//!
//! - one non-clipping ancestor must not block a descendant layer that renders
//!   outside the ancestor box
//! - one clipping ancestor must constrain descendants by creating a narrower
//!   cluster rect
//! - cluster membership and final layer hit are logical phases in one online
//!   traversal, not a requirement to materialize intermediate collections
//!
//! Allowed optimization directions:
//!
//! - `any layer` queries may stop at the first confirmed hit
//! - `topmost layer` queries may traverse in paint order and exit as soon as
//!   the first surviving candidate is proven topmost
//! - `all hit layers` queries may append every confirmed hit in paint order
//!   into caller-provided storage
//! - `specific layer` queries may skip global accumulation and evaluate only
//!   the relevant cluster path plus that layer's local-space hit
//! - cached metadata such as layer rects, clip ancestry, paint order, and
//!   coordinate transforms may be reused as long as the clustered layer model
//!   stays the source of truth
//!
//! Non-negotiable invariants:
//!
//! - optimizations must preserve the clustered layer model
//! - non-clipping ancestors must not gate descendant layers
//! - clipping ancestors must create new narrowed clusters
//! - the final hit decision must happen in each layer's own local coordinates
//!
//! Raised-layer integration contract:
//!
//! - raised-layer handling must reuse this canonical layer-hit search instead
//!   of inventing one separate raised traversal algorithm
//! - one raised prepass should run this same search once for each entry in
//!   `RootDom::raised_layer_roots()` in reverse registry order
//! - while that raised prepass is active, the reused search should skip
//!   descendant `Raised` layer roots so those descendants are left for their
//!   own registry entries
//! - if no raised-root search hits, one final ordinary root search should run
//! - that ordinary root search should skip all `Raised` layer roots entirely
//! - with those two skip rules in place, the combined raised prepass plus
//!   ordinary pass should touch each live layer exactly once
use bevy::prelude::{ChildOf, Children, Entity, World};
use smallvec::SmallVec;

use crate::domains::composite::BevyLayer;
use crate::domains::layout::{layer_kind_for_node, layer_offset_from_dom_climb, node_box_from_ref};
use crate::types::primitives::PointPx;
use crate::types::{DomError, DomLayerKind, DomNode, DomNodeSlotKey, DomRect, RootDom};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerPointHit {
    pub(crate) node_key: DomNodeSlotKey,
    pub(crate) point: PointPx,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayerClusterHit {
    pub(crate) node_key: DomNodeSlotKey,
    pub(crate) point: PointPx,
    pub(crate) effective_rect: DomRect,
}

#[derive(Clone, Copy)]
struct ClusterContext {
    effective_rect: DomRect,
    point: PointPx,
}

#[derive(Clone, Copy)]
struct LayerTraversalState {
    child_context: ClusterContext,
    pending_hit: Option<LayerClusterHit>,
}

#[derive(Clone, Copy)]
struct ReverseLayerVisitFrame {
    entity: Entity,
    offset: PointPx,
    context: ClusterContext,
    pending_hit: Option<LayerClusterHit>,
    emit_pending_hit: bool,
}

/// Returns whether any layer matches one root-space point in the canonical
/// cluster walk.
///
/// This function is the shared layer-hit search boundary. Raised-layer hit
/// selection reuses this search once per raised registry entry during the
/// raised prepass, then reuses the same search for the ordinary root pass with
/// raised roots skipped.
pub(crate) fn any_layer_hit_from_root_point(
    root_dom: &RootDom,
    point: PointPx,
) -> Result<bool, DomError> {
    let mut found = false;
    visit_layer_hits_from_root_point(root_dom, point, &mut |_layer_hit| {
        found = true;
        true
    })?;
    Ok(found)
}

/// Optimized entrypoint for the `any layer` query path.
///
/// The raised-layer algorithm keeps this query on the same canonical search
/// shape as every other layer-hit entrypoint: raised registry entries are
/// checked first with the shared search, then the ordinary root search runs
/// with raised roots skipped.
#[cfg_attr(not(test), allow(dead_code))]
pub fn optimized_any_layer_hit_from_root_point(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
) -> Result<bool, DomError> {
    Ok(
        optimized_topmost_layer_hit_from_root_point(root_dom, world, viewport_rect, root_point_px)?
            .is_some(),
    )
}

/// Streaming entrypoint for the optimized `topmost layer` query path.
///
/// The raised-layer algorithm for this query is:
///
/// - run the canonical search once per raised registry entry in reverse order
/// - skip descendant raised roots during each raised-root search
/// - stop on the first raised hit
/// - otherwise run one ordinary root search
/// - skip raised roots during the ordinary root search
#[cfg_attr(not(test), allow(dead_code))]
pub fn optimized_topmost_layer_hit_from_root_point(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
) -> Result<Option<LayerClusterHit>, DomError> {
    let mut topmost_hit = None;
    visit_bevy_layer_hits_in_reverse_paint_order(
        root_dom,
        world,
        viewport_rect,
        root_point_px,
        &mut |layer_hit| {
            topmost_hit = Some(layer_hit);
            Ok(true)
        },
    )?;
    Ok(topmost_hit)
}

/// Appending entrypoint for the optimized `all hit layers` query path.
///
/// The raised-layer algorithm preserves one shared search model here too:
/// raised registry entries own their own searches, descendant raised roots are
/// skipped during those raised-root searches, and the ordinary root walk skips
/// raised roots. With that partition, the combined pass touches each live
/// layer once.
#[cfg_attr(not(test), allow(dead_code))]
pub fn append_optimized_all_layer_hits_from_root_point(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
    layer_hits: &mut Vec<LayerClusterHit>,
) -> Result<(), DomError> {
    visit_bevy_layer_hits_in_reverse_paint_order(
        root_dom,
        world,
        viewport_rect,
        root_point_px,
        &mut |layer_hit| {
            layer_hits.push(layer_hit);
            Ok(false)
        },
    )
}

/// Direct entrypoint for the optimized `specific layer` query path.
///
/// Specific-layer resolution stays aligned with the canonical search model.
/// Raised layers do not introduce one separate search algorithm here; this
/// query follows the same raised-root ownership and skip rules as the other
/// layer-hit entrypoints.
#[cfg_attr(not(test), allow(dead_code))]
pub fn optimized_specific_layer_hit_from_root_point(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
    layer_root_key: DomNodeSlotKey,
) -> Result<Option<LayerClusterHit>, DomError> {
    resolve_specific_bevy_layer_hit_from_root_point(
        root_dom,
        world,
        viewport_rect,
        root_point_px,
        layer_root_key,
    )
}

/// Returns the topmost attached layer hit for one root-space point.
///
/// Attached hit selection uses the same raised-layer sequence as the optimized
/// topmost query: raised registry entries first, then the ordinary attached
/// root walk with raised roots skipped.
pub(crate) fn topmost_attached_layer_hit_from_root_point(
    root_dom: &RootDom,
    world: &World,
    root_point_px: PointPx,
) -> Result<Option<LayerClusterHit>, DomError> {
    optimized_topmost_layer_hit_from_root_point(
        root_dom,
        world,
        viewport_rect(root_dom)?,
        root_point_px,
    )
}

/// Visits attached layer hits for one root-space point in reverse paint order.
///
/// Raised-layer ordering here is still sequencing, not a different traversal
/// algorithm: each raised registry entry owns one reused attached-layer search,
/// then the ordinary attached-root walk runs with raised roots skipped.
pub(crate) fn visit_attached_layer_hits_from_root_point<F>(
    root_dom: &RootDom,
    world: &World,
    root_point_px: PointPx,
    on_hit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(LayerClusterHit) -> Result<bool, DomError>,
{
    visit_bevy_layer_hits_in_reverse_paint_order(
        root_dom,
        world,
        viewport_rect(root_dom)?,
        root_point_px,
        on_hit,
    )
}

/// Collects raw layer hits from one root-space point in canonical DOM order.
///
/// Raised-layer coverage for this collection follows the same partitioned hit
/// search: one reused walk per raised registry entry, then one ordinary root
/// walk that skips raised roots.
#[cfg(test)]
pub(crate) fn collect_layer_hits_from_root_point(
    root_dom: &RootDom,
    point: PointPx,
    layer_hits: &mut Vec<LayerPointHit>,
) -> Result<(), DomError> {
    visit_layer_hits_from_root_point(root_dom, point, &mut |layer_hit| {
        layer_hits.push(LayerPointHit {
            node_key: layer_hit.node_key,
            point: layer_hit.point,
        });
        false
    })?;
    Ok(())
}

/// Collects cluster-local layer hits from one root-space point.
///
/// With raised-layer ordering, this collection should still reflect one
/// partitioned search over the live layer set rather than duplicate visits.
pub(crate) fn collect_layer_cluster_hits_from_root_point(
    root_dom: &RootDom,
    point: PointPx,
    layer_hits: &mut Vec<LayerClusterHit>,
) -> Result<(), DomError> {
    visit_layer_hits_from_root_point(root_dom, point, &mut |layer_hit| {
        layer_hits.push(layer_hit);
        false
    })?;
    Ok(())
}

fn visit_layer_hits_from_root_point<F>(
    root_dom: &RootDom,
    point: PointPx,
    on_hit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(LayerClusterHit) -> bool,
{
    let context = root_cluster_context(root_dom, point)?;
    if visit_layer_subtree_hits_with_entry_offset(
        root_dom,
        root_dom.root,
        layer_offset_from_dom_climb(root_dom, root_dom.root)?,
        context,
        on_hit,
    )? {
        return Ok(());
    }
    let _ = visit_raised_layer_hits_from_root_context(root_dom, context, on_hit)?;
    Ok(())
}

fn visit_raised_layer_hits_from_root_context<F>(
    root_dom: &RootDom,
    context: ClusterContext,
    on_hit: &mut F,
) -> Result<bool, DomError>
where
    F: FnMut(LayerClusterHit) -> bool,
{
    for layer_root_key in root_dom.raised_layer_roots().iter().copied() {
        if visit_layer_subtree_hits_with_entry_offset(
            root_dom,
            layer_root_key,
            raised_layer_offset_from_root_climb(root_dom, layer_root_key)?,
            context,
            on_hit,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn visit_layer_subtree_hits_with_entry_offset<F>(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
    layer_offset: PointPx,
    context: ClusterContext,
    on_hit: &mut F,
) -> Result<bool, DomError>
where
    F: FnMut(LayerClusterHit) -> bool,
{
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let node_rect = rect_at_offset(layer_offset, node_box_from_ref(node).border_box);
    let child_context =
        nested_cluster_context(root_dom, layer_root_key, node_rect, layer_offset, context);
    if point_hits_node(context.point, layer_offset, node)
        && on_hit(LayerClusterHit {
            node_key: layer_root_key,
            point: child_context.point,
            effective_rect: child_context.effective_rect,
        })
    {
        return Ok(true);
    }
    let Some(nested_entry) = first_child_in_cluster(root_dom, layer_root_key)? else {
        return Ok(false);
    };
    visit_cluster_hits(
        root_dom,
        layer_root_key,
        Some(nested_entry),
        child_context,
        on_hit,
    )
}

fn visit_bevy_layer_hits_in_reverse_paint_order<F>(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
    on_hit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(LayerClusterHit) -> Result<bool, DomError>,
{
    let root_layer_entity = attached_root_layer_entity(root_dom, world)?;
    let root_context = ClusterContext {
        effective_rect: viewport_rect,
        point: root_point_px,
    };
    if visit_raised_bevy_layer_hits_in_reverse_registry_order(
        root_dom,
        world,
        root_context,
        on_hit,
    )? {
        return Ok(());
    }
    let _ = visit_bevy_layer_subtree_hits_in_reverse_paint_order(
        root_dom,
        world,
        root_layer_entity,
        layer_offset_from_dom_climb(root_dom, root_dom.root)?,
        root_context,
        on_hit,
    )?;
    Ok(())
}

fn visit_raised_bevy_layer_hits_in_reverse_registry_order<F>(
    root_dom: &RootDom,
    world: &World,
    root_context: ClusterContext,
    on_hit: &mut F,
) -> Result<bool, DomError>
where
    F: FnMut(LayerClusterHit) -> Result<bool, DomError>,
{
    for layer_root_key in root_dom.raised_layer_roots().iter().rev().copied() {
        let layer_entity = live_layer_entity(root_dom, world, layer_root_key)?;
        let layer_offset = raised_layer_offset_from_root_climb(root_dom, layer_root_key)?;
        if visit_bevy_layer_subtree_hits_in_reverse_paint_order(
            root_dom,
            world,
            layer_entity,
            layer_offset,
            root_context,
            on_hit,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn visit_bevy_layer_subtree_hits_in_reverse_paint_order<F>(
    root_dom: &RootDom,
    world: &World,
    traversal_root_entity: Entity,
    traversal_root_offset: PointPx,
    root_context: ClusterContext,
    on_hit: &mut F,
) -> Result<bool, DomError>
where
    F: FnMut(LayerClusterHit) -> Result<bool, DomError>,
{
    let mut layer_stack = SmallVec::<[ReverseLayerVisitFrame; 32]>::new();
    layer_stack.push(ReverseLayerVisitFrame {
        entity: traversal_root_entity,
        offset: traversal_root_offset,
        context: root_context,
        pending_hit: None,
        emit_pending_hit: false,
    });
    while let Some(frame) = layer_stack.pop() {
        if frame.emit_pending_hit {
            if let Some(layer_hit) = frame.pending_hit
                && on_hit(layer_hit)?
            {
                return Ok(true);
            }
            continue;
        }
        if frame.entity != traversal_root_entity
            && is_raised_layer_entity(root_dom, world, frame.entity)?
        {
            continue;
        }
        let Some(layer_state) =
            layer_traversal_state(root_dom, world, frame.entity, frame.offset, frame.context)?
        else {
            continue;
        };
        layer_stack.push(ReverseLayerVisitFrame {
            entity: frame.entity,
            offset: frame.offset,
            context: layer_state.child_context,
            pending_hit: layer_state.pending_hit,
            emit_pending_hit: true,
        });
        push_bevy_layer_children_in_reverse_paint_order(
            root_dom,
            world,
            frame.entity,
            layer_state.child_context,
            &mut layer_stack,
        )?;
    }
    Ok(false)
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_specific_bevy_layer_hit_from_root_point(
    root_dom: &RootDom,
    world: &World,
    viewport_rect: DomRect,
    root_point_px: PointPx,
    layer_root_key: DomNodeSlotKey,
) -> Result<Option<LayerClusterHit>, DomError> {
    let root_layer_entity = attached_root_layer_entity(root_dom, world)?;
    let traversal_root_key = owning_hit_search_root_key(root_dom, layer_root_key)?;
    let traversal_root_entity = if traversal_root_key == root_dom.root {
        root_layer_entity
    } else {
        live_layer_entity(root_dom, world, traversal_root_key)?
    };
    if layer_root_key == traversal_root_key {
        let traversal_root_offset =
            hit_search_root_offset_from_root_context(root_dom, traversal_root_key)?;
        return Ok(layer_traversal_state(
            root_dom,
            world,
            traversal_root_entity,
            traversal_root_offset,
            ClusterContext {
                effective_rect: viewport_rect,
                point: root_point_px,
            },
        )?
        .and_then(|state| state.pending_hit));
    }
    let target_layer_entity = live_layer_entity(root_dom, world, layer_root_key)?;
    let mut context = ClusterContext {
        effective_rect: viewport_rect,
        point: root_point_px,
    };
    let traversal_root_offset =
        hit_search_root_offset_from_root_context(root_dom, traversal_root_key)?;
    let Some(traversal_root_state) = layer_traversal_state(
        root_dom,
        world,
        traversal_root_entity,
        traversal_root_offset,
        context,
    )?
    else {
        return Ok(None);
    };
    context = traversal_root_state.child_context;
    let mut entity_path =
        layer_entity_path_from_ancestor(world, traversal_root_entity, target_layer_entity)?;
    for layer_entity in entity_path.drain(..) {
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        let layer_offset = layer_offset_from_dom_climb(root_dom, layer_root_key)?;
        let Some(layer_state) =
            layer_traversal_state(root_dom, world, layer_entity, layer_offset, context)?
        else {
            return Ok(None);
        };
        if layer_entity == target_layer_entity {
            return Ok(layer_state.pending_hit);
        }
        context = layer_state.child_context;
    }
    Err(DomError::InvalidNodeKey)
}

fn attached_root_layer_entity(root_dom: &RootDom, world: &World) -> Result<Entity, DomError> {
    let attached_parent = root_dom
        .attached_bevy_parent_entity()
        .ok_or(DomError::InvalidParentTarget)?;
    let root_layer_entity = live_layer_entity(root_dom, world, root_dom.root)?;
    let root_parent = world
        .get::<ChildOf>(root_layer_entity)
        .ok_or(DomError::InvalidNodeKey)?
        .parent();
    if root_parent != attached_parent {
        return Err(DomError::InvalidParentTarget);
    }
    Ok(root_layer_entity)
}

fn live_layer_entity(
    root_dom: &RootDom,
    world: &World,
    layer_root_key: DomNodeSlotKey,
) -> Result<Entity, DomError> {
    let entity = root_dom
        .layer_entity(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    if world.get_entity(entity).is_err() || world.get::<BevyLayer>(entity).is_none() {
        return Err(DomError::InvalidNodeKey);
    }
    Ok(entity)
}

#[cfg_attr(not(test), allow(dead_code))]
fn layer_entity_path_from_ancestor(
    world: &World,
    ancestor_layer_entity: Entity,
    target_layer_entity: Entity,
) -> Result<SmallVec<[Entity; 16]>, DomError> {
    let mut entity_path = SmallVec::<[Entity; 16]>::new();
    let mut current_entity = target_layer_entity;
    loop {
        if current_entity == ancestor_layer_entity {
            break;
        }
        entity_path.push(current_entity);
        let parent = world
            .get::<ChildOf>(current_entity)
            .ok_or(DomError::InvalidNodeKey)?
            .parent();
        if world.get::<BevyLayer>(parent).is_none() {
            return Err(DomError::InvalidNodeKey);
        }
        current_entity = parent;
    }
    entity_path.reverse();
    Ok(entity_path)
}

fn owning_hit_search_root_key(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<DomNodeSlotKey, DomError> {
    let mut current_key = Some(layer_root_key);
    while let Some(node_key) = current_key {
        let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
        if node_key != root_dom.root
            && node.layer_order() == crate::types::LayerOrder::Raised
            && root_dom.layer_entity(node_key).is_some()
        {
            return Ok(node_key);
        }
        current_key = node.parent_key();
    }
    Ok(root_dom.root)
}

fn hit_search_root_offset_from_root_context(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<PointPx, DomError> {
    if layer_root_key == root_dom.root {
        return layer_offset_from_dom_climb(root_dom, layer_root_key);
    }
    raised_layer_offset_from_root_climb(root_dom, layer_root_key)
}

fn raised_layer_offset_from_root_climb(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<PointPx, DomError> {
    let layer_root = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    assert_eq!(
        layer_root.layer_order(),
        crate::types::LayerOrder::Raised,
        "raised layer root offsets should only be resolved for raised layers",
    );
    let mut root_offset = layer_offset_from_dom_climb(root_dom, layer_root_key)?;
    let mut current_key = layer_root.parent_key();
    while let Some(current_key_value) = current_key {
        let current_node = root_dom
            .node(current_key_value)
            .ok_or(DomError::InvalidNodeKey)?;
        if current_key_value == root_dom.root
            || layer_kind_for_node(current_node) != DomLayerKind::None
        {
            let ancestor_offset = layer_offset_from_dom_climb(root_dom, current_key_value)?;
            root_offset.x_px += ancestor_offset.x_px;
            root_offset.y_px += ancestor_offset.y_px;
        }
        current_key = current_node.parent_key();
    }
    Ok(root_offset)
}

fn layer_traversal_state(
    root_dom: &RootDom,
    world: &World,
    layer_entity: Entity,
    layer_offset: PointPx,
    context: ClusterContext,
) -> Result<Option<LayerTraversalState>, DomError> {
    if !rect_contains_point(context.effective_rect, context.point) {
        return Ok(None);
    }
    let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
    let Some(node) = root_dom.node(layer_root_key) else {
        return Ok(None);
    };
    let node_rect = rect_at_offset(layer_offset, node_box_from_ref(node).border_box);
    let child_context =
        nested_cluster_context(root_dom, layer_root_key, node_rect, layer_offset, context);
    let pending_hit = if point_hits_node(context.point, layer_offset, node) {
        Some(LayerClusterHit {
            node_key: layer_root_key,
            point: child_context.point,
            effective_rect: child_context.effective_rect,
        })
    } else {
        None
    };
    Ok(Some(LayerTraversalState {
        child_context,
        pending_hit,
    }))
}

fn bevy_layer_root_key(
    root_dom: &RootDom,
    world: &World,
    entity: Entity,
) -> Result<DomNodeSlotKey, DomError> {
    let bevy_layer = world
        .get::<BevyLayer>(entity)
        .ok_or(DomError::InvalidNodeKey)?;
    assert_eq!(
        root_dom.layer_entity(bevy_layer.dom_node_slot_key()),
        Some(entity),
        "BevyLayer identity should stay synchronized with RootDom layer registration",
    );
    Ok(bevy_layer.dom_node_slot_key())
}

fn is_raised_layer_entity(
    root_dom: &RootDom,
    world: &World,
    entity: Entity,
) -> Result<bool, DomError> {
    let layer_root_key = bevy_layer_root_key(root_dom, world, entity)?;
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    Ok(node.layer_order() == crate::types::LayerOrder::Raised)
}

fn push_bevy_layer_children_in_reverse_paint_order(
    root_dom: &RootDom,
    world: &World,
    layer_entity: Entity,
    context: ClusterContext,
    layer_stack: &mut SmallVec<[ReverseLayerVisitFrame; 32]>,
) -> Result<(), DomError> {
    let Some(children) = world.get::<Children>(layer_entity) else {
        return Ok(());
    };
    for child_entity in children {
        if let Some(bevy_layer) = world.get::<BevyLayer>(*child_entity) {
            let layer_root_key = bevy_layer.dom_node_slot_key();
            let offset = layer_offset_from_dom_climb(root_dom, layer_root_key)?;
            layer_stack.push(ReverseLayerVisitFrame {
                entity: *child_entity,
                offset,
                context,
                pending_hit: None,
                emit_pending_hit: false,
            });
        }
    }
    Ok(())
}

fn root_cluster_context(root_dom: &RootDom, point: PointPx) -> Result<ClusterContext, DomError> {
    Ok(ClusterContext {
        effective_rect: viewport_rect(root_dom)?,
        point,
    })
}

fn visit_cluster_hits<F>(
    root_dom: &RootDom,
    cluster_root_key: DomNodeSlotKey,
    mut current_key: Option<DomNodeSlotKey>,
    context: ClusterContext,
    on_hit: &mut F,
) -> Result<bool, DomError>
where
    F: FnMut(LayerClusterHit) -> bool,
{
    if !rect_contains_point(context.effective_rect, context.point) {
        return Ok(false);
    }
    while let Some(node_key) = current_key {
        let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
        let layer_kind = layer_kind_for_node(node);
        if node_key != cluster_root_key && layer_kind != DomLayerKind::None {
            current_key = next_key_in_cluster(root_dom, cluster_root_key, node_key, false)?;
            if node.layer_order() == crate::types::LayerOrder::Raised {
                continue;
            }
            let layer_offset = layer_offset_from_dom_climb(root_dom, node_key)?;
            let node_rect = rect_at_offset(layer_offset, node_box_from_ref(node).border_box);
            if !rects_intersect(node_rect, context.effective_rect) {
                continue;
            }
            let nested_context =
                nested_cluster_context(root_dom, node_key, node_rect, layer_offset, context);
            if point_hits_node(context.point, layer_offset, node)
                && on_hit(LayerClusterHit {
                    node_key,
                    point: nested_context.point,
                    effective_rect: nested_context.effective_rect,
                })
            {
                return Ok(true);
            }
            let nested_entry = first_child_in_cluster(root_dom, node_key)?;
            if let Some(nested_entry) = nested_entry
                && visit_cluster_hits(
                    root_dom,
                    node_key,
                    Some(nested_entry),
                    nested_context,
                    on_hit,
                )?
            {
                return Ok(true);
            }
            continue;
        }
        current_key = next_key_in_cluster(root_dom, cluster_root_key, node_key, true)?;
    }
    Ok(false)
}

fn next_key_in_cluster(
    root_dom: &RootDom,
    cluster_root_key: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
    descend: bool,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
    if descend && let Some(child_key) = node.first_child_key_for_store() {
        return Ok(Some(child_key));
    }
    if let Some(sibling_key) = node.next_sibling_key_for_store() {
        return Ok(Some(sibling_key));
    }
    let mut ancestor_key = node.parent_key();
    while let Some(current_ancestor_key) = ancestor_key {
        if current_ancestor_key == cluster_root_key {
            return Ok(None);
        }
        let ancestor = root_dom
            .node(current_ancestor_key)
            .ok_or(DomError::InvalidNodeKey)?;
        if let Some(sibling_key) = ancestor.next_sibling_key_for_store() {
            return Ok(Some(sibling_key));
        }
        ancestor_key = ancestor.parent_key();
    }
    Ok(None)
}

fn first_child_in_cluster(
    root_dom: &RootDom,
    cluster_root_key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    Ok(root_dom
        .node(cluster_root_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store())
}

fn nested_cluster_context(
    root_dom: &RootDom,
    cluster_root_key: DomNodeSlotKey,
    node_rect: DomRect,
    absolute_offset: PointPx,
    context: ClusterContext,
) -> ClusterContext {
    let narrowed_rect = if root_dom
        .node(cluster_root_key)
        .is_some_and(|node| layer_kind_for_node(node) == DomLayerKind::Clipped)
    {
        intersect_rects(context.effective_rect, node_rect)
    } else {
        context.effective_rect
    };
    let scroll_offset_y_px = root_dom.vertical_scroll_offset_y_px(cluster_root_key);
    ClusterContext {
        effective_rect: translate_rect(
            translate_rect(narrowed_rect, -absolute_offset.x_px, -absolute_offset.y_px),
            0.0,
            scroll_offset_y_px,
        ),
        point: layer_local_point(root_dom, cluster_root_key, absolute_offset, context.point),
    }
}

fn layer_local_point(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
    point: PointPx,
) -> PointPx {
    PointPx {
        x_px: point.x_px - absolute_offset.x_px,
        y_px: point.y_px - absolute_offset.y_px
            + root_dom.vertical_scroll_offset_y_px(layer_root_key),
    }
}

fn point_hits_node(point: PointPx, absolute_offset: PointPx, node: &DomNode) -> bool {
    let rect = node_box_from_ref(node).border_box;
    if rect.width_px <= 0.0 || rect.height_px <= 0.0 {
        return false;
    }
    point.x_px >= absolute_offset.x_px
        && point.x_px <= absolute_offset.x_px + rect.width_px
        && point.y_px >= absolute_offset.y_px
        && point.y_px <= absolute_offset.y_px + rect.height_px
}

fn viewport_rect(root_dom: &RootDom) -> Result<DomRect, DomError> {
    let root_node = root_dom
        .node(root_dom.root)
        .ok_or(DomError::InvalidNodeKey)?;
    let root_box = node_box_from_ref(root_node).border_box;
    if root_box.width_px > 1.0 && root_box.height_px > 1.0 {
        return Ok(DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: root_box.width_px,
            height_px: root_box.height_px,
        });
    }
    Ok(DomRect {
        left_px: -1_000_000.0,
        top_px: -1_000_000.0,
        width_px: 2_000_000.0,
        height_px: 2_000_000.0,
    })
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

fn rects_intersect(left: DomRect, right: DomRect) -> bool {
    if left.width_px <= 0.0
        || left.height_px <= 0.0
        || right.width_px <= 0.0
        || right.height_px <= 0.0
    {
        return false;
    }
    let left_right_px = left.left_px + left.width_px;
    let left_bottom_px = left.top_px + left.height_px;
    let right_right_px = right.left_px + right.width_px;
    let right_bottom_px = right.top_px + right.height_px;
    left.left_px < right_right_px
        && left_right_px > right.left_px
        && left.top_px < right_bottom_px
        && left_bottom_px > right.top_px
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
    use super::{
        LayerClusterHit, LayerPointHit, any_layer_hit_from_root_point,
        append_optimized_all_layer_hits_from_root_point,
        collect_layer_cluster_hits_from_root_point, collect_layer_hits_from_root_point,
        optimized_any_layer_hit_from_root_point, optimized_specific_layer_hit_from_root_point,
        optimized_topmost_layer_hit_from_root_point, viewport_rect,
    };
    use crate::domains::hit_test::{
        hit_layer_points_from_root_point, hit_test_from_dom_subtree_cluster_local,
    };
    use crate::types::primitives::PointPx;
    use crate::types::{
        DomNode, DomNodeSlotKey, DomPointerUpdate, DomRect, DomTag, RootDom, RootDomContext,
    };
    use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};
    use bevy::{image::Image, prelude::*};

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    #[test]
    fn any_layer_hit_is_true_when_the_document_matches() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 80.0, 80.0);

        assert!(
            any_layer_hit_from_root_point(&root, point(10.0, 10.0))
                .expect("root hit should succeed"),
            "document root should count as a layer hit",
        );
    }

    #[test]
    fn collect_layer_hits_keeps_raw_dom_order_for_overlapping_layers() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let underlay = root.create_element(DomTag::Button).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(underlay, "position", "absolute")
            .expect("underlay absolute");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("overlay absolute");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 80.0, 80.0);
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

        let mut layer_hits = Vec::new();
        collect_layer_hits_from_root_point(&root, point(5.0, 5.0), &mut layer_hits)
            .expect("layer hits should collect");

        assert_eq!(
            layer_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            vec![root.root, underlay, overlay],
            "collector raw order should now include the document root before descendant layers",
        );
    }

    #[test]
    fn collect_layer_hits_reports_scrolled_local_point_for_scroll_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");
        root.set_attr_str(scroll_root, "width", "80px")
            .expect("scroll width");
        root.set_attr_str(scroll_root, "height", "40px")
            .expect("scroll height");
        root.set_attr_str(scroll_root, "overflow", "scroll")
            .expect("scroll overflow");
        root.set_attr_str(first_item, "width", "80px")
            .expect("first width");
        root.set_attr_str(first_item, "height", "40px")
            .expect("first height");
        root.set_attr_str(second_item, "width", "80px")
            .expect("second width");
        root.set_attr_str(second_item, "height", "40px")
            .expect("second height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("scroll proof should present once");
        set_box(&mut root, scroll_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, first_item, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, second_item, 0.0, 40.0, 80.0, 40.0);
        assert!(
            root.apply_vertical_scroll_to_layer_root(scroll_root, -60.0)
                .expect("direct scroll should succeed"),
            "scroll root should accept retained scroll",
        );

        let layer_hits = hit_layer_points_from_root_point(&root, point(10.0, 10.0))
            .expect("layer hits should resolve");

        assert_eq!(
            layer_hits.first(),
            Some(&LayerPointHit {
                node_key: scroll_root,
                point: point(10.0, 50.0),
            }),
            "layer hit point should be expressed in scrolled content space for subtree-local use",
        );
    }

    #[test]
    fn scrolled_layer_hit_point_retargets_subtree_hit_under_a_stable_cursor() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");
        root.set_attr_str(scroll_root, "width", "80px")
            .expect("scroll width");
        root.set_attr_str(scroll_root, "height", "40px")
            .expect("scroll height");
        root.set_attr_str(scroll_root, "overflow", "scroll")
            .expect("scroll overflow");
        root.set_attr_str(first_item, "width", "80px")
            .expect("first width");
        root.set_attr_str(first_item, "height", "40px")
            .expect("first height");
        root.set_attr_str(second_item, "width", "80px")
            .expect("second width");
        root.set_attr_str(second_item, "height", "40px")
            .expect("second height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("scroll proof should present once");
        set_box(&mut root, scroll_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, first_item, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, second_item, 0.0, 40.0, 80.0, 40.0);
        assert!(
            root.apply_vertical_scroll_to_layer_root(scroll_root, -60.0)
                .expect("direct scroll should succeed"),
            "scroll root should accept retained scroll",
        );

        let mut layer_hits = Vec::new();
        collect_layer_cluster_hits_from_root_point(&root, point(10.0, 10.0), &mut layer_hits)
            .expect("layer hits should resolve");
        let layer_hit = layer_hits
            .into_iter()
            .next()
            .expect("scroll layer should be hit");
        let subtree_hit = hit_test_from_dom_subtree_cluster_local(
            &root,
            layer_hit.node_key,
            layer_hit.point,
            layer_hit.effective_rect,
        )
        .expect("subtree hit should succeed")
        .expect("subtree hit should exist");

        assert_eq!(
            subtree_hit.target_key, second_item,
            "subtree-local hit should move with retained scroll under a stable cursor",
        );
    }

    #[test]
    fn collect_layer_hits_reaches_absolute_child_layer_outside_parent_box() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let child_layer = root.create_element(DomTag::Div).expect("child layer");

        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, child_layer)
            .expect("append child layer");
        root.set_attr_str(parent_layer, "position", "relative")
            .expect("parent position");
        root.set_attr_str(parent_layer, "width", "20px")
            .expect("parent width");
        root.set_attr_str(parent_layer, "height", "20px")
            .expect("parent height");
        root.set_attr_str(child_layer, "position", "absolute")
            .expect("child absolute");
        root.set_attr_str(child_layer, "left", "100px")
            .expect("child left");
        root.set_attr_str(child_layer, "top", "100px")
            .expect("child top");
        root.set_attr_str(child_layer, "width", "160px")
            .expect("child width");
        root.set_attr_str(child_layer, "height", "120px")
            .expect("child height");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 320.0, 320.0);
        set_box(&mut root, parent_layer, 0.0, 0.0, 20.0, 20.0);
        set_box(&mut root, child_layer, 100.0, 100.0, 160.0, 120.0);

        let mut layer_hits = Vec::new();
        collect_layer_hits_from_root_point(&root, point(140.0, 160.0), &mut layer_hits)
            .expect("layer hits should collect");

        assert_eq!(
            layer_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            vec![root.root, child_layer],
            "cluster walk should include the document root before reaching the absolute child layer",
        );
    }

    #[test]
    fn clipped_layer_narrows_nested_cluster_rect_before_collecting_deeper_layers() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clip_root = root.create_element(DomTag::Div).expect("clip root");
        let child_layer = root.create_element(DomTag::Div).expect("child layer");

        root.append_child(root.root, clip_root)
            .expect("append clip root");
        root.append_child(clip_root, child_layer)
            .expect("append child layer");
        root.set_attr_str(clip_root, "width", "80px")
            .expect("clip width");
        root.set_attr_str(clip_root, "height", "40px")
            .expect("clip height");
        root.set_attr_str(clip_root, "overflow", "hidden")
            .expect("clip overflow");
        root.set_attr_str(child_layer, "position", "absolute")
            .expect("child absolute");
        root.set_attr_str(child_layer, "left", "0px")
            .expect("child left");
        root.set_attr_str(child_layer, "top", "60px")
            .expect("child top");
        root.set_attr_str(child_layer, "width", "80px")
            .expect("child width");
        root.set_attr_str(child_layer, "height", "40px")
            .expect("child height");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);
        set_box(&mut root, clip_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, child_layer, 0.0, 60.0, 80.0, 40.0);

        let mut layer_hits = Vec::new();
        collect_layer_hits_from_root_point(&root, point(10.0, 10.0), &mut layer_hits)
            .expect("layer hits should collect");

        assert_eq!(
            layer_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            vec![root.root, clip_root],
            "clipped cluster rect should still report the document root while excluding deeper layers outside the clipped viewport",
        );
    }

    #[test]
    fn raised_root_outside_viewport_does_not_block_descendant_layer_brought_back_inside() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let raised_root = root.create_element(DomTag::Div).expect("raised root");
        let child_layer = root.create_element(DomTag::Div).expect("child layer");

        root.append_child(root.root, raised_root)
            .expect("append raised root");
        root.append_child(raised_root, child_layer)
            .expect("append child layer");
        root.set_attr_str(raised_root, "position", "absolute")
            .expect("raised root position");
        root.set_attr_str(raised_root, "left", "200px")
            .expect("raised root left");
        root.set_attr_str(raised_root, "top", "0px")
            .expect("raised root top");
        root.set_attr_str(raised_root, "width", "20px")
            .expect("raised root width");
        root.set_attr_str(raised_root, "height", "20px")
            .expect("raised root height");
        root.set_layout_layer_order(raised_root, crate::types::LayerOrder::Raised)
            .expect("raised root layer order");
        root.set_attr_str(child_layer, "position", "absolute")
            .expect("child layer position");
        root.set_attr_str(child_layer, "left", "-190px")
            .expect("child layer left");
        root.set_attr_str(child_layer, "top", "0px")
            .expect("child layer top");
        root.set_attr_str(child_layer, "width", "80px")
            .expect("child layer width");
        root.set_attr_str(child_layer, "height", "40px")
            .expect("child layer height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("raised descendant proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);
        set_box(&mut root, raised_root, 200.0, 0.0, 20.0, 20.0);
        set_box(&mut root, child_layer, -190.0, 0.0, 80.0, 40.0);

        let layer_hits = crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(
            &root,
            point(20.0, 10.0),
        )
        .expect("raised descendant hit should resolve");

        assert_eq!(
            layer_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            vec![child_layer, root.root],
            "raised rooted search must not prune one non-clipping raised root before it can reach a descendant layer rendered back inside the viewport",
        );
    }

    #[test]
    fn raised_layer_hit_escapes_ordinary_ancestor_clip_and_specific_queries() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clip_root = root.create_element(DomTag::Div).expect("clip root");
        let raised_popup = root.create_element(DomTag::Div).expect("raised popup");
        let option_row = root.create_element(DomTag::Div).expect("option row");

        root.append_child(root.root, clip_root)
            .expect("append clip root");
        root.append_child(clip_root, raised_popup)
            .expect("append raised popup");
        root.append_child(raised_popup, option_row)
            .expect("append option row");
        root.set_attr_str(clip_root, "width", "80px")
            .expect("clip width");
        root.set_attr_str(clip_root, "height", "40px")
            .expect("clip height");
        root.set_attr_str(clip_root, "overflow", "hidden")
            .expect("clip overflow");
        root.set_attr_str(raised_popup, "position", "absolute")
            .expect("raised position");
        root.set_attr_str(raised_popup, "top", "60px")
            .expect("raised top");
        root.set_attr_str(raised_popup, "width", "80px")
            .expect("raised width");
        root.set_attr_str(raised_popup, "height", "40px")
            .expect("raised height");
        root.set_layout_layer_order(raised_popup, crate::types::LayerOrder::Raised)
            .expect("raised layer order");
        root.set_attr_str(option_row, "position", "absolute")
            .expect("row position");
        root.set_attr_str(option_row, "width", "80px")
            .expect("row width");
        root.set_attr_str(option_row, "height", "40px")
            .expect("row height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("raised clip proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);
        set_box(&mut root, clip_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, raised_popup, 0.0, 60.0, 80.0, 40.0);
        set_box(&mut root, option_row, 0.0, 0.0, 80.0, 40.0);

        let point = point(10.0, 70.0);
        let expected =
            crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(&root, point)
                .expect("legacy raised hits should succeed");
        let expected_keys = expected.iter().map(|hit| hit.node_key).collect::<Vec<_>>();
        let topmost = optimized_topmost_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point,
        )
        .expect("optimized topmost raised hit should succeed");
        let mut optimized_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point,
            &mut optimized_hits,
        )
        .expect("optimized raised hits should succeed");
        let specific = optimized_specific_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point,
            option_row,
        )
        .expect("optimized specific raised descendant hit should succeed");

        assert_eq!(
            expected_keys,
            vec![option_row, raised_popup, root.root],
            "raised ownership should escape the clipped ancestor while leaving ordinary root hits intact",
        );
        assert_eq!(
            optimized_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            expected_keys,
            "optimized raised ordering should match the canonical partitioned search",
        );
        assert_eq!(
            topmost.map(|hit| hit.node_key),
            Some(option_row),
            "raised descendant should win topmost hit selection outside the ordinary clip",
        );
        assert_eq!(
            specific.map(|hit| hit.node_key),
            Some(option_row),
            "specific-layer lookup should resolve through the owning raised-root pass",
        );
    }

    #[test]
    fn attached_raised_prepass_applies_ancestor_layer_offset() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let raised_popup = root.create_element(DomTag::Div).expect("raised popup");

        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, raised_popup)
            .expect("append raised popup");
        root.set_attr_str(parent_layer, "position", "fixed")
            .expect("parent position");
        root.set_attr_str(parent_layer, "left", "100px")
            .expect("parent left");
        root.set_attr_str(parent_layer, "top", "50px")
            .expect("parent top");
        root.set_attr_str(parent_layer, "width", "200px")
            .expect("parent width");
        root.set_attr_str(parent_layer, "height", "150px")
            .expect("parent height");
        root.set_attr_str(raised_popup, "position", "absolute")
            .expect("popup position");
        root.set_attr_str(raised_popup, "left", "20px")
            .expect("popup left");
        root.set_attr_str(raised_popup, "top", "30px")
            .expect("popup top");
        root.set_attr_str(raised_popup, "width", "60px")
            .expect("popup width");
        root.set_attr_str(raised_popup, "height", "40px")
            .expect("popup height");
        root.set_layout_layer_order(raised_popup, crate::types::LayerOrder::Raised)
            .expect("popup raised order");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("offset raised proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 400.0, 300.0);
        set_box(&mut root, parent_layer, 100.0, 50.0, 200.0, 150.0);
        set_box(&mut root, raised_popup, 20.0, 30.0, 60.0, 40.0);

        let topmost = optimized_topmost_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(125.0, 85.0),
        )
        .expect("offset raised hit should resolve");

        assert_eq!(
            topmost.map(|hit| hit.node_key),
            Some(raised_popup),
            "raised prepass should test the raised popup at parent world offset plus popup local offset",
        );
    }

    #[test]
    fn root_dom_raised_hits_apply_ancestor_layer_offset() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let raised_popup = root.create_element(DomTag::Div).expect("raised popup");

        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, raised_popup)
            .expect("append raised popup");
        root.set_attr_str(parent_layer, "position", "fixed")
            .expect("parent position");
        root.set_attr_str(parent_layer, "left", "100px")
            .expect("parent left");
        root.set_attr_str(parent_layer, "top", "50px")
            .expect("parent top");
        root.set_attr_str(parent_layer, "width", "200px")
            .expect("parent width");
        root.set_attr_str(parent_layer, "height", "150px")
            .expect("parent height");
        root.set_attr_str(raised_popup, "position", "absolute")
            .expect("popup position");
        root.set_attr_str(raised_popup, "left", "20px")
            .expect("popup left");
        root.set_attr_str(raised_popup, "top", "30px")
            .expect("popup top");
        root.set_attr_str(raised_popup, "width", "60px")
            .expect("popup width");
        root.set_attr_str(raised_popup, "height", "40px")
            .expect("popup height");
        root.set_layout_layer_order(raised_popup, crate::types::LayerOrder::Raised)
            .expect("popup raised order");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("offset raised proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 400.0, 300.0);
        set_box(&mut root, parent_layer, 100.0, 50.0, 200.0, 150.0);
        set_box(&mut root, raised_popup, 20.0, 30.0, 60.0, 40.0);

        let layer_hits = crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(
            &root,
            point(125.0, 85.0),
        )
        .expect("offset raised hit list should resolve");

        assert!(
            layer_hits.iter().any(|hit| hit.node_key == raised_popup),
            "root-DOM raised hit collection should include the raised popup at parent world offset plus popup local offset; got {:?}",
            layer_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn optimized_any_layer_hit_matches_legacy_any_result() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let layer = root.create_element(DomTag::Div).expect("layer");
        root.append_child(root.root, layer).expect("append layer");
        root.set_attr_str(layer, "position", "absolute")
            .expect("layer position");
        root.set_attr_str(layer, "width", "80px")
            .expect("layer width");
        root.set_attr_str(layer, "height", "40px")
            .expect("layer height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("optimized any proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);
        set_box(&mut root, layer, 0.0, 0.0, 80.0, 40.0);

        let expected = any_layer_hit_from_root_point(&root, point(10.0, 10.0))
            .expect("legacy any hit should succeed");
        let actual = optimized_any_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(10.0, 10.0),
        )
        .expect("optimized any hit should succeed");

        assert_eq!(
            actual, expected,
            "optimized any-layer query should preserve legacy hit truth",
        );
    }

    #[test]
    fn optimized_topmost_and_all_layer_hits_match_legacy_topmost_order() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let underlay = root.create_element(DomTag::Div).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(underlay, "position", "absolute")
            .expect("underlay absolute");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("overlay absolute");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 80.0, 80.0);
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("optimized order proof should present once");

        let expected = crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(
            &root,
            point(5.0, 5.0),
        )
        .expect("legacy ordered hits should succeed");
        let topmost = optimized_topmost_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(5.0, 5.0),
        )
        .expect("optimized topmost hit should succeed");
        let mut optimized_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(5.0, 5.0),
            &mut optimized_hits,
        )
        .expect("optimized all hits should succeed");

        assert_eq!(
            optimized_hits, expected,
            "optimized all-layer query should preserve topmost-first hit order",
        );
        assert_eq!(
            topmost,
            expected.first().copied(),
            "optimized topmost-layer query should match the first ordered hit",
        );
    }

    #[test]
    fn nested_raised_roots_partition_hits_without_duplicates() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let raised_parent = root.create_element(DomTag::Div).expect("raised parent");
        let raised_child = root.create_element(DomTag::Div).expect("raised child");
        let ordinary_overlay = root.create_element(DomTag::Div).expect("ordinary overlay");

        root.append_child(root.root, raised_parent)
            .expect("append raised parent");
        root.append_child(raised_parent, raised_child)
            .expect("append raised child");
        root.append_child(root.root, ordinary_overlay)
            .expect("append ordinary overlay");
        root.set_attr_str(raised_parent, "position", "absolute")
            .expect("raised parent position");
        root.set_attr_str(raised_parent, "width", "80px")
            .expect("raised parent width");
        root.set_attr_str(raised_parent, "height", "80px")
            .expect("raised parent height");
        root.set_layout_layer_order(raised_parent, crate::types::LayerOrder::Raised)
            .expect("raised parent layer order");
        root.set_attr_str(raised_child, "position", "absolute")
            .expect("raised child position");
        root.set_attr_str(raised_child, "left", "10px")
            .expect("raised child left");
        root.set_attr_str(raised_child, "top", "10px")
            .expect("raised child top");
        root.set_attr_str(raised_child, "width", "40px")
            .expect("raised child width");
        root.set_attr_str(raised_child, "height", "40px")
            .expect("raised child height");
        root.set_layout_layer_order(raised_child, crate::types::LayerOrder::Raised)
            .expect("raised child layer order");
        root.set_attr_str(ordinary_overlay, "position", "absolute")
            .expect("ordinary overlay position");
        root.set_attr_str(ordinary_overlay, "width", "80px")
            .expect("ordinary overlay width");
        root.set_attr_str(ordinary_overlay, "height", "80px")
            .expect("ordinary overlay height");

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("nested raised proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);
        set_box(&mut root, raised_parent, 0.0, 0.0, 80.0, 80.0);
        set_box(&mut root, raised_child, 10.0, 10.0, 40.0, 40.0);
        set_box(&mut root, ordinary_overlay, 0.0, 0.0, 80.0, 80.0);

        let point = point(20.0, 20.0);
        let expected =
            crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(&root, point)
                .expect("legacy nested raised hits should succeed");
        let expected_keys = expected.iter().map(|hit| hit.node_key).collect::<Vec<_>>();
        let topmost = optimized_topmost_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point,
        )
        .expect("optimized nested raised topmost should succeed");
        let mut optimized_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point,
            &mut optimized_hits,
        )
        .expect("optimized nested raised hits should succeed");

        assert_eq!(
            expected_keys,
            vec![raised_child, raised_parent, ordinary_overlay, root.root],
            "raised passes should own nested raised roots before the ordinary pass",
        );
        assert_eq!(
            optimized_hits
                .iter()
                .map(|hit| hit.node_key)
                .collect::<Vec<_>>(),
            expected_keys,
            "optimized nested raised ordering should match the canonical partitioned search",
        );
        assert_eq!(
            topmost.map(|hit| hit.node_key),
            Some(raised_child),
            "reverse raised registry order should prefer the nested raised root first",
        );
    }

    #[test]
    fn attached_nested_raised_roots_with_offsets_visit_every_layer_once() {
        let fixture = OffsetNestedRaisedHitTree::new();
        let point = OffsetNestedRaisedHitTree::hit_point();
        let mut optimized_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
            &mut optimized_hits,
        )
        .expect("attached offset raised hits should resolve");
        let optimized_keys = layer_hit_keys(&optimized_hits);
        let topmost = optimized_topmost_layer_hit_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
        )
        .expect("attached offset raised topmost should resolve");
        let specific = optimized_specific_layer_hit_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
            fixture.ordinary_child,
        )
        .expect("attached offset raised specific hit should resolve");

        assert_eq!(
            optimized_keys,
            fixture.expected_topmost_hit_keys(),
            "attached raised search should seed raised roots from root-space offsets, keep non-raised descendants in the owning raised search, and leave ordinary ancestors to the ordinary pass",
        );
        assert_unique_layer_keys(&optimized_keys);
        assert_eq!(
            topmost.map(|hit| hit.node_key),
            Some(fixture.raised_child),
            "reverse raised registry order should make the nested raised child the attached topmost hit",
        );
        assert_eq!(
            specific.map(|hit| hit.node_key),
            Some(fixture.ordinary_child),
            "specific lookup for a non-raised descendant should enter through its owning raised root and then use the normal child-layer descent",
        );
    }

    #[test]
    fn root_dom_and_attached_raised_partition_match_for_offset_nested_roots() {
        let fixture = OffsetNestedRaisedHitTree::new();
        let point = OffsetNestedRaisedHitTree::hit_point();
        let root_dom_hits =
            crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(&fixture.root, point)
                .expect("root-dom offset raised hits should resolve");
        let mut attached_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
            &mut attached_hits,
        )
        .expect("attached offset raised hits should resolve");
        let root_dom_keys = layer_hit_keys(&root_dom_hits);
        let attached_keys = layer_hit_keys(&attached_hits);

        assert_eq!(
            root_dom_keys,
            fixture.expected_topmost_hit_keys(),
            "root-DOM layer collection should expose the same raised-first partition after final topmost ordering",
        );
        assert_eq!(
            attached_keys, root_dom_keys,
            "attached Bevy traversal and root-DOM traversal should agree on raised-root ownership, descendant raised skips, and ancestor-offset entry geometry",
        );
        assert_unique_layer_keys(&attached_keys);
    }

    #[test]
    fn optimized_specific_layer_hit_matches_legacy_cluster_hit_for_target_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let child_layer = root.create_element(DomTag::Div).expect("child layer");

        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, child_layer)
            .expect("append child layer");
        root.set_attr_str(parent_layer, "position", "relative")
            .expect("parent position");
        root.set_attr_str(parent_layer, "width", "20px")
            .expect("parent width");
        root.set_attr_str(parent_layer, "height", "20px")
            .expect("parent height");
        root.set_attr_str(child_layer, "position", "absolute")
            .expect("child absolute");
        root.set_attr_str(child_layer, "left", "100px")
            .expect("child left");
        root.set_attr_str(child_layer, "top", "100px")
            .expect("child top");
        root.set_attr_str(child_layer, "width", "160px")
            .expect("child width");
        root.set_attr_str(child_layer, "height", "120px")
            .expect("child height");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 320.0, 320.0);
        set_box(&mut root, parent_layer, 0.0, 0.0, 20.0, 20.0);
        set_box(&mut root, child_layer, 100.0, 100.0, 160.0, 120.0);

        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("optimized specific proof should present once");

        let expected = crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(
            &root,
            point(140.0, 160.0),
        )
        .expect("legacy ordered hits should succeed")
        .into_iter()
        .find(|layer_hit| layer_hit.node_key == child_layer);
        let actual = optimized_specific_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(140.0, 160.0),
            child_layer,
        )
        .expect("optimized specific hit should succeed");

        assert_eq!(
            actual, expected,
            "optimized specific-layer query should preserve the target layer cluster hit",
        );
    }

    #[test]
    fn optimized_any_layer_hit_fails_fast_when_root_is_unattached() {
        let root = RootDom::new(RootDomContext::new_gpu());

        let error = optimized_any_layer_hit_from_root_point(
            &root,
            &World::new(),
            DomRect {
                left_px: 0.0,
                top_px: 0.0,
                width_px: 80.0,
                height_px: 80.0,
            },
            point(10.0, 10.0),
        )
        .expect_err("optimized any-layer hit should reject unattached roots");

        assert_eq!(error, crate::types::DomError::InvalidParentTarget);
    }

    #[test]
    fn optimized_specific_layer_hit_accepts_document_root_query() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("document-root specific proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 80.0, 80.0);

        let hit = optimized_specific_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(10.0, 10.0),
            root.root,
        )
        .expect("document root query should succeed");

        assert_eq!(
            hit,
            Some(crate::domains::layer_hit::LayerClusterHit {
                node_key: root.root,
                point: point(10.0, 10.0),
                effective_rect: viewport_rect(&root).expect("viewport rect"),
            }),
            "document root should resolve as a specific hit layer",
        );
    }

    #[test]
    fn optimized_topmost_layer_hit_rejects_root_entity_reparented_away_from_attached_parent() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let mut world = attach_root_to_world(&mut root);
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("reparented root proof should present once");
        let root_entity = root.layer_entity(root.root).expect("root layer entity");
        let wrong_parent = world.spawn_empty().id();
        world.entity_mut(root_entity).insert(ChildOf(wrong_parent));

        let error = optimized_topmost_layer_hit_from_root_point(
            &root,
            &world,
            viewport_rect(&root).expect("viewport rect"),
            point(10.0, 10.0),
        )
        .expect_err("reparented root layer should be rejected");

        assert_eq!(error, crate::types::DomError::InvalidParentTarget);
    }

    #[test]
    fn overlapping_tree_without_raised_layers_prefers_last_branch_topmost() {
        let fixture = OverlappingLayerTree::new(&[]);

        assert_overlapping_tree_hit_order(
            &fixture,
            &[
                fixture.layer_g,
                fixture.layer_f,
                fixture.layer_e,
                fixture.layer_d,
                fixture.layer_c,
                fixture.layer_b,
                fixture.layer_a,
            ],
            Some(fixture.layer_g),
        );
    }

    #[test]
    fn overlapping_tree_with_b_raised_prefers_b_subtree_before_ordinary_branches() {
        let fixture = OverlappingLayerTree::new(&["b"]);

        assert_overlapping_tree_hit_order(
            &fixture,
            &[
                fixture.layer_c,
                fixture.layer_b,
                fixture.layer_g,
                fixture.layer_f,
                fixture.layer_e,
                fixture.layer_d,
                fixture.layer_a,
            ],
            Some(fixture.layer_c),
        );
    }

    #[test]
    fn overlapping_tree_with_d_raised_prefers_d_subtree_before_ordinary_branches() {
        let fixture = OverlappingLayerTree::new(&["d"]);

        assert_overlapping_tree_hit_order(
            &fixture,
            &[
                fixture.layer_e,
                fixture.layer_d,
                fixture.layer_g,
                fixture.layer_f,
                fixture.layer_c,
                fixture.layer_b,
                fixture.layer_a,
            ],
            Some(fixture.layer_e),
        );
    }

    #[test]
    fn overlapping_tree_with_a_raised_preserves_subtree_local_order() {
        let fixture = OverlappingLayerTree::new(&["a"]);

        assert_overlapping_tree_hit_order(
            &fixture,
            &[
                fixture.layer_g,
                fixture.layer_f,
                fixture.layer_e,
                fixture.layer_d,
                fixture.layer_c,
                fixture.layer_b,
                fixture.layer_a,
            ],
            Some(fixture.layer_g),
        );
    }

    #[test]
    fn overlapping_tree_with_b_and_d_raised_prefers_later_raised_sibling_first() {
        let fixture = OverlappingLayerTree::new(&["b", "d"]);

        assert_overlapping_tree_hit_order(
            &fixture,
            &[
                fixture.layer_e,
                fixture.layer_d,
                fixture.layer_c,
                fixture.layer_b,
                fixture.layer_g,
                fixture.layer_f,
                fixture.layer_a,
            ],
            Some(fixture.layer_e),
        );
    }

    fn attach_root_to_world(root: &mut RootDom) -> World {
        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("test root should attach");
        world
    }

    fn point(x_px: f32, y_px: f32) -> PointPx {
        PointPx { x_px, y_px }
    }

    fn layer_hit_keys(layer_hits: &[LayerClusterHit]) -> Vec<DomNodeSlotKey> {
        layer_hits.iter().map(|hit| hit.node_key).collect()
    }

    struct OffsetNestedRaisedHitTree {
        root: RootDom,
        world: World,
        parent_layer: DomNodeSlotKey,
        raised_parent: DomNodeSlotKey,
        ordinary_child: DomNodeSlotKey,
        raised_child: DomNodeSlotKey,
    }

    impl OffsetNestedRaisedHitTree {
        fn new() -> Self {
            let mut root = RootDom::new(RootDomContext::new_gpu());
            let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
            let raised_parent = root.create_element(DomTag::Div).expect("raised parent");
            let ordinary_child = root.create_element(DomTag::Div).expect("ordinary child");
            let raised_child = root.create_element(DomTag::Div).expect("raised child");

            root.append_child(root.root, parent_layer)
                .expect("append parent layer");
            root.append_child(parent_layer, raised_parent)
                .expect("append raised parent");
            root.append_child(raised_parent, ordinary_child)
                .expect("append ordinary child");
            root.append_child(raised_parent, raised_child)
                .expect("append raised child");
            root.set_attr_str(parent_layer, "position", "fixed")
                .expect("parent position");
            root.set_attr_str(parent_layer, "left", "100px")
                .expect("parent left");
            root.set_attr_str(parent_layer, "top", "50px")
                .expect("parent top");
            root.set_attr_str(parent_layer, "width", "200px")
                .expect("parent width");
            root.set_attr_str(parent_layer, "height", "150px")
                .expect("parent height");
            root.set_attr_str(raised_parent, "position", "absolute")
                .expect("raised parent position");
            root.set_attr_str(raised_parent, "left", "20px")
                .expect("raised parent left");
            root.set_attr_str(raised_parent, "top", "30px")
                .expect("raised parent top");
            root.set_attr_str(raised_parent, "width", "120px")
                .expect("raised parent width");
            root.set_attr_str(raised_parent, "height", "80px")
                .expect("raised parent height");
            root.set_layout_layer_order(raised_parent, crate::types::LayerOrder::Raised)
                .expect("raised parent layer order");
            root.set_attr_str(ordinary_child, "position", "absolute")
                .expect("ordinary child position");
            root.set_attr_str(ordinary_child, "left", "10px")
                .expect("ordinary child left");
            root.set_attr_str(ordinary_child, "top", "10px")
                .expect("ordinary child top");
            root.set_attr_str(ordinary_child, "width", "80px")
                .expect("ordinary child width");
            root.set_attr_str(ordinary_child, "height", "40px")
                .expect("ordinary child height");
            root.set_attr_str(raised_child, "position", "absolute")
                .expect("raised child position");
            root.set_attr_str(raised_child, "left", "15px")
                .expect("raised child left");
            root.set_attr_str(raised_child, "top", "15px")
                .expect("raised child top");
            root.set_attr_str(raised_child, "width", "50px")
                .expect("raised child width");
            root.set_attr_str(raised_child, "height", "30px")
                .expect("raised child height");
            root.set_layout_layer_order(raised_child, crate::types::LayerOrder::Raised)
                .expect("raised child layer order");

            let mut world = attach_root_to_world(&mut root);
            root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
                .expect("offset nested raised proof should present once");
            let root_key = root.root;
            set_box(&mut root, root_key, 0.0, 0.0, 400.0, 300.0);
            set_box(&mut root, parent_layer, 100.0, 50.0, 200.0, 150.0);
            set_box(&mut root, raised_parent, 20.0, 30.0, 120.0, 80.0);
            set_box(&mut root, ordinary_child, 10.0, 10.0, 80.0, 40.0);
            set_box(&mut root, raised_child, 15.0, 15.0, 50.0, 30.0);

            Self {
                root,
                world,
                parent_layer,
                raised_parent,
                ordinary_child,
                raised_child,
            }
        }

        fn hit_point() -> PointPx {
            point(140.0, 100.0)
        }

        fn expected_topmost_hit_keys(&self) -> Vec<DomNodeSlotKey> {
            vec![
                self.raised_child,
                self.ordinary_child,
                self.raised_parent,
                self.parent_layer,
                self.root.root,
            ]
        }
    }

    struct OverlappingLayerTree {
        root: RootDom,
        world: World,
        layer_a: DomNodeSlotKey,
        layer_b: DomNodeSlotKey,
        layer_c: DomNodeSlotKey,
        layer_d: DomNodeSlotKey,
        layer_e: DomNodeSlotKey,
        layer_f: DomNodeSlotKey,
        layer_g: DomNodeSlotKey,
    }

    impl OverlappingLayerTree {
        fn new(raised_labels: &[&str]) -> Self {
            let mut root = RootDom::new(RootDomContext::new_gpu());
            let layer_a = root.create_element(DomTag::Div).expect("layer a");
            let layer_b = root.create_element(DomTag::Div).expect("layer b");
            let layer_c = root.create_element(DomTag::Div).expect("layer c");
            let layer_d = root.create_element(DomTag::Div).expect("layer d");
            let layer_e = root.create_element(DomTag::Div).expect("layer e");
            let layer_f = root.create_element(DomTag::Div).expect("layer f");
            let layer_g = root.create_element(DomTag::Div).expect("layer g");

            root.append_child(root.root, layer_a).expect("append a");
            root.append_child(layer_a, layer_b).expect("append b");
            root.append_child(layer_b, layer_c).expect("append c");
            root.append_child(layer_a, layer_d).expect("append d");
            root.append_child(layer_d, layer_e).expect("append e");
            root.append_child(layer_a, layer_f).expect("append f");
            root.append_child(layer_f, layer_g).expect("append g");

            for layer in [
                layer_a, layer_b, layer_c, layer_d, layer_e, layer_f, layer_g,
            ] {
                root.set_attr_str(layer, "position", "absolute")
                    .expect("layer position");
                root.set_attr_str(layer, "width", "80px")
                    .expect("layer width");
                root.set_attr_str(layer, "height", "80px")
                    .expect("layer height");
                set_box(&mut root, layer, 0.0, 0.0, 80.0, 80.0);
            }
            let root_key = root.root;
            set_box(&mut root, root_key, 0.0, 0.0, 160.0, 160.0);

            for label in raised_labels {
                let key = match *label {
                    "a" => layer_a,
                    "b" => layer_b,
                    "c" => layer_c,
                    "d" => layer_d,
                    "e" => layer_e,
                    "f" => layer_f,
                    "g" => layer_g,
                    _ => panic!("unknown overlapping layer label: {label}"),
                };
                root.set_layout_layer_order(key, crate::types::LayerOrder::Raised)
                    .expect("raised layer order");
            }

            let mut world = attach_root_to_world(&mut root);
            root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
                .expect("overlapping layer proof should present once");

            Self {
                root,
                world,
                layer_a,
                layer_b,
                layer_c,
                layer_d,
                layer_e,
                layer_f,
                layer_g,
            }
        }
    }

    fn assert_overlapping_tree_hit_order(
        fixture: &OverlappingLayerTree,
        expected_non_document_order: &[DomNodeSlotKey],
        expected_topmost: Option<DomNodeSlotKey>,
    ) {
        let point = point(10.0, 10.0);
        let expected =
            crate::domains::hit_test::hit_layer_cluster_hits_from_root_point(&fixture.root, point)
                .expect("legacy overlapping hits should succeed");
        let expected_non_document = non_document_layer_keys(&fixture.root, &expected);
        let topmost = optimized_topmost_layer_hit_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
        )
        .expect("optimized overlapping topmost hit should succeed");
        let mut optimized_hits = Vec::new();
        append_optimized_all_layer_hits_from_root_point(
            &fixture.root,
            &fixture.world,
            viewport_rect(&fixture.root).expect("viewport rect"),
            point,
            &mut optimized_hits,
        )
        .expect("optimized overlapping hits should succeed");
        let optimized_non_document = non_document_layer_keys(&fixture.root, &optimized_hits);

        assert_eq!(
            expected_non_document, expected_non_document_order,
            "legacy clustered search should define the expected non-document hit order for this overlapping tree case",
        );
        assert_eq!(
            optimized_non_document, expected_non_document_order,
            "optimized raised-aware hit ordering should match the expected overlapping-tree order",
        );
        assert_eq!(
            topmost.map(|hit| hit.node_key),
            expected_topmost,
            "optimized topmost hit should match the expected overlapping-tree winner",
        );
        assert_unique_layer_keys(&optimized_non_document);
        assert_eq!(
            optimized_non_document.len(),
            7,
            "the overlapping fixture should visit each non-document layer exactly once",
        );
    }

    fn non_document_layer_keys(
        root: &RootDom,
        layer_hits: &[crate::domains::layer_hit::LayerClusterHit],
    ) -> Vec<DomNodeSlotKey> {
        layer_hits
            .iter()
            .filter_map(|hit| {
                if hit.node_key == root.root {
                    None
                } else {
                    Some(hit.node_key)
                }
            })
            .collect()
    }

    fn assert_unique_layer_keys(layer_keys: &[DomNodeSlotKey]) {
        for (index, key) in layer_keys.iter().enumerate() {
            assert!(
                !layer_keys[index + 1..].contains(key),
                "layer hit order should not duplicate ownership for {key:?}",
            );
        }
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
