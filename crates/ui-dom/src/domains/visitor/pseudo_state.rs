//! Layer-aware pseudo-state visitor updates for `ui-dom`.
//!
//! Why this visitor exists:
//!
//! - hover and active are not scroll
//! - hover and active are not point-target event dispatch
//! - one global DOM chain is not accurate enough for pseudo-state hit
//!   resolution
//! - scroll offsets, clipped overflow, and absolute/fixed positioned layers
//!   mean one point can participate in several presented layers at once
//!
//! What this visitor does:
//!
//! - resolve the current ordered hovered layer sequence for `(x, y)`
//! - keep one retained element chain for each participating layer
//! - reconcile retained state with a strict 3-stage diff:
//!   - walk until divergence
//!   - clear the old suffix
//!   - set the new suffix
//! - reuse the same shape for layer sequences and per-layer element chains
//!
//! Ownership is split deliberately so each boundary is testable:
//!
//! - geometry resolution owns hit-testing and layer/element ordering
//! - retained diff owns old-vs-new chain comparison
//! - mutation owns direct pseudo-state writes and local hover transition events
//! - orchestration owns the current update flow across those boundaries
//!
//! The hot path is memory-stable because the retained layer sequence and its
//! per-layer element chains are reused across updates.
//!
//! Constraints this file is protecting:
//!
//! - do not collapse hover/active into a naive single DOM ancestry walk
//! - do not rebuild pseudo-state by returning paths for later replay
//! - do not move pseudo-state mutation back into `RootDom`
//! - do not mix scroll or bubbling point-target dispatch into this visitor
use bevy::prelude::World;

use crate::domains::hit_test::{
    hit_layer_cluster_hits_from_root_point, hit_test_from_dom_subtree_cluster_local,
    topmost_active_pseudo_target_from_dom_subtree_cluster_local,
};
use crate::domains::layer_hit::{LayerClusterHit, visit_attached_layer_hits_from_root_point};
use crate::types::primitives::PointPx;
use crate::types::{
    DomError, DomEvent, DomEventDetail, DomEventName, DomNodeSlotKey, DomPointerEvent,
    DomPseudoState, HoveredLayerEntry, RootDom, RootDomMetricName,
};
#[cfg(test)]
use crate::types::{DomModifierState, DomPointerType};

#[cfg(test)]
pub(crate) fn sync_hover_from_point(
    root_dom: &mut RootDom,
    point: PointPx,
) -> Result<(), DomError> {
    let previous_hovered_layers = root_dom.take_hovered_layers();
    let mut current_hovered_layers = root_dom.take_hovered_layers_work();
    current_hovered_layers.clear();
    resolve_hovered_layers_from_point(root_dom, point, &mut current_hovered_layers)?;
    apply_hovered_layers(
        root_dom,
        &previous_hovered_layers,
        &current_hovered_layers,
        hover_transition_pointer_event(point),
    )?;
    root_dom.store_hovered_layers(current_hovered_layers);
    root_dom.store_hovered_layers_work(previous_hovered_layers);
    Ok(())
}

pub(crate) fn sync_hover_from_attached_pointer(
    root_dom: &mut RootDom,
    world: &World,
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    let point = PointPx {
        x_px: pointer_event.client_x,
        y_px: pointer_event.client_y,
    };
    let previous_hovered_layers = root_dom.take_hovered_layers();
    let mut current_hovered_layers = root_dom.take_hovered_layers_work();
    current_hovered_layers.clear();
    resolve_hovered_layers_from_attached_point(
        root_dom,
        world,
        point,
        &mut current_hovered_layers,
    )?;
    apply_hovered_layers(
        root_dom,
        &previous_hovered_layers,
        &current_hovered_layers,
        pointer_event,
    )?;
    root_dom.store_hovered_layers(current_hovered_layers);
    root_dom.store_hovered_layers_work(previous_hovered_layers);
    Ok(())
}

pub(crate) fn active_pointer_target_from_point(
    root_dom: &RootDom,
    point: PointPx,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    resolve_active_pointer_target_from_point(root_dom, point)
}

pub(crate) fn active_pointer_target_from_attached_point(
    root_dom: &RootDom,
    world: &World,
    point: PointPx,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    resolve_active_pointer_target_from_attached_point(root_dom, world, point)
}

// Geometry resolution

#[cfg(test)]
fn resolve_hovered_layers_from_point(
    root_dom: &RootDom,
    point: PointPx,
    hovered_layers: &mut Vec<HoveredLayerEntry>,
) -> Result<(), DomError> {
    let layer_hits = hit_layer_cluster_hits_from_root_point(root_dom, point)?;
    for layer_hit in layer_hits {
        let Some(hit) = hit_test_from_dom_subtree_cluster_local(
            root_dom,
            layer_hit.node_key,
            layer_hit.point,
            layer_hit.effective_rect,
        )?
        else {
            continue;
        };
        push_hovered_layer_from_path(root_dom, hit.path, hovered_layers)?;
    }
    if hovered_layers.is_empty()
        && let Some(hit) = hit_test_from_dom_subtree_cluster_local(
            root_dom,
            root_dom.root,
            point,
            detached_root_effective_rect(root_dom)?,
        )?
    {
        push_hovered_layer_from_path(root_dom, hit.path, hovered_layers)?;
    }
    Ok(())
}

#[cfg(test)]
fn detached_root_effective_rect(root_dom: &RootDom) -> Result<crate::types::DomRect, DomError> {
    let root_node = root_dom
        .node(root_dom.root)
        .ok_or(DomError::InvalidNodeKey)?;
    let root_box = crate::domains::layout::node_box_from_ref(root_node).border_box;
    if root_box.width_px > 1.0 && root_box.height_px > 1.0 {
        return Ok(crate::types::DomRect {
            left_px: 0.0,
            top_px: 0.0,
            width_px: root_box.width_px,
            height_px: root_box.height_px,
        });
    }
    Ok(crate::types::DomRect {
        left_px: -1_000_000.0,
        top_px: -1_000_000.0,
        width_px: 2_000_000.0,
        height_px: 2_000_000.0,
    })
}

fn resolve_hovered_layers_from_attached_point(
    root_dom: &RootDom,
    world: &World,
    point: PointPx,
    hovered_layers: &mut Vec<HoveredLayerEntry>,
) -> Result<(), DomError> {
    visit_attached_layer_subtree_hits_from_point(root_dom, world, point, &mut |layer_hit| {
        let Some(hit) = hit_test_from_dom_subtree_cluster_local(
            root_dom,
            layer_hit.node_key,
            layer_hit.point,
            layer_hit.effective_rect,
        )?
        else {
            return Ok(false);
        };
        push_hovered_layer_from_path(root_dom, hit.path, hovered_layers)?;
        Ok(false)
    })?;
    Ok(())
}

fn resolve_active_pointer_target_from_point(
    root_dom: &RootDom,
    point: PointPx,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    for layer_hit in hit_layer_cluster_hits_from_root_point(root_dom, point)? {
        let Some(target_key) = topmost_active_pseudo_target_from_dom_subtree_cluster_local(
            root_dom,
            layer_hit.node_key,
            layer_hit.point,
            layer_hit.effective_rect,
        )?
        else {
            continue;
        };
        return Ok(Some(target_key));
    }
    Ok(None)
}

fn resolve_active_pointer_target_from_attached_point(
    root_dom: &RootDom,
    world: &World,
    point: PointPx,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let mut active_pointer_target = None;
    visit_attached_layer_subtree_hits_from_point(root_dom, world, point, &mut |layer_hit| {
        let Some(target_key) = topmost_active_pseudo_target_from_dom_subtree_cluster_local(
            root_dom,
            layer_hit.node_key,
            layer_hit.point,
            layer_hit.effective_rect,
        )?
        else {
            return Ok(false);
        };
        active_pointer_target = Some(target_key);
        Ok(true)
    })?;
    if active_pointer_target.is_some() {
        return Ok(active_pointer_target);
    }
    Ok(None)
}

fn visit_attached_layer_subtree_hits_from_point<F>(
    root_dom: &RootDom,
    world: &World,
    point: PointPx,
    on_hit: &mut F,
) -> Result<(), DomError>
where
    F: FnMut(LayerClusterHit) -> Result<bool, DomError>,
{
    visit_attached_layer_hits_from_root_point(root_dom, world, point, on_hit)
}

fn push_hovered_layer_from_path(
    root_dom: &RootDom,
    layer_path: Vec<DomNodeSlotKey>,
    hovered_layers: &mut Vec<HoveredLayerEntry>,
) -> Result<(), DomError> {
    let Some(layer_root_key) = layer_path.first().copied() else {
        return Ok(());
    };
    let mut element_chain_root_to_target = Vec::with_capacity(layer_path.len());
    for key in layer_path {
        let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
        if node.as_element().is_some() {
            element_chain_root_to_target.push(key);
        }
    }
    if element_chain_root_to_target.is_empty() {
        return Ok(());
    }
    hovered_layers.push(HoveredLayerEntry {
        layer_root_key,
        element_chain_root_to_target,
    });
    Ok(())
}

// Retained diff

fn first_layer_divergence_index(
    previous_hovered_layers: &[HoveredLayerEntry],
    current_hovered_layers: &[HoveredLayerEntry],
) -> usize {
    previous_hovered_layers
        .iter()
        .zip(current_hovered_layers.iter())
        .take_while(|(previous, current)| previous.layer_root_key == current.layer_root_key)
        .count()
}

fn first_element_divergence_index(
    previous_hovered_chain: &[DomNodeSlotKey],
    current_hovered_chain: &[DomNodeSlotKey],
) -> usize {
    previous_hovered_chain
        .iter()
        .zip(current_hovered_chain.iter())
        .take_while(|(previous, current)| previous == current)
        .count()
}

// Mutation

fn clear_hovered_layer_suffix(
    root_dom: &mut RootDom,
    hovered_layers: &[HoveredLayerEntry],
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    for hovered_layer in hovered_layers {
        clear_hover_chain_suffix(
            root_dom,
            &hovered_layer.element_chain_root_to_target,
            pointer_event,
        )?;
    }
    Ok(())
}

fn set_hovered_layer_suffix(
    root_dom: &mut RootDom,
    hovered_layers: &[HoveredLayerEntry],
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    for hovered_layer in hovered_layers {
        set_hover_chain_suffix(
            root_dom,
            &hovered_layer.element_chain_root_to_target,
            pointer_event,
        )?;
    }
    Ok(())
}

fn clear_hover_chain_suffix(
    root_dom: &mut RootDom,
    hovered_chain: &[DomNodeSlotKey],
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    for &element_key in hovered_chain.iter().rev() {
        if clear_hover_for_element(root_dom, element_key) {
            dispatch_hover_transition_event(
                root_dom,
                element_key,
                DomEventName::PointerLeave,
                pointer_event,
            )?;
        }
    }
    Ok(())
}

fn set_hover_chain_suffix(
    root_dom: &mut RootDom,
    hovered_chain: &[DomNodeSlotKey],
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    for &element_key in hovered_chain {
        if set_hover_for_element(root_dom, element_key)? {
            dispatch_hover_transition_event(
                root_dom,
                element_key,
                DomEventName::PointerEnter,
                pointer_event,
            )?;
        }
    }
    Ok(())
}

fn set_hover_for_element(
    root_dom: &mut RootDom,
    element_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let style_sensitive = hover_style_sensitive_for_element(root_dom, element_key)?;
    {
        let node = root_dom
            .nodes
            .get_mut(element_key)
            .ok_or(DomError::InvalidNodeKey)?;
        let element = node
            .as_element_mut()
            .ok_or(DomError::InvalidPseudoStateTarget)?;
        if element.has_pseudo_state(DomPseudoState::Hover) {
            return Ok(false);
        }
        element.set_pseudo_state(DomPseudoState::Hover);
    }
    root_dom.increment_metric(RootDomMetricName::HoverPseudoStateSet);
    if style_sensitive {
        root_dom.mark_style_dirty_for_attribute_change(element_key);
    }
    Ok(true)
}

fn clear_hover_for_element(root_dom: &mut RootDom, element_key: DomNodeSlotKey) -> bool {
    let style_sensitive = hover_style_sensitive_for_element(root_dom, element_key).unwrap_or(false);
    let Some(element) = root_dom
        .nodes
        .get_mut(element_key)
        .and_then(crate::types::DomNode::as_element_mut)
    else {
        return false;
    };
    if !element.has_pseudo_state(DomPseudoState::Hover) {
        return false;
    }
    element.clear_pseudo_state(DomPseudoState::Hover);
    root_dom.increment_metric(RootDomMetricName::HoverPseudoStateCleared);
    if style_sensitive {
        root_dom.mark_style_dirty_for_attribute_change(element_key);
    }
    true
}

fn hover_style_sensitive_for_element(
    root_dom: &RootDom,
    element_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let element = root_dom
        .nodes
        .get(element_key)
        .and_then(crate::types::DomNode::as_element)
        .ok_or(DomError::InvalidPseudoStateTarget)?;
    Ok(root_dom
        .context()
        .uses_pseudo_state_styles(DomPseudoState::Hover)
        || element
            .style()
            .has_pseudo_state_declarations(DomPseudoState::Hover))
}

// Orchestration

fn apply_hovered_layers(
    root_dom: &mut RootDom,
    previous_hovered_layers: &[HoveredLayerEntry],
    current_hovered_layers: &[HoveredLayerEntry],
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    let layer_divergence_index =
        first_layer_divergence_index(previous_hovered_layers, current_hovered_layers);

    for layer_index in 0..layer_divergence_index {
        apply_hovered_elements_in_shared_layer(
            root_dom,
            &previous_hovered_layers[layer_index],
            &current_hovered_layers[layer_index],
            pointer_event,
        )?;
    }

    clear_hovered_layer_suffix(
        root_dom,
        &previous_hovered_layers[layer_divergence_index..],
        pointer_event,
    )?;
    set_hovered_layer_suffix(
        root_dom,
        &current_hovered_layers[layer_divergence_index..],
        pointer_event,
    )?;
    Ok(())
}

fn apply_hovered_elements_in_shared_layer(
    root_dom: &mut RootDom,
    previous_hovered_layer: &HoveredLayerEntry,
    current_hovered_layer: &HoveredLayerEntry,
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    let divergence_index = first_element_divergence_index(
        &previous_hovered_layer.element_chain_root_to_target,
        &current_hovered_layer.element_chain_root_to_target,
    );
    clear_hover_chain_suffix(
        root_dom,
        &previous_hovered_layer.element_chain_root_to_target[divergence_index..],
        pointer_event,
    )?;
    set_hover_chain_suffix(
        root_dom,
        &current_hovered_layer.element_chain_root_to_target[divergence_index..],
        pointer_event,
    )
}

fn dispatch_hover_transition_event(
    root_dom: &mut RootDom,
    element_key: DomNodeSlotKey,
    event_name: DomEventName,
    pointer_event: DomPointerEvent,
) -> Result<(), DomError> {
    let event = DomEvent::new(event_name, DomEventDetail::Pointer(pointer_event));
    let _ = root_dom.dispatch_event_to_target_key_and_record_if_bubbled(element_key, event)?;
    Ok(())
}

#[cfg(test)]
fn hover_transition_pointer_event(point: PointPx) -> DomPointerEvent {
    DomPointerEvent {
        pointer_id: 1,
        pointer_type: DomPointerType::Mouse,
        is_primary: true,
        button: None,
        buttons: 0,
        client_x: point.x_px,
        client_y: point.y_px,
        modifiers: DomModifierState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_pointer_target_from_attached_point, active_pointer_target_from_point,
        first_element_divergence_index, first_layer_divergence_index,
        hover_transition_pointer_event, sync_hover_from_attached_pointer, sync_hover_from_point,
    };
    use crate::types::primitives::PointPx;
    use crate::types::{
        DomEventName, DomNode, DomNodeSlotKey, DomPseudoState, DomTag, HoveredLayerEntry, RootDom,
        RootDomContext, RootDomMetricName,
    };
    use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};
    use bevy::{image::Image, prelude::*};
    use slotmap::KeyData;
    use std::sync::{Arc, Mutex};

    fn key(value: u64) -> DomNodeSlotKey {
        DomNodeSlotKey::from(KeyData::from_ffi(value))
    }

    fn hovered_layer(layer_root_key: u64, element_chain: &[u64]) -> HoveredLayerEntry {
        HoveredLayerEntry {
            layer_root_key: key(layer_root_key),
            element_chain_root_to_target: element_chain.iter().copied().map(key).collect(),
        }
    }

    fn metric_count(root: &RootDom, name: RootDomMetricName) -> u64 {
        root.metrics().count(name)
    }

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    fn point(x_px: f32, y_px: f32) -> PointPx {
        PointPx { x_px, y_px }
    }

    #[test]
    fn element_divergence_is_full_length_when_chains_match() {
        let previous = [key(1), key(2), key(3)];
        let current = [key(1), key(2), key(3)];
        assert_eq!(first_element_divergence_index(&previous, &current), 3);
    }

    #[test]
    fn element_divergence_stops_at_first_changed_key() {
        let previous = [key(1), key(2), key(3)];
        let current = [key(1), key(9), key(3)];
        assert_eq!(first_element_divergence_index(&previous, &current), 1);
    }

    #[test]
    fn layer_divergence_stops_at_first_changed_layer_root() {
        let previous = [hovered_layer(10, &[1, 2]), hovered_layer(20, &[3, 4])];
        let current = [hovered_layer(10, &[1, 2]), hovered_layer(99, &[3, 4])];
        assert_eq!(first_layer_divergence_index(&previous, &current), 1);
    }

    #[test]
    fn sync_hover_marks_topmost_same_layer_overlay() {
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

        sync_hover_from_point(&mut root, point(5.0, 5.0)).expect("hover sync should succeed");

        assert!(element_has_state(&root, overlay, DomPseudoState::Hover));
        assert!(!element_has_state(&root, underlay, DomPseudoState::Hover));
    }

    #[test]
    fn sync_hover_emits_pointer_enter_only_for_exact_listeners() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        let seen = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let enter_seen = seen.clone();
        root.add_event_listener(
            button,
            DomEventName::PointerEnter,
            crate::types::dom_event::DomEventListener::new(move |_| {
                enter_seen.lock().expect("events should lock").push("enter");
            }),
            None,
        )
        .expect("pointer enter listener");

        let move_seen = seen.clone();
        root.add_event_listener(
            button,
            DomEventName::PointerMove,
            crate::types::dom_event::DomEventListener::new(move |_| {
                move_seen.lock().expect("events should lock").push("move");
            }),
            None,
        )
        .expect("pointer move listener");

        sync_hover_from_point(&mut root, point(5.0, 5.0)).expect("hover sync should succeed");

        assert_eq!(
            seen.lock().expect("events should lock").as_slice(),
            ["enter"],
            "hover sync should emit only the exact non-bubbling hover event",
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("unattached unchanged update should drain queued events");
        assert!(
            drained.is_empty(),
            "pointer enter should stay non-draining even when a listener sees it",
        );
    }

    #[test]
    fn sync_hover_emits_pointer_leave_only_for_exact_listeners() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        let seen = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let leave_seen = seen.clone();
        root.add_event_listener(
            button,
            DomEventName::PointerLeave,
            crate::types::dom_event::DomEventListener::new(move |_| {
                leave_seen.lock().expect("events should lock").push("leave");
            }),
            None,
        )
        .expect("pointer leave listener");

        let enter_seen = seen.clone();
        root.add_event_listener(
            button,
            DomEventName::PointerEnter,
            crate::types::dom_event::DomEventListener::new(move |_| {
                enter_seen.lock().expect("events should lock").push("enter");
            }),
            None,
        )
        .expect("pointer enter listener");

        sync_hover_from_point(&mut root, point(5.0, 5.0)).expect("first hover sync should succeed");
        seen.lock().expect("events should lock").clear();

        sync_hover_from_point(&mut root, point(60.0, 60.0))
            .expect("second hover sync should succeed");

        assert_eq!(
            seen.lock().expect("events should lock").as_slice(),
            ["leave"],
            "hover exit should emit only the exact non-bubbling leave event",
        );
    }

    #[test]
    fn repeated_nested_layer_hover_sync_keeps_hover_mutation_metrics_flat() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let graph_node = root.create_element(DomTag::Div).expect("graph node");
        let titlebar = root.create_element(DomTag::Div).expect("titlebar");
        let body = root.create_element(DomTag::Div).expect("body");

        root.append_child(root.root, graph_node)
            .expect("append graph node");
        root.append_child(graph_node, titlebar)
            .expect("append titlebar");
        root.append_child(graph_node, body).expect("append body");

        root.set_attr_str(graph_node, "position", "fixed")
            .expect("graph node fixed");
        root.set_attr_str(graph_node, "width", "120px")
            .expect("graph node width");
        root.set_attr_str(graph_node, "height", "80px")
            .expect("graph node height");
        root.set_attr_str(titlebar, "position", "absolute")
            .expect("titlebar absolute");
        root.set_attr_str(titlebar, "left", "0px")
            .expect("titlebar left");
        root.set_attr_str(titlebar, "top", "0px")
            .expect("titlebar top");
        root.set_attr_str(titlebar, "width", "120px")
            .expect("titlebar width");
        root.set_attr_str(titlebar, "height", "20px")
            .expect("titlebar height");
        root.set_attr_str(body, "position", "absolute")
            .expect("body absolute");
        root.set_attr_str(body, "left", "0px").expect("body left");
        root.set_attr_str(body, "top", "20px").expect("body top");
        root.set_attr_str(body, "width", "120px")
            .expect("body width");
        root.set_attr_str(body, "height", "60px")
            .expect("body height");
        set_box(&mut root, graph_node, 0.0, 0.0, 120.0, 80.0);
        set_box(&mut root, titlebar, 0.0, 0.0, 120.0, 20.0);
        set_box(&mut root, body, 0.0, 20.0, 120.0, 60.0);

        sync_hover_from_point(&mut root, point(10.0, 10.0))
            .expect("first nested hover sync should succeed");

        root.reset_metrics();

        sync_hover_from_point(&mut root, point(10.0, 10.0))
            .expect("second nested hover sync should succeed");

        assert_eq!(
            metric_count(&root, RootDomMetricName::HoverPseudoStateSet),
            0
        );
        assert_eq!(
            metric_count(&root, RootDomMetricName::HoverPseudoStateCleared),
            0
        );
    }

    #[test]
    fn moved_point_inside_same_nested_layer_stack_keeps_hover_mutation_metrics_flat() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let graph_node = root.create_element(DomTag::Div).expect("graph node");
        let titlebar = root.create_element(DomTag::Div).expect("titlebar");
        let body = root.create_element(DomTag::Div).expect("body");

        root.append_child(root.root, graph_node)
            .expect("append graph node");
        root.append_child(graph_node, titlebar)
            .expect("append titlebar");
        root.append_child(graph_node, body).expect("append body");

        root.set_attr_str(graph_node, "position", "fixed")
            .expect("graph node fixed");
        root.set_attr_str(graph_node, "width", "120px")
            .expect("graph node width");
        root.set_attr_str(graph_node, "height", "80px")
            .expect("graph node height");
        root.set_attr_str(titlebar, "position", "absolute")
            .expect("titlebar absolute");
        root.set_attr_str(titlebar, "left", "0px")
            .expect("titlebar left");
        root.set_attr_str(titlebar, "top", "0px")
            .expect("titlebar top");
        root.set_attr_str(titlebar, "width", "120px")
            .expect("titlebar width");
        root.set_attr_str(titlebar, "height", "20px")
            .expect("titlebar height");
        root.set_attr_str(body, "position", "absolute")
            .expect("body absolute");
        root.set_attr_str(body, "left", "0px").expect("body left");
        root.set_attr_str(body, "top", "20px").expect("body top");
        root.set_attr_str(body, "width", "120px")
            .expect("body width");
        root.set_attr_str(body, "height", "60px")
            .expect("body height");
        set_box(&mut root, graph_node, 0.0, 0.0, 120.0, 80.0);
        set_box(&mut root, titlebar, 0.0, 0.0, 120.0, 20.0);
        set_box(&mut root, body, 0.0, 20.0, 120.0, 60.0);

        sync_hover_from_point(&mut root, point(10.0, 10.0))
            .expect("first nested hover sync should succeed");

        root.reset_metrics();

        sync_hover_from_point(&mut root, point(11.0, 11.0))
            .expect("moved nested hover sync should succeed");

        assert_eq!(
            metric_count(&root, RootDomMetricName::HoverPseudoStateSet),
            0
        );
        assert_eq!(
            metric_count(&root, RootDomMetricName::HoverPseudoStateCleared),
            0
        );
    }

    #[test]
    fn active_pointer_target_picks_underlay_from_overlapping_absolute_layers() {
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

        let target = active_pointer_target_from_point(
            &root,
            crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
        )
        .expect("active target lookup should succeed");

        assert_eq!(target, Some(underlay));
    }

    #[test]
    fn sync_hover_follows_retained_scroll_inside_a_clipped_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Button).expect("second item");

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

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(80.0, 80.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");
        set_box(&mut root, scroll_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, first_item, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, second_item, 0.0, 40.0, 80.0, 40.0);
        assert!(
            root.apply_vertical_scroll_to_layer_root(scroll_root, -60.0)
                .expect("scroll should succeed"),
            "scroll root should accept retained scroll",
        );

        sync_hover_from_point(&mut root, point(10.0, 10.0)).expect("hover sync should succeed");

        assert!(element_has_state(&root, second_item, DomPseudoState::Hover));
        assert!(!element_has_state(&root, first_item, DomPseudoState::Hover));
    }

    #[test]
    fn active_pointer_target_follows_retained_scroll_inside_a_clipped_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Button).expect("second item");

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

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(80.0, 80.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");
        set_box(&mut root, scroll_root, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, first_item, 0.0, 0.0, 80.0, 40.0);
        set_box(&mut root, second_item, 0.0, 40.0, 80.0, 40.0);
        assert!(
            root.apply_vertical_scroll_to_layer_root(scroll_root, -60.0)
                .expect("scroll should succeed"),
            "scroll root should accept retained scroll",
        );

        let target = active_pointer_target_from_point(
            &root,
            crate::types::primitives::PointPx {
                x_px: 10.0,
                y_px: 10.0,
            },
        )
        .expect("active target lookup should succeed");

        assert_eq!(
            target,
            Some(second_item),
            "active pointer target should follow retained scroll under a stable cursor",
        );
    }

    #[test]
    fn attached_root_keeps_same_active_pointer_target_resolution() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let mut world = World::new();
        install_direct_gpu_support(&mut world);
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

        let target = active_pointer_target_from_point(
            &root,
            crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
        )
        .expect("active target lookup should succeed");

        assert_eq!(target, Some(underlay));
    }

    #[test]
    fn attached_hover_passes_through_when_document_root_disallows_pointer_events() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(root.root, "pointer-events", "none")
            .expect("root pointer-events should apply");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");

        sync_hover_from_attached_pointer(
            &mut root,
            &world,
            hover_transition_pointer_event(crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            }),
        )
        .expect("attached hover sync should succeed");

        assert!(
            !element_has_state(&root, button, DomPseudoState::Hover),
            "attached hover should stay empty when no presented layer allows pointer events",
        );
    }

    #[test]
    fn attached_hover_passes_through_when_non_layer_target_disallows_pointer_events() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(button, "pointer-events", "none")
            .expect("button pointer-events should apply");
        root.set_attr_str(button, "width", "40px")
            .expect("button width");
        root.set_attr_str(button, "height", "40px")
            .expect("button height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");

        sync_hover_from_attached_pointer(
            &mut root,
            &world,
            hover_transition_pointer_event(crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            }),
        )
        .expect("attached hover sync should succeed");

        assert!(
            !element_has_state(&root, button, DomPseudoState::Hover),
            "ordinary non-layer descendants with pointer-events none should not gain hover state",
        );
    }

    #[test]
    fn detached_hover_targets_explicit_auto_descendant_inside_pointer_events_none_ancestor() {
        // Remaining pseudo-state gaps:
        // - attached positive active-target opt-in under inherited pointer-events none
        // - hover/active reconciliation when the pass-through ancestor is also a layer root
        // - mixed keyboard focus plus pointer hover transitions on the opt-in descendant
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let blocker = root.create_element(DomTag::Div).expect("blocker");
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, blocker)
            .expect("append blocker");
        root.append_child(blocker, button).expect("append button");
        root.set_attr_str(blocker, "pointer-events", "none")
            .expect("blocker pointer-events should apply");
        root.set_attr_str(button, "pointer-events", "auto")
            .expect("button pointer-events should apply");
        root.set_attr_str(blocker, "width", "40px")
            .expect("blocker width");
        root.set_attr_str(blocker, "height", "40px")
            .expect("blocker height");
        root.set_attr_str(button, "width", "40px")
            .expect("button width");
        root.set_attr_str(button, "height", "40px")
            .expect("button height");
        set_box(&mut root, blocker, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        sync_hover_from_point(&mut root, point(5.0, 5.0)).expect("hover sync should succeed");

        assert!(
            element_has_state(&root, button, DomPseudoState::Hover),
            "explicit pointer-events auto descendants should opt back into hover under a none ancestor",
        );
    }

    #[test]
    fn attached_active_pointer_target_passes_through_when_document_root_disallows_pointer_events() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(root.root, "pointer-events", "none")
            .expect("root pointer-events should apply");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");

        let target = active_pointer_target_from_attached_point(
            &root,
            &world,
            crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
        )
        .expect("attached active target lookup should succeed");

        assert_eq!(
            target, None,
            "attached active pointer target lookup should pass through when no presented layer allows pointer events",
        );
    }

    #[test]
    fn attached_active_pointer_target_passes_through_when_non_layer_target_disallows_pointer_events()
     {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(button, "pointer-events", "none")
            .expect("button pointer-events should apply");
        root.set_attr_str(button, "width", "40px")
            .expect("button width");
        root.set_attr_str(button, "height", "40px")
            .expect("button height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, crate::types::DomPointerUpdate::Unchanged)
            .expect("present once");

        let target = active_pointer_target_from_attached_point(
            &root,
            &world,
            crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
        )
        .expect("attached active target lookup should succeed");

        assert_eq!(
            target, None,
            "ordinary non-layer descendants with pointer-events none should not become active pseudo targets",
        );
    }

    #[test]
    fn detached_active_pointer_target_passes_through_when_document_root_disallows_pointer_events() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(root.root, "pointer-events", "none")
            .expect("root pointer-events should apply");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

        let target = active_pointer_target_from_point(
            &root,
            crate::types::primitives::PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
        )
        .expect("detached active target lookup should succeed");

        assert_eq!(
            target, None,
            "detached active pointer target lookup should also pass through when no layer allows pointer events",
        );
    }

    fn element_has_state(
        root: &RootDom,
        key: DomNodeSlotKey,
        pseudo_state: DomPseudoState,
    ) -> bool {
        root.node(key)
            .and_then(DomNode::as_element)
            .is_some_and(|element| element.has_pseudo_state(pseudo_state))
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
