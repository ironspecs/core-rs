//! Shared scenario builders and assertion helpers for the layout proofs.
//!
//! This file owns the hermetic GPU fixture (a `RootDom` attached under a
//! Bevy `World` with direct-GPU support resources), the `assert_append_*`
//! node builders, the scenario `*_setup` functions with their documented
//! return-tuple orders, stylesheet loading helpers, and the two assertion
//! surfaces: retained layout (`assert_node_layout`, `assert_close`) and
//! staged paint plans (`take_staged_layer_paint_plans`,
//! `assert_rect_op_geometry`).
//!
//! Invariants and rules:
//!
//! - proofs build scenarios only through these helpers; new scenario shapes
//!   grow a named `*_setup` here rather than inline world setup in a proof
//! - `*_setup` functions document their return tuples as
//!   `(world, root, ...keys)`; destructured names in proofs must match that
//!   documented order
//! - staged plans persist until drained: a proof drains exactly once per
//!   asserted update via `take_staged_layer_paint_plans`; the document root
//!   layer is entry zero, and layer-local geometry equals screen geometry in
//!   single-layer proofs
//! - layout and geometry float assertions go through `assert_close` (1e-3
//!   tolerance). Typed computed-value assertions (computed group fields,
//!   intrinsic metrics, counters) compare exactly under a per-file
//!   `#![allow(clippy::float_cmp)]` with a stated rationale — the values are
//!   deterministic typed projections, so exactness is the honest check
//! - this file is test-only support: it owns no product behavior and asserts
//!   nothing itself beyond the `assert_*` contract

use bevy::asset::Assets;
use bevy::image::Image;
use bevy::prelude::{Vec2, World};
use bevy::sprite::Sprite;
use ui_dom::{
    DomLayoutState, DomNode, DomNodeSlotKey, DomPointerUpdate, DomTag, LayerConfig, LayerPaintPlan,
    LayerPool, RectPaintOp, RootDom, RootDomContext, StyleRuleOrigin, UiDomRenderSupport,
};

pub fn attached_gpu_root_dom_default_viewport_setup() -> (World, RootDom) {
    attached_gpu_root_dom_setup(320.0, 320.0)
}

pub fn attached_gpu_root_dom_setup(width_px: f32, height_px: f32) -> (World, RootDom) {
    attached_gpu_root_dom_setup_with_context(RootDomContext::new_gpu(), width_px, height_px)
}

/// Same attached GPU setup as [`attached_gpu_root_dom_setup`], but the root
/// is constructed from the caller-provided context so stylesheet state can be
/// prepared before `RootDom::new`.
pub fn attached_gpu_root_dom_setup_with_context(
    context: std::sync::Arc<RootDomContext>,
    width_px: f32,
    height_px: f32,
) -> (World, RootDom) {
    let mut world = World::new();
    install_direct_gpu_support(&mut world);

    let parent = world
        .spawn((Sprite {
            custom_size: Some(Vec2::new(width_px, height_px)),
            ..Default::default()
        },))
        .id();

    let mut root = RootDom::new(context);
    root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
        .expect("layout proof root should attach");

    (world, root)
}

pub fn assert_append_div_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root.create_element(DomTag::Div).expect("div should insert");
    root.append_child(parent_key, key)
        .expect("div should append");
    key
}

pub fn assert_append_span_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root
        .create_element(DomTag::Span)
        .expect("span should insert");
    root.append_child(parent_key, key)
        .expect("span should append");
    key
}

pub fn assert_append_button_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root
        .create_element(DomTag::Button)
        .expect("button should insert");
    root.append_child(parent_key, key)
        .expect("button should append");
    key
}

pub fn assert_append_img_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root.create_element(DomTag::Img).expect("img should insert");
    root.append_child(parent_key, key)
        .expect("img should append");
    key
}

pub fn assert_append_input_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root
        .create_element(DomTag::Input)
        .expect("input should insert");
    root.append_child(parent_key, key)
        .expect("input should append");
    key
}

pub fn assert_append_textarea_to(root: &mut RootDom, parent_key: DomNodeSlotKey) -> DomNodeSlotKey {
    let key = root
        .create_element(DomTag::Textarea)
        .expect("textarea should insert");
    root.append_child(parent_key, key)
        .expect("textarea should append");
    key
}

pub fn assert_append_text_to(
    root: &mut RootDom,
    parent_key: DomNodeSlotKey,
    text: &str,
) -> DomNodeSlotKey {
    let key = root.create_text_node(text).expect("text should insert");
    root.append_child(parent_key, key)
        .expect("text should append");
    key
}

/// Returns `(world, root, container, parent, child)`.
pub fn nested_parent_child_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let parent = assert_append_div_to(&mut root, container);
    let child = assert_append_div_to(&mut root, parent);
    (world, root, container, parent, child)
}

/// Returns `(world, root, container, first_parent, second_parent)`.
pub fn container_with_two_parents_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let first_parent = assert_append_div_to(&mut root, container);
    let second_parent = assert_append_div_to(&mut root, container);
    (world, root, container, first_parent, second_parent)
}

/// Returns `(world, root, parent, first_child, second_child)`.
pub fn parent_with_two_children_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let first_child = assert_append_div_to(&mut root, parent);
    let second_child = assert_append_div_to(&mut root, parent);
    (world, root, parent, first_child, second_child)
}

/// Returns `(world, root, img)`.
pub fn img_under_document_setup() -> (World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let img = assert_append_img_to(&mut root, document_root);
    (world, root, img)
}

/// Returns `(world, root, fixed_layer, block, text)`.
pub fn fixed_layer_with_block_and_text_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_layer = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, fixed_layer);
    let text = assert_append_text_to(&mut root, block, "Fixed layer label");
    (world, root, fixed_layer, block, text)
}

/// Returns `(world, root, fixed_layer, first_block, second_block)`.
pub fn fixed_layer_with_two_blocks_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_layer = assert_append_div_to(&mut root, document_root);
    let first_block = assert_append_div_to(&mut root, fixed_layer);
    let second_block = assert_append_div_to(&mut root, fixed_layer);
    (world, root, fixed_layer, first_block, second_block)
}

/// Returns `(world, root, fixed_layer, scroll_box, first_block, second_block)`.
pub fn fixed_layer_with_absolute_scroll_box_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_layer = assert_append_div_to(&mut root, document_root);
    let scroll_box = assert_append_div_to(&mut root, fixed_layer);
    let first_block = assert_append_div_to(&mut root, scroll_box);
    let second_block = assert_append_div_to(&mut root, scroll_box);
    (
        world,
        root,
        fixed_layer,
        scroll_box,
        first_block,
        second_block,
    )
}

/// Returns `(world, root, fixed_panel, scroll_box)`.
pub fn fixed_panel_with_nested_scroll_box_setup() -> (World, RootDom, DomNodeSlotKey, DomNodeSlotKey)
{
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_panel = assert_append_div_to(&mut root, document_root);
    let scroll_box = assert_append_div_to(&mut root, fixed_panel);
    (world, root, fixed_panel, scroll_box)
}

/// Returns `(world, root, fixed_panel, scroll_box)`.
pub fn document_with_panel_and_scroll_sibling_setup()
-> (World, RootDom, DomNodeSlotKey, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_panel = assert_append_div_to(&mut root, document_root);
    let scroll_box = assert_append_div_to(&mut root, document_root);
    (world, root, fixed_panel, scroll_box)
}

/// Returns `(world, root, popup, items)`.
pub fn fixed_popup_menu_with_items_setup(
    item_count: usize,
) -> (World, RootDom, DomNodeSlotKey, Vec<DomNodeSlotKey>) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let popup = assert_append_div_to(&mut root, document_root);
    let items = assert_append_div_children_to(&mut root, popup, item_count);
    (world, root, popup, items)
}

/// Returns `(world, root, fixed_layer, popup, items)`.
pub fn fixed_layer_with_absolute_popup_menu_setup(
    item_count: usize,
) -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    Vec<DomNodeSlotKey>,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let fixed_layer = assert_append_div_to(&mut root, document_root);
    let popup = assert_append_div_to(&mut root, fixed_layer);
    let items = assert_append_div_children_to(&mut root, popup, item_count);
    (world, root, fixed_layer, popup, items)
}

/// Returns `(world, root, popup, content, first_row, first_text, second_row, second_text)`.
pub fn fixed_popup_with_text_rows_setup() -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let popup = assert_append_div_to(&mut root, document_root);
    let content = assert_append_div_to(&mut root, popup);
    let first_row = assert_append_button_to(&mut root, content);
    let first_text = assert_append_text_to(&mut root, first_row, "Scene reload");
    let second_row = assert_append_button_to(&mut root, content);
    let second_text = assert_append_text_to(&mut root, second_row, "Other");
    (
        world,
        root,
        popup,
        content,
        first_row,
        first_text,
        second_row,
        second_text,
    )
}

pub fn assert_set_attr_str(root: &mut RootDom, key: DomNodeSlotKey, name: &str, value: &str) {
    root.set_attr_str(key, name, value)
        .expect("attribute should parse and apply");
}

/// Migrated presentation-write helper (spine 3.3g-2a): presentation properties
/// are written through `style_mut`, never `set_attr_str`.
pub fn assert_style_set(root: &mut RootDom, key: DomNodeSlotKey, property: &str, value: &str) {
    root.style_mut(key)
        .set(property, value)
        .expect("style declaration should parse and apply");
}

pub fn assert_load_root_stylesheet(root: &RootDom, origin: StyleRuleOrigin, css_text: &str) {
    root.context()
        .load_stylesheet(origin, css_text)
        .expect("stylesheet should load");
}

pub fn theme_stylesheet_origin() -> StyleRuleOrigin {
    StyleRuleOrigin::new("theme", 0)
}

/// Loads a stylesheet on a FRESH root and asserts the per-load report counts
/// AND the cumulative drop counter (fresh root ⇒ cumulative == per-load).
///
/// The `css` argument is a complete stylesheet (selector + declaration block),
/// e.g. `.x { padding-left: -10px }`. The fresh root has no authored elements,
/// so nothing matches — but `declarations_applied`/`declarations_dropped` are
/// parse-time counts, not match counts, so the report is still the whole story.
pub fn assert_decl_drop(css: &str, applied: usize, dropped: usize) {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load");
    assert_eq!(
        report.declarations_applied, applied,
        "declarations_applied for {css:?}",
    );
    assert_eq!(
        report.declarations_dropped, dropped,
        "declarations_dropped for {css:?}",
    );
    assert_eq!(
        root.context().declaration_drop_count(),
        dropped as u64,
        "cumulative drop counter for {css:?}",
    );
}

pub fn assert_root_dom_update(
    world: &mut World,
    root: &mut RootDom,
    now_s: f64,
    update: DomPointerUpdate,
) {
    root.update(world, now_s, update)
        .expect("layout proof update should succeed");
}

pub fn assert_node_layout(root: &RootDom, key: DomNodeSlotKey) -> DomLayoutState {
    let node = root.node(key).expect("layout proof node should exist");
    clone_node_layout(node)
}

pub fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "{label} should be {expected}, got {actual}",
    );
}

/// Convenience for the `BoxLength` API break (PR 1 item 7): a resolved-px box
/// length, so box-group assertions read `box_px(20.0)` instead of a thousand
/// `BoxLength::Px(20.0)` literals.
pub fn box_px(pixels: f32) -> ui_dom::BoxLength {
    ui_dom::BoxLength::Px(pixels)
}

/// Drains the staged render ops produced by the most recent `RootDom::update`
/// and returns each staged layer's paint plan, in stage order (stage order =
/// paint order = DOM layer order, root layer first).
///
/// In a proof with no render app installed, staged ops persist after the update
/// until drained: drain exactly once per update phase you assert on. The
/// document root layer is the first entry; its content sits at layer zero, so
/// layer-local rect geometry equals screen geometry in single-layer proofs.
pub fn take_staged_layer_paint_plans(root: &RootDom) -> Vec<LayerPaintPlan> {
    root.context()
        .render_ops()
        .take_staged()
        .applies
        .into_iter()
        .map(|apply| apply.plan)
        .collect()
}

/// Asserts one rect op's layer-local geometry via the file's `assert_close`
/// tolerance. `left_px`/`top_px` are the layer-local top-left (screen position
/// for document-root-layer content); `width_px`/`height_px` are the border-box
/// extent. The paint stage emits `size_px` as the border box clamped to
/// non-negative (`paintable_node_size`), and emits no rect at all for a
/// zero-size element.
pub fn assert_rect_op_geometry(
    rect: &RectPaintOp,
    left_px: f32,
    top_px: f32,
    width_px: f32,
    height_px: f32,
    label: &str,
) {
    assert_close(rect.position_px[0], left_px, &format!("{label} left"));
    assert_close(rect.position_px[1], top_px, &format!("{label} top"));
    assert_close(rect.size_px[0], width_px, &format!("{label} width"));
    assert_close(rect.size_px[1], height_px, &format!("{label} height"));
}

/// Asserts one rect op's fill color via the file's `assert_close` tolerance.
/// `expected_rgba` is the normalized `[red, green, blue, alpha]` channel set;
/// the per-channel labels are derived from `label`.
pub fn assert_rect_op_fill(rect: &RectPaintOp, expected_rgba: [f32; 4], label: &str) {
    assert_close(
        rect.fill_rgba[0],
        expected_rgba[0],
        &format!("{label} fill red"),
    );
    assert_close(
        rect.fill_rgba[1],
        expected_rgba[1],
        &format!("{label} fill green"),
    );
    assert_close(
        rect.fill_rgba[2],
        expected_rgba[2],
        &format!("{label} fill blue"),
    );
    assert_close(
        rect.fill_rgba[3],
        expected_rgba[3],
        &format!("{label} fill alpha"),
    );
}

fn install_direct_gpu_support(world: &mut World) {
    world.init_resource::<Assets<Image>>();
    world.insert_resource(LayerPool::new(LayerConfig::default()));
    world.insert_resource(UiDomRenderSupport::direct_gpu());
}

fn assert_append_div_children_to(
    root: &mut RootDom,
    parent_key: DomNodeSlotKey,
    child_count: usize,
) -> Vec<DomNodeSlotKey> {
    let mut children = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        children.push(assert_append_div_to(root, parent_key));
    }
    children
}

fn clone_node_layout(node: &DomNode) -> DomLayoutState {
    match node {
        DomNode::Document(node) => node.base.layout.clone(),
        DomNode::DocumentFragment(node) => node.base.layout.clone(),
        DomNode::Element(node) => node.base.layout.clone(),
        DomNode::Text(node) => node.base.layout.clone(),
    }
}
