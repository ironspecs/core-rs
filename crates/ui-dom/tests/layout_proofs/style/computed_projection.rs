//! Computed-group resolution tests (PLAN.md spine 3.3g).
//!
//! These tests assert the seven computed groups written by the style pass's
//! typed winners fold agree with the declared values, read through the public
//! view API (`RootDom::computed_style` → `ComputedStyleRef`). The fold resolves
//! stylesheet rules and inline `style_mut`/`style`-attribute declarations into
//! the groups in one pass (spine 3.3g-2a: the legacy attr-projection channel
//! is gone).
//!
//! Convention:
//!
//! - stylesheet-driven cases load a rule and style the node through its class;
//! - direct-write cases write presentation through `style_mut().set` (which
//!   marks style-dirty only; the next `RootDom::update` resolves the groups);
//! - the resolver maps px/em to exact `f32` literals (integer px and
//!   round-tripped transition times are bit-exact), so exact comparisons are
//!   deliberate (`#![allow(clippy::float_cmp)]`).

#![allow(clippy::float_cmp)]

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_input_to, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup,
};

use bevy::prelude::World;
use ui_dom::{
    DomAttributeValue, DomNodeSlotKey, DomPointerUpdate, FlexBasisKw, FlexDirectionKw, FlexWrapKw,
    Keyword, PresentationProperty, RootDom, StyleRuleOrigin, TransitionLonghandProperty,
    parse_css_color_rgba,
};

fn theme_origin() -> StyleRuleOrigin {
    StyleRuleOrigin::new("theme", 0)
}

fn update(world: &mut World, root: &mut RootDom) {
    assert_root_dom_update(world, root, 0.0, DomPointerUpdate::Unchanged);
}

/// Returns `(world, root, container)` with a single styled (style-armed) div
/// under the document root.
fn armed_div_setup() -> (World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "panel");
    (world, root, container)
}

// — Box group — //

#[test]
fn box_group_agrees_with_stylesheet_lengths_and_unset() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { width: 120px; height: 40px; min-width: 20px; }",
    );
    update(&mut world, &mut root);

    let box_ = root.computed_style(container).expect("container").as_box();
    assert_eq!(box_.width, Some(120.0));
    assert_eq!(box_.height, Some(40.0));
    assert_eq!(box_.min_width, Some(20.0));
    assert_eq!(box_.min_height, None);

    // `unset` width removes the longhand, so the group field is `None`.
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("late", 1),
        ".panel { width: unset; }",
    );
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_box()
            .width,
        None
    );
}

#[test]
fn box_group_agrees_with_margin_edges_and_negative_margins() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { margin-top: 4px; margin-right: 6px; margin-bottom: -10px; margin-left: 2px; }",
    );
    update(&mut world, &mut root);

    let box_ = root.computed_style(container).expect("container").as_box();
    assert_eq!(box_.margin, [4.0, 6.0, -10.0, 2.0]);
    assert_eq!(box_.padding, [0.0; 4]);
}

// — Flex group: shorthand fallback — //

#[test]
fn flex_group_agrees_with_direct_gap_shorthand() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "gap", "12px");
    update(&mut world, &mut root);

    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_flex()
            .gap,
        [12.0, 12.0]
    );
}

#[test]
fn flex_group_gap_two_value_shorthand_and_longhand_precedence() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "gap", "4px 8px");
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_flex()
            .gap,
        [4.0, 8.0]
    );

    // A row-gap longhand declared after the gap shorthand beats the shorthand
    // for its axis only (shorthands expand to longhands at parse time, so the
    // later declaration wins by cascade order).
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "gap", "4px 8px");
    assert_style_set(&mut root, container, "row-gap", "2px");
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_flex()
            .gap,
        [2.0, 8.0]
    );
}

#[test]
fn flex_group_agrees_with_direct_flex_and_flex_flow_shorthands() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex", "1 0 auto");
    assert_style_set(&mut root, container, "flex-flow", "column wrap");
    update(&mut world, &mut root);

    let flex = root.computed_style(container).expect("container").as_flex();
    assert_eq!(flex.grow, 1.0);
    assert_eq!(flex.shrink, 0.0);
    assert_eq!(flex.basis, FlexBasisKw::Auto);
    assert_eq!(flex.direction, FlexDirectionKw::Column);
    assert_eq!(flex.wrap, FlexWrapKw::Wrap);
}

// — Visual group: radius and colors — //

#[test]
fn visual_group_agrees_with_stylesheet_radius_expansion() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(&root, theme_origin(), ".panel { border-radius: 3px 4px; }");
    update(&mut world, &mut root);

    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_visual()
            .radius,
        [3.0, 4.0, 3.0, 4.0]
    );
}

#[test]
fn visual_group_agrees_with_direct_border_radius_shorthand_fallback() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "border-radius", "1px 2px 3px 4px");
    update(&mut world, &mut root);

    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_visual()
            .radius,
        [1.0, 2.0, 3.0, 4.0]
    );
}

#[test]
fn visual_and_color_groups_agree_with_stylesheet_colors() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { background-color: #112233; color: #abcdef; }",
    );
    update(&mut world, &mut root);

    let view = root.computed_style(container).expect("container");
    assert_eq!(
        view.as_visual().background_rgba,
        parse_css_color_rgba("#112233").expect("background")
    );
    assert_eq!(
        view.as_color().color_rgba,
        parse_css_color_rgba("#abcdef").expect("color")
    );
}

#[test]
fn visual_group_agrees_with_direct_border_color_fallback() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "border-color", "#112233");
    update(&mut world, &mut root);

    let rgba = parse_css_color_rgba("#112233").expect("border color");
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_visual()
            .border_rgba,
        [rgba; 4]
    );
}

// — Font group — //

#[test]
fn font_group_resolves_em_chain_and_line_height() {
    let (mut world, mut root, container) = armed_div_setup();
    let parent = assert_append_div_to(&mut root, container);
    let child = assert_append_div_to(&mut root, parent);
    assert_set_attr_str(&mut root, parent, "class", "base");
    assert_set_attr_str(&mut root, child, "class", "scaled");
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".base { font-size: 10px; } .scaled { font-size: 1.5em; line-height: 1.5; }",
    );
    update(&mut world, &mut root);

    let font = root.computed_style(child).expect("child").as_font();
    assert_eq!(font.size_px, 15.0);
    assert_eq!(font.line_height_px, Some(22.5));
}

#[test]
fn font_group_line_height_normal_is_none() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(&root, theme_origin(), ".panel { line-height: normal; }");
    update(&mut world, &mut root);

    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_font()
            .line_height_px,
        None
    );
}

#[test]
fn font_group_resolves_weight_keywords_and_bolder_chain() {
    // bold → 700, 600 → 600.
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(&root, theme_origin(), ".panel { font-weight: bold; }");
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_font()
            .weight,
        700
    );

    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(&root, theme_origin(), ".panel { font-weight: 600; }");
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_font()
            .weight,
        600
    );

    // Parent 400 (default), child bolder → 700 (CSS Fonts 4 relative table).
    let (mut world, mut root, container) = armed_div_setup();
    let parent = assert_append_div_to(&mut root, container);
    let child = assert_append_div_to(&mut root, parent);
    assert_set_attr_str(&mut root, child, "class", "bolder");
    assert_load_root_stylesheet(&root, theme_origin(), ".bolder { font-weight: bolder; }");
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(child).expect("child").as_font().weight,
        700
    );
}

// — Text group: materialized inheritance + future-correct font-family — //

#[test]
fn text_group_carries_materialized_text_align_and_direction() {
    let (mut world, mut root, container) = armed_div_setup();
    let child = assert_append_div_to(&mut root, container);
    assert_set_attr_str(&mut root, child, "class", "leaf");
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { text-align: center; direction: rtl; }",
    );
    update(&mut world, &mut root);

    let text = root.computed_style(child).expect("child").as_text();
    assert_eq!(text.align, ui_dom::DomTextAlign::Center);
    assert_eq!(text.direction, ui_dom::DomTextDirection::Rtl);
}

#[test]
fn font_family_inherits_the_future_correct_group_value() {
    // The off-spec proof `off_spec_font_family_weight_and_style_do_not_inherit`
    // pins non-inheritance at the ATTRIBUTE level; the group projects the
    // CSS-correct inherited family (3.3g behavior arriving early in group-land).
    let (mut world, mut root, container) = armed_div_setup();
    let child = assert_append_div_to(&mut root, container);
    assert_set_attr_str(&mut root, child, "class", "leaf");
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { font-family: ui-sans, sans-serif; }",
    );
    update(&mut world, &mut root);

    assert_eq!(
        root.computed_style(child).expect("child").font_family(),
        Some("ui-sans, sans-serif")
    );
}

// — Transform group — //

#[test]
fn transform_group_agrees_with_direct_translate_and_none() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "transform", "translateX(10px)");
    update(&mut world, &mut root);
    let transform = root
        .computed_style(container)
        .expect("container")
        .as_transform();
    assert_eq!(transform.translate_px, [10.0, 0.0]);
    assert!(transform.has_transform);

    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "transform", "none");
    update(&mut world, &mut root);
    let transform = root
        .computed_style(container)
        .expect("container")
        .as_transform();
    assert_eq!(transform.translate_px, [0.0, 0.0]);
    assert!(!transform.has_transform);
}

// — Transition longhand slot — //

#[test]
fn transition_slot_agrees_with_stylesheet_shorthand_expansion() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { transition: left 180ms ease-out, width 1s; }",
    );
    update(&mut world, &mut root);

    let view = root.computed_style(container).expect("container");
    let longhands = view.transition_longhands();
    assert_eq!(
        longhands.properties.as_slice(),
        &[
            TransitionLonghandProperty::Property(PresentationProperty::Left),
            TransitionLonghandProperty::Property(PresentationProperty::Width),
        ]
    );
    assert_eq!(longhands.durations_s.as_slice(), &[0.18, 1.0]);
    assert_eq!(
        longhands.timing_functions.as_slice(),
        &[Keyword::EaseOut, Keyword::Ease]
    );
    assert_eq!(longhands.delays_s.as_slice(), &[0.0, 0.0]);
}

#[test]
fn transition_slot_rejects_over_capacity_duration_list_strictly() {
    let (mut world, mut root, container) = armed_div_setup();
    // The transition-longhand slot is bounded at TRANSITION_LONGHAND_CAPACITY
    // (4). The strict `style_mut().set` path rejects an over-capacity list
    // (fail fast) rather than silently truncating it.
    assert_eq!(
        root.style_mut(container)
            .set("transition-duration", "1s, 2s, 3s, 4s, 5s"),
        Err(ui_dom::DomError::InvalidAttributeValue)
    );
    update(&mut world, &mut root);

    // A capacity-sized list is admitted and fills the slot in order.
    assert_style_set(
        &mut root,
        container,
        "transition-duration",
        "1s, 2s, 3s, 4s",
    );
    update(&mut world, &mut root);

    let view = root.computed_style(container).expect("container");
    let longhands = view.transition_longhands();
    assert_eq!(longhands.durations_s.as_slice(), &[1.0, 2.0, 3.0, 4.0]);
}

// — Behavior sheet: focus outline — //

#[test]
fn visual_group_agrees_with_focus_behavior_outline() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let input = assert_append_input_to(&mut root, document_root);
    root.focus(input).expect("input should focus");
    update(&mut world, &mut root);

    let visual = root.computed_style(input).expect("input").as_visual();
    assert_eq!(visual.outline_width, 2.0);
    assert_eq!(visual.outline_offset, 0.0);
    assert_eq!(
        visual.outline_rgba,
        parse_css_color_rgba("#7aa2ff").expect("focus outline")
    );
}

// — style_mut writes resolve on the next pass (D2 one-timing) — //

#[test]
fn direct_width_write_applies_on_the_next_pass() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_load_root_stylesheet(
        &root,
        theme_origin(),
        ".panel { width: 20px; height: 40px; }",
    );
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_box()
            .width,
        Some(20.0)
    );

    assert_style_set(&mut root, container, "width", "10px");

    // D2 one-timing: a `style_mut` write marks style-dirty only; the pass
    // resolves the inline declaration on the next update, not synchronously.
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_box()
            .width,
        Some(20.0),
        "the computed group should not change until the next pass",
    );
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_box()
            .width,
        Some(10.0)
    );
}

#[test]
fn direct_gap_shorthand_write_applies_on_the_next_pass() {
    let (mut world, mut root, container) = armed_div_setup();
    assert_style_set(&mut root, container, "display", "flex");
    update(&mut world, &mut root);

    assert_style_set(&mut root, container, "gap", "12px");

    // D2 one-timing: the gap write is not reflected until the next pass.
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_flex()
            .gap,
        [0.0, 0.0],
        "the flex group should not change until the next pass",
    );
    update(&mut world, &mut root);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_flex()
            .gap,
        [12.0, 12.0]
    );
}

#[test]
fn transition_overlay_merges_mid_flight_left_into_computed_box() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let panel = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, panel, "position", "fixed");
    assert_style_set(&mut root, panel, "top", "0px");
    assert_style_set(&mut root, panel, "width", "120px");
    assert_style_set(&mut root, panel, "height", "120px");
    assert_style_set(&mut root, panel, "left", "-100px");
    assert_style_set(&mut root, panel, "transition", "left 180ms ease-out");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, panel, "left", "0px");
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition; the second advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.09, DomPointerUpdate::Unchanged);

    let left = root.computed_style(panel).expect("panel").as_box().left;
    let Some(left) = left else {
        panic!("left should present a mid-flight value via the transition overlay");
    };
    assert!(
        left > -100.0 && left < 0.0,
        "left should be strictly between -100 and 0 mid-flight, got {left}"
    );

    // The cascade value still reads the authored target (0px); the overlay
    // lives only in the transition record, not the cascade. `compute_style_map`
    // re-folds the winners without the transition overlay, so it exposes the
    // target while `computed_style` (above) exposes the mid-flight overlay.
    assert_eq!(
        root.compute_style_map(panel)
            .expect("cascade query should succeed")
            .get(&PresentationProperty::Left),
        Some(&DomAttributeValue::String("0px".to_string()))
    );

    // A left transition must not disturb the visual group.
    assert_eq!(
        root.computed_style(panel)
            .expect("panel")
            .as_visual()
            .background_rgba,
        [0.0; 4]
    );
}

#[test]
fn transition_overlay_merges_mid_flight_color_into_computed_visual() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let panel = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, panel, "background-color", "#000000");
    assert_style_set(
        &mut root,
        panel,
        "transition",
        "background-color 180ms linear",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_style_set(&mut root, panel, "background-color", "#ffffff");
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change and starts the transition; the second advances it to the mid-time.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.09, DomPointerUpdate::Unchanged);

    let background = root
        .computed_style(panel)
        .expect("panel")
        .as_visual()
        .background_rgba;
    assert!(
        background[0] > 0.0 && background[0] < 1.0,
        "background red channel should be mid-flight, got {background:?}"
    );
}
