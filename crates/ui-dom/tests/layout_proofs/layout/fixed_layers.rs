#![allow(rustdoc::bare_urls)]

//! Fixed-position layer layout proofs.
//!
//! Spec references:
//! - CSS 2.1 10.3.7 absolutely positioned, non-replaced elements:
//!   <https://www.w3.org/TR/CSS21/visudet.html#abs-non-replaced-width>
//! - CSS 2.1 10.6.4 absolutely positioned, non-replaced elements:
//!   <https://www.w3.org/TR/CSS21/visudet.html#abs-non-replaced-height>
//! - CSS 2.1 9.3.1 positioning schemes:
//!   <https://www.w3.org/TR/CSS21/visuren.html#positioning-scheme>

use crate::layout_proofs::support::{
    assert_append_div_to, assert_close, assert_load_root_stylesheet, assert_node_layout,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, attached_gpu_root_dom_setup,
    fixed_layer_with_absolute_scroll_box_setup, fixed_layer_with_block_and_text_setup,
    fixed_layer_with_two_blocks_setup, theme_stylesheet_origin,
};
use ui_dom::{DomIntrinsic, DomNode, DomPointerUpdate, node_box};

#[test]
// CSS 2.1 10.3.7: fixed left+right anchoring resolves the layer width against
// the viewport-sized containing block.
fn fixed_left_right_layer_and_block_fill_viewport_width() {
    let (mut world, mut root, fixed_layer, block, _text) = fixed_layer_with_block_and_text_setup();

    assert_style_set(&mut root, fixed_layer, "position", "fixed");
    assert_style_set(&mut root, fixed_layer, "top", "0px");
    assert_style_set(&mut root, fixed_layer, "left", "0px");
    assert_style_set(&mut root, fixed_layer, "right", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let fixed_layer_layout = assert_node_layout(&root, fixed_layer);
    let block_layout = assert_node_layout(&root, block);

    assert_close(fixed_layer_layout.width_px, 320.0, "fixed layer width");
    assert_close(
        block_layout.width_px,
        320.0,
        "block width inside fixed layer",
    );
}

#[test]
// CSS 2.1 10.3.3 plus the shared text layout contract used elsewhere in
// `ui-dom`: a block inside a fixed layer still gives its text child the
// constrained block width, while the text node keeps its intrinsic metrics for
// shaping and line-height.
fn text_inside_block_in_fixed_layer_inherits_constrained_block_width() {
    let (mut world, mut root, fixed_layer, block, text) = fixed_layer_with_block_and_text_setup();

    assert_style_set(&mut root, fixed_layer, "position", "fixed");
    assert_style_set(&mut root, fixed_layer, "top", "0px");
    assert_style_set(&mut root, fixed_layer, "left", "0px");
    assert_style_set(&mut root, fixed_layer, "right", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let block_layout = assert_node_layout(&root, block);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic_width_px = match root.node(text).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.max_content_width_px,
        _ => panic!("text proof node should keep text intrinsic state"),
    };

    assert!(
        text_intrinsic_width_px > 0.0,
        "text intrinsic width should be positive"
    );
    assert_close(
        text_layout.width_px,
        block_layout.width_px,
        "text layout width should inherit the constrained block width",
    );
}

#[test]
// CSS 2.1 10.6.4: fixed top+bottom anchoring resolves the layer height against
// the viewport-sized containing block.
fn fixed_top_bottom_layer_keeps_viewport_height_with_two_block_children() {
    let (mut world, mut root, fixed_layer, first_block, second_block) =
        fixed_layer_with_two_blocks_setup();

    assert_style_set(&mut root, fixed_layer, "position", "fixed");
    assert_style_set(&mut root, fixed_layer, "top", "0px");
    assert_style_set(&mut root, fixed_layer, "right", "0px");
    assert_style_set(&mut root, fixed_layer, "bottom", "0px");
    assert_style_set(&mut root, first_block, "height", "30px");
    assert_style_set(&mut root, second_block, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let fixed_layer_layout = assert_node_layout(&root, fixed_layer);

    assert_close(fixed_layer_layout.height_px, 320.0, "fixed layer height");
    assert_close(
        fixed_layer_layout.content_height_px,
        320.0,
        "fixed layer retained content height",
    );
}

#[test]
// CSS 2.1 10.6.4 plus 9.3.1: an absolutely positioned auto-height scroll box
// with no top/bottom anchors should size to its in-flow children, not fill
// the fixed parent's containing block height.
fn absolute_scroll_box_sizes_to_children_inside_fixed_top_bottom_parent() {
    let (mut world, mut root, fixed_layer, scroll_box, first_block, second_block) =
        fixed_layer_with_absolute_scroll_box_setup();

    assert_style_set(&mut root, fixed_layer, "position", "fixed");
    assert_style_set(&mut root, fixed_layer, "top", "0px");
    assert_style_set(&mut root, fixed_layer, "right", "0px");
    assert_style_set(&mut root, fixed_layer, "bottom", "0px");
    assert_style_set(&mut root, scroll_box, "position", "absolute");
    assert_style_set(&mut root, scroll_box, "overflow", "scroll");
    assert_style_set(&mut root, first_block, "height", "30px");
    assert_style_set(&mut root, second_block, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let fixed_layer_layout = assert_node_layout(&root, fixed_layer);
    let scroll_box_layout = assert_node_layout(&root, scroll_box);

    assert_close(fixed_layer_layout.height_px, 320.0, "fixed layer height");
    assert_close(
        fixed_layer_layout.content_height_px,
        320.0,
        "fixed layer retained content height",
    );
    assert_close(
        scroll_box_layout.height_px,
        70.0,
        "absolute scroll box height",
    );
    assert_close(
        scroll_box_layout.content_height_px,
        70.0,
        "absolute scroll box content height",
    );
}

#[test]
// CSS 2.1 10.3.7: a nested fixed-position block with `right` anchored and
// auto `left`/`width` uses shrink-to-fit width, then pins that width against
// the viewport instead of expanding to the full available width and sliding
// off the left side.
fn nested_fixed_right_anchored_auto_width_shrink_wraps_block_children() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let fixed_panel = assert_append_div_to(&mut root, wrapper);
    let first_block = assert_append_div_to(&mut root, fixed_panel);
    let second_block = assert_append_div_to(&mut root, fixed_panel);

    assert_style_set(&mut root, wrapper, "width", "140px");
    assert_style_set(&mut root, wrapper, "margin-left", "80px");
    assert_style_set(&mut root, fixed_panel, "position", "fixed");
    assert_style_set(&mut root, fixed_panel, "top", "20px");
    assert_style_set(&mut root, fixed_panel, "right", "20px");
    assert_style_set(&mut root, first_block, "width", "80px");
    assert_style_set(&mut root, first_block, "height", "24px");
    assert_style_set(&mut root, second_block, "width", "100px");
    assert_style_set(&mut root, second_block, "height", "32px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let panel_snapshot = root
        .canonical_snapshot(fixed_panel)
        .expect("fixed panel should remain readable after layout");
    let first_block_box = node_box(&root, first_block).expect("first block should expose a box");
    let second_block_box = node_box(&root, second_block).expect("second block should expose a box");

    assert_close(
        panel_snapshot.layout.used_width_px,
        100.0,
        "fixed panel shrink-to-fit width",
    );
    assert_close(
        panel_snapshot.layout.used_height_px,
        56.0,
        "fixed panel stacked block height",
    );
    assert_close(
        panel_snapshot.layout.available_width_px,
        320.0,
        "fixed panel viewport available width",
    );
    assert_close(
        first_block_box.border_box.left_px,
        0.0,
        "first block local left inside right-anchored fixed panel",
    );
    assert_close(
        second_block_box.border_box.left_px,
        0.0,
        "second block local left inside right-anchored fixed panel",
    );
}

#[test]
// CSS 2.1 10.6.4: a nested fixed-position block with `bottom` anchored and
// auto `top`/`height` uses its in-flow content height, then pins that height
// against the viewport instead of stretching to the full available height or
// drifting upward from the bottom edge.
fn nested_fixed_bottom_anchored_auto_height_shrink_wraps_block_children() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let fixed_panel = assert_append_div_to(&mut root, wrapper);
    let first_block = assert_append_div_to(&mut root, fixed_panel);
    let second_block = assert_append_div_to(&mut root, fixed_panel);

    assert_style_set(&mut root, wrapper, "height", "140px");
    assert_style_set(&mut root, wrapper, "margin-top", "80px");
    assert_style_set(&mut root, fixed_panel, "position", "fixed");
    assert_style_set(&mut root, fixed_panel, "left", "20px");
    assert_style_set(&mut root, fixed_panel, "bottom", "20px");
    assert_style_set(&mut root, first_block, "width", "80px");
    assert_style_set(&mut root, first_block, "height", "24px");
    assert_style_set(&mut root, second_block, "width", "100px");
    assert_style_set(&mut root, second_block, "height", "32px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let panel_snapshot = root
        .canonical_snapshot(fixed_panel)
        .expect("fixed panel should remain readable after layout");
    let panel_box = node_box(&root, fixed_panel).expect("fixed panel should expose a box");
    let first_block_box = node_box(&root, first_block).expect("first block should expose a box");
    let second_block_box = node_box(&root, second_block).expect("second block should expose a box");

    assert_close(
        panel_snapshot.layout.used_width_px,
        100.0,
        "fixed panel shrink-to-fit width from block children",
    );
    assert_close(
        panel_snapshot.layout.used_height_px,
        56.0,
        "fixed panel content-based auto height",
    );
    assert_close(
        panel_snapshot.layout.available_height_px,
        320.0,
        "fixed panel viewport available height",
    );
    assert_close(
        panel_box.border_box.top_px,
        244.0,
        "fixed panel bottom anchoring uses viewport height minus auto height",
    );
    assert_close(
        first_block_box.border_box.top_px,
        0.0,
        "first block local top inside bottom-anchored fixed panel",
    );
    assert_close(
        second_block_box.border_box.top_px,
        24.0,
        "second block local top inside bottom-anchored fixed panel",
    );
}

#[test]
// CSS 2.1 10.3.7: a nested fixed-position box with explicit width and only a
// `right` anchor must pin its border box against the viewport right edge
// (left = containing-block width - right offset - width), matching WPT
// `css/css-position/position-fixed-at-bottom-right-on-viewport.html`.
fn nested_fixed_right_anchored_explicit_width_stays_on_viewport_right_edge() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let fixed_panel = assert_append_div_to(&mut root, wrapper);
    let slot = assert_append_div_to(&mut root, fixed_panel);

    assert_style_set(&mut root, wrapper, "width", "140px");
    assert_style_set(&mut root, wrapper, "margin-left", "80px");
    assert_style_set(&mut root, fixed_panel, "position", "fixed");
    assert_style_set(&mut root, fixed_panel, "top", "25px");
    assert_style_set(&mut root, fixed_panel, "right", "25px");
    assert_style_set(&mut root, fixed_panel, "width", "196px");
    assert_style_set(&mut root, fixed_panel, "height", "82px");
    assert_style_set(&mut root, slot, "width", "27.5px");
    assert_style_set(&mut root, slot, "height", "27.5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let panel_box = node_box(&root, fixed_panel).expect("fixed panel should expose a box");
    let panel_snapshot = root
        .canonical_snapshot(fixed_panel)
        .expect("fixed panel should remain readable after layout");

    assert_close(
        panel_snapshot.layout.used_width_px,
        196.0,
        "fixed panel should keep its explicit width",
    );
    assert_close(
        panel_box.border_box.left_px,
        99.0,
        "fixed panel local left should pin 196px width 25px from the 320px viewport right edge",
    );
    assert_close(
        panel_box.border_box.top_px,
        25.0,
        "fixed panel top anchor should stay unchanged",
    );
}

#[test]
// CSS 2.1 10.3.7 plus the retained stylesheet path used by the in-game UI:
// a class-driven fixed toolbar with absolute children must still pin its
// explicit width against the viewport right edge.
fn stylesheet_driven_fixed_right_toolbar_with_absolute_slots_stays_on_viewport_right_edge() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .ingame-toolbar {
                display: block;
                position: fixed;
                top: 25px;
                right: 25px;
                width: 196px;
                height: 82px;
            }
            .ingame-toolbar-slot {
                display: block;
                position: absolute;
                width: 27.5px;
                height: 27.5px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let toolbar = assert_append_div_to(&mut root, wrapper);
    let first_slot = assert_append_div_to(&mut root, toolbar);
    let second_slot = assert_append_div_to(&mut root, toolbar);

    assert_style_set(&mut root, wrapper, "width", "140px");
    assert_style_set(&mut root, wrapper, "margin-left", "80px");
    assert_set_attr_str(&mut root, toolbar, "class", "ingame-toolbar");
    assert_set_attr_str(&mut root, first_slot, "class", "ingame-toolbar-slot");
    assert_style_set(&mut root, first_slot, "left", "6px");
    assert_style_set(&mut root, first_slot, "top", "6px");
    assert_set_attr_str(&mut root, second_slot, "class", "ingame-toolbar-slot");
    assert_style_set(&mut root, second_slot, "left", "39.5px");
    assert_style_set(&mut root, second_slot, "top", "6px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let toolbar_box = node_box(&root, toolbar).expect("toolbar should expose a box");
    let first_slot_box = node_box(&root, first_slot).expect("first slot should expose a box");
    let second_slot_box = node_box(&root, second_slot).expect("second slot should expose a box");

    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "stylesheet-driven fixed toolbar should pin to the viewport right edge",
    );
    assert_close(
        first_slot_box.border_box.left_px,
        6.0,
        "first absolute slot should stay local to the toolbar content box",
    );
    assert_close(
        second_slot_box.border_box.left_px,
        39.5,
        "second absolute slot should stay local to the toolbar content box",
    );
}

#[test]
// CSS 2.1 10.3.7 plus 10.6.4: a right-anchored fixed box with explicit width,
// auto height, a relative slot row, and a padded content block must still pin
// its border box against the viewport right edge, matching WPT
// `css/css-position/position-fixed-at-bottom-right-on-viewport.html`.
fn stylesheet_driven_fixed_right_toolbar_with_auto_height_stays_on_viewport_right_edge() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .ingame-debug-toolbar {
                display: block;
                position: fixed;
                top: 132px;
                right: 25px;
                width: 196px;
            }
            .ingame-debug-toolbar-slot-row {
                display: block;
                position: relative;
                width: 196px;
                height: 44px;
            }
            .ingame-debug-toolbar-content {
                display: block;
                padding-left: 6px;
                padding-right: 6px;
                padding-bottom: 6px;
            }
            .ingame-debug-toolbar-line {
                display: block;
                height: 16px;
            }
            .ingame-toolbar-slot {
                display: block;
                position: absolute;
                width: 27.5px;
                height: 27.5px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let toolbar = assert_append_div_to(&mut root, wrapper);
    let slot_row = assert_append_div_to(&mut root, toolbar);
    let slot = assert_append_div_to(&mut root, slot_row);
    let content = assert_append_div_to(&mut root, toolbar);
    let line = assert_append_div_to(&mut root, content);

    assert_style_set(&mut root, wrapper, "width", "140px");
    assert_style_set(&mut root, wrapper, "margin-left", "80px");
    assert_set_attr_str(&mut root, toolbar, "class", "ingame-debug-toolbar");
    assert_set_attr_str(
        &mut root,
        slot_row,
        "class",
        "ingame-debug-toolbar-slot-row",
    );
    assert_set_attr_str(&mut root, slot, "class", "ingame-toolbar-slot");
    assert_style_set(&mut root, slot, "left", "6px");
    assert_style_set(&mut root, slot, "top", "6px");
    assert_set_attr_str(&mut root, content, "class", "ingame-debug-toolbar-content");
    assert_set_attr_str(&mut root, line, "class", "ingame-debug-toolbar-line");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let toolbar_box = node_box(&root, toolbar).expect("toolbar should expose a box");
    let slot_row_box = node_box(&root, slot_row).expect("slot row should expose a box");
    let content_box = node_box(&root, content).expect("content block should expose a box");

    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "auto-height debug toolbar should still pin to the viewport right edge",
    );
    assert_close(
        slot_row_box.border_box.left_px,
        0.0,
        "relative slot row should stay local to the fixed toolbar",
    );
    assert_close(
        content_box.border_box.left_px,
        0.0,
        "content block should stay local to the fixed toolbar",
    );
}

#[test]
// CSS 2.1 10.3.7 plus CSS Cascade 6 rule merging: class composition (one rule
// supplying `position: fixed`, another supplying `top`/`right`/`width`) must
// produce the same computed style as one monolithic rule, so the merged
// right-anchored fixed box still pins against the viewport right edge per
// 10.3.7.
fn multi_class_fixed_right_toolbar_style_merge_stays_on_viewport_right_edge() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .ingame-toolbar {
                display: block;
                position: fixed;
            }
            .ingame-debug-toolbar {
                top: 132px;
                right: 25px;
                width: 196px;
            }
            .ingame-debug-toolbar-slot-row {
                display: block;
                position: relative;
                width: 196px;
                height: 44px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let toolbar = assert_append_div_to(&mut root, wrapper);
    let slot_row = assert_append_div_to(&mut root, toolbar);

    assert_style_set(&mut root, wrapper, "width", "140px");
    assert_style_set(&mut root, wrapper, "margin-left", "80px");
    assert_set_attr_str(
        &mut root,
        toolbar,
        "class",
        "ingame-toolbar ingame-debug-toolbar",
    );
    assert_set_attr_str(
        &mut root,
        slot_row,
        "class",
        "ingame-debug-toolbar-slot-row",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let toolbar_box = node_box(&root, toolbar).expect("toolbar should expose a box");
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "multi-class fixed toolbar should pin to the viewport right edge",
    );
    assert_close(
        toolbar_box.border_box.top_px,
        132.0,
        "multi-class fixed toolbar should keep the merged top anchor",
    );
    assert_close(
        toolbar_box.border_box.width_px,
        196.0,
        "multi-class fixed toolbar should keep the merged explicit width",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: a right-anchored fixed box must keep its
// viewport-resolved position across retained relayout passes, since its
// containing block is the viewport and not any flow ancestor; a second update
// must not drift the box left by its `right` offset.
fn multi_class_fixed_right_toolbar_stays_on_viewport_right_edge_across_second_update() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .ingame-toolbar {
                display: block;
                position: fixed;
            }
            .ingame-debug-toolbar {
                top: 132px;
                right: 25px;
                width: 196px;
            }
            .ingame-debug-toolbar-slot-row {
                display: block;
                position: relative;
                width: 196px;
                height: 44px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let toolbar = assert_append_div_to(&mut root, document_root);
    let slot_row = assert_append_div_to(&mut root, toolbar);

    assert_set_attr_str(
        &mut root,
        toolbar,
        "class",
        "ingame-toolbar ingame-debug-toolbar",
    );
    assert_set_attr_str(
        &mut root,
        slot_row,
        "class",
        "ingame-debug-toolbar-slot-row",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_box = node_box(&root, toolbar).expect("toolbar should expose a box");
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "second retained update should keep the fixed toolbar on the viewport right edge",
    );
}

struct DebugToolbarUpdateScenario {
    debug_toolbar: ui_dom::DomNodeSlotKey,
    line_text_keys: Vec<ui_dom::DomNodeSlotKey>,
}

fn load_ingame_debug_toolbar_styles(root: &ui_dom::RootDom) {
    assert_load_root_stylesheet(
        root,
        theme_stylesheet_origin(),
        r"
            .ingame-toolbar {
                display: block;
                position: fixed;
                background-color: #11182099;
                border-radius: 12px;
            }
            .ingame-view-toolbar {
                top: 25px;
                right: 25px;
                width: 196px;
                height: 82px;
            }
            .ingame-debug-toolbar {
                top: 132px;
                right: 25px;
                width: 196px;
            }
            .ingame-debug-toolbar-slot-row {
                display: block;
                position: relative;
                width: 196px;
                height: 44px;
            }
            .ingame-debug-toolbar-content {
                display: block;
                padding-left: 6px;
                padding-right: 6px;
                padding-bottom: 6px;
            }
            .ingame-debug-toolbar-line {
                display: block;
                color: #f2f2f2;
            }
            .ingame-toolbar-slot {
                display: block;
                position: absolute;
                width: 27.5px;
                height: 27.5px;
            }
        ",
    );
}

fn append_debug_toolbar_slots(root: &mut ui_dom::RootDom, slot_row: ui_dom::DomNodeSlotKey) {
    for left_px in ["6px", "44px", "82px", "120px", "158px"] {
        let slot = assert_append_div_to(root, slot_row);
        assert_set_attr_str(root, slot, "class", "ingame-toolbar-slot");
        assert_style_set(root, slot, "left", left_px);
        assert_style_set(root, slot, "top", "6px");
    }
}

fn append_debug_toolbar_lines(
    root: &mut ui_dom::RootDom,
    content: ui_dom::DomNodeSlotKey,
) -> Vec<ui_dom::DomNodeSlotKey> {
    let mut line_text_keys = Vec::new();
    for text in [
        "FPS: --",
        "Avian: 0",
        "volume: down",
        "npc: down",
        "galaxy: down",
    ] {
        let line = assert_append_div_to(root, content);
        assert_set_attr_str(root, line, "class", "ingame-debug-toolbar-line");
        let text_key = crate::layout_proofs::support::assert_append_text_to(root, line, text);
        line_text_keys.push(text_key);
    }
    line_text_keys
}

fn append_debug_toolbar_below_top_right_sibling(
    root: &mut ui_dom::RootDom,
) -> DebugToolbarUpdateScenario {
    let document_root = root.document_node().0;
    let top_right_toolbar = assert_append_div_to(root, document_root);
    let debug_toolbar = assert_append_div_to(root, document_root);
    let slot_row = assert_append_div_to(root, debug_toolbar);
    let content = assert_append_div_to(root, debug_toolbar);
    let line_text_keys = append_debug_toolbar_lines(root, content);

    assert_set_attr_str(
        root,
        top_right_toolbar,
        "class",
        "ingame-toolbar ingame-view-toolbar",
    );
    assert_set_attr_str(
        root,
        debug_toolbar,
        "class",
        "ingame-toolbar ingame-debug-toolbar",
    );
    assert_set_attr_str(root, debug_toolbar, "pointer-events", "none");
    assert_set_attr_str(root, slot_row, "class", "ingame-debug-toolbar-slot-row");
    assert_set_attr_str(root, content, "class", "ingame-debug-toolbar-content");
    append_debug_toolbar_slots(root, slot_row);

    DebugToolbarUpdateScenario {
        debug_toolbar,
        line_text_keys,
    }
}

fn set_exact_ingame_top_right_toolbar_attrs(
    root: &mut ui_dom::RootDom,
    toolbar: ui_dom::DomNodeSlotKey,
) {
    assert_set_attr_str(root, toolbar, "class", "ingame-toolbar ingame-view-toolbar");
    assert_style_set(root, toolbar, "position", "fixed");
    assert_style_set(root, toolbar, "top", "25px");
    assert_style_set(root, toolbar, "right", "25px");
    assert_style_set(root, toolbar, "width", "196px");
    assert_style_set(root, toolbar, "height", "82px");
    assert_set_attr_str(root, toolbar, "pointer-events", "auto");
}

fn set_exact_ingame_debug_toolbar_attrs(
    root: &mut ui_dom::RootDom,
    toolbar: ui_dom::DomNodeSlotKey,
) {
    assert_set_attr_str(
        root,
        toolbar,
        "class",
        "ingame-toolbar ingame-debug-toolbar",
    );
    assert_style_set(root, toolbar, "position", "fixed");
    assert_style_set(root, toolbar, "top", "132px");
    assert_style_set(root, toolbar, "right", "25px");
    assert_style_set(root, toolbar, "width", "196px");
    assert_set_attr_str(root, toolbar, "pointer-events", "none");
}

fn append_exact_ingame_debug_toolbar_below_top_right_sibling(
    root: &mut ui_dom::RootDom,
) -> DebugToolbarUpdateScenario {
    let document_root = root.document_node().0;
    let top_right_toolbar = assert_append_div_to(root, document_root);
    let debug_toolbar = assert_append_div_to(root, document_root);
    let slot_row = assert_append_div_to(root, debug_toolbar);
    let content = assert_append_div_to(root, debug_toolbar);
    let line_text_keys = append_debug_toolbar_lines(root, content);

    set_exact_ingame_top_right_toolbar_attrs(root, top_right_toolbar);
    set_exact_ingame_debug_toolbar_attrs(root, debug_toolbar);
    assert_set_attr_str(root, slot_row, "class", "ingame-debug-toolbar-slot-row");
    assert_set_attr_str(root, content, "class", "ingame-debug-toolbar-content");
    append_debug_toolbar_slots(root, slot_row);

    DebugToolbarUpdateScenario {
        debug_toolbar,
        line_text_keys,
    }
}

fn append_exact_nested_ingame_debug_toolbar(
    root: &mut ui_dom::RootDom,
    include_normal_flow_sibling: bool,
) -> DebugToolbarUpdateScenario {
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(root, document_root);
    let top_right_toolbar = assert_append_div_to(root, wrapper);
    let debug_toolbar = assert_append_div_to(root, wrapper);
    let slot_row = assert_append_div_to(root, debug_toolbar);
    let content = assert_append_div_to(root, debug_toolbar);
    let line_text_keys = append_debug_toolbar_lines(root, content);

    assert_style_set(root, wrapper, "display", "block");
    assert_style_set(root, wrapper, "position", "relative");
    if include_normal_flow_sibling {
        let flow_sibling = assert_append_div_to(root, wrapper);
        let flow_sibling_inner = assert_append_div_to(root, flow_sibling);
        crate::layout_proofs::support::assert_append_text_to(
            root,
            flow_sibling_inner,
            "preceding block sibling should not become the fixed toolbar containing width",
        );
        assert_style_set(root, flow_sibling, "display", "block");
        assert_style_set(root, flow_sibling, "padding-left", "10px");
        assert_style_set(root, flow_sibling, "padding-right", "10px");
        assert_style_set(root, flow_sibling_inner, "display", "block");
        assert_style_set(root, flow_sibling_inner, "width", "196px");
        assert_style_set(root, flow_sibling_inner, "white-space", "nowrap");
    }

    set_exact_ingame_top_right_toolbar_attrs(root, top_right_toolbar);
    set_exact_ingame_debug_toolbar_attrs(root, debug_toolbar);
    assert_set_attr_str(root, slot_row, "class", "ingame-debug-toolbar-slot-row");
    assert_set_attr_str(root, content, "class", "ingame-debug-toolbar-content");
    append_debug_toolbar_slots(root, slot_row);

    DebugToolbarUpdateScenario {
        debug_toolbar,
        line_text_keys,
    }
}

fn append_auto_height_fixed_sibling_above_debug_toolbar(root: &mut ui_dom::RootDom) {
    let document_root = root.document_node().0;
    let top_sibling = assert_append_div_to(root, document_root);
    let first_line = assert_append_div_to(root, top_sibling);
    let second_line = assert_append_div_to(root, top_sibling);
    crate::layout_proofs::support::assert_append_text_to(root, first_line, "top fixed sibling");
    crate::layout_proofs::support::assert_append_text_to(root, second_line, "keeps auto height");

    assert_style_set(root, top_sibling, "display", "block");
    assert_style_set(root, top_sibling, "position", "fixed");
    assert_style_set(root, top_sibling, "top", "25px");
    assert_style_set(root, top_sibling, "right", "25px");
    assert_style_set(root, top_sibling, "width", "196px");
    assert_style_set(root, top_sibling, "padding-left", "6px");
    assert_style_set(root, top_sibling, "padding-right", "6px");
    assert_style_set(root, top_sibling, "padding-top", "6px");
    assert_style_set(root, top_sibling, "padding-bottom", "6px");
    assert_style_set(root, first_line, "display", "block");
    assert_style_set(root, second_line, "display", "block");
}

fn set_ingame_document_root_screen_size(root: &mut ui_dom::RootDom, width_px: f32, height_px: f32) {
    // The document root is viewport-sized: its box model comes from the Bevy
    // sprite the root is attached under (1280x720 in these proofs), not from
    // authored width/height declarations — the style pass skips document
    // nodes, so there is no document-level inline declaration store (spine
    // 3.3g-2a). The size parameters are retained only to keep the helper's
    // call sites honest about the intended screen geometry.
    let _ = (width_px, height_px);
    let document_root = root.document_node().0;
    assert_set_attr_str(root, document_root, "pointer-events", "none");
}

fn append_ingame_surface_root(root: &mut ui_dom::RootDom, id: &str) {
    let document_root = root.document_node().0;
    let surface_root = assert_append_div_to(root, document_root);
    assert_set_attr_str(root, surface_root, "id", id);
}

fn append_exact_ingame_bottom_toolbar(root: &mut ui_dom::RootDom) {
    let document_root = root.document_node().0;
    let toolbar = assert_append_div_to(root, document_root);
    assert_set_attr_str(
        root,
        toolbar,
        "class",
        "ingame-toolbar ingame-bottom-toolbar",
    );
    assert_style_set(root, toolbar, "position", "fixed");
    assert_style_set(root, toolbar, "bottom", "25px");
    assert_style_set(root, toolbar, "width", "386px");
    assert_style_set(root, toolbar, "height", "44px");
}

fn append_exact_ingame_npc_toolbar(root: &mut ui_dom::RootDom) {
    let document_root = root.document_node().0;
    let toolbar = assert_append_div_to(root, document_root);
    assert_set_attr_str(root, toolbar, "class", "ingame-toolbar ingame-npc-toolbar");
    assert_style_set(root, toolbar, "position", "fixed");
    assert_style_set(root, toolbar, "top", "25px");
    assert_style_set(root, toolbar, "left", "25px");
    assert_style_set(root, toolbar, "width", "44px");
    assert_style_set(root, toolbar, "height", "82px");
}

fn append_normal_flow_width_probe_before_fixed_toolbars(root: &mut ui_dom::RootDom) {
    let document_root = root.document_node().0;
    let flow_block = assert_append_div_to(root, document_root);
    let inner_block = assert_append_div_to(root, flow_block);
    crate::layout_proofs::support::assert_append_text_to(
        root,
        inner_block,
        "normal flow sibling should not narrow later fixed siblings",
    );

    assert_style_set(root, flow_block, "display", "block");
    assert_style_set(root, flow_block, "padding-left", "10px");
    assert_style_set(root, flow_block, "padding-right", "10px");
    assert_style_set(root, inner_block, "display", "block");
    assert_style_set(root, inner_block, "width", "196px");
    assert_style_set(root, inner_block, "white-space", "nowrap");
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: a right-anchored fixed box nested below an
// earlier fixed sibling keeps the viewport as its containing block, so a text
// content update must leave its viewport-resolved right anchor unchanged.
fn debug_toolbar_below_top_right_sibling_keeps_right_anchor_after_text_update() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    let scenario = append_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "debug toolbar should keep the viewport-sized available width after text updates",
    );
    assert_close(
        toolbar_snapshot.layout.used_width_px,
        196.0,
        "debug toolbar should keep its explicit width after text updates",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "debug toolbar should stay pinned to the viewport right edge after text updates",
    );
    assert_close(
        toolbar_box.border_box.top_px,
        132.0,
        "debug toolbar should stay below the top-right toolbar after text updates",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: mixing stylesheet classes with direct attrs for
// fixed positioning must not change the containing block; the recompute after
// a text update must keep the viewport-sized reference width instead of
// collapsing to the box's explicit width.
fn exact_ingame_debug_toolbar_attrs_keep_right_anchor_after_text_update() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    let scenario = append_exact_ingame_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "mixed attr and stylesheet fixed toolbar should keep viewport available width",
    );
    assert_close(
        toolbar_snapshot.layout.used_width_px,
        196.0,
        "mixed attr and stylesheet fixed toolbar should keep explicit width",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "mixed attr and stylesheet fixed toolbar should stay pinned to the viewport right edge",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: fixed positioning resolves against the initial
// containing block (WPT `css/CSS2/abspos/abspos-containing-block-initial-001.xht`),
// so a preceding normal-flow sibling with its own explicit-width child must
// not narrow a later fixed toolbar's reference width.
fn normal_flow_sibling_before_exact_fixed_debug_toolbar_does_not_narrow_right_anchor() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    append_normal_flow_width_probe_before_fixed_toolbars(&mut root);
    let scenario = append_exact_ingame_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "preceding normal-flow sibling should not narrow fixed toolbar available width",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "preceding normal-flow sibling should not push fixed toolbar off the left edge",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: nesting a fixed box under an ordinary block
// wrapper must not change its viewport containing block, so a non-root
// right-anchored fixed box keeps its viewport-resolved position after a text
// update.
fn nested_exact_fixed_debug_toolbar_keeps_right_anchor_after_text_update() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    let scenario = append_exact_nested_ingame_debug_toolbar(&mut root, false);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "nested fixed toolbar should keep viewport available width after text updates",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "nested fixed toolbar should stay pinned to the viewport right edge",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: a nested non-root fixed box stays viewport
// right-anchored when its auto-width wrapper also contains an in-flow
// sibling with an explicit-width child; in-flow siblings never become the
// containing block.
fn nested_fixed_debug_toolbar_with_flow_sibling_keeps_right_anchor_after_text_update() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    let scenario = append_exact_nested_ingame_debug_toolbar(&mut root, true);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "nested fixed toolbar should ignore sibling normal-flow width on relayout",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "nested fixed toolbar should stay pinned right despite sibling normal-flow width",
    );
}

#[test]
// CSS 2.1 10.3.7, 10.6.4, and 9.3.1: each fixed box resolves against the
// viewport independently, so a right-anchored fixed box below an earlier
// auto-height fixed sibling must not inherit the sibling's explicit width as
// its reference width.
fn auto_height_fixed_sibling_above_debug_toolbar_does_not_narrow_right_anchor() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    load_ingame_debug_toolbar_styles(&root);
    append_auto_height_fixed_sibling_above_debug_toolbar(&mut root);
    let scenario = append_exact_ingame_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        320.0,
        "auto-height fixed sibling should not narrow debug toolbar available width",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        99.0,
        "auto-height fixed sibling should not push debug toolbar off the left edge",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: a correct first update followed by a later text
// sync under the same fixed-sibling surface set must keep the fixed box's
// available width at the document width; a relayout must not shrink the
// reference width to the box's explicit width.
fn ingame_surface_second_update_keeps_debug_toolbar_viewport_width() {
    let (mut world, mut root) = attached_gpu_root_dom_setup(1280.0, 720.0);
    load_ingame_debug_toolbar_styles(&root);
    set_ingame_document_root_screen_size(&mut root, 1280.0, 720.0);
    append_ingame_surface_root(&mut root, "ingame-menu-surfaces");
    append_ingame_surface_root(&mut root, "ingame-modal-surfaces");
    append_exact_ingame_bottom_toolbar(&mut root);
    append_exact_ingame_npc_toolbar(&mut root);
    let scenario = append_exact_ingame_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    root.set_text_content(scenario.line_text_keys[3], "npc: ok")
        .expect("npc line should update");
    root.set_text_content(scenario.line_text_keys[4], "galaxy: ok")
        .expect("galaxy line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        1280.0,
        "second in-game style update should keep debug toolbar available width at document width",
    );
    assert_close(
        toolbar_snapshot.layout.used_width_px,
        196.0,
        "second in-game style update should keep explicit debug toolbar width",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        1059.0,
        "second in-game style update should keep debug toolbar pinned to the right edge",
    );
}

#[test]
// CSS 2.1 10.3.7 and 9.3.1: repeated runtime text syncs must keep using the
// document-sized available width for every re-entry into the retained
// relayout path, not only the first one.
fn ingame_surface_third_update_keeps_debug_toolbar_viewport_width() {
    let (mut world, mut root) = attached_gpu_root_dom_setup(1280.0, 720.0);
    load_ingame_debug_toolbar_styles(&root);
    set_ingame_document_root_screen_size(&mut root, 1280.0, 720.0);
    append_ingame_surface_root(&mut root, "ingame-menu-surfaces");
    append_ingame_surface_root(&mut root, "ingame-modal-surfaces");
    append_exact_ingame_bottom_toolbar(&mut root);
    append_exact_ingame_npc_toolbar(&mut root);
    let scenario = append_exact_ingame_debug_toolbar_below_top_right_sibling(&mut root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 60.0")
        .expect("fps line should update");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 1")
        .expect("avian line should update");
    root.set_text_content(scenario.line_text_keys[3], "npc: ok")
        .expect("npc line should update");
    root.set_text_content(scenario.line_text_keys[4], "galaxy: ok")
        .expect("galaxy line should update");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    root.set_text_content(scenario.line_text_keys[0], "FPS: 144.0")
        .expect("fps line should update again");
    root.set_text_content(scenario.line_text_keys[1], "Avian: 9")
        .expect("avian line should update again");
    root.set_text_content(scenario.line_text_keys[2], "volume: ok")
        .expect("volume line should update again");
    assert_root_dom_update(&mut world, &mut root, 2.0, DomPointerUpdate::Unchanged);

    let toolbar_snapshot = root
        .canonical_snapshot(scenario.debug_toolbar)
        .expect("debug toolbar should remain readable");
    let toolbar_box =
        node_box(&root, scenario.debug_toolbar).expect("debug toolbar should expose a box");

    assert_close(
        toolbar_snapshot.layout.available_width_px,
        1280.0,
        "third in-game style update should keep debug toolbar available width at document width",
    );
    assert_close(
        toolbar_snapshot.layout.used_width_px,
        196.0,
        "third in-game style update should keep explicit debug toolbar width",
    );
    assert_close(
        toolbar_box.border_box.left_px,
        1059.0,
        "third in-game style update should keep debug toolbar pinned to the right edge",
    );
}
