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
//! - delegate capture/target/bubble propagation to the E5 `propagate`
//!   primitive in `types/event_propagation.rs`
//!
//! Invariants and rules:
//!
//! - point target resolution stays in one contained file
//! - Bevy-layer selection stays iterative and allocation-free
//! - `propagate(...)` returns the full finished event (with default-prevented
//!   and propagation-stop state written back) for owners that need
//!   cancellation truth; `dispatch_event_to_target(...)` returns the
//!   sink-eligible finished event, or `None` when prevented or stopped
//!
//! Constraints this file is protecting:
//!
//! - do not build path products for later replay
//! - do not move hover/active bookkeeping into event dispatch
//! - do not re-own the propagation walk (its home is `event_propagation.rs`)
//! - do not borrow out the owned mutable event context to listeners
use bevy::prelude::{Entity, World};

use crate::domains::hit_test::hit_test_from_dom_subtree_cluster_local;
use crate::domains::layer_hit::topmost_attached_layer_hit_from_root_point;
use crate::types::dom_event::DomEvent;
use crate::types::event_propagation::{dispatch_event_to_target, propagate};
use crate::types::primitives::PointPx;
use crate::types::{DomError, DomNodeSlotKey, RootDom};

#[cfg(test)]
use crate::domains::hit_test::hit_test_from_dom_subtree_local;
#[cfg(test)]
use crate::types::{
    DomEventDetail, DomEventListenerContext, DomEventName, DomModifierState, DomMouseButton,
    DomNode, DomPointerEvent, DomPointerState, DomPointerType, DomPointerUpdate, DomTag,
    RootDomContext,
};
#[cfg(test)]
use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};

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
    let event = propagate(root_dom, target.target_key, event)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{image::Image, prelude::*};

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
        root.style_mut(underlay)
            .set("position", "absolute")
            .expect("underlay layer");
        root.style_mut(overlay)
            .set("position", "absolute")
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
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
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
                DomPointerUpdate::Set(DomPointerState {
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
        root.style_mut(scroll_root)
            .set("width", "80px")
            .expect("scroll width");
        root.style_mut(scroll_root)
            .set("height", "40px")
            .expect("scroll height");
        root.style_mut(scroll_root)
            .set("overflow", "scroll")
            .expect("scroll overflow");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "40px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach root");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
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
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
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
            DomEventDetail::Pointer(DomPointerEvent {
                pointer_id: 1,
                pointer_type: DomPointerType::Mouse,
                is_primary: true,
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 5.0,
                client_y: 5.0,
                modifiers: DomModifierState::default(),
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
        root.style_mut(button)
            .set("width", "40px")
            .expect("button width");
        root.style_mut(button)
            .set("height", "40px")
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
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
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
        root.style_mut(blocker)
            .set("width", "40px")
            .expect("blocker width");
        root.style_mut(blocker)
            .set("height", "40px")
            .expect("blocker height");
        root.style_mut(button)
            .set("width", "40px")
            .expect("button width");
        root.style_mut(button)
            .set("height", "40px")
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
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
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

    fn prevent_default(context: &mut DomEventListenerContext<'_>) {
        context.prevent_default();
    }

    fn previous_event_target_oracle(
        root_dom: &RootDom,
        subtree_root: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<Option<DomNodeSlotKey>, DomError> {
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
