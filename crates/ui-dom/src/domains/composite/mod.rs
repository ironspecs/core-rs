//! Retained composite boundary for `ui-dom`.
//!
//! Composite scheduling is layer-first: lifecycle reconciliation creates or
//! removes Bevy-backed layers, then style, layout, and paint walk the live
//! Bevy layer hierarchy in DOM order. Retained surface order is published once
//! per attached root after those passes.

use bevy::{
    asset::{Assets, Handle, RenderAssetUsages},
    camera::visibility::RenderLayers,
    image::Image,
    mesh::{Indices, Mesh, Mesh2d},
    prelude::{
        Children, Component, Entity, GlobalTransform, InheritedVisibility, Name, Transform, Vec2,
        Vec4, ViewVisibility, Visibility, World,
    },
    render::render_resource::PrimitiveTopology,
    render::sync_world::MainEntity,
    sprite_render::MeshMaterial2d,
};
use smallvec::SmallVec;

#[derive(Clone, Copy)]
struct BevyLayerClipStackEntry {
    entity: Entity,
    inherited_clip_world: Option<Vec4>,
    raised_root_carry: Option<DomNodeSlotKey>,
    parent_layer_world_offset: PointPx,
}

type BevyLayerClipStack = SmallVec<[BevyLayerClipStackEntry; 32]>;

use crate::domains::{
    LayerPoolRenderJobQueue,
    layer_pool::LayerPool,
    layout::{
        IdentityDomLayoutResolver, layer_kind_for_node, layer_offset_from_dom_climb,
        node_box_from_ref, relayout_tree_with_inherited, root_key, tree_needs_layout,
    },
    paint::{layer_tree_needs_paint, paint_layer},
    visitor::inherited_style::{InheritedStyleContext, inherited_style_context_from_ancestors},
    visitor::scroll::apply_vertical_scroll_from_point as apply_vertical_scroll_via_visitor,
    visitor::style_recompute::{
        recompute_dirty_style_subtree_for_layer_root_with_inherited, style_tree_needs_recompute,
    },
};
use crate::numeric::{u32_to_f32, usize_to_f32};
use crate::render_plugin::UiDomBaseMaterial;
use crate::types::primitives::PointPx;
use crate::types::{
    DomAttribute, DomAttributeNode, DomAttributeValue, DomError, DomLayerKind, DomNode,
    DomNodeSlotKey, RootDom, RootDomMetricName,
};

#[derive(Clone, Copy)]
struct LayerPresentationInputs {
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
    viewport_size_px: Vec2,
    target_size_px: Vec2,
    effective_clip_world: Vec4,
    scroll_offset_y_px: f32,
    opacity: f32,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BevyLayer {
    dom_node_slot_key: DomNodeSlotKey,
}

impl BevyLayer {
    pub(crate) fn dom_node_slot_key(self) -> DomNodeSlotKey {
        self.dom_node_slot_key
    }
}

#[derive(Component, Clone)]
struct UiDomLayerPresentation {
    mesh: Handle<Mesh>,
    material: Handle<UiDomBaseMaterial>,
    absolute_offset: PointPx,
    viewport_size_px: Vec2,
    target_size_px: Vec2,
    clip_world: Vec4,
    scroll_offset_y_px: f32,
}

pub(crate) fn create(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(), DomError> {
    validate_parent(world, parent)?;
    run_composite_passes(root_dom, world, parent)
}

pub(crate) fn destroy(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(), DomError> {
    validate_parent(world, parent)?;
    destroy_ui_dom_descendants(root_dom, world, parent);
    queue_root_surface_order_detach(world, parent.into());
    Ok(())
}

pub(crate) fn update(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
    _now_s: f64,
) -> Result<(), DomError> {
    validate_parent(world, parent)?;
    run_composite_passes(root_dom, world, parent)
}

pub(crate) fn update_style_layout(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(), DomError> {
    validate_parent(world, parent)?;
    run_style_layout_passes(root_dom, world, parent)
}

pub(crate) fn apply_vertical_scroll_from_point(
    root_dom: &mut RootDom,
    world: &World,
    parent: Entity,
    point: PointPx,
    delta_y_px: f32,
) -> Result<bool, DomError> {
    apply_vertical_scroll_via_visitor(root_dom, world, parent, point, delta_y_px)
}

fn run_composite_passes(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(), DomError> {
    let (root_layer_entity, viewport_size) = prepare_layer_frame(root_dom, world, parent)?;
    run_style_phase_over_bevy_layers(root_dom, world, root_layer_entity)?;
    run_layout_phase_over_bevy_layers(root_dom, world, root_layer_entity, viewport_size)?;
    run_paint_phase_over_bevy_layers(root_dom, world, root_layer_entity)?;
    sync_root_presentation_and_publish_retained_order(
        root_dom,
        world,
        root_layer_entity,
        parent.into(),
    )
}

fn run_style_layout_passes(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(), DomError> {
    let (root_layer_entity, viewport_size) = prepare_layer_frame(root_dom, world, parent)?;
    run_style_phase_over_bevy_layers(root_dom, world, root_layer_entity)?;
    run_layout_phase_over_bevy_layers(root_dom, world, root_layer_entity, viewport_size)
}

fn validate_parent(world: &World, parent: Entity) -> Result<(), DomError> {
    if world.get_entity(parent).is_err() {
        return Err(DomError::InvalidParentTarget);
    }
    Ok(())
}

fn apply_pending_layer_removals(root_dom: &mut RootDom, world: &mut World) {
    for layer_root_key in root_dom.drain_pending_layer_remove_roots() {
        remove_layer(root_dom, world, layer_root_key);
    }
}

// Best-effort layer teardown should always drop the Bevy projection. The DOM
// subtree remains live for layer-kind changes; only closing popup shells use
// this queue as their deferred DOM removal boundary.
fn remove_layer(root_dom: &mut RootDom, world: &mut World, layer_root_key: DomNodeSlotKey) {
    if let Some(entity) = root_dom.layer_entity(layer_root_key) {
        if world.get_entity(entity).is_err() {
            root_dom.unregister_layer_entity(layer_root_key);
        } else {
            despawn_registered_layer_subtree(root_dom, world, entity);
        }
    }
    if layer_root_key == root_key(root_dom)
        || !node_is_closing_presented_options_shell(root_dom, layer_root_key)
    {
        return;
    }
    let _ = root_dom.remove_node(layer_root_key);
}

fn node_is_closing_presented_options_shell(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> bool {
    matches!(
        root_dom
        .node(layer_root_key)
        .and_then(DomNode::as_element)
        .and_then(|element| {
            element
                .attrs
                .attributes
                .get(&DomAttribute::PresentedOptionsState)
        }),
        Some(DomAttributeValue::String(state)) if state == "closing"
    )
}

fn despawn_registered_layer_subtree(
    root_dom: &mut RootDom,
    world: &mut World,
    root_entity: Entity,
) {
    let mut traversal_stack = SmallVec::<[Entity; 32]>::new();
    let mut subtree_entities = SmallVec::<[Entity; 32]>::new();
    traversal_stack.push(root_entity);
    while let Some(entity) = traversal_stack.pop() {
        if world.get_entity(entity).is_err() {
            continue;
        }
        subtree_entities.push(entity);
        let Some(children) = world.get::<Children>(entity) else {
            continue;
        };
        for child_entity in children {
            if world.get::<BevyLayer>(*child_entity).is_some() {
                traversal_stack.push(*child_entity);
            }
        }
    }
    for entity in subtree_entities.into_iter().rev() {
        unregister_destroyed_layer_entity(root_dom, world, entity);
        let despawned = world.despawn(entity);
        assert!(
            despawned,
            "ui-dom layer entity should exist during subtree teardown",
        );
    }
}

fn viewport_size_from_parent_or_one(world: &World, parent: Entity) -> Result<(f32, f32), DomError> {
    let parent_ref = world
        .get_entity(parent)
        .map_err(|_| DomError::InvalidParentTarget)?;
    let Some(sprite) = parent_ref.get::<bevy::prelude::Sprite>() else {
        return Ok((1.0, 1.0));
    };
    let Some(size) = sprite.custom_size else {
        return Ok((1.0, 1.0));
    };
    Ok((size.x.max(1.0), size.y.max(1.0)))
}

fn prepare_layer_frame(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<(Entity, (f32, f32)), DomError> {
    let viewport_size = viewport_size_from_parent_or_one(world, parent)?;
    let root_layer_entity = reconcile_layer_frame_lifecycle(root_dom, world, parent)?;
    Ok((root_layer_entity, viewport_size))
}

fn ensure_root_layer_entity(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<Entity, DomError> {
    ensure_layer_entity(
        root_dom,
        world,
        parent,
        root_key(root_dom),
        PointPx::default(),
    )
}

fn reconcile_layer_frame_lifecycle(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
) -> Result<Entity, DomError> {
    apply_pending_layer_removals(root_dom, world);
    let root_layer_entity = ensure_root_layer_entity(root_dom, world, parent)?;
    apply_pending_layer_appends(root_dom, world, parent)?;
    materialize_missing_descendant_layers_for_layout_or_paint_dirty_layers(
        root_dom,
        world,
        root_layer_entity,
    )?;
    Ok(root_layer_entity)
}

fn run_style_phase_over_bevy_layers(
    root_dom: &mut RootDom,
    world: &mut World,
    root_layer_entity: Entity,
) -> Result<(), DomError> {
    let mut layer_stack = SmallVec::<[Entity; 32]>::new();
    layer_stack.push(root_layer_entity);
    while let Some(layer_entity) = layer_stack.pop() {
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        if root_dom.node(layer_root_key).is_none() {
            continue;
        }
        if style_tree_needs_recompute(root_dom, layer_root_key)? {
            let inherited = inherited_style_context_from_ancestors(root_dom, layer_root_key)?;
            recompute_dirty_style_subtree_for_layer_root_with_inherited(
                root_dom,
                layer_root_key,
                &inherited,
            )?;
        }
        push_bevy_layer_children_reverse(world, layer_entity, &mut layer_stack);
    }
    Ok(())
}

fn run_layout_phase_over_bevy_layers(
    root_dom: &mut RootDom,
    world: &mut World,
    root_layer_entity: Entity,
    viewport_size: (f32, f32),
) -> Result<(), DomError> {
    let mut layer_stack = SmallVec::<[Entity; 32]>::new();
    layer_stack.push(root_layer_entity);
    while let Some(layer_entity) = layer_stack.pop() {
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        if root_dom.node(layer_root_key).is_none() {
            continue;
        }
        if tree_needs_layout(root_dom, layer_root_key)?
            || fixed_layer_root_needs_viewport_relayout(root_dom, layer_root_key, viewport_size)?
        {
            let inherited = inherited_style_context_from_ancestors(root_dom, layer_root_key)?;
            relayout_layer_root(root_dom, layer_root_key, viewport_size, &inherited)?;
        }
        push_bevy_layer_children_reverse(world, layer_entity, &mut layer_stack);
    }
    Ok(())
}

fn fixed_layer_root_needs_viewport_relayout(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
    viewport_size: (f32, f32),
) -> Result<bool, DomError> {
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    if !matches!(
        node.presented_attr(&DomAttribute::Position),
        Some(DomAttributeValue::String(value)) if value == "fixed"
    ) {
        return Ok(false);
    }
    let layout = node.layout();
    Ok((layout.available_width_px - viewport_size.0).abs() > 0.01
        || (layout.available_height_px - viewport_size.1).abs() > 0.01)
}

fn run_paint_phase_over_bevy_layers(
    root_dom: &mut RootDom,
    world: &mut World,
    root_layer_entity: Entity,
) -> Result<(), DomError> {
    let mut layer_stack = SmallVec::<[Entity; 32]>::new();
    layer_stack.push(root_layer_entity);
    while let Some(layer_entity) = layer_stack.pop() {
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        if root_dom.node(layer_root_key).is_none() {
            continue;
        }
        if layer_tree_needs_paint(root_dom, layer_root_key)? {
            let inherited = inherited_style_context_from_ancestors(root_dom, layer_root_key)?;
            let layer_offset = layer_offset_from_dom_climb(root_dom, layer_root_key)?;
            sync_layer_surface_for_root(
                root_dom,
                world,
                layer_root_key,
                layer_offset,
                layer_entity,
            )?;
            let lease = root_dom
                .layer_lease(layer_root_key)
                .ok_or(DomError::InvalidNodeKey)?;
            paint_layer(
                root_dom,
                world,
                layer_root_key,
                lease,
                target_layer_size(root_dom, layer_root_key)?,
                &inherited,
            )?;
        }
        push_bevy_layer_children_reverse(world, layer_entity, &mut layer_stack);
    }
    Ok(())
}

fn apply_pending_layer_appends(
    root_dom: &mut RootDom,
    world: &mut World,
    attached_parent: Entity,
) -> Result<(), DomError> {
    for subtree_root_key in root_dom.drain_pending_layer_append_roots() {
        if root_dom.node(subtree_root_key).is_none() {
            continue;
        }
        let subtree_parent =
            ensure_attached_ancestor_layers(root_dom, world, attached_parent, subtree_root_key)?;
        create_layer_entities_in_connected_subtree(
            root_dom,
            world,
            subtree_root_key,
            subtree_parent,
        )?;
    }
    Ok(())
}

fn ensure_attached_ancestor_layers(
    root_dom: &mut RootDom,
    world: &mut World,
    attached_parent: Entity,
    subtree_root_key: DomNodeSlotKey,
) -> Result<Entity, DomError> {
    let mut ancestor_layer_keys = SmallVec::<[DomNodeSlotKey; 16]>::new();
    let mut current_key = root_dom
        .node(subtree_root_key)
        .and_then(DomNode::parent_key);
    while let Some(ancestor_key) = current_key {
        if is_layer_root_key(root_dom, ancestor_key) {
            ancestor_layer_keys.push(ancestor_key);
        }
        current_key = root_dom.node(ancestor_key).and_then(DomNode::parent_key);
    }
    let mut parent_entity = attached_parent;
    for ancestor_layer_key in ancestor_layer_keys.into_iter().rev() {
        let layer_offset = layer_offset_from_dom_climb(root_dom, ancestor_layer_key)?;
        parent_entity = ensure_layer_entity(
            root_dom,
            world,
            parent_entity,
            ancestor_layer_key,
            layer_offset,
        )?;
    }
    Ok(parent_entity)
}

fn create_layer_entities_in_connected_subtree(
    root_dom: &mut RootDom,
    world: &mut World,
    subtree_root_key: DomNodeSlotKey,
    parent_entity: Entity,
) -> Result<(), DomError> {
    let mut node_stack = SmallVec::<[(DomNodeSlotKey, Entity); 32]>::new();
    node_stack.push((subtree_root_key, parent_entity));
    while let Some((node_key, node_parent_entity)) = node_stack.pop() {
        let mut subtree_parent_entity = node_parent_entity;
        if is_layer_root_key(root_dom, node_key) {
            let layer_offset = layer_offset_from_dom_climb(root_dom, node_key)?;
            subtree_parent_entity =
                ensure_layer_entity(root_dom, world, node_parent_entity, node_key, layer_offset)?;
        }
        push_dom_child_keys_reverse(root_dom, node_key, subtree_parent_entity, &mut node_stack)?;
    }
    Ok(())
}

fn create_descendant_layer_entities_under_layer_root(
    root_dom: &mut RootDom,
    world: &mut World,
    layer_root_key: DomNodeSlotKey,
    layer_entity: Entity,
) -> Result<(), DomError> {
    let mut child_key = first_child_key(root_dom, layer_root_key)?;
    while let Some(current_child_key) = child_key {
        child_key = next_sibling_key(root_dom, current_child_key)?;
        create_layer_entities_in_connected_subtree(
            root_dom,
            world,
            current_child_key,
            layer_entity,
        )?;
    }
    Ok(())
}

fn materialize_missing_descendant_layers_for_layout_or_paint_dirty_layers(
    root_dom: &mut RootDom,
    world: &mut World,
    root_layer_entity: Entity,
) -> Result<(), DomError> {
    let mut layer_stack = SmallVec::<[Entity; 32]>::new();
    layer_stack.push(root_layer_entity);
    while let Some(layer_entity) = layer_stack.pop() {
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        if root_dom.node(layer_root_key).is_none() {
            continue;
        }
        if tree_needs_layout(root_dom, layer_root_key)?
            || layer_tree_needs_paint(root_dom, layer_root_key)?
        {
            create_descendant_layer_entities_under_layer_root(
                root_dom,
                world,
                layer_root_key,
                layer_entity,
            )?;
        }
        push_bevy_layer_children_reverse(world, layer_entity, &mut layer_stack);
    }
    Ok(())
}

fn relayout_layer_root(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    viewport_size: (f32, f32),
    inherited_style: &InheritedStyleContext,
) -> Result<(), DomError> {
    let layer_size = layout_tree_size(root_dom, node_key, viewport_size)?;
    let mut resolver = IdentityDomLayoutResolver;
    let _ = relayout_tree_with_inherited(
        root_dom,
        node_key,
        layer_size,
        inherited_style,
        &mut resolver,
    )?;
    Ok(())
}

fn first_child_key(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    Ok(node.first_child_key_for_store())
}

fn next_sibling_key(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    Ok(node.next_sibling_key_for_store())
}

fn is_layer_root_key(root_dom: &RootDom, node_key: DomNodeSlotKey) -> bool {
    if node_key == root_key(root_dom) {
        return true;
    }
    root_dom
        .node(node_key)
        .is_some_and(|node| layer_kind_for_node(node) != DomLayerKind::None)
}

fn layout_tree_size(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    viewport_size: (f32, f32),
) -> Result<(f32, f32), DomError> {
    if node_key == root_key(root_dom) {
        return Ok(viewport_size);
    }
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    if node_uses_viewport_sized_layout_tree(node) {
        return Ok((viewport_size.0.max(1.0), viewport_size.1.max(1.0)));
    }
    let border_box = node_box_from_ref(node).border_box;
    Ok((border_box.width_px.max(1.0), border_box.height_px.max(1.0)))
}

fn node_uses_viewport_sized_layout_tree(node: &DomNode) -> bool {
    matches!(
        node.presented_attr(&DomAttribute::Position),
        Some(DomAttributeValue::String(value)) if value == "fixed"
    )
}

fn ensure_layer_entity(
    root_dom: &mut RootDom,
    world: &mut World,
    parent: Entity,
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
) -> Result<Entity, DomError> {
    if let Some(entity) = root_dom.layer_entity(layer_root_key) {
        if world.get_entity(entity).is_err() {
            if let Some(lease) = root_dom.layer_lease(layer_root_key) {
                let _ = LayerPool::destroy_layer_lease_in_world(world, lease);
            }
            root_dom.unregister_layer_entity(layer_root_key);
        } else {
            sync_layer_surface_for_root(root_dom, world, layer_root_key, absolute_offset, entity)?;
            world.entity_mut(parent).add_child(entity);
            return Ok(entity);
        }
    }
    ensure_presentation_assets(world);
    let image_handle =
        ensure_layer_surface_image(root_dom, world, layer_root_key, absolute_offset)?;
    let target_size = target_layer_size(root_dom, layer_root_key)?;
    let mesh_handle = world
        .resource_mut::<Assets<Mesh>>()
        .add(top_left_layer_mesh(Vec2::new(target_size.0, target_size.1)));
    let material_handle = world
        .resource_mut::<Assets<UiDomBaseMaterial>>()
        .add(UiDomBaseMaterial::new(image_handle.clone(), 1.0));
    let render_layers = world.get::<RenderLayers>(parent).cloned();
    let entity = world
        .spawn((
            Name::new("UiDomLayer"),
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material_handle.clone()),
            BevyLayer {
                dom_node_slot_key: layer_root_key,
            },
            UiDomLayerPresentation {
                mesh: mesh_handle,
                material: material_handle,
                absolute_offset: PointPx::default(),
                viewport_size_px: Vec2::ONE,
                target_size_px: Vec2::ONE,
                clip_world: unclipped_world_rect(),
                scroll_offset_y_px: 0.0,
            },
            Transform::default(),
            GlobalTransform::default(),
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ))
        .id();
    if let Some(render_layers) = render_layers {
        world.entity_mut(entity).insert(render_layers);
    }
    tracing::debug!(
        parent = ?parent,
        entity = ?entity,
        layer_root_key = ?layer_root_key,
        dom_parent_key = ?layer_dom_parent_key(root_dom, layer_root_key),
        layer_tag = ?try_layer_tag_name(root_dom, layer_root_key),
        layer_id = ?try_layer_id_attr(root_dom, layer_root_key),
        layer_class = ?try_layer_class_attr(root_dom, layer_root_key),
        layer_kind = ?layer_kind_value(root_dom, layer_root_key),
        event = "layer_entity_created"
    );
    world.entity_mut(parent).add_child(entity);
    root_dom.register_layer_entity(layer_root_key, entity);
    Ok(entity)
}

fn ensure_layer_surface_image(
    root_dom: &mut RootDom,
    world: &mut World,
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
) -> Result<Handle<Image>, DomError> {
    let layer_rect = current_layer_rect(root_dom, layer_root_key, absolute_offset)?;
    let lease = if let Some(lease) = root_dom.layer_lease(layer_root_key) {
        LayerPool::update_layer_lease_in_world(world, lease, layer_rect)
            .map_err(|_| DomError::InvalidNodeKey)?;
        lease
    } else {
        let lease = LayerPool::create_layer_lease_in_world(world, layer_rect);
        root_dom.register_layer_lease(layer_root_key, lease);
        lease
    };
    LayerPool::cloned_surface_image_in_world(world, lease).map_err(|_| DomError::InvalidNodeKey)
}

fn sync_layer_surface_for_root(
    root_dom: &mut RootDom,
    world: &mut World,
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
    entity: Entity,
) -> Result<(), DomError> {
    let image_handle =
        ensure_layer_surface_image(root_dom, world, layer_root_key, absolute_offset)?;
    let presentation_material = world
        .get::<UiDomLayerPresentation>(entity)
        .ok_or(DomError::InvalidNodeKey)?
        .clone();
    let Some(materials) = world.get_resource_mut::<Assets<UiDomBaseMaterial>>() else {
        return Err(DomError::InvalidNodeKey);
    };
    let Some(material) = materials
        .into_inner()
        .get_mut(presentation_material.material.id())
    else {
        return Err(DomError::InvalidNodeKey);
    };
    material.color_texture = image_handle;
    Ok(())
}

fn ensure_presentation_assets(world: &mut World) {
    if world.get_resource::<Assets<Mesh>>().is_none() {
        world.insert_resource(Assets::<Mesh>::default());
    }
    if world.get_resource::<Assets<UiDomBaseMaterial>>().is_none() {
        world.insert_resource(Assets::<UiDomBaseMaterial>::default());
    }
    if world.get_resource::<Assets<Image>>().is_none() {
        world.insert_resource(Assets::<Image>::default());
    }
}

fn destroy_ui_dom_descendants(root_dom: &mut RootDom, world: &mut World, parent: Entity) {
    let Some(children) = world.get::<bevy::prelude::Children>(parent) else {
        return;
    };
    let child_entities = children
        .iter()
        .copied()
        .filter(|child_entity| world.get::<BevyLayer>(*child_entity).is_some())
        .collect::<SmallVec<[Entity; 8]>>();
    for child in child_entities {
        despawn_registered_layer_subtree(root_dom, world, child);
    }
}

fn unregister_destroyed_layer_entity(root_dom: &mut RootDom, world: &mut World, entity: Entity) {
    let Some(layer_root_key) = root_dom.layer_root_for_entity(entity) else {
        return;
    };
    if let Some(lease) = root_dom.layer_lease(layer_root_key) {
        let _ = LayerPool::destroy_layer_lease_in_world(world, lease);
    }
    root_dom.unregister_layer_entity(layer_root_key);
}

fn refresh_layer_presentation_if_needed(
    root_dom: &mut RootDom,
    world: &mut World,
    entity: Entity,
    inputs: LayerPresentationInputs,
) -> Result<(), DomError> {
    let presentation = current_presentation(world, entity)?;
    if presentation_needs_refresh(&presentation, inputs) {
        apply_full_presentation_update(root_dom, world, entity, &presentation, inputs)?;
        return Ok(());
    }
    if scroll_offset_changed(presentation.scroll_offset_y_px, inputs.scroll_offset_y_px) {
        apply_scroll_translation_update(root_dom, world, entity, &presentation, inputs)?;
    }
    Ok(())
}

fn current_presentation(world: &World, entity: Entity) -> Result<UiDomLayerPresentation, DomError> {
    world
        .get::<UiDomLayerPresentation>(entity)
        .cloned()
        .ok_or(DomError::InvalidNodeKey)
}

fn presentation_needs_refresh(
    presentation: &UiDomLayerPresentation,
    inputs: LayerPresentationInputs,
) -> bool {
    presentation.absolute_offset != inputs.absolute_offset
        || presentation.viewport_size_px != inputs.viewport_size_px
        || presentation.target_size_px != inputs.target_size_px
        || presentation.clip_world != inputs.effective_clip_world
}

fn apply_full_presentation_update(
    root_dom: &mut RootDom,
    world: &mut World,
    entity: Entity,
    presentation: &UiDomLayerPresentation,
    inputs: LayerPresentationInputs,
) -> Result<(), DomError> {
    root_dom.increment_metric(RootDomMetricName::LayerPresented);
    if presentation.clip_world != inputs.effective_clip_world {
        root_dom.increment_metric(RootDomMetricName::ClipUpdated);
    }
    update_mesh_asset(world, &presentation.mesh, inputs.target_size_px);
    update_material_asset(
        world,
        &presentation.material,
        inputs.target_size_px,
        inputs.effective_clip_world,
        inputs.opacity,
    )?;
    let (local_x, local_y) = {
        let mut entity_mut = world
            .get_entity_mut(entity)
            .map_err(|_| DomError::InvalidNodeKey)?;
        let Some(mut transform) = entity_mut.get_mut::<Transform>() else {
            return Err(DomError::InvalidNodeKey);
        };
        update_layer_translation(&mut transform, inputs);
        (transform.translation.x, transform.translation.y)
    };
    tracing::debug!(
        entity = ?entity,
        layer_root_key = ?inputs.layer_root_key,
        dom_parent_key = ?layer_dom_parent_key(root_dom, inputs.layer_root_key),
        layer_tag = ?try_layer_tag_name(root_dom, inputs.layer_root_key),
        layer_id = ?try_layer_id_attr(root_dom, inputs.layer_root_key),
        layer_class = ?try_layer_class_attr(root_dom, inputs.layer_root_key),
        layer_kind = ?layer_kind_value(root_dom, inputs.layer_root_key),
        x = local_x,
        y = local_y,
        width = inputs.target_size_px.x,
        height = inputs.target_size_px.y,
        scroll_offset_y = inputs.scroll_offset_y_px,
        opacity = inputs.opacity,
        event = "layer_presented"
    );
    let mut entity_mut = world
        .get_entity_mut(entity)
        .map_err(|_| DomError::InvalidNodeKey)?;
    let Some(mut retained) = entity_mut.get_mut::<UiDomLayerPresentation>() else {
        return Err(DomError::InvalidNodeKey);
    };
    retained.absolute_offset = inputs.absolute_offset;
    retained.viewport_size_px = inputs.viewport_size_px;
    retained.target_size_px = inputs.target_size_px;
    retained.clip_world = inputs.effective_clip_world;
    retained.scroll_offset_y_px = inputs.scroll_offset_y_px;
    Ok(())
}

fn apply_scroll_translation_update(
    root_dom: &mut RootDom,
    world: &mut World,
    entity: Entity,
    presentation: &UiDomLayerPresentation,
    inputs: LayerPresentationInputs,
) -> Result<(), DomError> {
    root_dom.increment_metric(RootDomMetricName::LayerTranslated);
    let (local_x, local_y) = {
        let mut entity_mut = world
            .get_entity_mut(entity)
            .map_err(|_| DomError::InvalidNodeKey)?;
        let Some(mut transform) = entity_mut.get_mut::<Transform>() else {
            return Err(DomError::InvalidNodeKey);
        };
        update_layer_translation(&mut transform, inputs);
        (transform.translation.x, transform.translation.y)
    };
    tracing::debug!(
        entity = ?entity,
        layer_root_key = ?inputs.layer_root_key,
        dom_parent_key = ?layer_dom_parent_key(root_dom, inputs.layer_root_key),
        layer_tag = ?try_layer_tag_name(root_dom, inputs.layer_root_key),
        layer_id = ?try_layer_id_attr(root_dom, inputs.layer_root_key),
        layer_class = ?try_layer_class_attr(root_dom, inputs.layer_root_key),
        layer_kind = ?layer_kind_value(root_dom, inputs.layer_root_key),
        x = local_x,
        y = local_y,
        scroll_offset_y = inputs.scroll_offset_y_px,
        event = "layer_translated"
    );
    let mut entity_mut = world
        .get_entity_mut(entity)
        .map_err(|_| DomError::InvalidNodeKey)?;
    let Some(mut retained) = entity_mut.get_mut::<UiDomLayerPresentation>() else {
        return Err(DomError::InvalidNodeKey);
    };
    retained.scroll_offset_y_px = inputs.scroll_offset_y_px;
    retained.absolute_offset = presentation.absolute_offset;
    Ok(())
}

fn update_mesh_asset(world: &mut World, handle: &Handle<Mesh>, target_size_px: Vec2) {
    let next_mesh = top_left_layer_mesh(target_size_px);
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    if let Some(current) = meshes.get_mut(handle.id()) {
        *current = next_mesh;
    } else {
        let _ = meshes.insert(handle.id(), next_mesh);
    }
}

fn update_material_asset(
    world: &mut World,
    handle: &Handle<UiDomBaseMaterial>,
    target_size_px: Vec2,
    clip_world: Vec4,
    opacity: f32,
) -> Result<(), DomError> {
    let color_texture = {
        let Some(materials) = world.get_resource::<Assets<UiDomBaseMaterial>>() else {
            return Err(DomError::InvalidNodeKey);
        };
        let Some(material) = materials.get(handle.id()) else {
            return Err(DomError::InvalidNodeKey);
        };
        material.color_texture.clone()
    };
    let pixel_density_multiplier = world
        .get_resource::<LayerPool>()
        .expect("ui-dom GPU presentation should read the installed LayerPool")
        .pixel_density()
        .multiplier();
    let uv_window = {
        let Some(images) = world.get_resource::<Assets<Image>>() else {
            return Err(DomError::InvalidNodeKey);
        };
        let Some(image) = images.get(color_texture.id()) else {
            return Err(DomError::InvalidNodeKey);
        };
        layer_uv_window_for_target_size(target_size_px, image, pixel_density_multiplier)
    };
    let mut materials = world.resource_mut::<Assets<UiDomBaseMaterial>>();
    let Some(material) = materials.get_mut(handle.id()) else {
        return Err(DomError::InvalidNodeKey);
    };
    material.uniform.clip_world = clip_world;
    material.uniform.color.w = opacity;
    material.uniform.uv_window = uv_window;
    Ok(())
}

fn layer_uv_window_for_target_size(
    target_size_px: Vec2,
    image: &Image,
    pixel_density_multiplier: usize,
) -> Vec4 {
    let image_width_px = u32_to_f32(image.texture_descriptor.size.width).max(1.0);
    let image_height_px = u32_to_f32(image.texture_descriptor.size.height).max(1.0);
    let density_scale = usize_to_f32(pixel_density_multiplier).max(1.0);
    Vec4::new(
        0.0,
        0.0,
        ((target_size_px.x * density_scale) / image_width_px).clamp(0.0, 1.0),
        ((target_size_px.y * density_scale) / image_height_px).clamp(0.0, 1.0),
    )
}

fn update_layer_translation(transform: &mut Transform, inputs: LayerPresentationInputs) {
    transform.translation.x = inputs.absolute_offset.x_px;
    transform.translation.y = -(inputs.absolute_offset.y_px - inputs.scroll_offset_y_px);
}

fn layer_dom_parent_key(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    root_dom.node(layer_root_key).and_then(DomNode::parent_key)
}

fn try_layer_tag_name(root_dom: &RootDom, layer_root_key: DomNodeSlotKey) -> Option<&str> {
    match root_dom.node(layer_root_key) {
        Some(DomNode::Document(_)) => Some("document"),
        Some(DomNode::DocumentFragment(_)) => Some("document_fragment"),
        Some(DomNode::Element(element)) => Some(element.tag.selector_name()),
        Some(DomNode::Text(_)) => Some("text"),
        None => None,
    }
}

fn try_layer_id_attr(root_dom: &RootDom, layer_root_key: DomNodeSlotKey) -> Option<&str> {
    root_dom
        .node(layer_root_key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::Id))
        .and_then(try_attribute_string_value)
}

fn try_layer_class_attr(root_dom: &RootDom, layer_root_key: DomNodeSlotKey) -> Option<&str> {
    root_dom
        .node(layer_root_key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::Class))
        .and_then(try_attribute_string_value)
}

fn try_attribute_string_value(value: &DomAttributeValue) -> Option<&str> {
    match value {
        DomAttributeValue::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn layer_kind_value(root_dom: &RootDom, layer_root_key: DomNodeSlotKey) -> DomLayerKind {
    root_dom
        .node(layer_root_key)
        .map_or(DomLayerKind::None, layer_kind_for_node)
}

fn top_left_layer_mesh(size_px: Vec2) -> Mesh {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [size_px.x, 0.0, 0.0],
        [size_px.x, -size_px.y, 0.0],
        [0.0, -size_px.y, 0.0],
    ];
    let normals = vec![[0.0, 0.0, 1.0]; 4];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

fn layer_world_offset_from_parent_layer(
    parent_layer_world_offset: PointPx,
    layer_offset: PointPx,
) -> PointPx {
    PointPx {
        x_px: parent_layer_world_offset.x_px + layer_offset.x_px,
        y_px: parent_layer_world_offset.y_px + layer_offset.y_px,
    }
}

fn push_dom_child_keys_reverse(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    parent_entity: Entity,
    node_stack: &mut SmallVec<[(DomNodeSlotKey, Entity); 32]>,
) -> Result<(), DomError> {
    let mut reversed_child_keys = SmallVec::<[DomNodeSlotKey; 16]>::new();
    let mut child_key = first_child_key(root_dom, node_key)?;
    while let Some(current_child_key) = child_key {
        reversed_child_keys.push(current_child_key);
        child_key = next_sibling_key(root_dom, current_child_key)?;
    }
    for child_key in reversed_child_keys.into_iter().rev() {
        node_stack.push((child_key, parent_entity));
    }
    Ok(())
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
        root_dom.layer_entity(bevy_layer.dom_node_slot_key),
        Some(entity),
        "BevyLayer identity should stay synchronized with RootDom layer registration",
    );
    Ok(bevy_layer.dom_node_slot_key)
}

fn push_bevy_layer_children_reverse(
    world: &World,
    entity: Entity,
    layer_stack: &mut SmallVec<[Entity; 32]>,
) {
    let Some(children) = world.get::<Children>(entity) else {
        return;
    };
    for child_entity in children.iter().rev() {
        if world.get::<BevyLayer>(*child_entity).is_some() {
            layer_stack.push(*child_entity);
        }
    }
}

fn push_bevy_layer_children_with_clip_reverse(
    world: &World,
    entity: Entity,
    inherited_clip_world: Option<Vec4>,
    raised_root_carry: Option<DomNodeSlotKey>,
    parent_layer_world_offset: PointPx,
    layer_stack: &mut BevyLayerClipStack,
) {
    let Some(children) = world.get::<Children>(entity) else {
        return;
    };
    for child_entity in children.iter().rev() {
        if world.get::<BevyLayer>(*child_entity).is_some() {
            layer_stack.push(BevyLayerClipStackEntry {
                entity: *child_entity,
                inherited_clip_world,
                raised_root_carry,
                parent_layer_world_offset,
            });
        }
    }
}

fn layer_presentation_inputs(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    absolute_offset: PointPx,
    clip_world_offset: PointPx,
    inherited_clip_world: Option<Vec4>,
) -> Result<LayerPresentationInputs, DomError> {
    let viewport_size_px = viewport_layer_size(root_dom, node_key)?;
    let target_size_px = target_layer_size(root_dom, node_key)?;
    let scroll_offset_y_px = root_dom.vertical_scroll_offset_y_px(node_key);
    let local_clip_world = local_clip_inputs(root_dom, node_key, clip_world_offset)?;
    Ok(LayerPresentationInputs {
        layer_root_key: node_key,
        absolute_offset,
        viewport_size_px: Vec2::new(viewport_size_px.0, viewport_size_px.1),
        target_size_px: Vec2::new(target_size_px.0, target_size_px.1),
        effective_clip_world: intersect_clip_world(inherited_clip_world, local_clip_world),
        scroll_offset_y_px,
        opacity: node_opacity(root_dom, node_key)?,
    })
}

fn sync_root_presentation_and_publish_retained_order(
    root_dom: &mut RootDom,
    world: &mut World,
    root_layer_entity: Entity,
    attached_root_parent: MainEntity,
) -> Result<(), DomError> {
    if world.get_entity(root_layer_entity).is_err() {
        return Err(DomError::InvalidNodeKey);
    }
    let mut ordered_surface_entities = Vec::new();
    let mut raised_surface_entities = Vec::new();
    let mut layer_stack = BevyLayerClipStack::new();
    layer_stack.push(BevyLayerClipStackEntry {
        entity: root_layer_entity,
        inherited_clip_world: None,
        raised_root_carry: None,
        parent_layer_world_offset: PointPx::default(),
    });
    while let Some(stack_entry) = layer_stack.pop() {
        let layer_entity = stack_entry.entity;
        let layer_root_key = bevy_layer_root_key(root_dom, world, layer_entity)?;
        if root_dom.node(layer_root_key).is_none() {
            continue;
        }
        let next_raised_root_carry = if stack_entry.raised_root_carry.is_none()
            && root_dom
                .node(layer_root_key)
                .is_some_and(|node| node.layer_order() == crate::types::LayerOrder::Raised)
        {
            Some(layer_root_key)
        } else {
            stack_entry.raised_root_carry
        };
        let layer_offset = layer_offset_from_dom_climb(root_dom, layer_root_key)?;
        let layer_world_offset = layer_world_offset_from_parent_layer(
            stack_entry.parent_layer_world_offset,
            layer_offset,
        );
        let inputs = layer_presentation_inputs(
            root_dom,
            layer_root_key,
            layer_offset,
            layer_world_offset,
            if next_raised_root_carry == Some(layer_root_key) {
                None
            } else {
                stack_entry.inherited_clip_world
            },
        )?;
        refresh_layer_presentation_if_needed(root_dom, world, layer_entity, inputs)?;
        if next_raised_root_carry.is_some() {
            raised_surface_entities.push(MainEntity::from(layer_entity));
        } else {
            ordered_surface_entities.push(MainEntity::from(layer_entity));
        }
        push_bevy_layer_children_with_clip_reverse(
            world,
            layer_entity,
            Some(inputs.effective_clip_world),
            next_raised_root_carry,
            layer_world_offset,
            &mut layer_stack,
        );
    }
    ordered_surface_entities.extend(raised_surface_entities);
    let parent_z_order = attached_root_parent_z_order_or_zero(world, attached_root_parent.id())?;
    world
        .get_resource_mut::<LayerPoolRenderJobQueue>()
        .expect("ui-dom ordered surface updates require LayerPoolRenderJobQueue")
        .replace_root_surface_order(
            attached_root_parent,
            parent_z_order,
            ordered_surface_entities,
        );
    Ok(())
}

fn attached_root_parent_z_order_or_zero(
    world: &World,
    attached_root_parent: Entity,
) -> Result<f32, DomError> {
    let parent_ref = world
        .get_entity(attached_root_parent)
        .map_err(|_| DomError::InvalidParentTarget)?;
    if let Some(global_transform) = parent_ref.get::<GlobalTransform>() {
        return Ok(global_transform.translation().z);
    }
    Ok(parent_ref
        .get::<Transform>()
        .map_or(0.0, |transform| transform.translation.z))
}

fn queue_root_surface_order_detach(world: &mut World, attached_root_parent: MainEntity) {
    world
        .get_resource_mut::<LayerPoolRenderJobQueue>()
        .expect("ui-dom ordered surface detach requires LayerPoolRenderJobQueue")
        .detach_root_surface_order(attached_root_parent);
}

fn viewport_layer_size(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<(f32, f32), DomError> {
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let border_box = node_box_from_ref(node).border_box;
    Ok((border_box.width_px.max(1.0), border_box.height_px.max(1.0)))
}

fn target_layer_size(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<(f32, f32), DomError> {
    let node = root_dom
        .node(layer_root_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let border_box = node_box_from_ref(node).border_box;
    let height_px = if layer_target_expands_for_vertical_overflow(node) {
        node.layout().content_height_px.max(border_box.height_px)
    } else {
        border_box.height_px
    };
    Ok((border_box.width_px.max(1.0), height_px.max(1.0)))
}

fn layer_target_expands_for_vertical_overflow(node: &DomNode) -> bool {
    if layer_kind_for_node(node) != DomLayerKind::Clipped {
        return false;
    }
    matches!(
        node.presented_attr(&DomAttribute::Overflow),
        Some(DomAttributeValue::String(value))
            if matches!(value.as_str(), "scroll" | "auto" | "hidden")
    )
}

fn current_layer_rect(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
    absolute_offset: PointPx,
) -> Result<(f32, f32, f32, f32), DomError> {
    let target_size = target_layer_size(root_dom, layer_root_key)?;
    Ok((
        absolute_offset.x_px,
        absolute_offset.y_px,
        target_size.0,
        target_size.1,
    ))
}

fn local_clip_world(
    _node_key: DomNodeSlotKey,
    node: &DomNode,
    clip_world_offset: PointPx,
) -> Option<Vec4> {
    if layer_kind_for_node(node) != DomLayerKind::Clipped {
        return None;
    }
    let border_box = node_box_from_ref(node).border_box;
    Some(world_rect(
        clip_world_offset.x_px,
        clip_world_offset.y_px,
        border_box.width_px.max(1.0),
        border_box.height_px.max(1.0),
    ))
}

fn layer_presentation_node(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<&DomNode, DomError> {
    root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)
}

fn scroll_offset_changed(current_scroll_offset_y_px: f32, next_scroll_offset_y_px: f32) -> bool {
    (current_scroll_offset_y_px - next_scroll_offset_y_px).abs() > f32::EPSILON
}

fn local_clip_inputs(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    clip_world_offset: PointPx,
) -> Result<Option<Vec4>, DomError> {
    let node = layer_presentation_node(root_dom, node_key)?;
    Ok(local_clip_world(node_key, node, clip_world_offset))
}

fn node_opacity(root_dom: &RootDom, node_key: DomNodeSlotKey) -> Result<f32, DomError> {
    let node = layer_presentation_node(root_dom, node_key)?;
    Ok(match node.presented_custom_attr("opacity") {
        Some(crate::types::DomAttributeValue::Float(value)) => *value,
        Some(crate::types::DomAttributeValue::String(value)) => value
            .parse::<f32>()
            .expect("presented opacity should parse as f32"),
        _ => 1.0,
    })
}

fn world_rect(left_px: f32, top_px: f32, width_px: f32, height_px: f32) -> Vec4 {
    Vec4::new(left_px, -(top_px + height_px), left_px + width_px, -top_px)
}

fn unclipped_world_rect() -> Vec4 {
    Vec4::new(-1_000_000.0, -1_000_000.0, 1_000_000.0, 1_000_000.0)
}

fn intersect_clip_world(inherited: Option<Vec4>, local: Option<Vec4>) -> Vec4 {
    match (inherited, local) {
        (Some(parent), Some(current)) => Vec4::new(
            parent.x.max(current.x),
            parent.y.max(current.y),
            parent.z.min(current.z),
            parent.w.min(current.w),
        ),
        (Some(parent), None) => parent,
        (None, Some(current)) => current,
        (None, None) => unclipped_world_rect(),
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        asset::Assets,
        image::Image,
        prelude::{Entity, Transform, Vec2, Vec4, World},
    };

    use crate::{
        domains::layout::relative_offset_for_node,
        domains::style::StyleRuleOrigin,
        domains::{LayerConfig, LayerPool, LayerPoolRenderJobQueue},
        render_plugin::{UiDomBaseMaterial, UiDomRenderSupport},
        types::{DomNodeSlotKey, DomTag, PixelDensity, RootDom, RootDomContext, RootDomFragment},
    };

    use super::{UiDomLayerPresentation, unclipped_world_rect, update};

    struct PopupCompositeScenario {
        root: RootDom,
        world: World,
        parent: bevy::prelude::Entity,
        clipped_root: DomNodeSlotKey,
        rows: Vec<DomNodeSlotKey>,
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_vec4_close(actual: Vec4, expected: Vec4) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
        assert_close(actual.z, expected.z);
        assert_close(actual.w, expected.w);
    }

    fn install_direct_gpu_support(world: &mut World) {
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let layer_config = LayerConfig::default();
        if !world.contains_resource::<LayerPool>() {
            world.insert_resource(LayerPool::new(layer_config));
        }
        if !world.contains_resource::<LayerPoolRenderJobQueue>() {
            world.insert_resource(LayerPoolRenderJobQueue::default());
        }
    }

    fn presentation_material_uniform(world: &World, entity: bevy::prelude::Entity) -> Vec4 {
        let presentation = world
            .get::<UiDomLayerPresentation>(entity)
            .expect("layer presentation should exist");
        let materials = world
            .get_resource::<Assets<UiDomBaseMaterial>>()
            .expect("ui-dom material assets should exist");
        materials
            .get(presentation.material.id())
            .expect("layer presentation material should exist")
            .uniform
            .uv_window
    }

    fn presentation_image_height_px(world: &World, entity: bevy::prelude::Entity) -> u32 {
        let presentation = world
            .get::<UiDomLayerPresentation>(entity)
            .expect("layer presentation should exist");
        let materials = world
            .get_resource::<Assets<UiDomBaseMaterial>>()
            .expect("ui-dom material assets should exist");
        let material = materials
            .get(presentation.material.id())
            .expect("layer presentation material should exist");
        let images = world
            .get_resource::<Assets<Image>>()
            .expect("image assets should exist");
        images
            .get(material.color_texture.id())
            .expect("layer surface image should exist")
            .texture_descriptor
            .size
            .height
    }

    fn popup_composite_scenario(row_count: usize) -> PopupCompositeScenario {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "left", "16px")
            .expect("clipped root left");
        root.set_attr_str(clipped_root, "top", "12px")
            .expect("clipped root top");
        root.set_attr_str(clipped_root, "width", "180px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "max-height", "160px")
            .expect("clipped root max height");
        root.set_attr_str(clipped_root, "overflow", "scroll")
            .expect("clipped root overflow");
        root.set_attr_str(content, "width", "172px")
            .expect("content width");
        root.set_attr_str(content, "padding-left", "4px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "4px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-top", "4px")
            .expect("content top padding");
        root.set_attr_str(content, "padding-bottom", "4px")
            .expect("content bottom padding");

        let mut rows = Vec::with_capacity(row_count);
        for index in 0..row_count {
            let row = root.create_element(DomTag::Button).expect("row");
            let label = root
                .create_text_node(format!("Scene {index}"))
                .expect("label");
            root.append_child(content, row).expect("append row");
            root.append_child(row, label).expect("append label");
            root.set_attr_str(row, "display", "block")
                .expect("row display");
            root.set_attr_str(row, "margin-left", "2px")
                .expect("row margin left");
            root.set_attr_str(row, "margin-right", "2px")
                .expect("row margin right");
            root.set_attr_str(row, "margin-top", "1px")
                .expect("row margin top");
            root.set_attr_str(row, "margin-bottom", "1px")
                .expect("row margin bottom");
            root.set_attr_str(row, "padding-left", "2px")
                .expect("row padding left");
            root.set_attr_str(row, "padding-right", "2px")
                .expect("row padding right");
            root.set_attr_str(row, "padding-top", "1px")
                .expect("row padding top");
            root.set_attr_str(row, "padding-bottom", "1px")
                .expect("row padding bottom");
            rows.push(row);
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        PopupCompositeScenario {
            root,
            world,
            parent,
            clipped_root,
            rows,
        }
    }

    #[test]
    fn nested_layer_transform_uses_layer_relative_offset_without_ancestor_accumulation() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let root_layer = root.create_element(DomTag::Div).expect("root layer");
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let nested_layer = root.create_element(DomTag::Div).expect("nested layer");
        root.append_child(root.root, root_layer)
            .expect("append root layer");
        root.append_child(root_layer, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, nested_layer)
            .expect("append nested layer");
        root.set_attr_str(root_layer, "position", "fixed")
            .expect("root layer position");
        root.set_attr_str(root_layer, "width", "160px")
            .expect("root layer width");
        root.set_attr_str(root_layer, "height", "120px")
            .expect("root layer height");
        root.set_attr_str(wrapper, "margin-left", "14px")
            .expect("wrapper margin left");
        root.set_attr_str(wrapper, "margin-top", "11px")
            .expect("wrapper margin top");
        root.set_attr_str(wrapper, "padding-left", "7px")
            .expect("wrapper padding left");
        root.set_attr_str(wrapper, "padding-top", "5px")
            .expect("wrapper padding top");
        root.set_attr_str(wrapper, "width", "50px")
            .expect("wrapper width");
        root.set_attr_str(wrapper, "height", "40px")
            .expect("wrapper height");
        root.set_attr_str(nested_layer, "transform", "translateX(1px)")
            .expect("nested layer transform");
        root.set_attr_str(nested_layer, "width", "13px")
            .expect("nested layer width");
        root.set_attr_str(nested_layer, "height", "9px")
            .expect("nested layer height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let nested_entity = root.layer_entity(nested_layer).expect("nested entity");
        let nested_transform = world
            .get::<Transform>(nested_entity)
            .expect("nested transform");
        let nested_offset = relative_offset_for_node(&root, nested_layer);

        assert_close(nested_transform.translation.x, nested_offset.x_px);
        assert_close(nested_transform.translation.y, -(nested_offset.y_px));
    }

    #[test]
    fn fixed_right_anchored_toolbar_layer_transform_stays_on_viewport_right_edge() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let toolbar = root.create_element(DomTag::Div).expect("toolbar");
        let slot_row = root.create_element(DomTag::Div).expect("slot row");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, toolbar).expect("append toolbar");
        root.append_child(toolbar, slot_row)
            .expect("append slot row");
        root.append_child(toolbar, content).expect("append content");
        root.set_attr_str(wrapper, "width", "140px")
            .expect("wrapper width");
        root.set_attr_str(wrapper, "margin-left", "80px")
            .expect("wrapper margin left");
        root.set_attr_str(toolbar, "position", "fixed")
            .expect("toolbar position");
        root.set_attr_str(toolbar, "top", "132px")
            .expect("toolbar top");
        root.set_attr_str(toolbar, "right", "25px")
            .expect("toolbar right");
        root.set_attr_str(toolbar, "width", "196px")
            .expect("toolbar width");
        root.set_attr_str(slot_row, "position", "relative")
            .expect("slot row position");
        root.set_attr_str(slot_row, "width", "196px")
            .expect("slot row width");
        root.set_attr_str(slot_row, "height", "44px")
            .expect("slot row height");
        root.set_attr_str(content, "padding-left", "6px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "6px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-bottom", "6px")
            .expect("content bottom padding");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let toolbar_entity = root.layer_entity(toolbar).expect("toolbar layer entity");
        let toolbar_transform = world
            .get::<Transform>(toolbar_entity)
            .expect("toolbar transform");
        let toolbar_box = crate::domains::layout::node_box(&root, toolbar)
            .expect("toolbar box")
            .border_box;
        let toolbar_layout = root.node(toolbar).expect("toolbar node").layout();
        let toolbar_offset = relative_offset_for_node(&root, toolbar);

        assert_close(toolbar_layout.available_width_px, 320.0);
        assert_close(toolbar_box.left_px, 99.0);
        assert_close(toolbar_offset.x_px, 99.0);
        assert_close(toolbar_offset.y_px, 132.0);
        assert_close(toolbar_transform.translation.x, 99.0);
        assert_close(toolbar_transform.translation.y, -132.0);
    }

    #[test]
    fn fixed_right_anchored_toolbar_keeps_viewport_anchor_after_text_update() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let top_right_toolbar = root.create_element(DomTag::Div).expect("top-right toolbar");
        let toolbar = root.create_element(DomTag::Div).expect("toolbar");
        let slot_row = root.create_element(DomTag::Div).expect("slot row");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, top_right_toolbar)
            .expect("append top-right toolbar");
        root.append_child(root.root, toolbar)
            .expect("append toolbar");
        root.append_child(toolbar, slot_row)
            .expect("append slot row");
        root.append_child(toolbar, content).expect("append content");
        root.set_attr_str(top_right_toolbar, "position", "fixed")
            .expect("top-right toolbar position");
        root.set_attr_str(top_right_toolbar, "top", "25px")
            .expect("top-right toolbar top");
        root.set_attr_str(top_right_toolbar, "right", "25px")
            .expect("top-right toolbar right");
        root.set_attr_str(top_right_toolbar, "width", "196px")
            .expect("top-right toolbar width");
        root.set_attr_str(top_right_toolbar, "height", "82px")
            .expect("top-right toolbar height");
        root.set_attr_str(toolbar, "position", "fixed")
            .expect("toolbar position");
        root.set_attr_str(toolbar, "top", "132px")
            .expect("toolbar top");
        root.set_attr_str(toolbar, "right", "25px")
            .expect("toolbar right");
        root.set_attr_str(toolbar, "width", "196px")
            .expect("toolbar width");
        root.set_attr_str(toolbar, "pointer-events", "none")
            .expect("toolbar pointer-events");
        root.set_attr_str(slot_row, "position", "relative")
            .expect("slot row position");
        root.set_attr_str(slot_row, "width", "196px")
            .expect("slot row width");
        root.set_attr_str(slot_row, "height", "44px")
            .expect("slot row height");
        root.set_attr_str(content, "padding-left", "6px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "6px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-bottom", "6px")
            .expect("content bottom padding");

        let mut line_text_keys = Vec::new();
        for text in [
            "FPS: --",
            "Avian: 0",
            "volume: down",
            "npc: down",
            "galaxy: down",
        ] {
            let line = root.create_element(DomTag::Div).expect("line");
            let line_text = root.create_text_node(text).expect("line text");
            root.append_child(content, line).expect("append line");
            root.append_child(line, line_text)
                .expect("append line text");
            line_text_keys.push(line_text);
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_text_content(line_text_keys[0], "FPS: 60.0")
            .expect("fps line should update");
        root.set_text_content(line_text_keys[1], "Avian: 1")
            .expect("avian line should update");
        update(&mut root, &mut world, parent, 17.0).expect("second composite update");

        let toolbar_entity = root.layer_entity(toolbar).expect("toolbar layer entity");
        let toolbar_transform = world
            .get::<Transform>(toolbar_entity)
            .expect("toolbar transform");
        let toolbar_box = crate::domains::layout::node_box(&root, toolbar)
            .expect("toolbar box")
            .border_box;
        let toolbar_layout = root.node(toolbar).expect("toolbar node").layout();

        assert_close(toolbar_layout.available_width_px, 320.0);
        assert_close(toolbar_box.left_px, 99.0);
        assert_close(toolbar_transform.translation.x, 99.0);
        assert_close(toolbar_transform.translation.y, -132.0);
    }

    #[test]
    fn fixed_right_anchored_toolbar_keeps_viewport_anchor_across_multiple_text_updates() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let top_right_toolbar = root.create_element(DomTag::Div).expect("top-right toolbar");
        let toolbar = root.create_element(DomTag::Div).expect("toolbar");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, top_right_toolbar)
            .expect("append top-right toolbar");
        root.append_child(root.root, toolbar)
            .expect("append toolbar");
        root.append_child(toolbar, content).expect("append content");
        root.set_attr_str(top_right_toolbar, "position", "fixed")
            .expect("top-right toolbar position");
        root.set_attr_str(top_right_toolbar, "top", "25px")
            .expect("top-right toolbar top");
        root.set_attr_str(top_right_toolbar, "right", "25px")
            .expect("top-right toolbar right");
        root.set_attr_str(top_right_toolbar, "width", "196px")
            .expect("top-right toolbar width");
        root.set_attr_str(top_right_toolbar, "height", "82px")
            .expect("top-right toolbar height");
        root.set_attr_str(toolbar, "position", "fixed")
            .expect("toolbar position");
        root.set_attr_str(toolbar, "top", "132px")
            .expect("toolbar top");
        root.set_attr_str(toolbar, "right", "25px")
            .expect("toolbar right");
        root.set_attr_str(toolbar, "width", "196px")
            .expect("toolbar width");
        root.set_attr_str(content, "padding-left", "6px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "6px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-bottom", "6px")
            .expect("content bottom padding");

        let mut line_text_keys = Vec::new();
        for text in ["FPS: --", "Avian: 0", "volume: down"] {
            let line = root.create_element(DomTag::Div).expect("line");
            let line_text = root.create_text_node(text).expect("line text");
            root.append_child(content, line).expect("append line");
            root.append_child(line, line_text)
                .expect("append line text");
            line_text_keys.push(line_text);
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_text_content(line_text_keys[0], "FPS: 60.0")
            .expect("fps line should update");
        root.set_text_content(line_text_keys[1], "Avian: 1")
            .expect("avian line should update");
        update(&mut root, &mut world, parent, 17.0).expect("second composite update");

        root.set_text_content(line_text_keys[0], "FPS: 144.0")
            .expect("fps line should update again");
        root.set_text_content(line_text_keys[1], "Avian: 9")
            .expect("avian line should update again");
        root.set_text_content(line_text_keys[2], "volume: ok")
            .expect("volume line should update");
        update(&mut root, &mut world, parent, 18.0).expect("third composite update");

        let toolbar_entity = root.layer_entity(toolbar).expect("toolbar layer entity");
        let toolbar_transform = world
            .get::<Transform>(toolbar_entity)
            .expect("toolbar transform");
        let toolbar_box = crate::domains::layout::node_box(&root, toolbar)
            .expect("toolbar box")
            .border_box;
        let toolbar_layout = root.node(toolbar).expect("toolbar node").layout();

        assert_close(toolbar_layout.available_width_px, 320.0);
        assert_close(toolbar_box.left_px, 99.0);
        assert_close(toolbar_transform.translation.x, 99.0);
        assert_close(toolbar_transform.translation.y, -132.0);
    }

    #[test]
    fn fixed_right_anchored_toolbar_uses_resized_viewport_width_after_text_update() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let top_right_toolbar = root.create_element(DomTag::Div).expect("top-right toolbar");
        let toolbar = root.create_element(DomTag::Div).expect("toolbar");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, top_right_toolbar)
            .expect("append top-right toolbar");
        root.append_child(root.root, toolbar)
            .expect("append toolbar");
        root.append_child(toolbar, content).expect("append content");
        root.set_attr_str(top_right_toolbar, "position", "fixed")
            .expect("top-right toolbar position");
        root.set_attr_str(top_right_toolbar, "top", "25px")
            .expect("top-right toolbar top");
        root.set_attr_str(top_right_toolbar, "right", "25px")
            .expect("top-right toolbar right");
        root.set_attr_str(top_right_toolbar, "width", "196px")
            .expect("top-right toolbar width");
        root.set_attr_str(top_right_toolbar, "height", "82px")
            .expect("top-right toolbar height");
        root.set_attr_str(toolbar, "position", "fixed")
            .expect("toolbar position");
        root.set_attr_str(toolbar, "top", "132px")
            .expect("toolbar top");
        root.set_attr_str(toolbar, "right", "25px")
            .expect("toolbar right");
        root.set_attr_str(toolbar, "width", "196px")
            .expect("toolbar width");
        root.set_attr_str(content, "padding-left", "6px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "6px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-bottom", "6px")
            .expect("content bottom padding");

        let line = root.create_element(DomTag::Div).expect("line");
        let line_text = root.create_text_node("FPS: --").expect("line text");
        root.append_child(content, line).expect("append line");
        root.append_child(line, line_text)
            .expect("append line text");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(1280.0, 720.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_text_content(line_text, "FPS: 60.0")
            .expect("fps line should update");
        world.entity_mut(parent).insert(bevy::sprite::Sprite {
            custom_size: Some(Vec2::new(900.0, 700.0)),
            ..Default::default()
        });
        update(&mut root, &mut world, parent, 17.0).expect("second composite update");

        let toolbar_entity = root.layer_entity(toolbar).expect("toolbar layer entity");
        let toolbar_transform = world
            .get::<Transform>(toolbar_entity)
            .expect("toolbar transform");
        let toolbar_box = crate::domains::layout::node_box(&root, toolbar)
            .expect("toolbar box")
            .border_box;
        let toolbar_layout = root.node(toolbar).expect("toolbar node").layout();

        assert_close(toolbar_layout.available_width_px, 900.0);
        assert_close(toolbar_box.left_px, 679.0);
        assert_close(toolbar_transform.translation.x, 679.0);
        assert_close(toolbar_transform.translation.y, -132.0);
    }

    #[test]
    fn fixed_bottom_anchored_panel_keeps_viewport_anchor_after_text_update() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let top_left_panel = root.create_element(DomTag::Div).expect("top-left panel");
        let panel = root.create_element(DomTag::Div).expect("panel");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, top_left_panel)
            .expect("append top-left panel");
        root.append_child(root.root, panel).expect("append panel");
        root.append_child(panel, content).expect("append content");
        root.set_attr_str(top_left_panel, "position", "fixed")
            .expect("top-left panel position");
        root.set_attr_str(top_left_panel, "top", "25px")
            .expect("top-left panel top");
        root.set_attr_str(top_left_panel, "left", "25px")
            .expect("top-left panel left");
        root.set_attr_str(top_left_panel, "width", "96px")
            .expect("top-left panel width");
        root.set_attr_str(top_left_panel, "height", "40px")
            .expect("top-left panel height");
        root.set_attr_str(panel, "position", "fixed")
            .expect("panel position");
        root.set_attr_str(panel, "left", "25px")
            .expect("panel left");
        root.set_attr_str(panel, "bottom", "20px")
            .expect("panel bottom");
        root.set_attr_str(panel, "width", "196px")
            .expect("panel width");
        root.set_attr_str(content, "padding-left", "6px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "6px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-top", "6px")
            .expect("content top padding");
        root.set_attr_str(content, "padding-bottom", "6px")
            .expect("content bottom padding");

        let mut line_text_keys = Vec::new();
        for text in ["One", "Two"] {
            let line = root.create_element(DomTag::Div).expect("line");
            let line_text = root.create_text_node(text).expect("line text");
            root.append_child(content, line).expect("append line");
            root.append_child(line, line_text)
                .expect("append line text");
            line_text_keys.push(line_text);
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_text_content(line_text_keys[0], "One expanded line")
            .expect("first line should update");
        root.set_text_content(line_text_keys[1], "Two expanded line")
            .expect("second line should update");
        update(&mut root, &mut world, parent, 17.0).expect("second composite update");

        let panel_entity = root.layer_entity(panel).expect("panel layer entity");
        let panel_transform = world
            .get::<Transform>(panel_entity)
            .expect("panel transform");
        let panel_box = crate::domains::layout::node_box(&root, panel)
            .expect("panel box")
            .border_box;
        let panel_layout = root.node(panel).expect("panel node").layout();

        assert_close(panel_layout.available_height_px, 320.0);
        assert_close(panel_box.left_px, 25.0);
        assert_close(panel_box.top_px + panel_box.height_px, 300.0);
        assert_close(panel_transform.translation.x, 25.0);
        assert_close(panel_transform.translation.y, -(panel_box.top_px));
    }

    #[test]
    fn fixed_left_right_panel_keeps_viewport_width_after_text_update() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let top_bar = root.create_element(DomTag::Div).expect("top bar");
        let panel = root.create_element(DomTag::Div).expect("panel");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, top_bar)
            .expect("append top bar");
        root.append_child(root.root, panel).expect("append panel");
        root.append_child(panel, content).expect("append content");
        root.set_attr_str(top_bar, "position", "fixed")
            .expect("top bar position");
        root.set_attr_str(top_bar, "top", "12px")
            .expect("top bar top");
        root.set_attr_str(top_bar, "left", "12px")
            .expect("top bar left");
        root.set_attr_str(top_bar, "width", "100px")
            .expect("top bar width");
        root.set_attr_str(top_bar, "height", "20px")
            .expect("top bar height");
        root.set_attr_str(panel, "position", "fixed")
            .expect("panel position");
        root.set_attr_str(panel, "top", "60px").expect("panel top");
        root.set_attr_str(panel, "left", "20px")
            .expect("panel left");
        root.set_attr_str(panel, "right", "20px")
            .expect("panel right");
        root.set_attr_str(content, "padding-left", "4px")
            .expect("content left padding");
        root.set_attr_str(content, "padding-right", "4px")
            .expect("content right padding");
        root.set_attr_str(content, "padding-top", "4px")
            .expect("content top padding");
        root.set_attr_str(content, "padding-bottom", "4px")
            .expect("content bottom padding");

        let text = root.create_text_node("Compact").expect("content text");
        root.append_child(content, text)
            .expect("append content text");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world
            .spawn((bevy::sprite::Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_text_content(
            text,
            "Expanded content that should not change anchored width",
        )
        .expect("content text should update");
        update(&mut root, &mut world, parent, 17.0).expect("second composite update");

        let panel_entity = root.layer_entity(panel).expect("panel layer entity");
        let panel_transform = world
            .get::<Transform>(panel_entity)
            .expect("panel transform");
        let panel_box = crate::domains::layout::node_box(&root, panel)
            .expect("panel box")
            .border_box;
        let panel_layout = root.node(panel).expect("panel node").layout();

        assert_close(panel_layout.available_width_px, 320.0);
        assert_close(panel_box.left_px, 20.0);
        assert_close(panel_box.width_px, 280.0);
        assert_close(panel_transform.translation.x, 20.0);
        assert_close(panel_transform.translation.y, -60.0);
    }

    #[test]
    fn clipped_child_layer_uses_world_clip_inside_offset_parent_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let panel = root.create_element(DomTag::Div).expect("panel");
        let viewport = root.create_element(DomTag::Div).expect("viewport");
        root.append_child(root.root, panel).expect("append panel");
        root.append_child(panel, viewport).expect("append viewport");
        root.set_attr_str(panel, "position", "fixed")
            .expect("panel position");
        root.set_attr_str(panel, "left", "920px")
            .expect("panel left");
        root.set_attr_str(panel, "top", "22px").expect("panel top");
        root.set_attr_str(panel, "width", "360px")
            .expect("panel width");
        root.set_attr_str(panel, "height", "686px")
            .expect("panel height");
        root.set_attr_str(viewport, "position", "absolute")
            .expect("viewport position");
        root.set_attr_str(viewport, "top", "30px")
            .expect("viewport top");
        root.set_attr_str(viewport, "width", "360px")
            .expect("viewport width");
        root.set_attr_str(viewport, "height", "656px")
            .expect("viewport height");
        root.set_attr_str(viewport, "overflow", "scroll")
            .expect("viewport overflow");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let viewport_entity = root.layer_entity(viewport).expect("viewport layer entity");
        let viewport_transform = world
            .get::<Transform>(viewport_entity)
            .expect("viewport transform");
        assert_close(viewport_transform.translation.x, 0.0);
        assert_close(viewport_transform.translation.y, -30.0);

        let viewport_presentation = world
            .get::<UiDomLayerPresentation>(viewport_entity)
            .expect("viewport presentation");
        assert_vec4_close(
            viewport_presentation.clip_world,
            Vec4::new(920.0, -708.0, 1280.0, -52.0),
        );
    }

    #[test]
    fn composite_update_queues_bevy_layer_dfs_surface_order() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let root_layer = root.create_element(DomTag::Div).expect("root layer");
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let nested_layer = root.create_element(DomTag::Div).expect("nested layer");
        let sibling_layer = root.create_element(DomTag::Div).expect("sibling layer");
        root.append_child(root.root, root_layer)
            .expect("append root layer");
        root.append_child(root_layer, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, nested_layer)
            .expect("append nested layer");
        root.append_child(root.root, sibling_layer)
            .expect("append sibling layer");
        for layer_key in [root_layer, nested_layer, sibling_layer] {
            root.set_attr_str(layer_key, "width", "80px")
                .expect("layer width");
            root.set_attr_str(layer_key, "height", "40px")
                .expect("layer height");
        }
        root.set_attr_str(root_layer, "position", "fixed")
            .expect("root layer position");
        root.set_attr_str(nested_layer, "position", "fixed")
            .expect("nested layer position");
        root.set_attr_str(sibling_layer, "position", "fixed")
            .expect("sibling layer position");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");
        world
            .resource_mut::<LayerPoolRenderJobQueue>()
            .clear_jobs_for_test();
        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let ordered_surface_entities = world
            .resource::<LayerPoolRenderJobQueue>()
            .ordered_surface_entities_for_test(parent.into())
            .expect("update should queue retained root order");
        assert_eq!(
            world
                .resource::<LayerPoolRenderJobQueue>()
                .queued_job_count_for_test(),
            1
        );

        assert_eq!(
            ordered_surface_entities,
            [
                root.layer_entity(root.root)
                    .expect("document layer entity")
                    .into(),
                root.layer_entity(root_layer)
                    .expect("root layer entity")
                    .into(),
                root.layer_entity(nested_layer)
                    .expect("nested layer entity")
                    .into(),
                root.layer_entity(sibling_layer)
                    .expect("sibling layer entity")
                    .into(),
            ]
        );
    }

    #[test]
    fn stale_registered_layer_entity_unregisters_during_pending_remove() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let layer = root.create_element(DomTag::Div).expect("layer");
        root.append_child(root.root, layer).expect("append layer");
        root.set_attr_str(layer, "transform", "translateX(1px)")
            .expect("layer transform");
        root.set_attr_str(layer, "width", "80px")
            .expect("layer width");
        root.set_attr_str(layer, "height", "40px")
            .expect("layer height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("initial composite update");

        let layer_entity = root.layer_entity(layer).expect("registered layer entity");
        let despawned = world.despawn(layer_entity);
        assert!(despawned, "test should despawn layer entity out-of-band");

        root.remove_node(layer).expect("queue layer removal");

        update(&mut root, &mut world, parent, 32.0).expect("removal update");

        assert!(
            root.layer_entity(layer).is_none(),
            "stale layer registration should clear during removal"
        );
    }

    #[test]
    fn append_fragment_materializes_nested_layer_under_non_layer_wrapper() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let target = root.create_element(DomTag::Div).expect("target");
        root.append_child(root.root, target).expect("append target");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");
        update(&mut root, &mut world, parent, 16.0).expect("initial composite update");

        let mut fragment = RootDomFragment::new();
        let fragment_root = fragment.document_fragment_key();
        let wrapper = fragment.create_element(DomTag::Div).expect("wrapper");
        let nested_layer = fragment.create_element(DomTag::Div).expect("nested layer");
        fragment
            .append_child(fragment_root, wrapper)
            .expect("attach wrapper");
        fragment
            .append_child(wrapper, nested_layer)
            .expect("attach nested layer");
        fragment
            .set_attr_str(nested_layer, "position", "fixed")
            .expect("nested layer position");
        fragment
            .set_attr_str(nested_layer, "width", "64px")
            .expect("nested layer width");
        fragment
            .set_attr_str(nested_layer, "height", "24px")
            .expect("nested layer height");

        root.append_fragment(target, fragment)
            .expect("fragment append should succeed");
        update(&mut root, &mut world, parent, 32.0).expect("fragment composite update");

        let imported_wrapper = root
            .node(target)
            .expect("target node")
            .first_child(|key| root.node(key))
            .map(|(key, _)| key)
            .expect("imported wrapper");
        let imported_nested_layer = root
            .node(imported_wrapper)
            .expect("wrapper node")
            .first_child(|key| root.node(key))
            .map(|(key, _)| key)
            .expect("imported nested layer");

        assert!(
            root.layer_entity(imported_nested_layer).is_some(),
            "append reconciliation should materialize the imported nested layer",
        );
    }

    #[test]
    fn stylesheet_promotion_waits_until_the_next_composite_update() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let promoted = root.create_element(DomTag::Div).expect("promoted");
        root.append_child(root.root, promoted)
            .expect("append promoted");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("initial composite update");
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".floating { position: fixed; width: 64px; height: 24px; }",
            )
            .expect("stylesheet");
        root.set_attr_str(promoted, "class", "floating")
            .expect("floating class");

        update(&mut root, &mut world, parent, 32.0).expect("promotion update");
        assert!(
            root.layer_entity(promoted).is_none(),
            "style-pass layer promotion should wait until the next composite update",
        );

        update(&mut root, &mut world, parent, 48.0).expect("follow-up composite update");
        assert!(
            root.layer_entity(promoted).is_some(),
            "queued layer promotion should materialize on the next composite update",
        );
    }

    #[test]
    fn removing_parent_layer_tears_down_nested_layer_subtree_once() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        let nested_layer = root.create_element(DomTag::Div).expect("nested layer");
        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, wrapper)
            .expect("append wrapper");
        root.append_child(wrapper, nested_layer)
            .expect("append nested layer");
        for layer_key in [parent_layer, nested_layer] {
            root.set_attr_str(layer_key, "position", "fixed")
                .expect("layer position");
            root.set_attr_str(layer_key, "width", "80px")
                .expect("layer width");
            root.set_attr_str(layer_key, "height", "40px")
                .expect("layer height");
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");
        update(&mut root, &mut world, parent, 16.0).expect("initial composite update");

        let parent_entity = root
            .layer_entity(parent_layer)
            .expect("parent layer entity");
        let nested_entity = root
            .layer_entity(nested_layer)
            .expect("nested layer entity");

        root.remove_node(parent_layer)
            .expect("remove parent layer subtree");
        update(&mut root, &mut world, parent, 32.0).expect("removal composite update");

        assert!(
            root.layer_entity(parent_layer).is_none(),
            "parent layer registration should clear during subtree removal",
        );
        assert!(
            root.layer_entity(nested_layer).is_none(),
            "nested layer registration should clear during subtree removal",
        );
        assert!(
            root.layer_lease(parent_layer).is_none(),
            "parent layer lease should be destroyed during subtree removal",
        );
        assert!(
            root.layer_lease(nested_layer).is_none(),
            "nested layer lease should be destroyed during subtree removal",
        );
        assert!(
            world.get_entity(parent_entity).is_err(),
            "parent layer entity should despawn during subtree removal",
        );
        assert!(
            world.get_entity(nested_entity).is_err(),
            "nested layer entity should despawn during subtree removal",
        );
    }

    #[test]
    fn first_update_initializes_nested_content_layer_presentation_without_placeholder_state() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content_layer = root.create_element(DomTag::Div).expect("content layer");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content_layer)
            .expect("append content layer");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "width", "220px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "height", "84px")
            .expect("clipped root height");
        root.set_attr_str(clipped_root, "overflow", "scroll")
            .expect("clipped root overflow");
        root.set_attr_str(content_layer, "transform", "translateX(1px)")
            .expect("content layer transform");
        root.set_attr_str(content_layer, "width", "220px")
            .expect("content layer width");
        root.set_attr_str(content_layer, "height", "84px")
            .expect("content layer height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let content_entity = root
            .layer_entity(content_layer)
            .expect("content layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(content_entity)
            .expect("content layer presentation");

        assert_eq!(
            presentation.viewport_size_px,
            Vec2::new(220.0, 84.0),
            "first update should set the nested content layer viewport size immediately",
        );
        assert_eq!(
            presentation.target_size_px,
            Vec2::new(220.0, 84.0),
            "first update should set the nested content layer target size immediately",
        );
        assert_ne!(
            presentation.clip_world,
            unclipped_world_rect(),
            "first update should not leave the nested content layer clip in the placeholder unclipped state",
        );
    }

    #[test]
    fn nested_clipped_layer_clip_world_uses_layer_tree_world_offset() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let clipped_child = root.create_element(DomTag::Div).expect("clipped child");
        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, clipped_child)
            .expect("append clipped child");
        root.set_attr_str(parent_layer, "position", "fixed")
            .expect("parent position");
        root.set_attr_str(parent_layer, "left", "40px")
            .expect("parent left");
        root.set_attr_str(parent_layer, "top", "20px")
            .expect("parent top");
        root.set_attr_str(parent_layer, "width", "120px")
            .expect("parent width");
        root.set_attr_str(parent_layer, "height", "80px")
            .expect("parent height");
        root.set_attr_str(clipped_child, "overflow", "scroll")
            .expect("child overflow");
        root.set_attr_str(clipped_child, "margin-left", "7px")
            .expect("child margin left");
        root.set_attr_str(clipped_child, "margin-top", "9px")
            .expect("child margin top");
        root.set_attr_str(clipped_child, "width", "60px")
            .expect("child width");
        root.set_attr_str(clipped_child, "height", "30px")
            .expect("child height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let clipped_entity = root
            .layer_entity(clipped_child)
            .expect("clipped child layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped child presentation");

        assert_vec4_close(
            presentation.clip_world,
            Vec4::new(47.0, -59.0, 107.0, -29.0),
        );
    }

    #[test]
    fn clipped_layer_shrink_requires_presentation_uv_window_to_match_live_target_height() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "width", "120px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "height", "40px")
            .expect("clipped root height");
        root.set_attr_str(clipped_root, "overflow", "scroll")
            .expect("clipped root overflow");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content tall height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_attr_str(content, "height", "20px")
            .expect("content short height");
        update(&mut root, &mut world, parent, 32.0).expect("second composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");
        let retained_image_height_px = presentation_image_height_px(&world, clipped_entity);
        let uv_window = presentation_material_uniform(&world, clipped_entity);

        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "clipped layer should present the live viewport height after content shrinks below it",
        );
        assert!(
            retained_image_height_px > 40,
            "test oracle requires a larger retained image than the live target height",
        );
        assert!(
            uv_window.w < 1.0,
            "presentation should not sample the full retained image after the clipped target height shrinks; retained_image_height_px={retained_image_height_px}, uv_window={uv_window:?}",
        );
    }

    #[test]
    // CSS Overflow 3 plus the retained-layer invariant used for scroll roots:
    // `overflow:auto` should keep the live viewport height while cropping the
    // larger retained target after content shrinks.
    fn auto_overflow_layer_shrink_requires_presentation_uv_window_to_match_live_target_height() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "width", "120px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "height", "40px")
            .expect("clipped root height");
        root.set_attr_str(clipped_root, "overflow", "auto")
            .expect("clipped root overflow");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content tall height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_attr_str(content, "height", "20px")
            .expect("content short height");
        update(&mut root, &mut world, parent, 32.0).expect("second composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");
        let retained_image_height_px = presentation_image_height_px(&world, clipped_entity);
        let uv_window = presentation_material_uniform(&world, clipped_entity);

        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "overflow:auto should present the live viewport height after content shrinks below it",
        );
        assert!(
            retained_image_height_px > 40,
            "test oracle requires a larger retained image than the live target height",
        );
        assert!(
            uv_window.w < 1.0,
            "presentation should not sample the full retained image after the auto-overflow target height shrinks; retained_image_height_px={retained_image_height_px}, uv_window={uv_window:?}",
        );
    }

    #[test]
    // CSS Overflow 3 plus the retained-layer expansion contract:
    // `overflow:clip` clips to the viewport and is not a scroll container, so
    // the retained target should stay viewport-sized instead of expanding to
    // the clipped content height.
    fn clip_overflow_layer_keeps_viewport_sized_target_height_for_tall_content() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "width", "120px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "height", "40px")
            .expect("clipped root height");
        root.set_attr_str(clipped_root, "overflow", "clip")
            .expect("clipped root overflow");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content tall height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");
        let uv_window = presentation_material_uniform(&world, clipped_entity);

        assert_eq!(
            presentation.viewport_size_px,
            Vec2::new(120.0, 40.0),
            "overflow:clip viewport should stay bound to the authored box height",
        );
        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "overflow:clip should not expand the retained target to the clipped content height",
        );
        assert_eq!(
            uv_window,
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            "overflow:clip should keep the full UV window because the retained target matches the viewport",
        );
    }

    #[test]
    // CSS Overflow 3 plus the current retained-layer clip contract:
    // `overflow:hidden` clips the viewport but still uses a larger retained
    // target so the hidden scrolled content remains addressable. In this
    // retained path, viewport clipping comes from `clip_world`, not from UV
    // cropping when the retained image exactly matches the expanded target.
    fn hidden_overflow_layer_expands_target_height_for_tall_content() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "position", "fixed")
            .expect("clipped root position");
        root.set_attr_str(clipped_root, "width", "120px")
            .expect("clipped root width");
        root.set_attr_str(clipped_root, "height", "40px")
            .expect("clipped root height");
        root.set_attr_str(clipped_root, "overflow", "hidden")
            .expect("clipped root overflow");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content tall height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");
        let uv_window = presentation_material_uniform(&world, clipped_entity);
        let retained_image_height_px = presentation_image_height_px(&world, clipped_entity);

        assert_eq!(
            presentation.viewport_size_px,
            Vec2::new(120.0, 40.0),
            "overflow:hidden viewport should stay bound to the authored box height",
        );
        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 100.0),
            "overflow:hidden should expand the retained target to the clipped content height",
        );
        assert!(
            retained_image_height_px >= 100,
            "overflow:hidden retained image should be tall enough to cover the expanded target height",
        );
        assert_eq!(
            uv_window,
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            "overflow:hidden should keep the full UV window when the retained image matches the expanded target height",
        );
        assert_eq!(
            presentation.clip_world,
            Vec4::new(0.0, -40.0, 120.0, 0.0),
            "overflow:hidden should clip the expanded target back to the viewport through the layer clip rect",
        );
    }

    #[test]
    fn stylesheet_auto_overflow_layer_shrink_updates_live_target_height() {
        let (mut world, mut root, clipped_root, content, parent) =
            stylesheet_overflow_composite_fixture("scroll-auto", "overflow: auto;");

        update(&mut root, &mut world, parent, 16.0).expect("first composite update");

        root.set_attr_str(content, "height", "20px")
            .expect("content short height");
        root.set_attr_str(clipped_root, "color", "#ffffff")
            .expect("style recompute attr");
        update(&mut root, &mut world, parent, 32.0).expect("second composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");

        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "stylesheet overflow:auto should present the live viewport height after content shrinks below it",
        );
        assert!(
            presentation_material_uniform(&world, clipped_entity).w < 1.0,
            "stylesheet overflow:auto should keep sampling a cropped UV window from the already-retained tall image after the live target height shrinks",
        );
    }

    #[test]
    fn stylesheet_clip_overflow_layer_keeps_viewport_sized_target_height() {
        let (mut world, mut root, clipped_root, _content, parent) =
            stylesheet_overflow_composite_fixture("scroll-clip", "overflow: clip;");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");

        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "stylesheet overflow:clip should keep the retained target viewport-sized",
        );
        assert_eq!(
            presentation_material_uniform(&world, clipped_entity),
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            "stylesheet overflow:clip should keep the full UV window when the retained target stays viewport-sized",
        );
    }

    #[test]
    fn stylesheet_hidden_overflow_layer_expands_target_height() {
        let (mut world, mut root, clipped_root, _content, parent) =
            stylesheet_overflow_composite_fixture("scroll-hidden", "overflow: hidden;");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("clipped root layer entity");
        let presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped layer presentation");

        assert_eq!(
            presentation.target_size_px,
            Vec2::new(120.0, 100.0),
            "stylesheet overflow:hidden should expand the retained target to the content height",
        );
        assert_eq!(
            presentation_material_uniform(&world, clipped_entity),
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            "stylesheet overflow:hidden should keep the full UV window when the retained image matches the expanded target",
        );
        assert_eq!(
            presentation.clip_world,
            Vec4::new(0.0, -40.0, 120.0, 0.0),
            "stylesheet overflow:hidden should clip the expanded target back to the viewport through the layer clip rect",
        );
    }

    #[test]
    fn switching_stylesheet_overflow_class_from_hidden_to_clip_shrinks_target_presentation() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "
                    .scroll-hidden { position: fixed; width: 120px; height: 40px; overflow: hidden; }
                    .scroll-clip { position: fixed; width: 120px; height: 40px; overflow: clip; }
                ",
            )
            .expect("stylesheet");
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "class", "scroll-hidden")
            .expect("hidden class");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("hidden update");

        let clipped_entity = root
            .layer_entity(clipped_root)
            .expect("layer entity after hidden update");
        let hidden_presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("hidden presentation")
            .clone();
        assert_eq!(
            hidden_presentation.target_size_px,
            Vec2::new(120.0, 100.0),
            "hidden class should expand the retained target before the class switch",
        );

        root.set_attr_str(clipped_root, "class", "scroll-clip")
            .expect("clip class");
        update(&mut root, &mut world, parent, 32.0).expect("clip update");

        let clip_presentation = world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clip presentation");
        assert_eq!(
            clip_presentation.target_size_px,
            Vec2::new(120.0, 40.0),
            "switching to overflow:clip should shrink the retained target back to the viewport height",
        );
        assert!(
            presentation_material_uniform(&world, clipped_entity).w < 1.0,
            "switching to overflow:clip should keep sampling a cropped UV window from the existing tall retained image instead of rebuilding it immediately",
        );
    }

    fn stylesheet_overflow_composite_fixture(
        class_name: &str,
        overflow_rule: &str,
    ) -> (World, RootDom, DomNodeSlotKey, DomNodeSlotKey, Entity) {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                &format!(
                    ".{class_name} {{ position: fixed; width: 120px; height: 40px; {overflow_rule} }}"
                ),
            )
            .expect("stylesheet");
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let content = root.create_element(DomTag::Div).expect("content");
        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, content)
            .expect("append content");
        root.set_attr_str(clipped_root, "class", class_name)
            .expect("clipped root class");
        root.set_attr_str(content, "width", "120px")
            .expect("content width");
        root.set_attr_str(content, "height", "100px")
            .expect("content tall height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        (world, root, clipped_root, content, parent)
    }

    #[test]
    fn auto_height_clipped_popup_shrink_updates_live_viewport_and_target_height() {
        let mut scenario = popup_composite_scenario(10);

        update(
            &mut scenario.root,
            &mut scenario.world,
            scenario.parent,
            16.0,
        )
        .expect("first composite update");

        for &row in scenario.rows.iter().skip(7) {
            scenario.root.remove_node(row).expect("remove row");
        }
        update(
            &mut scenario.root,
            &mut scenario.world,
            scenario.parent,
            32.0,
        )
        .expect("second composite update");

        let clipped_entity = scenario
            .root
            .layer_entity(scenario.clipped_root)
            .expect("clipped root layer entity");
        let presentation = scenario
            .world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped root presentation");
        let retained_image_height_px =
            presentation_image_height_px(&scenario.world, clipped_entity);
        let uv_window = presentation_material_uniform(&scenario.world, clipped_entity);
        let live_box = crate::domains::layout::node_box(&scenario.root, scenario.clipped_root)
            .expect("clipped root should expose a box");
        let live_height_px = live_box.border_box.height_px;

        assert!(
            live_height_px < 160.0,
            "test oracle requires the live auto-height popup to shrink below max-height; got {live_height_px}",
        );
        assert_eq!(
            presentation.viewport_size_px,
            Vec2::new(180.0, live_height_px),
            "clipped popup viewport should match the live auto-height after shrinking below max-height",
        );
        assert_eq!(
            presentation.target_size_px,
            Vec2::new(180.0, live_height_px),
            "clipped popup target should match the live auto-height after shrinking below max-height",
        );
        assert!(
            retained_image_height_px > 160,
            "test oracle requires the retained image to stay larger than the live popup height; got {retained_image_height_px}",
        );
        assert!(
            uv_window.w < 1.0,
            "presentation should crop the retained image after the auto-height popup shrinks; retained_image_height_px={retained_image_height_px}, uv_window={uv_window:?}",
        );
    }

    #[test]
    fn auto_height_clipped_popup_shrink_to_padding_updates_live_viewport_and_target_height() {
        let mut scenario = popup_composite_scenario(10);

        update(
            &mut scenario.root,
            &mut scenario.world,
            scenario.parent,
            16.0,
        )
        .expect("first composite update");

        for &row in &scenario.rows {
            scenario.root.remove_node(row).expect("remove row");
        }
        update(
            &mut scenario.root,
            &mut scenario.world,
            scenario.parent,
            32.0,
        )
        .expect("second composite update");

        let clipped_entity = scenario
            .root
            .layer_entity(scenario.clipped_root)
            .expect("clipped root layer entity");
        let presentation = scenario
            .world
            .get::<UiDomLayerPresentation>(clipped_entity)
            .expect("clipped root presentation");
        let retained_image_height_px =
            presentation_image_height_px(&scenario.world, clipped_entity);
        let uv_window = presentation_material_uniform(&scenario.world, clipped_entity);
        let live_box = crate::domains::layout::node_box(&scenario.root, scenario.clipped_root)
            .expect("clipped root should expose a box");
        let live_height_px = live_box.border_box.height_px;

        assert!(
            live_height_px < 16.0,
            "empty auto-height popup should shrink close to its padding height; got {live_height_px}",
        );
        assert_eq!(
            presentation.viewport_size_px,
            Vec2::new(180.0, live_height_px),
            "clipped popup viewport should match the live padding-only height after all rows are removed",
        );
        assert_eq!(
            presentation.target_size_px,
            Vec2::new(180.0, live_height_px),
            "clipped popup target should match the live padding-only height after all rows are removed",
        );
        assert!(
            retained_image_height_px > 160,
            "test oracle requires the retained image to stay larger than the live popup height; got {retained_image_height_px}",
        );
        assert!(
            uv_window.w < 0.1,
            "presentation should crop to a small portion of the retained image after all rows are removed; retained_image_height_px={retained_image_height_px}, uv_window={uv_window:?}",
        );
    }

    #[test]
    fn x2_density_presentation_keeps_full_uv_window_for_matching_logical_target() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let layer = root.create_element(DomTag::Div).expect("layer");
        root.append_child(root.root, layer).expect("append layer");
        root.set_attr_str(layer, "position", "fixed")
            .expect("layer position");
        root.set_attr_str(layer, "width", "120px")
            .expect("layer width");
        root.set_attr_str(layer, "height", "40px")
            .expect("layer height");

        let mut world = World::new();
        world.insert_resource(LayerPool::new(LayerConfig::new(PixelDensity::X2)));
        install_direct_gpu_support(&mut world);
        world.init_resource::<Assets<Image>>();
        let parent = world.spawn_empty().id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        update(&mut root, &mut world, parent, 16.0).expect("composite update");

        let entity = root.layer_entity(layer).expect("layer entity");
        let uv_window = presentation_material_uniform(&world, entity);
        let retained_image_height_px = presentation_image_height_px(&world, entity);

        assert_eq!(
            retained_image_height_px, 80,
            "x2 density should allocate the retained image in physical pixels",
        );
        assert_eq!(
            uv_window,
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            "matching logical and physical layer sizes should not crop the presentation material at x2 density",
        );
    }
}
