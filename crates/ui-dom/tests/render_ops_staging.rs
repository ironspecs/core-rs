//! Gate scenarios G1–G9 for the `RenderOpsHandler` staging seam.
//!
//! These tests observe the exact render ops a `RootDom::update` produces, after
//! the update and before the render world's extract moves them. They pin the
//! staged-apply geometry the paint stage emits so the offset-accumulation
//! helpers can eventually be replaced by direct op assertions.
//!
//! Layer-local pixel geometry is asserted against exactly-representable
//! constants where possible; non-exact floats use `assert_close`, matching the
//! crate's existing test style.

#[allow(dead_code)]
#[path = "layout_proofs/support.rs"]
mod support;

use std::sync::Arc;

use bevy::asset::Assets;
use bevy::image::Image;
use bevy::prelude::{Entity, Vec2, World};
use bevy::sprite::Sprite;
use ui_dom::{
    DomError, DomPointerUpdate, DomTag, LayerConfig, LayerPaintPlan, LayerPool, LayerPoolPolicy,
    RectPaintOp, RenderOpsHandler, RootDom, RootDomContext, RootDomContextConfig, StagedLayerApply,
    UiDomRenderSupport,
};

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "expected {expected}, got {actual}"
    );
}

fn assert_vec2_close(actual: [f32; 2], expected: [f32; 2]) {
    assert_close(actual[0], expected[0]);
    assert_close(actual[1], expected[1]);
}

fn assert_vec4_close(actual: [f32; 4], expected: [f32; 4]) {
    assert_close(actual[0], expected[0]);
    assert_close(actual[1], expected[1]);
    assert_close(actual[2], expected[2]);
    assert_close(actual[3], expected[3]);
}

/// Builds one direct-GPU world with the same retained support as
/// `integration_vendor_boundaries.rs`, for tests that need several roots in one
/// world or no `RootDom` at all.
fn direct_gpu_world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<Image>>();
    world.insert_resource(LayerPool::new(LayerConfig::default()));
    world.insert_resource(UiDomRenderSupport::direct_gpu());
    world
}

fn spawn_sprite_parent(world: &mut World, width_px: f32, height_px: f32) -> Entity {
    world
        .spawn((Sprite {
            custom_size: Some(Vec2::new(width_px, height_px)),
            ..Default::default()
        },))
        .id()
}

fn append_sized_div(
    root: &mut RootDom,
    parent: ui_dom::DomNodeSlotKey,
    width: &str,
    height: &str,
) -> ui_dom::DomNodeSlotKey {
    let key = root.create_element(DomTag::Div).expect("div should insert");
    root.append_child(parent, key).expect("div should append");
    root.style_mut(key)
        .set("width", width)
        .expect("width should parse");
    root.style_mut(key)
        .set("height", height)
        .expect("height should parse");
    key
}

#[test]
fn g1_exact_rect_geometry_single_layer() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    append_sized_div(&mut root, document, "100px", "40px");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        let plan = &applies[0].plan;
        // A bare `Div` emits exactly one background rect; the `Document` node
        // emits none.
        assert_eq!(plan.rects.len(), 1);
        assert_vec2_close(plan.rects[0].position_px, [0.0, 0.0]);
        assert_vec2_close(plan.rects[0].size_px, [100.0, 40.0]);
        assert!(plan.text_runs.is_empty());
        assert!(plan.icons.is_empty());
        assert!(plan.images.is_empty());
        assert!(plan.svgs.is_empty());
        assert!(plan.stroke_paths.is_empty());
    });
}

#[test]
fn g2_op_order_within_layer_is_dom_order() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    append_sized_div(&mut root, document, "50px", "20px");
    append_sized_div(&mut root, document, "80px", "30px");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        let plan = &applies[0].plan;
        assert_eq!(plan.rects.len(), 2);
        // Distinct sizes identify the rects, so asserting sizes is asserting
        // DOM child order: rects[0] is the first child, rects[1] the second.
        assert_vec2_close(plan.rects[0].size_px, [50.0, 20.0]);
        assert_vec2_close(plan.rects[1].size_px, [80.0, 30.0]);
        assert_vec2_close(plan.rects[0].position_px, [0.0, 0.0]);
    });
}

#[test]
fn g3_multi_layer_document() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    // A normal-flow sibling gives the `Document` root layer its own content;
    // the positioned sibling is its own layer root.
    append_sized_div(&mut root, document, "100px", "40px");
    let child = append_sized_div(&mut root, document, "120px", "80px");
    root.style_mut(child)
        .set("position", "fixed")
        .expect("position should parse");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    assert!(
        root.layer_lease(child).is_some(),
        "a positioned child should own a live layer lease after update"
    );

    root.context().render_ops().with_staged_applies(|applies| {
        // One staged apply per painted layer root, in deterministic order: the
        // `Document` root layer first (carrying the normal sibling's rect),
        // then the fixed child layer (carrying its own rect). The fixed
        // child's ops are NOT merged into the document layer's plan.
        assert_eq!(applies.len(), 2);
        assert_eq!(applies[0].plan.rects.len(), 1);
        assert_vec2_close(applies[0].plan.rects[0].size_px, [100.0, 40.0]);
        assert_eq!(applies[1].plan.rects.len(), 1);
        assert_vec2_close(applies[1].plan.rects[0].size_px, [120.0, 80.0]);
    });
}

#[test]
fn g4_text_ops() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    let block = append_sized_div(&mut root, document, "200px", "40px");
    let text = root.create_text_node("hello").expect("text should insert");
    root.append_child(block, text).expect("text should append");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        let plan = &applies[0].plan;
        assert_eq!(plan.text_runs.len(), 1);
        let run = &plan.text_runs[0];
        // Encodes current behavior: the default font size is 14.0 and the text
        // run is laid out at the block's origin with non-zero extent.
        assert_close(run.font_size_px, 14.0);
        assert!(run.size_px[0] > 0.0, "text width should be positive");
        assert!(run.size_px[1] > 0.0, "text height should be positive");
        assert!(run.line_height_px > 0.0, "line height should be positive");
        assert_vec2_close(run.position_px, [0.0, 0.0]);
    });
}

#[test]
fn g5_empty_plan_stages_nothing() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    append_sized_div(&mut root, document, "0px", "0px");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    root.context().render_ops().with_staged_applies(|applies| {
        assert!(applies.is_empty(), "a zero-size layer stages no ops");
    });
}

#[test]
fn g6_steady_state_is_silent() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    append_sized_div(&mut root, document, "100px", "40px");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("first update");
    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
    });
    let _ = root.context().render_ops().take_staged();

    root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
        .expect("second update");
    root.context().render_ops().with_staged_applies(|applies| {
        assert!(applies.is_empty(), "a clean update stages no ops");
    });
}

#[test]
fn g7_handler_sharing_granularity() {
    // (a) Two roots sharing one handler: both roots' applies arrive in update
    // order in the shared handler.
    let mut world = direct_gpu_world();
    let shared = Arc::new(RenderOpsHandler::new());
    let context_a = Arc::new(RootDomContext::new_with_render_ops(
        RootDomContextConfig::gpu(),
        Arc::clone(&shared),
    ));
    let context_b = Arc::new(RootDomContext::new_with_render_ops(
        RootDomContextConfig::gpu(),
        Arc::clone(&shared),
    ));
    let parent_a = spawn_sprite_parent(&mut world, 320.0, 320.0);
    let parent_b = spawn_sprite_parent(&mut world, 320.0, 320.0);
    let mut root_a = RootDom::new(context_a);
    let mut root_b = RootDom::new(context_b);
    root_a
        .attach_to_bevy_parent_entity(&mut world, parent_a, 0.0)
        .expect("root a should attach");
    root_b
        .attach_to_bevy_parent_entity(&mut world, parent_b, 0.0)
        .expect("root b should attach");
    let doc_a = root_a.document_node().0;
    append_sized_div(&mut root_a, doc_a, "100px", "40px");
    let doc_b = root_b.document_node().0;
    append_sized_div(&mut root_b, doc_b, "80px", "30px");
    root_a
        .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("root a update");
    root_b
        .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("root b update");
    shared.with_staged_applies(|applies| {
        assert_eq!(applies.len(), 2);
        assert_vec2_close(applies[0].plan.rects[0].size_px, [100.0, 40.0]);
        assert_vec2_close(applies[1].plan.rects[0].size_px, [80.0, 30.0]);
    });

    // (b) Separate handlers (the default): full isolation per root.
    let mut world = direct_gpu_world();
    let parent_a = spawn_sprite_parent(&mut world, 320.0, 320.0);
    let parent_b = spawn_sprite_parent(&mut world, 320.0, 320.0);
    let mut root_a = RootDom::new(RootDomContext::new_gpu());
    let mut root_b = RootDom::new(RootDomContext::new_gpu());
    root_a
        .attach_to_bevy_parent_entity(&mut world, parent_a, 0.0)
        .expect("root a should attach");
    root_b
        .attach_to_bevy_parent_entity(&mut world, parent_b, 0.0)
        .expect("root b should attach");
    let doc_a = root_a.document_node().0;
    append_sized_div(&mut root_a, doc_a, "100px", "40px");
    let doc_b = root_b.document_node().0;
    append_sized_div(&mut root_b, doc_b, "80px", "30px");
    root_a
        .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("root a update");
    root_b
        .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("root b update");
    root_a
        .context()
        .render_ops()
        .with_staged_applies(|applies| {
            assert_eq!(applies.len(), 1);
            assert_vec2_close(applies[0].plan.rects[0].size_px, [100.0, 40.0]);
        });
    root_b
        .context()
        .render_ops()
        .with_staged_applies(|applies| {
            assert_eq!(applies.len(), 1);
            assert_vec2_close(applies[0].plan.rects[0].size_px, [80.0, 30.0]);
        });
}

#[test]
fn g8_detach_eviction() {
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    append_sized_div(&mut root, document, "100px", "40px");

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");
    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
    });

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach should succeed");

    root.context().render_ops().with_staged_applies(|applies| {
        assert!(
            applies.is_empty(),
            "detach should evict the root's staged applies"
        );
    });
}

#[test]
fn g9_external_producer_no_root_dom() {
    let mut world = direct_gpu_world();
    let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
    LayerPool::set_layer_policy_in_world(&mut world, lease, LayerPoolPolicy::External)
        .expect("external policy should update");
    let target = LayerPool::acquire_external_layer_target_in_world(&mut world, lease)
        .expect("external target should acquire");

    let handler = RenderOpsHandler::new();
    let mut plan = LayerPaintPlan::default();
    plan.rects.push(RectPaintOp {
        position_px: [0.0, 0.0],
        size_px: [32.0, 16.0],
        fill_rgba: [1.0, 0.0, 0.0, 1.0],
        corner_radii_px: [0.0; 4],
        border_widths_px: [0.0; 4],
        border_top_rgba: [0.0; 4],
        border_right_rgba: [0.0; 4],
        border_bottom_rgba: [0.0; 4],
        border_left_rgba: [0.0; 4],
    });
    let _ = handler.stage(
        StagedLayerApply {
            lease: target.lease,
            image_handle: target.image_handle.clone(),
            layer_zero_in_image_px: target.origin_offset_px,
            plan,
        },
        LayerPoolPolicy::External,
    );

    handler.with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        assert_eq!(applies[0].lease, target.lease);
    });
    let batch = handler.take_staged();
    assert_eq!(batch.applies.len(), 1);
    assert!(handler.take_staged().applies.is_empty());

    // The re-homed upload path stages through the same handler.
    let byte_len = usize::try_from(target.image_extent_px.x).expect("width fits usize")
        * usize::try_from(target.image_extent_px.y).expect("height fits usize")
        * 4;
    let bytes = vec![0u8; byte_len];
    LayerPool::submit_external_rgba_in_world(&mut world, &handler, &target, &bytes)
        .expect("external upload should stage");
    handler.with_staged_external_uploads(|uploads| {
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].lease, target.lease);
    });
}

#[test]
fn g11_commit_error_preserves_dirt_and_retry_paints() {
    // Pin C3's error contract: a failed paint commit must NOT clear paint dirt.
    // A world without `UiDomRenderSupport` reaches the paint stage and fails the
    // direct-GPU executor check (`RetainedGpuApplyFailed`). The empty document's
    // plan is empty and never reaches the executor check, so attach succeeds;
    // the later div makes the document layer paint-dirty.
    let mut world = World::new();
    world.init_resource::<Assets<Image>>();
    world.insert_resource(LayerPool::new(LayerConfig::default()));
    let parent = spawn_sprite_parent(&mut world, 320.0, 320.0);
    let mut root = RootDom::new(RootDomContext::new_gpu());
    root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
        .expect("empty document should attach (empty plan never reaches the executor check)");

    let document = root.document_node().0;
    append_sized_div(&mut root, document, "100px", "40px");

    let err = root
        .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect_err("paint commit should fail without a direct-GPU executor");
    assert_eq!(err, DomError::RetainedGpuApplyFailed);
    root.context().render_ops().with_staged_applies(|applies| {
        assert!(applies.is_empty(), "a failed commit stages nothing");
    });

    world.insert_resource(UiDomRenderSupport::direct_gpu());
    root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
        .expect("retry should succeed once the executor is installed");

    // The retry painting at all proves the failed commit left the layer's paint
    // dirt intact (C3): a cleared layer would stage nothing on the second update.
    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        let plan = &applies[0].plan;
        assert_eq!(plan.rects.len(), 1);
        assert_vec2_close(plan.rects[0].size_px, [100.0, 40.0]);
    });
}

#[test]
fn g12_number_input_icons_stage_with_element_color() {
    // Cover the icon op family at the staging seam (G1–G9 cover rects/text only):
    // an `<input type="number">` emits exactly two stepper `IconPaintOp`s, both
    // tinted with the element's effective `color`.
    let (mut world, mut root) = support::attached_gpu_root_dom_setup(320.0, 320.0);
    let document = root.document_node().0;
    let input = support::assert_append_input_to(&mut root, document);
    root.set_attr_str(input, "type", "number")
        .expect("type should parse");
    root.style_mut(input)
        .set("width", "120px")
        .expect("width should parse");
    root.style_mut(input)
        .set("height", "24px")
        .expect("height should parse");
    root.set_attr_str(input, "value", "4")
        .expect("value should parse");
    root.set_attr_str(input, "class", "accent")
        .expect("class should parse");
    support::assert_load_root_stylesheet(
        &root,
        support::theme_stylesheet_origin(),
        ".accent { color: #3366ff; }",
    );

    root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
        .expect("update");

    root.context().render_ops().with_staged_applies(|applies| {
        assert_eq!(applies.len(), 1);
        let plan = &applies[0].plan;
        assert_eq!(plan.icons.len(), 2);
        for icon in &plan.icons {
            assert!(
                icon.size_px[0] > 0.0,
                "stepper icon width should be positive"
            );
            assert!(
                icon.size_px[1] > 0.0,
                "stepper icon height should be positive"
            );
            assert_vec4_close(icon.color_rgba, [51.0 / 255.0, 102.0 / 255.0, 1.0, 1.0]);
        }
    });
}
