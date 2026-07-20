use bevy::asset::Assets;
use bevy::image::Image;
use bevy::prelude::{Vec2, World};
use bevy::sprite::Sprite;
use ui_dom::{
    DomPointerUpdate, DomTag, LayerConfig, LayerPool, RootDom, RootDomContext, UiDomRenderSupport,
};

fn integration_world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<Image>>();
    world.insert_resource(LayerPool::new(LayerConfig::default()));
    world.insert_resource(UiDomRenderSupport::direct_gpu());
    world
}

#[test]
fn public_external_layer_target_exposes_document_image() {
    let mut world = integration_world();
    let parent = world
        .spawn((Sprite {
            custom_size: Some(Vec2::new(64.0, 64.0)),
            ..Default::default()
        },))
        .id();
    let mut root = RootDom::new(RootDomContext::new_gpu());
    root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
        .expect("root should attach");

    let target = root
        .acquire_external_layer_target(&mut world, root.document_node().0)
        .expect("public external target should expose the document layer image");

    assert_eq!(target.image_extent_px.x, 64);
    assert!(
        target.image_extent_px.y >= 1,
        "target image height should be a valid texture extent"
    );
    assert!(
        target
            .origin_offset_px
            .iter()
            .all(|value| value.abs() <= f32::EPSILON)
    );
}

#[test]
fn root_dom_retries_after_dead_bevy_parent_rejection() {
    let mut root = RootDom::new(RootDomContext::new_gpu());
    let node = root.create_element(DomTag::Div).expect("node");
    root.append_child(root.document_node().0, node)
        .expect("append");

    let mut world = integration_world();
    let dead_parent = world.spawn_empty().id();
    assert!(world.despawn(dead_parent), "dead parent should despawn");
    assert!(
        root.attach_to_bevy_parent_entity(&mut world, dead_parent, 0.0)
            .is_err(),
        "dead parent should fail attach",
    );

    let live_parent = world.spawn_empty().id();
    root.attach_to_bevy_parent_entity(&mut world, live_parent, 0.0)
        .expect("retry attach should succeed");
    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("attached root should update after retry");
}

#[test]
fn layer_pool_reuses_a_surface_asset_for_matching_extents() {
    let mut world = integration_world();
    let first = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
    LayerPool::acquire_layer_surface_in_world(&mut world, first)
        .expect("first lease should acquire a surface");
    assert!(
        LayerPool::release_layer_surface_in_world(&mut world, first).is_ok(),
        "first lease should release its surface",
    );
    assert!(
        LayerPool::destroy_layer_lease_in_world(&mut world, first).is_ok(),
        "first lease should destroy cleanly",
    );

    let second = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
    LayerPool::acquire_layer_surface_in_world(&mut world, second)
        .expect("second lease should reuse a matching surface");

    let image_count = world.resource::<Assets<Image>>().len();
    assert_eq!(
        image_count, 1,
        "matching extents should reuse one image asset"
    );
}
