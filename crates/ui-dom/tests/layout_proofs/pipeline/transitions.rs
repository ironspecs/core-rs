#![allow(rustdoc::bare_urls)]
// The transition behavior family asserts bit-exact interpolated px values (the
// linear timing and 1s/100px/100ms shapes are exact in f32, and the progress
// ratios land on exact f32 midpoints), so the exact typed comparisons in this
// module are deliberate.
#![allow(clippy::float_cmp)]

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

use ui_dom::{DomPointerUpdate, RootDomMetricName};

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

// — the transition behavior family — //

/// Unwraps a computed box length to its resolved pixel value. The behavior
/// family asserts bit-exact interpolated px values, so a non-px length is a
/// test-authoring error, not a value to fudge.
fn expect_px(length: ui_dom::BoxLength, label: &str) -> f32 {
    match length {
        ui_dom::BoxLength::Px(px) => px,
        other => panic!("{label} should present a px value, got {other:?}"),
    }
}

/// The cubic-bézier coordinate at parameter `t` for one axis (CSS Easing 1 §3
/// shares this basis for both the x and y axes).
fn cubic_bezier_axis_value(t: f32, p1: f32, p2: f32) -> f32 {
    let inverse_t = 1.0 - t;
    3.0 * inverse_t * inverse_t * t * p1 + 3.0 * inverse_t * t * t * p2 + t * t * t
}

/// Solves the x-axis of a cubic-bézier timing function for `t` at the given
/// input progress, by Newton–Raphson (deliberately NOT the engine's bisection):
/// this is the spec-side evaluator the pin measures the engine against.
fn cubic_bezier_t_for_progress(progress: f32, x1: f32, x2: f32) -> f32 {
    // x(t) = p3 t^3 + p2 t^2 + p1 t (the constant term is 0); coefficients
    // derived from the control points.
    let p3 = 3.0 * x1 - 3.0 * x2 + 1.0;
    let p2 = 3.0 * x2 - 6.0 * x1;
    let p1 = 3.0 * x1;
    let mut t = progress;
    for _ in 0..32 {
        let value = ((p3 * t + p2) * t + p1) * t - progress;
        if value.abs() < 1e-9 {
            break;
        }
        let derivative = (3.0 * p3 * t + 2.0 * p2) * t + p1;
        t = (t - value / derivative).clamp(0.0, 1.0);
    }
    t
}

/// The spec's eased progress for one cubic-bézier timing function (CSS Easing 1
/// §3): solve the x-axis for `t`, then read the y-axis at that `t`.
fn spec_eased_progress(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    cubic_bezier_axis_value(cubic_bezier_t_for_progress(progress, x1, x2), y1, y2)
}

/// One timing-function row of the easing table: the keyword name plus its
/// cubic-bézier control points `[x1, y1, x2, y2]` (`None` for `linear`).
struct EasingCurve {
    keyword: &'static str,
    control_points: Option<[f32; 4]>,
}

/// The five timing functions (CSS Easing 1 §3): `linear` is the identity; each
/// keyword curve carries its cubic-bézier control points.
const EASING_CURVES: [EasingCurve; 5] = [
    EasingCurve {
        keyword: "linear",
        control_points: None,
    },
    EasingCurve {
        keyword: "ease",
        control_points: Some([0.25, 0.1, 0.25, 1.0]),
    },
    EasingCurve {
        keyword: "ease-in",
        control_points: Some([0.42, 0.0, 1.0, 1.0]),
    },
    EasingCurve {
        keyword: "ease-out",
        control_points: Some([0.0, 0.0, 0.58, 1.0]),
    },
    EasingCurve {
        keyword: "ease-in-out",
        control_points: Some([0.42, 0.0, 0.58, 1.0]),
    },
];

#[test]
// CSS Easing 1 §3 (cubic-bézier timing functions) + CSS Transitions 1 §2
// (`transition-timing-function`): the engine's `cubic_bezier_progress`
// (`dom_transition.rs`) must reproduce each keyword curve's control points, so
// the presented `left` at each sampled progress equals the SPEC's curve value.
// <https://drafts.csswg.org/css-easing-1/#cubic-bezier-easing-functions>
// The expectation is derived from a local cubic-bézier evaluator (Newton on the
// x-axis) so the pin compares the engine's solver to the spec's control points,
// never to itself. The comparison normalizes `left` back to eased progress
// (from 0px, to 100px) so the crate's 1e-3 tolerance applies to the [0, 1]
// progress domain, where the engine's bisection lands within ~2e-4 of the spec.
fn transition_easing_curves_match_css_control_points() {
    for curve in EASING_CURVES {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let target = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, target, "left", "0px");
        assert_style_set(&mut root, target, "transition-property", "left");
        assert_style_set(&mut root, target, "transition-duration", "1s");
        assert_style_set(
            &mut root,
            target,
            "transition-timing-function",
            curve.keyword,
        );

        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_style_set(&mut root, target, "left", "100px");
        // D2 one-timing: the detection update schedules the transition
        // (progress 0); each later update presents the eased value.
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        for progress in [0.25_f32, 0.5, 0.75] {
            assert_root_dom_update(
                &mut world,
                &mut root,
                f64::from(progress),
                DomPointerUpdate::Unchanged,
            );
            let expected_eased = match curve.control_points {
                Some([x1, y1, x2, y2]) => spec_eased_progress(progress, x1, y1, x2, y2),
                None => progress,
            };
            let actual_px = expect_px(
                root.computed_style(target).expect("target").as_box().left,
                curve.keyword,
            );
            assert_close(
                actual_px / 100.0,
                expected_eased,
                &format!("{} eased progress at {progress}", curve.keyword),
            );
        }
    }
}

#[test]
// CSS Transitions 1 §2 (`transition-delay`, negative allowed) + §3 (the active
// interval): a negative delay pulls the transition's effective start into the
// past, so the transition is already partway through at its first advance.
// <https://drafts.csswg.org/css-transitions-1/#starting>
// In D2 one-timing the detection update schedules and presents `from` (0.0);
// the FIRST sync (the next update, at the same clock time) is already at raw
// progress 0.5 — presented 50.0, not 0.0. Linear timing keeps that exact.
fn transition_negative_delay_starts_partway_through() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, target, "left", "0px");
    assert_style_set(&mut root, target, "transition-property", "left");
    assert_style_set(&mut root, target, "transition-duration", "100ms");
    assert_style_set(&mut root, target, "transition-delay", "-50ms");
    assert_style_set(&mut root, target, "transition-timing-function", "linear");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, target, "left", "100px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "detection left",
        ),
        0.0,
        "the detection update schedules the transition and presents `from` (0.0)",
    );

    // First sync at the same clock time: effective start is 50ms in the past,
    // so raw progress is 0.5 (the transition has already started).
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "first-sync left",
        ),
        50.0,
        "the negative delay should present the transition partway through (50.0), not 0.0",
    );
}

#[test]
// CSS Transitions 1 §3 (interrupting a running transition): a re-target reads
// the CURRENT presented value as the new `from`, so the replacement transition
// continues from ≈50 rather than jumping to the new target or resetting to 0.
// <https://drafts.csswg.org/css-transitions-1/#reversing>
// The replacement also increments `TransitionScheduleAllocation` and fires the
// `TransitionCancel` event (counted by `TransitionEventPathAllocation`). Linear
// timing keeps the 50.0/125.0 midpoints exact.
fn transition_retargeting_continues_from_the_presented_value() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, target, "left", "0px");
    assert_style_set(&mut root, target, "transition-property", "left");
    assert_style_set(&mut root, target, "transition-duration", "200ms");
    assert_style_set(&mut root, target, "transition-timing-function", "linear");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, target, "left", "100px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.1, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "mid left",
        ),
        50.0,
        "the first transition should be at its 50px midpoint",
    );

    assert_style_set(&mut root, target, "left", "200px");
    assert_root_dom_update(&mut world, &mut root, 0.1, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "re-target left",
        ),
        50.0,
        "the re-target should continue from the presented 50.0, not jump to 200 or reset to 0",
    );
    assert_eq!(
        root.count(RootDomMetricName::TransitionScheduleAllocation),
        2,
        "the replacement transition should schedule once more",
    );
    assert_eq!(
        root.count(RootDomMetricName::TransitionEventPathAllocation),
        1,
        "replacing the running transition should dispatch one TransitionCancel event",
    );

    assert_root_dom_update(&mut world, &mut root, 0.2, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "later left",
        ),
        125.0,
        "the re-targeted transition should reach the 50→200 midpoint (125.0)",
    );
}

#[test]
// CSS Transitions 1 §2 (list-valued `transition-*` longhands repeat to the
// longest list): (a) a single duration repeats to every listed property; (b) a
// shorter duration list repeats by index (width and left share the 100ms slot,
// height the 200ms slot — 100/200/100), so width and left complete at 100ms
// while height is still mid-flight. Linear timing keeps the mid-flight values
// exact.
// <https://drafts.csswg.org/css-transitions-1/#transition-shorthand-property>
fn transition_lists_repeat_to_the_longest() {
    // (a) `width, height` with one 100ms duration: both properties transition.
    {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let target = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, target, "width", "20px");
        assert_style_set(&mut root, target, "height", "40px");
        assert_style_set(&mut root, target, "transition-property", "width, height");
        assert_style_set(&mut root, target, "transition-duration", "100ms");
        assert_style_set(&mut root, target, "transition-timing-function", "linear");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_style_set(&mut root, target, "width", "100px");
        assert_style_set(&mut root, target, "height", "200px");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_root_dom_update(&mut world, &mut root, 0.05, DomPointerUpdate::Unchanged);
        let box_ = root.computed_style(target).expect("target").as_box();
        assert_eq!(
            box_.width,
            ui_dom::BoxLength::Px(60.0),
            "(a) width mid-flight"
        );
        assert_eq!(
            box_.height,
            ui_dom::BoxLength::Px(120.0),
            "(a) height mid-flight"
        );
    }

    // (b) `width, height, left` with `100ms, 200ms`: durations repeat 100/200/100.
    {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let target = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, target, "width", "20px");
        assert_style_set(&mut root, target, "height", "40px");
        assert_style_set(&mut root, target, "left", "0px");
        assert_style_set(
            &mut root,
            target,
            "transition-property",
            "width, height, left",
        );
        assert_style_set(&mut root, target, "transition-duration", "100ms, 200ms");
        assert_style_set(&mut root, target, "transition-timing-function", "linear");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_style_set(&mut root, target, "width", "100px");
        assert_style_set(&mut root, target, "height", "200px");
        assert_style_set(&mut root, target, "left", "100px");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_root_dom_update(&mut world, &mut root, 0.1, DomPointerUpdate::Unchanged);
        let box_ = root.computed_style(target).expect("target").as_box();
        assert_eq!(
            box_.width,
            ui_dom::BoxLength::Px(100.0),
            "(b) width completes at its 100ms slot",
        );
        assert_eq!(
            box_.left,
            ui_dom::BoxLength::Px(100.0),
            "(b) left repeats the 100ms slot (repeat-to-longest)",
        );
        assert_eq!(
            box_.height,
            ui_dom::BoxLength::Px(120.0),
            "(b) height is mid-flight at its 200ms slot",
        );
    }
}

#[test]
// CSS Transitions 1 §3 (completion removes the running transition; the
// transition tier disappears): after the finishing update the presented value
// is exactly the target (the stored cascaded value with no residual overlay), a
// further update still reads the target, and a later change schedules a fresh
// transition that completes cleanly.
// <https://drafts.csswg.org/css-transitions-1/#completed>
fn transition_completion_removes_the_overlay() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, target, "left", "0px");
    assert_style_set(&mut root, target, "transition-property", "left");
    assert_style_set(&mut root, target, "transition-duration", "100ms");
    assert_style_set(&mut root, target, "transition-timing-function", "linear");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, target, "left", "100px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.05, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "mid left",
        ),
        50.0,
        "the transition should be at its 50px midpoint",
    );

    // Past the duration: the overlay is removed and the target is presented.
    assert_root_dom_update(&mut world, &mut root, 0.2, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "finished left",
        ),
        100.0,
        "after the finishing update the overlay is removed and the target is presented exactly",
    );

    // A further update still reads the stored target (no residual overlay).
    assert_root_dom_update(&mut world, &mut root, 0.3, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "later left",
        ),
        100.0,
        "a further update still reads the stored target with no residual overlay",
    );

    // A fresh change schedules cleanly (the scheduler is not stuck) and, once
    // it completes, presents the new target.
    let schedule_before = root.count(RootDomMetricName::TransitionScheduleAllocation);
    assert_style_set(&mut root, target, "left", "50px");
    assert_root_dom_update(&mut world, &mut root, 0.3, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.count(RootDomMetricName::TransitionScheduleAllocation),
        schedule_before + 1,
        "the later change should schedule a fresh transition",
    );
    assert_root_dom_update(&mut world, &mut root, 0.5, DomPointerUpdate::Unchanged);
    assert_eq!(
        expect_px(
            root.computed_style(target).expect("target").as_box().left,
            "re-transitioned left",
        ),
        50.0,
        "the new transition should complete to the new 50.0 target",
    );
}

#[test]
// CSS Transitions 1 §3 (a transition starts only when the value changes; a
// from == to write schedules nothing): writing the SAME width the element
// already has produces no transition schedule, no transition event, and no
// active transition, so `TransitionScheduleAllocation` is unchanged.
// <https://drafts.csswg.org/css-transitions-1/#starting>
fn transition_from_equals_to_schedules_nothing() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, target, "width", "20px");
    assert_style_set(&mut root, target, "transition", "width 100ms");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let schedule_before = root.count(RootDomMetricName::TransitionScheduleAllocation);
    assert_eq!(
        schedule_before, 0,
        "the baseline pass should never schedule a transition",
    );

    assert_style_set(&mut root, target, "width", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.count(RootDomMetricName::TransitionScheduleAllocation),
        schedule_before,
        "a from == to write must not schedule a transition",
    );
    assert_eq!(
        root.count(RootDomMetricName::TransitionEventPathAllocation),
        0,
        "a from == to write must not fire a transition event",
    );
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Px(20.0),
        "the width should stay at its authored 20px",
    );
}

#[test]
// CSS Cascade 5 §6.1 (transition declarations sit ABOVE even important author
// declarations): while a width transition runs, the presented value is the
// interpolated transition value, not the important author declaration.
// <https://drafts.csswg.org/css-cascade-5/#cascade-origin>
// The target must be written as important inline (`width: 100px !important`) so
// it wins the cascade over the important rule `.a { width: 10px !important }` —
// a NORMAL inline write loses to that rule (CSS Cascade 5 §6.4.1), leaves the
// cascaded value unchanged, and schedules nothing. Linear timing keeps the
// 10→100 midpoint (55.0) exact.
fn transition_overlay_beats_important_declarations_while_running() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "a");
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), ".a { width: 10px !important }")
        .expect("the important rule should load");
    assert_style_set(&mut root, target, "transition-property", "width");
    assert_style_set(&mut root, target, "transition-duration", "200ms");
    assert_style_set(&mut root, target, "transition-timing-function", "linear");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Px(10.0),
        "the important rule should set the baseline width to 10px",
    );

    assert_style_set(&mut root, target, "width", "100px !important");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.1, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Px(55.0),
        "the running transition's overlay (55.0) should beat the important declarations",
    );

    // Past the duration: the overlay is gone and the cascaded important-inline
    // target (100.0) is presented — not the important rule's 10.0.
    assert_root_dom_update(&mut world, &mut root, 0.4, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Px(100.0),
        "after completion the cascaded important-inline target (100.0) is presented, not the important rule's 10.0",
    );
}

#[test]
// CSS Transitions 1 interpolates length-percentage endpoints (the percentage
// resolves against the containing block and the px pair lerps). Our
// scheduler's `interpolate_value` (`dom_transition.rs`) only lerps `Px → Px`,
// so a `DeclLength::Percent` endpoint carries no used basis at interpolation
// time and steps today: mid-flight presents the from-shape's used value and
// the target is taken only at completion. Follow-up (deferred, admission-gated
// per D6): resolve percentage endpoints against the containing-block width at
// schedule time and lerp the px pair.
// <https://www.w3.org/TR/css-transitions-1/>
fn percentage_authored_transitions_step_at_completion_for_now() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let target = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, target, "width", "50%");
    assert_style_set(&mut root, target, "transition", "width 200ms linear");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, target, "width", "100px");
    // Detection update schedules the transition (presented == from-shape).
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    // Progress 0.5: the step rule keeps the symbolic from-shape, which resolves
    // to 100.0 (50% of the 200px container) — not an interpolated intermediate.
    assert_root_dom_update(&mut world, &mut root, 0.1, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Percent(50.0),
        "mid-flight should keep the symbolic from-shape (Percent(50.0)), not an interpolated px",
    );
    assert_eq!(
        ui_dom::node_box(&root, target)
            .expect("box")
            .border_box
            .width_px,
        100.0,
        "mid-flight width should present the from-shape's used value (100.0)",
    );

    // Completion: the target is taken exactly.
    assert_root_dom_update(&mut world, &mut root, 0.4, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        ui_dom::BoxLength::Px(100.0),
        "completion should take the target (Px(100.0))",
    );
    assert_eq!(
        ui_dom::node_box(&root, target)
            .expect("box")
            .border_box
            .width_px,
        100.0,
        "completion width should be exactly 100.0",
    );
}
