#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Point-event visitor for `ui-dom`.
//!
//! Why this visitor exists:
//!
//! - point event dispatch is its own traversal boundary
//! - it must stay separate from scroll
//! - it must stay separate from hover/active pseudo-state reconciliation
//! - the visitor must own one event context throughout dispatch
//!
//! What this visitor does:
//!
//! - traverse presented Bevy layers by rect check
//! - descend the matching DOM subtree by rect check
//! - resolve the final event target
//! - perform capture/target/bubble propagation from that resolved target
//!
//! Invariants and rules:
//!
//! - point dispatch stays in one contained file
//! - the visitor owns the mutable dispatch context for the full flow
//! - listeners see only the dispatch facade, never the owned event directly
//! - Bevy-layer selection stays iterative and allocation-free
//! - propagation walks parents iteratively and never allocates an ancestry path
//! - `dispatch_event_to_target(...)` returns sink-eligible finished events,
//!   while `dispatch_event_to_target_and_finish(...)` exposes the full finished
//!   event for owners that need cancellation truth
//!
//! Constraints this file is protecting:
//!
//! - do not split target search and propagation into separate visitor files
//! - do not build path products for later replay
//! - do not move hover/active bookkeeping into event dispatch
//! - do not borrow out the owned mutable event context to listeners
use bevy::prelude::{Entity, World};

use crate::domains::hit_test::hit_test_from_dom_subtree_cluster_local;
use crate::domains::layer_hit::topmost_attached_layer_hit_from_root_point;
use crate::types::dom_event::{DomEvent, DomEventDispatchContext};
use crate::types::primitives::PointPx;
use crate::types::{DomError, DomEventName, DomNodeSlotKey, RootDom};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DomPointEventTarget {
    pub(crate) target_key: DomNodeSlotKey,
    pub(crate) subtree_point: PointPx,
}

#[derive(Debug, PartialEq)]
pub(crate) struct DomPointEventDispatch {
    pub(crate) target: DomPointEventTarget,
    pub(crate) event: DomEvent,
}

#[allow(dead_code)]
pub(crate) fn dispatch_event_from_point(
    root_dom: &mut RootDom,
    world: &World,
    parent: Entity,
    point: PointPx,
    event: DomEvent,
) -> Result<Option<DomEvent>, DomError> {
    let Some(target_key) = topmost_event_target_from_point(root_dom, world, parent, point)? else {
        return Ok(None);
    };
    dispatch_event_to_target(root_dom, target_key, event)
}

pub(crate) fn dispatch_event_from_point_and_finish(
    root_dom: &mut RootDom,
    world: &World,
    parent: Entity,
    point: PointPx,
    event: DomEvent,
) -> Result<Option<DomPointEventDispatch>, DomError> {
    let Some(target) = topmost_point_event_target_from_point(root_dom, world, parent, point)?
    else {
        return Ok(None);
    };
    let event = dispatch_event_to_target_and_finish(root_dom, target.target_key, event)?;
    Ok(Some(DomPointEventDispatch { target, event }))
}

pub(crate) fn topmost_event_target_from_point(
    root_dom: &RootDom,
    world: &World,
    parent: Entity,
    point: PointPx,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    Ok(
        topmost_point_event_target_from_point(root_dom, world, parent, point)?
            .map(|target| target.target_key),
    )
}

pub(crate) fn topmost_point_event_target_from_point(
    root_dom: &RootDom,
    world: &World,
    _parent: Entity,
    point: PointPx,
) -> Result<Option<DomPointEventTarget>, DomError> {
    let Some(layer_hit) = topmost_attached_layer_hit_from_root_point(root_dom, world, point)?
    else {
        return Ok(None);
    };
    let Some(subtree_hit) = hit_test_from_dom_subtree_cluster_local(
        root_dom,
        layer_hit.node_key,
        layer_hit.point,
        layer_hit.effective_rect,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(DomPointEventTarget {
        target_key: nearest_non_text_target(root_dom, subtree_hit.target_key)?,
        subtree_point: layer_hit.point,
    }))
}

#[allow(dead_code)]
pub(crate) fn dispatch_event_to_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    event: DomEvent,
) -> Result<Option<DomEvent>, DomError> {
    let dispatched_event = dispatch_event_to_target_and_finish(root_dom, target_key, event)?;
    if dispatched_event.default_prevented || dispatched_event.propagation.stopped {
        return Ok(None);
    }
    Ok(Some(dispatched_event))
}

pub(crate) fn dispatch_event_to_target_and_finish(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    event: DomEvent,
) -> Result<DomEvent, DomError> {
    root_dom.node(target_key).ok_or(DomError::InvalidNodeKey)?;
    let mut dispatch = DomEventDispatchContext::new(event);
    dispatch.set_target_key(target_key);
    dispatch_capture_to_target(root_dom, target_key, &mut dispatch)?;
    if !dispatch.propagation_stopped() && !dispatch.immediate_propagation_stopped() {
        dispatch_target(root_dom, target_key, &mut dispatch);
    }
    if dispatch.event().bubbles()
        && !dispatch.propagation_stopped()
        && !dispatch.immediate_propagation_stopped()
    {
        dispatch_bubble_from_target_parent(root_dom, target_key, &mut dispatch)?;
    }
    Ok(dispatch.finish_with_event())
}

#[allow(dead_code)]
pub(crate) fn visit_topmost_event_target_from_point<F>(
    root_dom: &RootDom,
    world: &World,
    parent: Entity,
    point: PointPx,
    mut visit: F,
) -> Result<(), DomError>
where
    F: FnMut(DomNodeSlotKey),
{
    let target_key = topmost_event_target_from_point(root_dom, world, parent, point)?;
    if let Some(target_key) = target_key {
        visit(target_key);
    }
    Ok(())
}

fn nearest_non_text_target(
    root_dom: &RootDom,
    mut key: DomNodeSlotKey,
) -> Result<DomNodeSlotKey, DomError> {
    loop {
        let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
        if !matches!(node, crate::types::DomNode::Text(_)) {
            return Ok(key);
        }
        key = node.parent_key().ok_or(DomError::InvalidNodeKey)?;
    }
}

fn dispatch_capture_to_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) -> Result<(), DomError> {
    let ancestor_count = ancestor_count_to_root(root_dom, target_key)?;
    for distance_up in (1..=ancestor_count).rev() {
        let ancestor_key = ancestor_key_at_distance(root_dom, target_key, distance_up)?;
        dispatch.enter_phase(crate::types::DomEventPhase::Capturing, ancestor_key);
        dispatch_node_phase_listeners(root_dom, ancestor_key, dispatch, true);
        if dispatch.propagation_stopped() || dispatch.immediate_propagation_stopped() {
            return Ok(());
        }
    }
    Ok(())
}

fn dispatch_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) {
    dispatch.enter_phase(crate::types::DomEventPhase::AtTarget, target_key);
    dispatch_node_phase_listeners(root_dom, target_key, dispatch, true);
    if dispatch.immediate_propagation_stopped() {
        return;
    }
    dispatch_node_phase_listeners(root_dom, target_key, dispatch, false);
}

fn dispatch_bubble_from_target_parent(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) -> Result<(), DomError> {
    let mut current_key = node_parent_key(root_dom, target_key)?;
    while let Some(key) = current_key {
        dispatch.enter_phase(crate::types::DomEventPhase::Bubbling, key);
        dispatch_node_phase_listeners(root_dom, key, dispatch, false);
        if dispatch.propagation_stopped() || dispatch.immediate_propagation_stopped() {
            return Ok(());
        }
        current_key = node_parent_key(root_dom, key)?;
    }
    Ok(())
}

fn dispatch_node_phase_listeners(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
    capture: bool,
) {
    let event_name = dispatch.event().name;
    let Some(node) = root_dom.node(key) else {
        return;
    };
    let listeners = node.dispatched_listeners_for_phase(event_name, capture);
    for dispatched_listener in listeners {
        dispatched_listener.listener.invoke(root_dom, dispatch);
        if dispatched_listener.options.once {
            remove_once_listener_from_node(root_dom, key, event_name, &dispatched_listener);
        }
        if dispatch.immediate_propagation_stopped() {
            return;
        }
    }
}

fn remove_once_listener_from_node(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    event_name: DomEventName,
    dispatched_listener: &crate::types::dom_node::DomDispatchedEventListener,
) {
    let Some(node) = root_dom.nodes.get_mut(key) else {
        return;
    };
    let _removed = node.remove_event_listener(
        event_name,
        dispatched_listener.listener.clone(),
        Some(dispatched_listener.options),
    );
}

fn ancestor_count_to_root(
    root_dom: &RootDom,
    target_key: DomNodeSlotKey,
) -> Result<usize, DomError> {
    let mut count = 0;
    let mut current_key = node_parent_key(root_dom, target_key)?;
    while let Some(key) = current_key {
        count += 1;
        current_key = node_parent_key(root_dom, key)?;
    }
    Ok(count)
}

fn ancestor_key_at_distance(
    root_dom: &RootDom,
    target_key: DomNodeSlotKey,
    distance_up: usize,
) -> Result<DomNodeSlotKey, DomError> {
    let mut remaining = distance_up;
    let mut current_key = target_key;
    while remaining > 0 {
        let Some(parent_key) = node_parent_key(root_dom, current_key)? else {
            return Err(DomError::InvalidNodeKey);
        };
        current_key = parent_key;
        remaining -= 1;
    }
    Ok(current_key)
}

fn node_parent_key(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
    Ok(node.parent_key())
}

#[cfg(test)]
mod tests {
    use bevy::{image::Image, prelude::*};

    use crate::domains::hit_test::hit_test_from_dom_subtree_local;
    use crate::types::primitives::PointPx;
    use crate::types::{DomEvent, DomEventName, DomNode, DomTag, RootDom, RootDomContext};
    use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};

    use super::{dispatch_event_from_point, visit_topmost_event_target_from_point};

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    #[test]
    fn point_visitor_matches_previous_path_hit_oracle_after_layer_selection() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let underlay = root.create_element(DomTag::Button).expect("underlay");
        let overlay = root.create_element(DomTag::Div).expect("overlay");

        root.append_child(root.root, underlay)
            .expect("append underlay");
        root.append_child(root.root, overlay)
            .expect("append overlay");
        root.set_attr_str(underlay, "position", "absolute")
            .expect("underlay layer");
        root.set_attr_str(overlay, "position", "absolute")
            .expect("overlay layer");
        set_box(&mut root, underlay, 0.0, 0.0, 40.0, 40.0);
        set_box(&mut root, overlay, 0.0, 0.0, 40.0, 40.0);

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

        let point = PointPx {
            x_px: 5.0,
            y_px: 5.0,
        };
        let mut target = None;
        visit_topmost_event_target_from_point(&root, &world, parent, point, |key| {
            target = Some(key);
        })
        .expect("point visitor should succeed");

        assert_eq!(
            target,
            previous_event_target_oracle(&root, overlay, point).expect("oracle should resolve"),
        );
    }

    #[test]
    fn point_dispatch_drops_prevented_event_from_current_update_sink() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);
        root.add_event_listener(button, DomEventName::PointerMove, prevent_default, None)
            .expect("pointer move listener should register");
        root.add_event_listener(button, DomEventName::PointerDown, prevent_default, None)
            .expect("pointer down listener should register");

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

        let events = root
            .update(
                &mut world,
                0.0,
                crate::types::DomPointerUpdate::Set(crate::types::DomPointerState {
                    x_px: 5.0,
                    y_px: 5.0,
                    primary_button_down: true,
                    scroll_delta_y_px: 0.0,
                }),
            )
            .expect("pointer down should succeed");

        assert!(
            events.is_empty(),
            "prevented point event should not enter sink"
        );
    }

    #[test]
    fn point_visitor_follows_retained_scroll_inside_a_clipped_layer() {
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

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
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

        let mut target = None;
        visit_topmost_event_target_from_point(
            &root,
            &world,
            parent,
            PointPx {
                x_px: 10.0,
                y_px: 10.0,
            },
            |key| target = Some(key),
        )
        .expect("point visitor should succeed");

        assert_eq!(
            target,
            Some(second_item),
            "topmost event target should follow retained scroll under a stable cursor",
        );
    }

    #[test]
    fn attached_point_passes_through_when_document_root_disallows_pointer_events() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        root.set_attr_str(root.root, "pointer-events", "none")
            .expect("root pointer-events should apply");
        set_box(&mut root, button, 0.0, 0.0, 40.0, 40.0);

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

        let mut target = None;
        visit_topmost_event_target_from_point(
            &root,
            &world,
            parent,
            PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
            |key| target = Some(key),
        )
        .expect("point visitor should succeed");
        assert_eq!(
            target, None,
            "attached point visitor should pass through when no presented layer allows pointer events",
        );

        let event = DomEvent::new(
            DomEventName::PointerDown,
            crate::types::DomEventDetail::Pointer(crate::types::DomPointerEvent {
                pointer_id: 1,
                pointer_type: crate::types::DomPointerType::Mouse,
                is_primary: true,
                button: Some(crate::types::DomMouseButton::Primary),
                buttons: 1,
                client_x: 5.0,
                client_y: 5.0,
                modifiers: crate::types::DomModifierState::default(),
            }),
        );
        let dispatched = dispatch_event_from_point(
            &mut root,
            &world,
            parent,
            PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
            event,
        )
        .expect("dispatch should succeed");
        assert_eq!(
            dispatched, None,
            "attached dispatch should not target root-subtree content after pass-through",
        );
    }

    #[test]
    fn attached_point_passes_through_when_non_layer_target_disallows_pointer_events() {
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

        let mut target = None;
        visit_topmost_event_target_from_point(
            &root,
            &world,
            parent,
            PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
            |key| target = Some(key),
        )
        .expect("point visitor should succeed");

        assert_ne!(
            target,
            Some(button),
            "ordinary non-layer descendants with pointer-events none should not win event targeting",
        );
    }

    #[test]
    fn attached_point_targets_explicit_auto_descendant_inside_pointer_events_none_ancestor() {
        // Remaining attached targeting gaps:
        // - nested layer roots where the ancestor and opt-in descendant split across layers
        // - overlapping sibling z-order when the pass-through branch sits above another target
        // - transformed or scrolled ancestors on the opt-in branch
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

        let mut target = None;
        visit_topmost_event_target_from_point(
            &root,
            &world,
            parent,
            PointPx {
                x_px: 5.0,
                y_px: 5.0,
            },
            |key| target = Some(key),
        )
        .expect("point visitor should succeed");

        assert_eq!(
            target,
            Some(button),
            "explicit pointer-events auto descendants should opt back into event targeting under a none ancestor",
        );
    }

    fn prevent_default(context: &mut crate::types::DomEventListenerContext<'_>) {
        context.prevent_default();
    }

    fn previous_event_target_oracle(
        root_dom: &RootDom,
        subtree_root: crate::types::DomNodeSlotKey,
        point: PointPx,
    ) -> Result<Option<crate::types::DomNodeSlotKey>, crate::types::DomError> {
        let Some(mut hit) = hit_test_from_dom_subtree_local(root_dom, subtree_root, point)? else {
            return Ok(None);
        };
        while hit
            .path
            .last()
            .copied()
            .and_then(|key| root_dom.node(key))
            .is_some_and(|node| node.as_element().is_none())
        {
            hit.path.pop();
        }
        Ok(hit.path.last().copied())
    }

    fn set_box(
        root: &mut RootDom,
        key: crate::types::DomNodeSlotKey,
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
