use bevy::asset::Assets;
use bevy::image::Image;
use bevy::prelude::{Vec2, World};
use bevy::sprite::Sprite;
use ui_dom::{
    DomLayoutState, DomNode, DomNodeSlotKey, DomPointerUpdate, DomRect, DomTag, LayerConfig,
    LayerPool, RootDom, RootDomContext, StyleRuleOrigin, UiDomRenderSupport, node_box,
};

pub fn attached_gpu_root_dom_default_viewport_setup() -> (World, RootDom) {
    attached_gpu_root_dom_setup(320.0, 320.0)
}

pub fn attached_gpu_root_dom_setup(width_px: f32, height_px: f32) -> (World, RootDom) {
    let mut world = World::new();
    install_direct_gpu_support(&mut world);

    let parent = world
        .spawn((Sprite {
            custom_size: Some(Vec2::new(width_px, height_px)),
            ..Default::default()
        },))
        .id();

    let mut root = RootDom::new(RootDomContext::new_gpu());
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

pub fn assert_load_root_stylesheet(root: &RootDom, origin: StyleRuleOrigin, css_text: &str) {
    root.context()
        .load_stylesheet(origin, css_text)
        .expect("stylesheet should load");
}

pub fn theme_stylesheet_origin() -> StyleRuleOrigin {
    StyleRuleOrigin::new("theme", 0)
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

pub fn assert_node_border_box_screen_left_px(root: &RootDom, key: DomNodeSlotKey) -> f32 {
    accumulated_border_box_edge_px(root, key, border_box_left_px)
}

pub fn assert_node_border_box_screen_top_px(root: &RootDom, key: DomNodeSlotKey) -> f32 {
    accumulated_border_box_edge_px(root, key, border_box_top_px)
}

fn accumulated_border_box_edge_px(
    root: &RootDom,
    key: DomNodeSlotKey,
    edge_selector: fn(&DomRect) -> f32,
) -> f32 {
    let mut current_key = Some(key);
    let mut edge_px = 0.0;
    let mut saw_element = false;

    while let Some(key) = current_key {
        let node = root.node(key).expect("layout proof node should exist");
        edge_px += edge_selector(
            &node_box(root, key)
                .expect("layout proof node should expose a box model")
                .border_box,
        );
        current_key = node.parent_key();
        saw_element = true;
    }

    assert!(
        saw_element,
        "layout proof target should resolve to a real node"
    );
    edge_px
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

fn border_box_left_px(border_box: &DomRect) -> f32 {
    border_box.left_px
}

fn border_box_top_px(border_box: &DomRect) -> f32 {
    border_box.top_px
}
