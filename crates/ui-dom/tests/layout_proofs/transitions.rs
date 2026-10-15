#![allow(rustdoc::bare_urls)]

//! Transition-position proofs for nested and sibling layers.
//!
//! Spec references:
//! - CSS 2.1 9.3.1 positioning schemes:
//!   <https://www.w3.org/TR/CSS21/visuren.html#positioning-scheme>
//! - CSS Transitions Level 1 interpolation model:
//!   <https://www.w3.org/TR/css-transitions-1/>

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_close, assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    document_with_panel_and_scroll_sibling_setup, fixed_panel_with_nested_scroll_box_setup,
};

const PANEL_TRANSITION: &str = "left 180ms ease-out";
const CLOSED_LEFT_PX: &str = "-100px";
const OPEN_LEFT_PX: &str = "0px";
const MID_TRANSITION_TIME_S: f64 = 0.09;

#[test]
// CSS positioning plus ui-dom retained-layout invariant: an absolutely
// positioned nested box should move with a transitioning fixed parent.
fn absolute_nested_scroll_layer_follows_fixed_parent_mid_transition() {
    let (mut world, mut root, fixed_panel, scroll_box) = fixed_panel_with_nested_scroll_box_setup();

    author_transitioning_fixed_panel(&mut root, fixed_panel);
    author_scroll_box(&mut root, scroll_box);
    assert_set_attr_str(&mut root, scroll_box, "position", "absolute");
    assert_set_attr_str(&mut root, scroll_box, "left", "0px");
    assert_set_attr_str(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_set_attr_str(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    assert_root_dom_update(
        &mut world,
        &mut root,
        MID_TRANSITION_TIME_S,
        DomPointerUpdate::Unchanged,
    );

    let after_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let after_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let after_scroll_layout = assert_node_layout(&root, scroll_box);
    let panel_delta_x = after_panel_x - before_panel_x;
    let scroll_delta_x = after_scroll_x - before_scroll_x;

    assert!(
        panel_delta_x.abs() > 0.0,
        "fixed panel should move during the active left transition",
    );
    assert_close(
        scroll_delta_x,
        panel_delta_x,
        "absolute nested child layer delta x",
    );
    assert_close(
        after_scroll_layout.width_px,
        before_scroll_layout.width_px,
        "absolute nested child width",
    );
    assert_close(
        after_scroll_layout.height_px,
        before_scroll_layout.height_px,
        "absolute nested child height",
    );
}

#[test]
// CSS positioning plus ui-dom retained-layout invariant: a fixed-position
// nested box stays pinned to its own fixed offset even while its fixed parent
// transitions.
fn fixed_nested_scroll_layer_stays_pinned_while_fixed_parent_moves_mid_transition() {
    let (mut world, mut root, fixed_panel, scroll_box) = fixed_panel_with_nested_scroll_box_setup();

    author_transitioning_fixed_panel(&mut root, fixed_panel);
    author_scroll_box(&mut root, scroll_box);
    assert_set_attr_str(&mut root, scroll_box, "position", "fixed");
    assert_set_attr_str(&mut root, scroll_box, "left", "0px");
    assert_set_attr_str(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_set_attr_str(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    assert_root_dom_update(
        &mut world,
        &mut root,
        MID_TRANSITION_TIME_S,
        DomPointerUpdate::Unchanged,
    );

    let after_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let after_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let after_scroll_layout = assert_node_layout(&root, scroll_box);
    let panel_delta_x = after_panel_x - before_panel_x;
    let scroll_delta_x = after_scroll_x - before_scroll_x;

    assert!(
        panel_delta_x.abs() > 0.0,
        "fixed panel should move during the active left transition",
    );
    assert_close(scroll_delta_x, 0.0, "fixed nested child layer delta x");
    assert_close(
        after_scroll_layout.width_px,
        before_scroll_layout.width_px,
        "fixed nested child width",
    );
    assert_close(
        after_scroll_layout.height_px,
        before_scroll_layout.height_px,
        "fixed nested child height",
    );
}

#[test]
// CSS positioning plus ui-dom retained-layout invariant: a sibling fixed box
// with the same initial left offset does not inherit the transitioning panel's
// movement.
fn fixed_sibling_scroll_layer_stays_pinned_while_panel_moves_mid_transition() {
    let (mut world, mut root, fixed_panel, scroll_box) =
        document_with_panel_and_scroll_sibling_setup();

    author_transitioning_fixed_panel(&mut root, fixed_panel);
    author_scroll_box(&mut root, scroll_box);
    assert_set_attr_str(&mut root, scroll_box, "position", "fixed");
    assert_set_attr_str(&mut root, scroll_box, "left", OPEN_LEFT_PX);
    assert_set_attr_str(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_set_attr_str(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    assert_root_dom_update(
        &mut world,
        &mut root,
        MID_TRANSITION_TIME_S,
        DomPointerUpdate::Unchanged,
    );

    let after_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let after_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let after_scroll_layout = assert_node_layout(&root, scroll_box);
    let panel_delta_x = after_panel_x - before_panel_x;
    let scroll_delta_x = after_scroll_x - before_scroll_x;

    assert!(
        panel_delta_x.abs() > 0.0,
        "fixed panel should move during the active left transition",
    );
    assert_close(scroll_delta_x, 0.0, "fixed sibling child layer delta x");
    assert_close(
        after_scroll_layout.width_px,
        before_scroll_layout.width_px,
        "fixed sibling child width",
    );
    assert_close(
        after_scroll_layout.height_px,
        before_scroll_layout.height_px,
        "fixed sibling child height",
    );
}

fn author_transitioning_fixed_panel(
    root: &mut ui_dom::RootDom,
    fixed_panel: ui_dom::DomNodeSlotKey,
) {
    assert_set_attr_str(root, fixed_panel, "position", "fixed");
    assert_set_attr_str(root, fixed_panel, "top", "0px");
    assert_set_attr_str(root, fixed_panel, "width", "120px");
    assert_set_attr_str(root, fixed_panel, "height", "120px");
    assert_set_attr_str(root, fixed_panel, "left", CLOSED_LEFT_PX);
    assert_set_attr_str(root, fixed_panel, "transition", PANEL_TRANSITION);
}

fn author_scroll_box(root: &mut ui_dom::RootDom, scroll_box: ui_dom::DomNodeSlotKey) {
    assert_set_attr_str(root, scroll_box, "width", "80px");
    assert_set_attr_str(root, scroll_box, "height", "70px");
    assert_set_attr_str(root, scroll_box, "overflow", "scroll");
}
