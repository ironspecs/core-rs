#![allow(rustdoc::bare_urls)]

//! Transition-position proofs for nested and sibling layers.
//!
//! Transitions start at the update that detects the change — D2's one-timing
//! model: the first update after the value-changing write schedules the
//! transition (presented == from), and the next update presents mid-flight.
//!
//! Spec references:
//! - CSS 2.1 9.3.1 positioning schemes:
//!   <https://www.w3.org/TR/CSS21/visuren.html#positioning-scheme>
//! - CSS Transitions Level 1 interpolation model:
//!   <https://www.w3.org/TR/css-transitions-1/>

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_span_to, assert_append_text_to, assert_close,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, document_with_panel_and_scroll_sibling_setup,
    fixed_panel_with_nested_scroll_box_setup, theme_stylesheet_origin,
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
    assert_style_set(&mut root, scroll_box, "position", "absolute");
    assert_style_set(&mut root, scroll_box, "left", "0px");
    assert_style_set(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_style_set(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition (presented == from); the second update
    // advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
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
    assert_style_set(&mut root, scroll_box, "position", "fixed");
    assert_style_set(&mut root, scroll_box, "left", "0px");
    assert_style_set(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_style_set(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition (presented == from); the second update
    // advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
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
    assert_style_set(&mut root, scroll_box, "position", "fixed");
    assert_style_set(&mut root, scroll_box, "left", OPEN_LEFT_PX);
    assert_style_set(&mut root, scroll_box, "top", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_panel_x = root
        .layer_offset_px(fixed_panel)
        .expect("fixed panel should be a live layer root")[0];
    let before_scroll_x = root
        .layer_offset_px(scroll_box)
        .expect("scroll box should be a live layer root")[0];
    let before_scroll_layout = assert_node_layout(&root, scroll_box);

    assert_style_set(&mut root, fixed_panel, "left", OPEN_LEFT_PX);
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition (presented == from); the second update
    // advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
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
    assert_style_set(root, fixed_panel, "position", "fixed");
    assert_style_set(root, fixed_panel, "top", "0px");
    assert_style_set(root, fixed_panel, "width", "120px");
    assert_style_set(root, fixed_panel, "height", "120px");
    assert_style_set(root, fixed_panel, "left", CLOSED_LEFT_PX);
    assert_style_set(root, fixed_panel, "transition", PANEL_TRANSITION);
}

fn author_scroll_box(root: &mut ui_dom::RootDom, scroll_box: ui_dom::DomNodeSlotKey) {
    assert_style_set(root, scroll_box, "width", "80px");
    assert_style_set(root, scroll_box, "height", "70px");
    assert_style_set(root, scroll_box, "overflow", "scroll");
}

#[test]
// CSS Transitions 1 §2 (the `transition` shorthand) + CSS Cascade 5 §3
// (shorthand expansion): the `transition` shorthand is a valid stylesheet
// declaration and expands to its four longhands exactly as if written in
// place, so a value change schedules the transition the longhands would.
// Gecko: servo/components/style/properties/declaration_block.rs (shorthands
// expand to longhand drains at parse); Blink:
// core/css/css_property_value_set.h.
// Closest WPT family: css/css-transitions/ (shorthand parsing and
// serialization).
//
// Relabeled from `off_spec_transition_shorthand_rejected_in_stylesheet_declarations`
// in spine step 2: the typed declaration store expands the shorthand at parse
// time (A1), so it loads with no drops and schedules exactly as the longhands.
fn transition_shorthand_expands_in_stylesheet_declarations() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let panel = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, panel, "class", "panel");
    assert_style_set(&mut root, panel, "position", "fixed");
    assert_style_set(&mut root, panel, "left", CLOSED_LEFT_PX);

    let report = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".panel { transition: left 180ms ease-out; }",
        )
        .expect("the transition shorthand is a valid stylesheet declaration");
    assert_eq!(
        report.declarations_dropped, 0,
        "the transition shorthand expands to longhands with no drops"
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, panel, "left", OPEN_LEFT_PX);
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition; the second advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(
        &mut world,
        &mut root,
        MID_TRANSITION_TIME_S,
        DomPointerUpdate::Unchanged,
    );

    let left = root
        .computed_style(panel)
        .expect("panel should remain readable")
        .as_box()
        .left;
    match left {
        ui_dom::BoxLength::Px(value) => {
            assert!(
                value > -100.0 && value < 0.0,
                "left should be mid-flight (strictly between -100 and 0), got {value}"
            );
        }
        other => panic!(
            "left should present a mid-flight px value via the shorthand-scheduled transition, got {other:?}"
        ),
    }
}

#[test]
// CSS Transitions 1 §2 (transitioning a property) + CSS Cascade 5 §6.1
// (transition declarations win the cascade; interpolated values inherit): a
// parent's mid-flight `color` is the cascaded value, so a descendant inherits
// the *interpolated* color, not the stored target. Replaces the pre-redesign
// stale-at-start quirk, where descendants read the parent's stored (target)
// color until the transition completed.
fn interpolated_inherited_values_transmit_to_descendants_mid_transition() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let child = assert_append_span_to(&mut root, parent);
    assert_append_text_to(&mut root, child, "inherit mid-flight");

    assert_style_set(&mut root, parent, "color", "#000000");
    assert_style_set(&mut root, parent, "transition-property", "color");
    assert_style_set(&mut root, parent, "transition-duration", "180ms");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, parent, "color", "#ffffff");
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition; the second advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(
        &mut world,
        &mut root,
        MID_TRANSITION_TIME_S,
        DomPointerUpdate::Unchanged,
    );

    let [red, green, blue, alpha] = root
        .computed_style(child)
        .expect("child should remain readable")
        .as_color()
        .color_rgba;
    assert!(
        red > 0.0 && red < 1.0,
        "child color should inherit the parent's mid-flight interpolated color, got red channel {red}"
    );
    assert!(
        (green - red).abs() <= f32::EPSILON && (blue - red).abs() <= f32::EPSILON,
        "child color should stay achromatic mid-flight, got {red} {green} {blue}"
    );
    assert!(
        (alpha - 1.0).abs() <= f32::EPSILON,
        "child color should stay fully opaque, got alpha {alpha}"
    );
}
