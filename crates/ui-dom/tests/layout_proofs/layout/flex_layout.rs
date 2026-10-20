#![allow(rustdoc::bare_urls)]

//! Flex layout proofs backed by the researched browser seams.
//!
//! Spec references:
//! - CSS Flexbox Level 1 layout algorithm:
//!   <https://drafts.csswg.org/css-flexbox-1/#layout-algorithm>
//! - CSS Flexbox Level 1 automatic minimum size of flex items:
//!   <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>
//!
//! Closest working examples used as oracles:
//! - WPT `flex-grow-001.xht`
//! - WPT `flex-shrink-001.html`
//! - WPT `flex-wrap-002.html`
//! - WPT `flexbox-min-width-auto-001.html`
//! - WPT `flexbox_direction-row-reverse.html`
//! - WPT `flexbox_direction-column.html`
//! - WPT `flex-order.html`
//! - WPT `flexbox-justify-content-horiz-001a.xhtml`
//! - WPT `flexbox_align-items-center.html`
//! - WPT `flexbox_align-self-center.html`
//! - WPT `flexbox-column-row-gap-001.html`
//!
//! Browser implementation references:
//! - Gecko flex container frame:
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/generic/nsFlexContainerFrame.cpp>
//! - Blink flex layout algorithm:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc>
//! - Blink explicit line collection:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/flex/flex_line_breaker.cc>
//! - Blink per-line flexible length resolution:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/flex/line_flexer.cc>
//! - Blink line data model:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/flex/flex_line.h>
//!
//! These proofs lock the admitted active-solver behavior in place. The flex
//! arc step 4 auto-min derivation is shape-general and grow-blind: every
//! visible-overflow item's automatic minimum is `min(min_content_main,
//! definite)` read from its record (D1/D2a), with no shape gates and no
//! measured-size fallback. The former two graceful gates
//! (`flex_container_content_based_auto_min_is_supported` and the grow-factor
//! boundary in `horizontal_flex_container_content_based_auto_min_content_width_px`)
//! died with the recursive auto-min walk.

use ui_dom::{
    DomIntrinsic, DomLayerKind, DomNode, DomNodeSlotKey, DomPointerUpdate, RootDom,
    RootDomMetricName, TextAlign, node_box,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_img_to, assert_append_span_to, assert_append_text_to,
    assert_close, assert_load_root_stylesheet, assert_node_layout, assert_rect_op_fill,
    assert_rect_op_geometry, assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, take_staged_layer_paint_plans,
    theme_stylesheet_origin,
};

const TITLEBAR_ICON_WIDTH_PX: f32 = 16.0;
const TITLEBAR_ICON_BASIS_PX: &str = "16px";
const TITLEBAR_ICON_HEIGHT_PX: &str = "16px";
const TITLEBAR_HOST_HEIGHT_PX: &str = "18px";

#[test]
// CSS Flexbox layout algorithm and WPT flex-grow-001.xht: after line
// collection, positive free space is distributed by grow factor rather than
// leaving every item at its authored basis. In this row case the container has
// 60px of positive free space, so grow factors 0, 1, and 2 yield final widths
// of 10px, 30px, and 50px.
fn flex_row_distributes_positive_free_space_by_grow_factor() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "90px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, first, "10px", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "1");
    assert_set_flex_basis_and_cross_size(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "flex-grow", "1");
    assert_style_set(&mut root, second, "flex-shrink", "1");
    assert_set_flex_basis_and_cross_size(&mut root, third, "10px", "10px");
    assert_style_set(&mut root, third, "flex-grow", "2");
    assert_style_set(&mut root, third, "flex-shrink", "1");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "grow sizing should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "grow sizing should preserve DOM sibling links",
    );
    assert_close(
        assert_node_layout(&root, first).width_px,
        10.0,
        "first grow item width",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        30.0,
        "second grow item width",
    );
    assert_close(
        assert_node_layout(&root, third).width_px,
        50.0,
        "third grow item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first grow item");
    assert_rect_op_geometry(&rects[2], 10.0, 0.0, 30.0, 10.0, "second grow item");
    assert_rect_op_geometry(&rects[3], 40.0, 0.0, 50.0, 10.0, "third grow item");
}

#[test]
// CSS Flexbox 5.1 and WPT flexbox_direction-row-reverse.html: row-reverse
// flips the main-start and main-end directions without mutating DOM order. The
// first DOM child is still the first child in the tree, but it is laid out at
// the right edge of the row and subsequent children proceed leftward.
fn row_reverse_positions_items_from_the_opposite_main_start() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "90px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "flex-direction", "row-reverse");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "row-reverse should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "row-reverse should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 80.0, 0.0, 10.0, 10.0, "first reversed-row item");
    assert_rect_op_geometry(&rects[2], 70.0, 0.0, 10.0, 10.0, "second reversed-row item");
    assert_rect_op_geometry(&rects[3], 60.0, 0.0, 10.0, 10.0, "third reversed-row item");
}

#[test]
// CSS Flexbox 5.1: `row` uses the inline axis of the current writing mode.
// Under `direction: rtl`, the row main-start edge is the physical right edge
// even without `row-reverse`.
fn rtl_row_direction_uses_inline_start_for_main_axis_progression() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "90px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "direction", "rtl");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 80.0, 0.0, 10.0, 10.0, "first rtl row item");
    assert_rect_op_geometry(&rects[2], 70.0, 0.0, 10.0, 10.0, "second rtl row item");
    assert_rect_op_geometry(&rects[3], 60.0, 0.0, 10.0, 10.0, "third rtl row item");
}

#[test]
// CSS Flexbox 5.1: `row-reverse` swaps main-start and main-end after `row`
// inherits the inline direction. In RTL, that means `row-reverse` cancels back
// to left-to-right physical progression.
fn rtl_row_reverse_cancels_back_to_left_to_right_progression() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "90px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "direction", "rtl");
    assert_style_set(&mut root, container, "flex-direction", "row-reverse");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "first rtl row-reverse item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "second rtl row-reverse item",
    );
    assert_rect_op_geometry(
        &rects[3],
        20.0,
        0.0,
        10.0,
        10.0,
        "third rtl row-reverse item",
    );
}

#[test]
// CSS Align 3 distinguishes logical `start` from flex-relative `flex-start`.
// In an LTR `row-reverse` container, `justify-content:start` should still pin
// the item group to inline-start (physical left), while `flex-start` would pin
// it to main-start (physical right).
fn justify_content_start_tracks_inline_start_in_row_reverse() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "flex-direction", "row-reverse");
    assert_style_set(&mut root, container, "justify-content", "start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        10.0,
        0.0,
        10.0,
        10.0,
        "first row-reverse start-aligned item",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        0.0,
        10.0,
        10.0,
        "second row-reverse start-aligned item",
    );
}

#[test]
// CSS Align 3 distinguishes logical `end` from flex-relative `flex-end`.
// In an LTR `row-reverse` container, `justify-content:end` should still pin
// the item group to inline-end (physical right), while `flex-end` would pin it
// to main-end (physical left).
fn justify_content_end_tracks_inline_end_in_row_reverse() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "flex-direction", "row-reverse");
    assert_style_set(&mut root, container, "justify-content", "end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        50.0,
        0.0,
        10.0,
        10.0,
        "first row-reverse end-aligned item",
    );
    assert_rect_op_geometry(
        &rects[2],
        40.0,
        0.0,
        10.0,
        10.0,
        "second row-reverse end-aligned item",
    );
}

#[test]
// CSS Flexbox 5.1 and WPT flexbox_direction-column.html: column changes the
// main axis from horizontal to vertical, so flex items progress downward in the
// block direction while staying aligned at the same cross-axis start edge.
fn column_direction_uses_vertical_main_axis_for_item_progression() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "20px", "90px");
    assert_set_inflexible_column_item(&mut root, first, "10px", "20px");
    assert_set_inflexible_column_item(&mut root, second, "10px", "20px");
    assert_set_inflexible_column_item(&mut root, third, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "column should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "column should keep the DOM order intact",
    );
    assert_close(
        assert_node_layout(&root, container).width_px,
        20.0,
        "column container width",
    );
    assert_close(
        assert_node_layout(&root, container).height_px,
        90.0,
        "column container height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 20.0, 10.0, "first column item");
    assert_rect_op_geometry(&rects[2], 0.0, 10.0, 20.0, 10.0, "second column item");
    assert_rect_op_geometry(&rects[3], 0.0, 20.0, 20.0, 10.0, "third column item");
}

#[test]
// CSS Flexbox 5.2 and WPT flexbox-flex-wrap-wrap-reverse.htm: wrap-reverse
// keeps each line's item order intact while reversing the cross-axis stacking
// order of flex lines (left-to-right items, bottom-to-top lines in
// horizontal-tb/ltr). Gecko models this as a reversed cross axis in
// nsFlexContainerFrame.cpp and Blink applies it as reversed line order plus
// flipped cross-axis placement in flex_layout_algorithm.cc.
fn wrap_reverse_stacks_wrapped_rows_from_the_opposite_cross_start() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "25px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "wrap-reverse should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "wrap-reverse should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 15.0, 10.0, 10.0, "first wrap-reverse item");
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        15.0,
        10.0,
        10.0,
        "second wrap-reverse item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        2.5,
        10.0,
        10.0,
        "third wrap-reverse item: align-content's initial `normal` behaves as `stretch` \
         (CSS Align 3 5.1.3), so the 5px leftover splits equally across the two lines \
         (CSS Flexbox 9.4 #algo-line-stretch) and the third item bottom-aligns at 2.5",
    );
}

#[test]
// CSS Align 3 `start`/`end` are logical, while `flex-start`/`flex-end` are
// flex-relative. In `wrap-reverse`, those differ on the cross axis even for a
// single-line row container. Mozilla and Igalia added a WPT for this exact
// boundary in `css/css-align/self-alignment/self-align-start-end-flex-001.html`,
// which sets `align-content: stretch` explicitly so the line fills the
// container cross size; the same geometry falls out of the stretch-behaving
// `normal` initial value here.
fn align_items_start_ignores_wrap_reverse_cross_flip() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let tall = assert_append_div_to(&mut root, container);
    let short = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "30px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "align-items", "start");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = tall, rects[2] = short.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        20.0,
        "tall sibling at logical block-start",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "short sibling at logical start",
    );
}

#[test]
// CSS Align 3 `end` stays logical even when `wrap-reverse` flips flexbox's
// cross-axis `flex-start`/`flex-end` meaning. The short item should therefore
// move to the line's logical end, not the wrap-reversed flex end.
fn align_items_end_ignores_wrap_reverse_cross_flip() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let tall = assert_append_div_to(&mut root, container);
    let short = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "30px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "align-items", "end");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = tall, rects[2] = short.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        30.0,
        10.0,
        20.0,
        "tall sibling at wrap-reverse line anchor",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "short sibling at logical end",
    );
}

#[test]
// CSS Align 3 also applies the logical/flex-relative split to `align-self`.
// In the same `wrap-reverse` row, an item-level `align-self:end` override
// should land at logical block-end while its sibling remains at logical start.
fn align_self_end_overrides_wrap_reverse_logical_start() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let tall = assert_append_div_to(&mut root, container);
    let short = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "30px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "align-items", "start");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");
    assert_style_set(&mut root, short, "align-self", "end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = tall, rects[2] = short.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 20.0, "logical-start sibling");
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "logical end override sibling",
    );
}

#[test]
// CSS Align 3 `align-content:start` packs the entire flex line stack to the
// logical cross-axis start edge. In `wrap-reverse`, that differs from
// `flex-start`, which would pack the lines toward the opposite physical side.
fn align_content_start_ignores_wrap_reverse_cross_flip() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "align-content", "start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        10.0,
        10.0,
        10.0,
        "second wrap-reversed line (first)",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        10.0,
        10.0,
        10.0,
        "second wrap-reversed line (second)",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        0.0,
        10.0,
        10.0,
        "startmost wrap-reversed line (third)",
    );
}

#[test]
// CSS Flexbox 5.1: column-reverse flips the vertical main-start and main-end
// directions without mutating DOM order. The first DOM child stays first in
// the tree, but it is laid out against the container's block-end edge and
// subsequent children proceed upward.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` sets `mIsRowOriented = false` and
//   `mIsMainAxisReversed = true` for `StyleFlexDirection::ColumnReverse`.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink runs an explicit `LayoutColumnReverse(...)` pass after flex item
//   placement for reversed vertical containers.
//   https://chromium.googlesource.com/chromium/src/+/4f00fff14d34f11e71fa0c6e5aec7c46490f5de3/third_party/blink/renderer/core/layout/ng/ng_flex_layout_algorithm.cc
fn column_reverse_positions_items_from_the_opposite_vertical_main_start() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "20px", "90px");
    assert_style_set(&mut root, container, "flex-direction", "column-reverse");
    assert_set_inflexible_column_item(&mut root, first, "10px", "20px");
    assert_set_inflexible_column_item(&mut root, second, "10px", "20px");
    assert_set_inflexible_column_item(&mut root, third, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "column-reverse should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "column-reverse should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        80.0,
        20.0,
        10.0,
        "first reversed-column item",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        70.0,
        20.0,
        10.0,
        "second reversed-column item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        60.0,
        20.0,
        10.0,
        "third reversed-column item",
    );
}

#[test]
// CSS Flexbox 5.1 plus CSS Align 3: in a column flex container, the cross
// axis is the inline axis. `align-items:flex-start` should therefore honor
// `direction: rtl` and place items against the physical right edge.
fn rtl_column_align_items_flex_start_uses_inline_start_cross_axis() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let child = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "50px", "60px");
    assert_style_set(&mut root, container, "direction", "rtl");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_column_item(&mut root, child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = child.
    assert_rect_op_geometry(
        &rects[1],
        40.0,
        0.0,
        10.0,
        10.0,
        "rtl column cross-start child",
    );
}

#[test]
// CSS Flexbox layout algorithm and WPT flex-shrink-001.html: negative free
// space is distributed by scaled shrink factors, not by equal subtraction. Two
// 40px items with shrink factors 2 and 3 in a 50px container should resolve to
// 28px and 22px because 30px of negative free space is divided by the scaled
// shrink weights 80 and 120.
fn flex_row_distributes_negative_free_space_by_scaled_shrink_factor() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, first, "40px", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "2");
    assert_set_flex_basis_and_cross_size(&mut root, second, "40px", "10px");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "flex-shrink", "3");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "shrink sizing should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        None,
        "shrink sizing should preserve the final DOM sibling link",
    );
    assert_close(
        assert_node_layout(&root, first).width_px,
        28.0,
        "first shrink item width",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        22.0,
        "second shrink item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 28.0, 10.0, "first shrink item");
    assert_rect_op_geometry(&rects[2], 28.0, 0.0, 22.0, 10.0, "second shrink item");
}

#[test]
// CSS Flexbox 7.1.1 shorthand expansion: `flex` should feed the same grow,
// shrink, and basis variables as the equivalent longhands. A `flex: 1 1 10px`
// item beside `flex: 0 0 20px` therefore grows from a 10px base to 80px in a
// 100px row.
fn flex_shorthand_distributes_positive_free_space_without_longhands() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let growing = assert_append_div_to(&mut root, container);
    let fixed = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, growing, "flex", "1 1 10px");
    assert_style_set(&mut root, growing, "height", "10px");
    assert_style_set(&mut root, fixed, "flex", "0 0 20px");
    assert_style_set(&mut root, fixed, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, growing).width_px,
        80.0,
        "flex shorthand growing item width",
    );
    assert_close(
        assert_node_layout(&root, fixed).width_px,
        20.0,
        "flex shorthand fixed item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = growing, rects[2] = fixed.
    assert_rect_op_geometry(
        &rects[2],
        80.0,
        0.0,
        20.0,
        10.0,
        "flex shorthand fixed item",
    );
}

#[test]
// CSS Flexbox 9.7 resolves flexible lengths against each item's min/max main
// size before the final pass continues. A `max-width:20px` item must freeze at
// 20px and leave the remaining positive free space to its sibling.
fn flex_item_max_width_clamps_growth_before_remaining_space_distribution() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    for child in [first, second] {
        assert_style_set(&mut root, child, "flex-basis", "10px");
        assert_style_set(&mut root, child, "flex-grow", "1");
        assert_style_set(&mut root, child, "flex-shrink", "1");
        assert_style_set(&mut root, child, "height", "10px");
    }
    assert_style_set(&mut root, first, "max-width", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).width_px,
        20.0,
        "max-width clamped item width",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        80.0,
        "sibling width after max-width freeze",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[2],
        20.0,
        0.0,
        80.0,
        10.0,
        "sibling after max-width freeze",
    );
}

#[test]
// CSS Flexbox 9.7 also clamps negative free-space resolution by min-width
// before the remaining deficit is redistributed. A `min-width:15px` item must
// freeze there, leaving the remaining 5px to its sibling inside a 20px row.
fn flex_item_min_width_clamps_shrink_before_remaining_space_distribution() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "20px");
    assert_style_set(&mut root, container, "height", "10px");
    for child in [first, second] {
        assert_style_set(&mut root, child, "flex-basis", "20px");
        assert_style_set(&mut root, child, "flex-grow", "0");
        assert_style_set(&mut root, child, "flex-shrink", "1");
        assert_style_set(&mut root, child, "height", "10px");
    }
    assert_style_set(&mut root, first, "min-width", "15px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).width_px,
        15.0,
        "min-width clamped item width",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        5.0,
        "sibling width after min-width freeze",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[2],
        15.0,
        0.0,
        5.0,
        10.0,
        "sibling after min-width freeze",
    );
}

#[test]
// CSS Flexbox layout algorithm and the wrap seam represented by WPT
// flex-wrap-002.html: wrapping is chosen from outer hypothetical main sizes,
// not from a single-row placement pass. In this row case the second 30px item
// no longer fits after the first 30px item consumes the current 50px line, so
// it starts the next line and the trailing 20px item shares that second line.
fn flex_row_wraps_items_when_the_next_outer_hypothetical_main_size_no_longer_fits() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_set_inflexible_row_item(&mut root, first, "30px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "30px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "20px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "row wrap should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "row wrap should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 10.0, "first wrapped item");
    assert_rect_op_geometry(&rects[2], 0.0, 10.0, 30.0, 10.0, "second wrapped item");
    assert_rect_op_geometry(&rects[3], 30.0, 10.0, 20.0, 10.0, "third wrapped item");
    assert_close(container_layout.height_px, 20.0, "wrapped container height");
    assert_close(
        container_layout.content_height_px,
        20.0,
        "wrapped container content height",
    );
}

#[test]
// CSS Flexbox 5.3, 5.2, and WPT flex-wrap-002.html: the flex-flow shorthand
// combines column main-axis behavior with wrapping. Once the third 10px-high
// item no longer fits in a 25px column, it starts a new column and resets to
// the cross-axis start edge of that new flex line.
fn flex_flow_column_wrap_moves_overflowing_items_into_the_next_column() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-flow", "column wrap");
    assert_style_set(&mut root, container, "width", "40px");
    assert_style_set(&mut root, container, "height", "25px");
    assert_set_inflexible_column_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_column_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_column_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "column wrap should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "column wrap should keep the DOM order intact",
    );
    assert_close(
        container_layout.width_px,
        40.0,
        "wrapped-column container width",
    );
    assert_close(
        container_layout.height_px,
        25.0,
        "wrapped-column container height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first wrapped-column item");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        10.0,
        10.0,
        10.0,
        "second wrapped-column item",
    );
    assert_rect_op_geometry(
        &rects[3],
        20.0,
        0.0,
        10.0,
        10.0,
        "third wrapped-column item: align-content's initial `normal` behaves as `stretch` \
         (CSS Align 3 5.1.3), so the 20px cross-axis leftover splits equally across the two \
         column lines and the second column starts at 20",
    );
}

#[test]
// CSS Display 3 order property, CSS Flexbox line ordering, and WPT flex-order.html:
// the visual layout uses order-modified document order, but the DOM sibling
// chain itself remains unchanged. The flex item with order 1 appears first,
// then order 2, then order 3.
fn order_changes_visual_order_without_mutating_dom_sibling_order() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let dom_first = assert_append_div_to(&mut root, container);
    let dom_second = assert_append_div_to(&mut root, container);
    let dom_third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "90px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_set_inflexible_row_item(&mut root, dom_first, "10px", "10px");
    assert_style_set(&mut root, dom_first, "order", "3");
    assert_set_inflexible_row_item(&mut root, dom_second, "10px", "10px");
    assert_style_set(&mut root, dom_second, "order", "1");
    assert_set_inflexible_row_item(&mut root, dom_third, "10px", "10px");
    assert_style_set(&mut root, dom_third, "order", "2");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, dom_first),
        Some(dom_second),
        "order should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, dom_second),
        Some(dom_third),
        "order should preserve DOM sibling links",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container; rects stage in order-modified order (flex arc step 2
    // paint-order fix), so rects[1] = dom_second (order 1), rects[2] = dom_third
    // (order 2), rects[3] = dom_first (order 3). The positions carry the same
    // order-modified visual placement.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "order 1 item (dom_second)");
    assert_rect_op_geometry(&rects[2], 10.0, 0.0, 10.0, 10.0, "order 2 item (dom_third)");
    assert_rect_op_geometry(&rects[3], 20.0, 0.0, 10.0, 10.0, "order 3 item (dom_first)");
}

#[test]
// CSS Flexbox 8.2 and WPT flexbox-justify-content-horiz-001a.xhtml:
// justify-content applies after flex item sizing and offsets the whole group
// within the remaining main-axis space. Two 10px items in a 100px row leave
// 80px free, so center alignment starts the group at 40px.
fn justify_content_center_offsets_items_after_main_axis_sizing() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "justify-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "justify-content should not rewrite DOM sibling links",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 40.0, 0.0, 10.0, 10.0, "first centered item");
    assert_rect_op_geometry(&rects[2], 50.0, 0.0, 10.0, 10.0, "second centered item");
}

#[test]
// CSS Flexbox 8.2: justify-content:flex-end places all remaining main-axis
// packing space before the first item after flex sizing is complete.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` advances its main-axis position tracker
//   by all packing space for `FLEX_END`.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: the flex alignment helpers treat `flex-end` as a leading offset in
//   the post-sizing main-axis placement step.
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn justify_content_flex_end_packs_items_to_main_end() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "justify-content", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 80.0, 0.0, 10.0, 10.0, "first flex-end item");
    assert_rect_op_geometry(&rects[2], 90.0, 0.0, 10.0, 10.0, "second flex-end item");
}

#[test]
// CSS Flexbox 8.2: justify-content:space-between distributes remaining
// main-axis space between the items, with no leading or trailing packing
// space. Three 10px items in a 100px row leave 70px free, so each inter-item
// gap becomes 35px.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` routes `SPACE_BETWEEN` through
//   `CalculatePackingSpace` in the main-axis position tracker.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: older and current flex implementations keep this in the same
//   main-axis alignment phase as `center` and `flex-end`.
//   https://chromium.googlesource.com/chromium/blink/+/d76db5d68485d8f972a1d6cb9758e1a0cd72ec34/Source/core/rendering/RenderFlexibleBox.cpp
fn justify_content_space_between_distributes_remaining_main_space_between_items() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "justify-content", "space-between");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first space-between item");
    assert_rect_op_geometry(
        &rects[2],
        45.0,
        0.0,
        10.0,
        10.0,
        "second space-between item",
    );
    assert_rect_op_geometry(&rects[3], 90.0, 0.0, 10.0, 10.0, "third space-between item");
}

#[test]
// CSS Flexbox 8.2: justify-content:space-around distributes remaining
// main-axis space so each item gets equal surrounding space, which means the
// edge gaps are half the size of the interior gaps.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` routes `SPACE_AROUND` through the shared
//   packing-space calculator and reserves one extra half-gap across the edges.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: `RenderFlexibleBox.cpp` computes the leading offset as
//   `availableFreeSpace / (2 * numberOfChildren)` and the inter-item gap as
//   `availableFreeSpace / numberOfChildren`.
//   https://chromium.googlesource.com/chromium/blink/+/d76db5d68485d8f972a1d6cb9758e1a0cd72ec34/Source/core/rendering/RenderFlexibleBox.cpp
fn justify_content_space_around_distributes_half_gaps_at_the_edges() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "justify-content", "space-around");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        35.0 / 3.0,
        0.0,
        10.0,
        10.0,
        "first space-around item",
    );
    assert_rect_op_geometry(&rects[2], 45.0, 0.0, 10.0, 10.0, "second space-around item");
    assert_rect_op_geometry(
        &rects[3],
        235.0 / 3.0,
        0.0,
        10.0,
        10.0,
        "third space-around item",
    );
}

#[test]
// CSS Flexbox 8.2: justify-content:space-evenly distributes remaining
// main-axis space into equal gaps, including the start and end edges.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` routes `SPACE_EVENLY` through the same
//   packing-space calculator as `space-around`, but reserves two full edge
//   spaces instead of one.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink follows the same equal-gap rule in its packing helpers.
fn justify_content_space_evenly_distributes_equal_gaps_including_edges() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "justify-content", "space-evenly");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 17.5, 0.0, 10.0, 10.0, "first space-evenly item");
    assert_rect_op_geometry(&rects[2], 45.0, 0.0, 10.0, 10.0, "second space-evenly item");
    assert_rect_op_geometry(&rects[3], 72.5, 0.0, 10.0, 10.0, "third space-evenly item");
}

#[test]
// CSS Flexbox 8.2 with CSS Align 3 gap properties and CSS Box 3 padding:
// justify-content packing happens inside the container content box after the
// fixed gap total is reserved. Padding shifts that packed result outward
// without changing the authored gap.
fn justify_content_center_with_gap_uses_remaining_space_inside_padded_content_box() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "80px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "padding-left", "4px");
    assert_style_set(&mut root, container, "padding-right", "4px");
    assert_style_set(&mut root, container, "column-gap", "5px");
    assert_style_set(&mut root, container, "justify-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        31.5,
        0.0,
        10.0,
        10.0,
        "first centered padded-gap item",
    );
    assert_rect_op_geometry(
        &rects[2],
        46.5,
        0.0,
        10.0,
        10.0,
        "second centered padded-gap item",
    );
}

#[test]
// CSS Flexbox 8.3 and WPT flexbox_align-items-center.html: align-items centers
// each flex item in the cross axis after line sizing. A 10px-tall item inside
// a 50px-tall row therefore sits at a top offset of 20px.
fn align_items_center_offsets_items_in_the_cross_axis() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "align-items should not rewrite DOM sibling links",
    );
    assert_close(
        container_layout.height_px,
        50.0,
        "align-items container height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        20.0,
        10.0,
        10.0,
        "first align-items center item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0,
        10.0,
        10.0,
        "second align-items center item",
    );
}

#[test]
// CSS Flexbox 8.3: align-items:flex-end places each flex item flush against
// the line cross-end edge after line sizing completes.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` advances cross-axis position by the full
//   difference between the line cross size and the item outer cross size for
//   `FLEX_END`.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: `flex_layout_algorithm.cc` keeps `kFlexEnd` in the same resolved
//   cross-axis item alignment step as `kCenter`.
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn align_items_flex_end_packs_items_to_the_cross_end() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        40.0,
        10.0,
        10.0,
        "first align-items flex-end item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "second align-items flex-end item",
    );
}

#[test]
// CSS Flexbox 8.3: align-items:stretch gives auto cross-size items the flex
// line's cross size, after subtracting their own border and padding, instead
// of only offsetting them inside the line.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` resolves stretched cross size when the
//   item uses `align-self: stretch`, has auto cross size, and no auto
//   cross-axis margins.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink applies stretch alignment to the child before final placement in the
//   flex layout algorithm.
//   https://chromium.googlesource.com/chromium/src/+/4d54f0a8e47abbf6d5d401c7d09a43ec2994d130/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn align_items_stretch_expands_auto_cross_size_items_to_line_cross_size() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "stretch");
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "flex-shrink", "0");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).height_px,
        50.0,
        "first align-items stretch item height",
    );
    assert_close(
        assert_node_layout(&root, second).height_px,
        50.0,
        "second align-items stretch item height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        50.0,
        "first align-items stretch item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        50.0,
        "second align-items stretch item",
    );
}

#[test]
// CSS Flexbox 8.3 and WPT flexbox_align-self-center.html: align-self overrides
// the container's align-items value for that one flex item. The container uses
// flex-start, but the centered item is vertically centered within the line.
fn align_self_center_overrides_container_align_items() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "align-self", "center");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "align-self should not rewrite DOM sibling links",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "flex-start item");
    assert_rect_op_geometry(&rects[2], 10.0, 20.0, 10.0, 10.0, "align-self center item");
}

#[test]
// CSS Flexbox 8.3: align-self overrides the container align-items value for a
// single item, and `flex-end` uses the same cross-end packing step as the
// container-wide `align-items:flex-end` case.
fn align_self_flex_end_overrides_container_align_items() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "align-self", "flex-end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "flex-start sibling");
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "align-self flex-end item",
    );
}

#[test]
// CSS Flexbox 8.3: align-self overrides the container's cross-axis alignment
// for one item, and `stretch` turns that item's auto cross size into the flex
// line's cross size.
fn align_self_stretch_overrides_container_align_items_for_auto_cross_size() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "height", "10px");
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "align-self", "stretch");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).height_px,
        10.0,
        "flex-start sibling keeps its authored cross size",
    );
    assert_close(
        assert_node_layout(&root, second).height_px,
        50.0,
        "align-self stretch item height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[2], 10.0, 0.0, 10.0, 50.0, "align-self stretch item");
}

#[test]
// CSS Align 3 `place-items` / `place-self` shorthands should feed the same
// cross-axis alignment variables as `align-items` / `align-self` for flex
// containers. The container's `place-items:end center` maps to cross-end
// alignment, while the item's `place-self:center auto` overrides only that
// item's cross-axis placement.
fn place_items_and_place_self_aliases_apply_cross_axis_alignment() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "place-items", "end center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "place-self", "center auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 40.0, 10.0, 10.0, "place-items first item");
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0,
        10.0,
        10.0,
        "place-self overriding item",
    );
}

#[test]
// CSS Align 3 `place-content` expands to `align-content` and
// `justify-content`. In a wrapped flex row, `place-content:center
// space-between` should center the lines in the cross axis while distributing
// each line's items with `space-between` in the main axis.
fn place_content_center_space_between_aliases_apply_to_wrapped_flex_container() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "30px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(
        &mut root,
        container,
        "place-content",
        "center space-between",
    );
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 5.0, 10.0, 10.0, "place-content first item");
    assert_rect_op_geometry(
        &rects[2],
        15.0,
        5.0,
        10.0,
        10.0,
        "place-content second item",
    );
    assert_rect_op_geometry(&rects[3], 0.0, 15.0, 10.0, 10.0, "place-content third item");
}

#[test]
// CSS Flexbox 8.4: in a wrapped flex container with definite cross size,
// align-content centers the flex lines as a group after each line's cross size
// is known. This proof pins the narrow `center` case that Gecko and Blink keep
// in their line-content placement phases.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` keeps cross-axis line packing in its
//   dedicated cross-axis alignment phase.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: `flex_layout_algorithm.cc` performs flex-line placement after line
//   sizing and flexible-length resolution.
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn align_content_center_offsets_wrapped_lines_within_container_cross_size() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        10.0,
        10.0,
        10.0,
        "first align-content center item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        10.0,
        10.0,
        10.0,
        "second align-content center item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        20.0,
        10.0,
        10.0,
        "third align-content center item",
    );
    assert_close(
        assert_node_layout(&root, container).height_px,
        40.0,
        "align-content center container height",
    );
}

#[test]
// CSS Flexbox 8.4: align-content:flex-end places all remaining cross-axis
// packing space before the first flex line once wrapped line sizes are known.
// Gecko and Blink both keep this in the same post-line-sizing cross-axis line
// placement phase as `center`.
fn align_content_flex_end_packs_wrapped_lines_to_container_cross_end() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        20.0,
        10.0,
        10.0,
        "first align-content flex-end item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0,
        10.0,
        10.0,
        "second align-content flex-end item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        30.0,
        10.0,
        10.0,
        "third align-content flex-end item",
    );
}

#[test]
// CSS Flexbox 8.4: align-content:space-between distributes the remaining
// cross-axis packing space between wrapped flex lines, with no leading packing
// space before the first line and no trailing packing space after the last.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` uses its packing-space calculation for
//   `SPACE_BETWEEN` in the cross-axis position tracker.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: older and current flex implementations keep `space-between` in the
//   same line-alignment phase via `alignContentSpaceBetweenChildren`.
//   https://chromium.googlesource.com/chromium/blink/+/d76db5d68485d8f972a1d6cb9758e1a0cd72ec34/Source/core/rendering/RenderFlexibleBox.cpp
fn align_content_space_between_distributes_remaining_cross_space_between_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "space-between");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "first align-content space-between item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "second align-content space-between item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        30.0,
        10.0,
        10.0,
        "third align-content space-between item",
    );
}

#[test]
// CSS Flexbox 8.4: align-content:space-around distributes remaining
// cross-axis space around wrapped lines, so the outer cross-axis gaps are half
// of the gap between the lines.
fn align_content_space_around_distributes_half_gaps_at_cross_edges() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "space-around");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        5.0,
        10.0,
        10.0,
        "first align-content space-around item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        5.0,
        10.0,
        10.0,
        "second align-content space-around item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        25.0,
        10.0,
        10.0,
        "third align-content space-around item",
    );
}

#[test]
// CSS Flexbox 8.4: align-content:space-evenly distributes equal cross-axis
// gaps before, between, and after wrapped lines.
fn align_content_space_evenly_distributes_equal_cross_axis_gaps() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "space-evenly");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        20.0 / 3.0,
        10.0,
        10.0,
        "first align-content space-evenly item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0 / 3.0,
        10.0,
        10.0,
        "second align-content space-evenly item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        70.0 / 3.0,
        10.0,
        10.0,
        "third align-content space-evenly item",
    );
}

#[test]
// CSS Flexbox 8.4: align-content:stretch expands wrapped flex line cross sizes
// to fill the container's definite cross size, instead of only adding a
// leading offset or inter-line spacing.
//
// Browser code:
// - Gecko: `nsFlexContainerFrame.cpp` keeps `stretch` in the wrapped-line
//   packing phase until remaining packing space is known, and a single line can
//   still stretch even when the `space-*` keywords fall back.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: `flex_layout_algorithm.cc` resolves line cross sizes before final
//   item placement, which is where wrapped-line stretch belongs.
//   https://chromium.googlesource.com/chromium/src/+/4d54f0a8e47abbf6d5d401c7d09a43ec2994d130/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn align_content_stretch_expands_wrapped_line_cross_sizes_to_fill_container() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "stretch");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "first align-content stretch item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "second align-content stretch item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        20.0,
        10.0,
        10.0,
        "third align-content stretch item",
    );
}

#[test]
// CSS 2.1 10.3.3 and CSS Flexbox 9.2: an auto-width block-level row flex
// container uses the containing block's available inline size as the definite
// main size for flex layout.
//
// Specs:
// - https://www.w3.org/TR/CSS21/visudet.html#blockwidth
// - https://www.w3.org/TR/css-flexbox-1/#line-sizing
//
// Browser code:
// - Gecko: nsFlexContainerFrame.cpp computes row-oriented main size from the
//   already-resolved inline size.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
// - Blink: FlexLayoutAlgorithm carries the constraint-space available inline
//   size into flex layout.
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
fn auto_width_block_level_row_flex_uses_containing_block_width_for_main_size() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let container = assert_append_div_to(&mut root, parent);
    let item = assert_append_div_to(&mut root, container);

    assert_style_set(&mut root, parent, "width", "90px");
    assert_style_set(&mut root, parent, "height", "10px");
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, item, "10px", "10px");
    assert_style_set(&mut root, item, "flex-grow", "1");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, container).width_px,
        90.0,
        "auto-width flex container width",
    );
    assert_close(
        assert_node_layout(&root, item).width_px,
        90.0,
        "grown item in auto-width flex container",
    );
}

#[test]
// CSS 2.1 10.3.3 gives an auto-width block box the available inline size, and
// CSS Flexbox 9.2/9.7 rebuilds flex items from the current child list before
// resolving flexible lengths. Blink carries the available inline size through
// constraint space into `FlexLayoutAlgorithm`; Gecko resolves the container's
// inline size before flex item construction in nsFlexContainerFrame. A dynamic
// row flex without an authored width must therefore use the containing block
// width on the first post-mutation pass, not fall back to placeholder geometry
// for one frame and shift on the next pass.
fn auto_width_titlebar_flex_item_count_changes_use_containing_block_width_without_delayed_shift() {
    let mut scenario = auto_width_titlebar_flex_mutation_setup();

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.container).width_px,
        120.0,
        "warm auto-width titlebar container width",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.title_host).width_px,
        88.0,
        "warm auto-width title width",
    );

    let inserted_end =
        assert_append_inflexible_titlebar_item_to(&mut scenario.root, scenario.container);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        16.0,
        DomPointerUpdate::Unchanged,
    );
    let plans = take_staged_layer_paint_plans(&scenario.root);
    let rects = &plans[0].rects;
    // rects[0] = parent, rects[1] = container, rects[2] = start, rects[3] = end,
    // rects[4] = inserted_end (title_host is a separate hidden layer).
    let first_append_pass = titlebar_mutation_probe(
        &scenario.root,
        scenario.container,
        scenario.title_host,
        rects[3].position_px[0],
    );
    assert_auto_width_titlebar_append_probe(first_append_pass);
    assert_rect_op_geometry(
        &rects[4],
        104.0,
        0.0,
        16.0,
        16.0,
        "first auto-width append pass inserted end",
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        32.0,
        DomPointerUpdate::Unchanged,
    );
    assert!(
        take_staged_layer_paint_plans(&scenario.root).is_empty(),
        "auto-width append should not shift on the second layout pass",
    );

    scenario
        .root
        .remove_node(inserted_end)
        .expect("auto-width dynamic titlebar proof should remove appended icon");

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        48.0,
        DomPointerUpdate::Unchanged,
    );
    let plans = take_staged_layer_paint_plans(&scenario.root);
    let rects = &plans[0].rects;
    // rects[0] = parent, rects[1] = container, rects[2] = start, rects[3] = end.
    let first_remove_pass = titlebar_mutation_probe(
        &scenario.root,
        scenario.container,
        scenario.title_host,
        rects[3].position_px[0],
    );
    assert_auto_width_titlebar_remove_probe(first_remove_pass);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        64.0,
        DomPointerUpdate::Unchanged,
    );
    assert!(
        take_staged_layer_paint_plans(&scenario.root).is_empty(),
        "auto-width remove should not shift on the second layout pass",
    );
}

#[test]
// CSS Flexbox 4 makes the nested `display:flex` child a flex item, and CSS
// Flexbox 9.7 resolves that item's used main size before final child layout.
//
// Specs:
// - https://www.w3.org/TR/css-flexbox-1/#flex-items
// - https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths
//
// Browser code:
// - Blink: `ConstructAndAppendFlexItems`,
//   `LineFlexer::ResolveFlexibleLengths`, then
//   `GiveItemsFinalPositionAndSize`/`BuildSpaceForLayout`.
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/line_flexer.cc
// - Gecko: `GenerateFlexItemForChild`, `FlexLine::ResolveFlexibleLengths`,
//   then final reflow in nsFlexContainerFrame.cpp.
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
//
// In ui-dom this must travel through the existing enter/measure/exit path and
// the final `relayout_tree_walker_node` placement.
fn nested_flex_item_uses_outer_grown_width_for_inner_justify_content() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "100px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "1");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_style_set(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_style_set(&mut root, sibling, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");
    assert_style_set(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "grown nested flex item width",
    );
    assert_close(
        root.layer_offset_px(sibling)
            .expect("sibling role should be a live layer root")[0],
        80.0,
        "sibling after grown nested item left",
    );
    assert_close(
        root.layer_offset_px(nested_child)
            .expect("nested child role should be a live layer root")[0],
        35.0,
        "nested child centered in grown flex item",
    );
}

#[test]
// CSS Flexbox 4/9.7: a nested `display:flex` element is still an ordinary flex
// item, and `overflow: visible` is the default. Blink applies automatic
// min-size handling during item construction rather than requiring authors to
// switch overflow modes; Gecko does the same in nsFlexContainerFrame's flex item
// generation and final reflow.
fn nested_flex_item_with_default_visible_overflow_uses_outer_grown_width() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "100px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "1");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "default-overflow nested flex item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = nested_child, rects[3] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        35.0,
        0.0,
        10.0,
        10.0,
        "default-overflow nested child centered in grown flex item",
    );
}

#[test]
// CSS Flexbox 9.9.1.2/9.9.3: a single-line flex container's min-content main
// size uses flex-item min-content contributions, and inflexible flex bases are
// preserved in the intrinsic-size calculation. This keeps a default-visible
// nested flex item from shrinking below an inflexible child basis.
//
// Source anchors:
// - CSS intrinsic main-size algorithm and flex-item intrinsic contributions:
//   https://www.w3.org/TR/css-flexbox-1/#intrinsic-main-sizes
//   https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions
// - Blink intrinsic flex container sizing:
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
// - Gecko intrinsic flex container sizing:
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
fn nested_flex_item_default_visible_overflow_uses_inflexible_child_basis_as_auto_min() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "5px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "0");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "0px", "20px");
    assert_style_set(&mut root, sibling, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        10.0,
        "default-visible nested flex item inflexible child auto-min width",
    );
}

#[test]
// CSS Flexbox `flex-basis:auto` retrieves the main-size property before falling
// back to content. Blink's `flex_basis.HasAuto()` path resolves width/height
// before final `BuildSpaceForLayout`; ui-dom must use that width as the base but
// still lay the child out at the outer resolved flexed width.
fn nested_flex_item_auto_basis_uses_width_as_base_before_final_flexed_layout() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "100px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "width", "20px");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "1");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "auto-basis nested flex item final width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = nested_child, rects[3] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        35.0,
        0.0,
        10.0,
        10.0,
        "auto-basis nested child centered after final flexed layout",
    );
}

#[test]
// CSS Flexbox 9.7 limits positive free-space distribution when flex-grow
// factors sum to less than 1. Blink's LineFlexer caps free space in this case:
// a single `flex-grow: .5` item receives half the free space, not all of it.
fn nested_flex_item_fractional_grow_consumes_only_that_fraction_of_free_space() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "100px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "0.5");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        50.0,
        "fractional-grow nested flex item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = nested_child, rects[3] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        20.0,
        0.0,
        10.0,
        10.0,
        "fractional-grow nested child",
    );
    assert_rect_op_geometry(&rects[3], 50.0, 0.0, 20.0, 20.0, "fractional-grow sibling");
}

#[test]
// CSS Flexbox automatic minimum size: visible-overflow flex items use their
// content-based minimum size before shrink. This pins the nested flex container
// case against the same Blink/Gecko auto-min item-construction phase.
fn nested_flex_item_with_visible_overflow_uses_content_based_auto_min_before_shrink() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);
    let text = assert_append_text_to(
        &mut root,
        nested_child,
        "UnbreakableContentUnbreakableContentUnbreakableContentUnbreakableContentUnbreakableContent",
    );

    assert_set_flex_row_container_width(&mut root, outer, "50px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "80px");
    assert_style_set(&mut root, nested, "flex-grow", "0");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    assert_style_set(&mut root, nested_child, "height", "10px");
    assert_style_set(&mut root, nested_child, "flex-basis", "0px");
    assert_style_set(&mut root, nested_child, "flex-grow", "0");
    assert_style_set(&mut root, nested_child, "flex-shrink", "1");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_min_width_px = assert_text_intrinsic(&root, text).max_content_width_px;
    assert!(
        text_min_width_px > 40.0,
        "proof requires text min-content wider than the unclamped shrink target; got {text_min_width_px}",
    );
    assert_close(
        assert_node_layout(&root, nested).width_px,
        text_min_width_px,
        "visible-overflow nested flex item content-based auto-min width",
    );
}

#[test]
// CSS Flexbox 9.7 shrink resolution feeds the same final child layout pass.
// Blink and Gecko both resolve flexible lengths for the item before the final
// child layout/reflow, using the same code paths cited in the growth proof.
// This catches implementations that measure the nested flex container once and
// keep the provisional measuring width instead of re-entering layout with the
// outer resolved item width.
fn nested_flex_item_uses_outer_shrunk_width_for_inner_justify_content() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "50px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "80px");
    assert_style_set(&mut root, nested, "flex-grow", "0");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_style_set(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    assert_style_set(&mut root, sibling, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");
    assert_style_set(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        40.0,
        "shrunk nested flex item width",
    );
    assert_close(
        root.layer_offset_px(sibling)
            .expect("sibling role should be a live layer root")[0],
        40.0,
        "sibling after shrunk nested item left",
    );
    assert_close(
        root.layer_offset_px(nested_child)
            .expect("nested child role should be a live layer root")[0],
        15.0,
        "nested child centered in shrunk flex item",
    );
}

#[test]
// CSS Flexbox 9.7 resolves each item's used main size from outer flex sizes,
// not by treating the flexed border-box size as a content-box width. Blink's
// `LineFlexer::ResolveFlexibleLengths`/final layout and Gecko's
// `FlexLine::ResolveFlexibleLengths`/final reflow keep that same split. A
// padded nested flex item should therefore consume its padding inside the
// resolved outer item width before its inner flex children are laid out.
//
// Source anchors:
// - CSS free-space and used-main-size resolution:
//   https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths
// - Blink flexible length resolution and final item layout:
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
// - Gecko flexible length resolution and final reflow:
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
fn padded_nested_flex_item_uses_resolved_outer_width_before_inner_layout() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let start = assert_append_div_to(&mut root, outer);
    let nested = assert_append_div_to(&mut root, outer);
    let end = assert_append_div_to(&mut root, outer);
    let nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "120px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_set_inflexible_row_item(&mut root, start, "16px", "20px");
    assert_set_inflexible_row_item(&mut root, end, "16px", "20px");
    assert_style_set(&mut root, start, "overflow", "scroll");
    assert_style_set(&mut root, end, "overflow", "scroll");

    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "0px");
    assert_style_set(&mut root, nested, "flex-grow", "1");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_style_set(&mut root, nested, "padding-left", "4px");
    assert_style_set(&mut root, nested, "padding-right", "4px");
    assert_style_set(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "20px", "10px");
    assert_style_set(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        88.0,
        "padded nested flex item outer width",
    );
    assert_close(
        root.layer_offset_px(end)
            .expect("end role should be a live layer root")[0],
        104.0,
        "end item after padded nested flex item",
    );
    assert_close(
        root.layer_offset_px(nested_child)
            .expect("nested child role should be a live layer root")[0],
        50.0,
        "nested child centered inside padded content box",
    );
}

#[test]
// CSS Flexbox 9.7 says min/max violation fixing floors the content box at
// zero. With positive padding, the final border-box main size therefore cannot
// shrink below border+padding even when overflow makes the automatic minimum
// size zero.
//
// Source anchors:
// - CSS content-box floor during flex length resolution:
//   https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths
// - Blink `LineFlexer::FreezeItems` violation clamp:
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/line_flexer.cc
// - Gecko `FlexLine::ResolveFlexibleLengths`:
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
fn padded_flex_item_floors_content_box_at_zero_after_shrink() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, outer);

    assert_set_flex_row_container_width(&mut root, outer, "4px");
    assert_style_set(&mut root, outer, "height", "10px");
    assert_style_set(&mut root, item, "height", "10px");
    assert_style_set(&mut root, item, "flex-basis", "0px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "padding-left", "4px");
    assert_style_set(&mut root, item, "padding-right", "4px");
    assert_style_set(&mut root, item, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, item).width_px,
        8.0,
        "padded flex item minimum border-box width",
    );
}

#[test]
// Gecko and Blink do not add a special "nested flex" traversal. Each flex child
// is measured as a flex item, flexible lengths are resolved for that line, and
// final child layout re-enters the ordinary layout path with resolved
// constraints. This deeper case proves ui-dom's existing enter/exit model is
// doing the work at both nested flex boundaries.
//
// Source anchors for the browser model:
// - CSS Flexbox item layout:
//   https://www.w3.org/TR/css-flexbox-1/#layout-algorithm
// - Blink flex item construction/final layout:
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/flex_layout_algorithm.cc
// - Gecko flex item construction/final reflow:
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
fn nested_flex_inside_nested_flex_uses_each_resolved_width_for_final_layout() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let middle = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let inner = assert_append_div_to(&mut root, middle);
    let leaf = assert_append_div_to(&mut root, inner);

    assert_set_flex_row_container_width(&mut root, outer, "120px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, middle, "display", "flex");
    assert_style_set(&mut root, middle, "height", "20px");
    assert_style_set(&mut root, middle, "flex-basis", "20px");
    assert_style_set(&mut root, middle, "flex-grow", "1");
    assert_style_set(&mut root, middle, "flex-shrink", "1");
    assert_style_set(&mut root, middle, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_style_set(&mut root, sibling, "overflow", "scroll");

    assert_style_set(&mut root, inner, "display", "flex");
    assert_style_set(&mut root, inner, "justify-content", "center");
    assert_style_set(&mut root, inner, "height", "20px");
    assert_style_set(&mut root, inner, "flex-basis", "20px");
    assert_style_set(&mut root, inner, "flex-grow", "1");
    assert_style_set(&mut root, inner, "flex-shrink", "1");
    assert_style_set(&mut root, inner, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, leaf, "10px", "10px");
    assert_style_set(&mut root, leaf, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, middle).width_px,
        100.0,
        "middle flex item width from outer line resolution",
    );
    assert_close(
        assert_node_layout(&root, inner).width_px,
        100.0,
        "inner flex item width from middle line resolution",
    );
    assert_close(
        root.layer_offset_px(leaf)
            .expect("leaf role should be a live layer root")[0],
        45.0,
        "leaf centered after both final layout passes",
    );
}

#[test]
// CSS Flexbox 9.7 resolves flexible lengths from the current flex-item list on
// every layout pass. Gecko rebuilds flex items for each reflow
// (`GenerateFlexItemForChild`), and Blink rebuilds line items during flex
// layout (`ConstructAndAppendFlexItems`). When a nested flex child is appended
// or removed, the first post-mutation pass should already use the current child
// list; there should not be one frame of placeholder sizing followed by a later
// shift.
fn nested_flex_item_append_and_remove_uses_current_child_list_without_delayed_shift() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let nested = assert_append_div_to(&mut root, outer);
    let sibling = assert_append_div_to(&mut root, outer);
    let first_nested_child = assert_append_div_to(&mut root, nested);

    assert_set_flex_row_container_width(&mut root, outer, "100px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, nested, "display", "flex");
    assert_style_set(&mut root, nested, "justify-content", "center");
    assert_style_set(&mut root, nested, "height", "20px");
    assert_style_set(&mut root, nested, "flex-basis", "20px");
    assert_style_set(&mut root, nested, "flex-grow", "1");
    assert_style_set(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, first_nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "warm nested flex width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = first_nested_child, rects[3] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        35.0,
        0.0,
        10.0,
        10.0,
        "warm single nested child centered",
    );

    let second_nested_child = assert_append_div_to(&mut root, nested);
    assert_set_inflexible_row_item(&mut root, second_nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 16.0, DomPointerUpdate::Unchanged);
    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "first append pass nested width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = first_nested_child,
    // rects[3] = second_nested_child, rects[4] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        30.0,
        0.0,
        10.0,
        10.0,
        "first append pass first child",
    );
    assert_rect_op_geometry(
        &rects[3],
        40.0,
        0.0,
        10.0,
        10.0,
        "first append pass second child",
    );
    assert_rect_op_geometry(
        &rects[4],
        80.0,
        0.0,
        20.0,
        20.0,
        "first append pass sibling",
    );

    assert_root_dom_update(&mut world, &mut root, 32.0, DomPointerUpdate::Unchanged);
    assert!(
        take_staged_layer_paint_plans(&root).is_empty(),
        "append should not need a second layout pass to reach stable flex geometry",
    );

    root.remove_node(second_nested_child)
        .expect("dynamic flex proof should remove appended child");

    assert_root_dom_update(&mut world, &mut root, 48.0, DomPointerUpdate::Unchanged);
    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "first remove pass nested width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = outer, rects[1] = nested, rects[2] = first_nested_child, rects[3] = sibling.
    assert_rect_op_geometry(
        &rects[2],
        35.0,
        0.0,
        10.0,
        10.0,
        "first remove pass first child",
    );
    assert_rect_op_geometry(
        &rects[3],
        80.0,
        0.0,
        20.0,
        20.0,
        "first remove pass sibling",
    );

    assert_root_dom_update(&mut world, &mut root, 64.0, DomPointerUpdate::Unchanged);
    assert!(
        take_staged_layer_paint_plans(&root).is_empty(),
        "remove should not need a second layout pass to reach stable flex geometry",
    );
}

#[test]
// CSS Align 3 gap properties and WPT flexbox-column-row-gap-001.html:
// column-gap inserts fixed space between adjacent items on the same flex line.
// Two 10px items with a 5px column gap therefore begin at 0px and 15px.
fn column_gap_separates_items_on_the_same_flex_line() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "column-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "column-gap should not rewrite DOM sibling links",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first same-line gap item");
    assert_rect_op_geometry(
        &rects[2],
        15.0,
        0.0,
        10.0,
        10.0,
        "second same-line gap item",
    );
}

#[test]
// CSS Align 3 gap properties and WPT flexbox-column-row-gap-001.html:
// row-gap inserts fixed space between wrapped flex lines. A third 10px item
// wraps below the first line in a 25px-wide, 40px-high container, and the
// stretch-behaving `normal` align-content initial (CSS Align 3 5.1.3) splits
// the 15px line-stretch leftover (40 - 20 - 5, with the inter-line gap
// subtracted per CSS Flexbox 9.4 #algo-line-stretch, Gecko's
// CrossAxisPositionTracker, and Blink's flex_layout_algorithm.cc) equally
// across the two lines, putting the second line's top at 17.5 + 5 = 22.5px.
fn row_gap_separates_wrapped_flex_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "row-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "row-gap should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "row-gap should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first wrapped-gap item");
    assert_rect_op_geometry(&rects[2], 10.0, 0.0, 10.0, 10.0, "second wrapped-gap item");
    assert_rect_op_geometry(&rects[3], 0.0, 22.5, 10.0, 10.0, "third wrapped-gap item");
}

#[test]
// CSS Align 3 gap shorthand and WPT flexbox-column-row-gap-001.html: the gap
// shorthand sets both row-gap and column-gap for flex containers. In this
// wrapped row case, `gap: 5px` produces the same geometry as `column-gap: 5px`
// plus `row-gap: 5px`, including the stretch-behaving `normal` align-content
// initial line distribution (CSS Align 3 5.1.3, CSS Flexbox 9.4
// #algo-line-stretch): the second line's top lands at 22.5px.
fn gap_shorthand_applies_row_and_column_spacing_to_wrapped_flex_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "gap shorthand should not rewrite DOM sibling links",
    );
    assert_eq!(
        next_sibling_key(&root, second),
        Some(third),
        "gap shorthand should keep the DOM order intact",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first shorthand-gap item");
    assert_rect_op_geometry(
        &rects[2],
        15.0,
        0.0,
        10.0,
        10.0,
        "second shorthand-gap item",
    );
    assert_rect_op_geometry(&rects[3], 0.0, 22.5, 10.0, 10.0, "third shorthand-gap item");
}

#[test]
// CSS Align 3 gap properties and Flexbox 9.2: when a wrapped flex line breaks,
// the next line starts with the first item's outer size, not with a phantom
// leading gap carried over from the previous line. Four 10px items with
// `column-gap: 5px` in a 25px row therefore occupy two full rows of two items
// each, not three rows.
fn column_gap_does_not_insert_a_phantom_leading_gap_on_wrapped_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);
    let fourth = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_style_set(&mut root, container, "height", "25px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, fourth, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third,
    // rects[4] = fourth.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "first phantom-gap item");
    assert_rect_op_geometry(&rects[2], 15.0, 0.0, 10.0, 10.0, "second phantom-gap item");
    assert_rect_op_geometry(&rects[3], 0.0, 15.0, 10.0, 10.0, "third phantom-gap item");
    assert_rect_op_geometry(&rects[4], 15.0, 15.0, 10.0, 10.0, "fourth phantom-gap item");
}

#[test]
// CSS Flexbox 5.2 plus CSS Align 3 gap properties: wrap-reverse still uses the
// container content box as the gap origin. Padding shifts both wrapped rows,
// while reversed line stacking keeps the earlier DOM items on the lower line.
fn wrap_reverse_gap_shorthand_uses_padded_content_box_edges() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);
    let fourth = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "28px");
    assert_style_set(&mut root, container, "height", "31px");
    assert_style_set(&mut root, container, "padding-left", "4px");
    assert_style_set(&mut root, container, "padding-right", "4px");
    assert_style_set(&mut root, container, "padding-top", "3px");
    assert_style_set(&mut root, container, "padding-bottom", "3px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, fourth, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third,
    // rects[4] = fourth.
    assert_rect_op_geometry(
        &rects[1],
        4.0,
        24.0,
        10.0,
        10.0,
        "first padded wrap-reverse item",
    );
    assert_rect_op_geometry(
        &rects[2],
        19.0,
        24.0,
        10.0,
        10.0,
        "second padded wrap-reverse item",
    );
    assert_rect_op_geometry(
        &rects[3],
        4.0,
        6.0,
        10.0,
        10.0,
        "third padded wrap-reverse item: the stretch-behaving `normal` align-content \
         initial (CSS Align 3 5.1.3) splits the 6px leftover (31 - 20 - 5, gap subtracted \
         per CSS Flexbox 9.4 #algo-line-stretch) across both lines, so the second line \
         bottom-aligns its items at 3 + 3",
    );
    assert_rect_op_geometry(
        &rects[4],
        19.0,
        6.0,
        10.0,
        10.0,
        "fourth padded wrap-reverse item",
    );
}

#[test]
// CSS Align 3 gap properties and CSS Box 3 padding edges: flex gaps are
// measured from the flex container's content box, not from its border edge, so
// container padding shifts the whole wrapped gap geometry without changing the
// authored gap sizes between items or lines.
fn gap_shorthand_uses_the_padded_content_box_as_its_origin() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "28px");
    assert_style_set(&mut root, container, "height", "31px");
    assert_style_set(&mut root, container, "padding-left", "4px");
    assert_style_set(&mut root, container, "padding-right", "4px");
    assert_style_set(&mut root, container, "padding-top", "3px");
    assert_style_set(&mut root, container, "padding-bottom", "3px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 4.0, 3.0, 10.0, 10.0, "first padded-gap item");
    assert_rect_op_geometry(&rects[2], 19.0, 3.0, 10.0, 10.0, "second padded-gap item");
    assert_rect_op_geometry(
        &rects[3],
        4.0,
        21.0,
        10.0,
        10.0,
        "third padded-gap item: the stretch-behaving `normal` align-content initial \
         (CSS Align 3 5.1.3) splits the 6px leftover (31 - 20 - 5, gap subtracted per CSS \
         Flexbox 9.4 #algo-line-stretch) across both lines, putting the second line's top \
         at 3 + 13 + 5",
    );
}

#[test]
// CSS Align 3 gap properties and CSS Box 3 padding edges: row-gap is measured
// between wrapped flex lines inside the content box. Container padding shifts
// both wrapped rows while preserving the authored line gap and the
// stretch-behaving `normal` align-content initial (CSS Align 3 5.1.3) line
// distribution.
fn row_gap_uses_the_padded_content_box_for_wrapped_line_offsets() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "28px");
    assert_style_set(&mut root, container, "height", "31px");
    assert_style_set(&mut root, container, "padding-left", "4px");
    assert_style_set(&mut root, container, "padding-right", "4px");
    assert_style_set(&mut root, container, "padding-top", "3px");
    assert_style_set(&mut root, container, "padding-bottom", "3px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "row-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(&rects[1], 4.0, 3.0, 10.0, 10.0, "first padded row-gap item");
    assert_rect_op_geometry(
        &rects[2],
        14.0,
        3.0,
        10.0,
        10.0,
        "second padded row-gap item",
    );
    assert_rect_op_geometry(
        &rects[3],
        4.0,
        21.0,
        10.0,
        10.0,
        "third padded row-gap item",
    );
}

#[test]
// CSS Align 3 gap properties and Flexbox 9.2: the gap sits between adjacent
// flex item border boxes, so each item's own padding expands its border box
// while preserving the authored inter-item gap.
fn column_gap_preserves_spacing_between_padded_item_border_boxes() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "column-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_style_set(&mut root, first, "padding-left", "4px");
    assert_style_set(&mut root, first, "padding-right", "4px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "padding-left", "4px");
    assert_style_set(&mut root, second, "padding-right", "4px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).width_px,
        18.0,
        "first padded-gap item width",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        18.0,
        "second padded-gap item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 18.0, 10.0, "first padded-gap item");
    assert_rect_op_geometry(&rects[2], 23.0, 0.0, 18.0, 10.0, "second padded-gap item");
}

#[test]
// CSS Flexbox 9.2 line collection and CSS Box margins: explicit main-axis
// margins participate in the same outer hypothetical main size that Blink and
// Gecko use for line breaking and final placement. The gap should be inserted
// after the first item's border box and margin, not before or inside them.
fn flex_item_margins_and_gap_participate_in_main_axis_offsets() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "column-gap", "7px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_style_set(&mut root, first, "margin-right", "5px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_style_set(&mut root, second, "margin-left", "3px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 10.0, 10.0, "margin+gap first item");
    assert_rect_op_geometry(&rects[2], 25.0, 0.0, 10.0, 10.0, "margin+gap second item");
}

#[test]
// CSS Flexbox 9.2: `flex-basis:auto` falls back to the main-size property when
// that property is definite. In a wrapped row, two 40px-wide items inside a
// 60px container therefore wrap because the first line consumes the definite
// 40px width from the first item before considering the second.
fn wrapped_row_auto_basis_uses_width_property_for_line_breaking() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    for child in [first, second] {
        assert_style_set(&mut root, child, "flex-basis", "auto");
        assert_style_set(&mut root, child, "flex-grow", "0");
        assert_style_set(&mut root, child, "flex-shrink", "1");
        assert_style_set(&mut root, child, "width", "40px");
        assert_style_set(&mut root, child, "height", "10px");
    }

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        40.0,
        10.0,
        "wrapped auto-basis first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        10.0,
        40.0,
        10.0,
        "wrapped auto-basis second item",
    );
}

#[test]
// CSS Flexbox 9.2 in a column container: `flex-basis:auto` uses the item's
// definite height as the flex base size when the main axis is vertical. Two
// items with heights 20px and 10px therefore stack at 0px and 20px in a 40px
// column.
fn column_auto_basis_uses_height_property_for_main_axis_layout() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "20px", "40px");
    for (child, height) in [(first, "20px"), (second, "10px")] {
        assert_style_set(&mut root, child, "flex-basis", "auto");
        assert_style_set(&mut root, child, "flex-grow", "0");
        assert_style_set(&mut root, child, "flex-shrink", "1");
        assert_style_set(&mut root, child, "width", "20px");
        assert_style_set(&mut root, child, "height", height);
    }

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        "column auto-basis first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        20.0,
        20.0,
        10.0,
        "column auto-basis second item",
    );
}

#[test]
// CSS Flexbox automatic minimum size and the browser behavior represented by
// WPT flexbox-min-width-auto-001.html: a scroll container resolves automatic
// minimum main size to zero, so the flex item may shrink all the way to the
// container width instead of being held open by its unbreakable text.
fn scroll_flex_item_resolves_automatic_min_main_size_to_zero_before_shrink() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");

    assert_set_flex_row_container_width(&mut root, container, "20px");
    assert_style_set(&mut root, item, "overflow", "scroll");
    assert_style_set(&mut root, item, "height", "10px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "flex-basis", "auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let container_layout = assert_node_layout(&root, container);
    let item_layout = assert_node_layout(&root, item);

    assert_eq!(
        next_sibling_key(&root, item),
        None,
        "scroll auto-min sizing should preserve the final DOM sibling link",
    );
    assert!(
        text_intrinsic.max_content_width_px > 20.0,
        "scroll min-size proof requires unbreakable text wider than the container; got {}",
        text_intrinsic.max_content_width_px,
    );
    assert_close(
        container_layout.width_px,
        20.0,
        "scroll min-size container width",
    );
    assert_close(item_layout.width_px, 20.0, "scroll flex item width");
    assert_close(
        root.layer_offset_px(item)
            .expect("item role should be a live layer root")[1],
        0.0,
        "scroll flex item top",
    );
}

#[test]
// CSS Flexbox automatic minimum size and WPT flexbox-min-width-auto-001.html:
// when the flex item keeps visible overflow, the automatic minimum main size
// stays content-based and constrains later shrink behavior. This unbreakable
// text case uses the text intrinsic max-content width as a stable proxy for
// the item's content-based minimum width.
fn visible_overflow_flex_item_uses_content_based_automatic_min_main_size_before_shrink() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");

    assert_set_flex_row_container_width(&mut root, container, "20px");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_style_set(&mut root, item, "height", "10px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "flex-basis", "auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let container_layout = assert_node_layout(&root, container);
    let item_layout = assert_node_layout(&root, item);

    assert_eq!(
        next_sibling_key(&root, item),
        None,
        "visible-overflow auto-min sizing should preserve the final DOM sibling link",
    );
    assert!(
        text_intrinsic.max_content_width_px > 20.0,
        "visible-overflow min-size proof requires unbreakable text wider than the container; got {}",
        text_intrinsic.max_content_width_px,
    );
    assert_close(
        container_layout.width_px,
        20.0,
        "visible-overflow min-size container width",
    );
    assert_close(
        item_layout.width_px,
        text_intrinsic.max_content_width_px,
        "visible-overflow flex item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item; the text child emits a text run, not a rect.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        item_layout.width_px,
        10.0,
        "visible-overflow flex item",
    );
}

#[test]
// CSS Flexbox automatic minimum size in the block axis (flex arc step 4, D3):
// a column flex item's automatic minimum main size is its v_min — the
// min-content block size folded from its children. A 30px-tall SPECIFIED-height
// nested block has an empty content sum (its specified height is a definite used
// size, not an intrinsic, per D3), so the item's v_min is 0 and it shrinks to
// the 20px container rather than its former measured height.
fn visible_overflow_column_flex_item_uses_v_min_before_shrink() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);
    let nested = assert_append_div_to(&mut root, item);

    assert_set_flex_column_container_size(&mut root, container, "20px", "20px");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_set_flex_column_basis_and_cross_size(&mut root, item, "30px", "20px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, nested, "width", "20px");
    assert_style_set(&mut root, nested, "height", "30px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, item),
        None,
        "column visible-overflow auto-min sizing should preserve the final DOM sibling link",
    );
    assert_close(
        assert_node_layout(&root, container).height_px,
        20.0,
        "column visible-overflow min-size container height",
    );
    // D3: the item's v_min is its content sum — the nested 30px div is a
    // specified-height block (v_min 0), so the item shrinks to the 20px
    // container, not its former 30px measured height.
    assert_close(
        assert_node_layout(&root, item).height_px,
        20.0,
        "column visible-overflow flex item height",
    );
    assert_close(
        assert_node_layout(&root, item).width_px,
        20.0,
        "column visible-overflow flex item width",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item, rects[2] = nested.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        "column visible-overflow flex item",
    );
}

#[test]
// Titlebar-shaped flex canary: fixed-width icon groups on the start and end
// sides should leave the remaining main-axis space for the middle title host.
// This exercises the same production pattern as ui-components titlebars while
// emitting a width report for multiple icon-count combinations under
// `--nocapture`.
fn titlebar_like_flex_reports_fixed_icon_and_growing_title_widths() {
    let cases = [
        TitlebarFlexCase {
            name: "one_start_one_end",
            container_width_px: 120.0,
            start_icon_count: 1,
            end_icon_count: 1,
            expected_title_width_px: 88.0,
        },
        TitlebarFlexCase {
            name: "two_start_one_end",
            container_width_px: 120.0,
            start_icon_count: 2,
            end_icon_count: 1,
            expected_title_width_px: 72.0,
        },
        TitlebarFlexCase {
            name: "one_start_two_end",
            container_width_px: 120.0,
            start_icon_count: 1,
            end_icon_count: 2,
            expected_title_width_px: 72.0,
        },
        TitlebarFlexCase {
            name: "two_start_two_end",
            container_width_px: 120.0,
            start_icon_count: 2,
            end_icon_count: 2,
            expected_title_width_px: 56.0,
        },
        TitlebarFlexCase {
            name: "three_start_two_end",
            container_width_px: 120.0,
            start_icon_count: 3,
            end_icon_count: 2,
            expected_title_width_px: 40.0,
        },
        TitlebarFlexCase {
            name: "two_start_two_end_wide",
            container_width_px: 160.0,
            start_icon_count: 2,
            end_icon_count: 2,
            expected_title_width_px: 96.0,
        },
    ];
    let mut report_rows = Vec::with_capacity(cases.len());

    for case in cases {
        let report = measured_titlebar_flex_case(case);
        assert_close(
            report.container_width_px,
            case.container_width_px,
            "titlebar canary container width",
        );
        assert_close(
            report.title_width_px,
            case.expected_title_width_px,
            "titlebar canary title width",
        );
        assert_close(
            report.title_left_px,
            TITLEBAR_ICON_WIDTH_PX * non_negative_f32_from_usize(case.start_icon_count),
            "titlebar canary title left",
        );
        assert_close(
            report.title_right_px,
            case.container_width_px
                - (TITLEBAR_ICON_WIDTH_PX * non_negative_f32_from_usize(case.end_icon_count)),
            "titlebar canary title right",
        );
        assert_widths_close(
            &report.start_icon_widths_px,
            &vec![TITLEBAR_ICON_WIDTH_PX; case.start_icon_count],
            "titlebar canary start icon widths",
        );
        assert_widths_close(
            &report.end_icon_widths_px,
            &vec![TITLEBAR_ICON_WIDTH_PX; case.end_icon_count],
            "titlebar canary end icon widths",
        );
        assert_titlebar_icon_positions(case, &report);
        assert_titlebar_cross_axis_tops(&report);
        report_rows.push(report);
    }

    println!("titlebar_flex_width_report");
    for row in report_rows {
        println!(
            "case={} container_width_px={} item_widths_px={:?} start_icon_widths_px={:?} title_width_px={} end_icon_widths_px={:?}",
            row.case_name,
            row.container_width_px,
            row.item_widths_px,
            row.start_icon_widths_px,
            row.title_width_px,
            row.end_icon_widths_px,
        );
    }
}

#[test]
// CSS Flexbox 9.2/9.7 rebuilds the flex item list from the live child list
// before resolving flexible lengths. Blink and Gecko do not preserve a stale
// previous item set across child insert/remove; the first layout after a
// titlebar item count change should already give the growing title host its
// new width.
fn titlebar_like_flex_item_count_changes_do_not_disappear_or_shift_on_second_pass() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let start = assert_append_div_to(&mut root, container);
    let title_host = assert_append_div_to(&mut root, container);
    let end = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "120px");
    assert_style_set(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_inflexible_row_item(
        &mut root,
        start,
        TITLEBAR_ICON_BASIS_PX,
        TITLEBAR_ICON_HEIGHT_PX,
    );
    assert_style_set(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_style_set(&mut root, title_host, "flex-basis", "0px");
    assert_style_set(&mut root, title_host, "flex-grow", "1");
    assert_style_set(&mut root, title_host, "flex-shrink", "1");
    assert_style_set(&mut root, title_host, "overflow", "hidden");
    let _title_text = assert_append_text_to(&mut root, title_host, "Camera");
    assert_set_inflexible_row_item(
        &mut root,
        end,
        TITLEBAR_ICON_BASIS_PX,
        TITLEBAR_ICON_HEIGHT_PX,
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        assert_node_layout(&root, title_host).width_px,
        88.0,
        "warm title width",
    );

    let inserted_end = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(
        &mut root,
        inserted_end,
        TITLEBAR_ICON_BASIS_PX,
        TITLEBAR_ICON_HEIGHT_PX,
    );

    assert_root_dom_update(&mut world, &mut root, 16.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = start, rects[2] = end, rects[3] = inserted_end
    // (title_host is a separate hidden layer).
    let first_append_pass =
        titlebar_mutation_probe(&root, container, title_host, rects[2].position_px[0]);
    assert_close(
        first_append_pass.container_width_px,
        120.0,
        "first append pass container width",
    );
    assert_close(
        first_append_pass.title_width_px,
        72.0,
        "first append pass title width",
    );
    assert_close(
        first_append_pass.end_left_px,
        88.0,
        "first append pass original end left",
    );
    assert_rect_op_geometry(
        &rects[3],
        104.0,
        0.0,
        16.0,
        16.0,
        "first append pass inserted end",
    );

    assert_root_dom_update(&mut world, &mut root, 32.0, DomPointerUpdate::Unchanged);
    assert!(
        take_staged_layer_paint_plans(&root).is_empty(),
        "titlebar append should not shift on the second layout pass",
    );

    root.remove_node(inserted_end)
        .expect("dynamic titlebar proof should remove appended icon");

    assert_root_dom_update(&mut world, &mut root, 48.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = start, rects[2] = end.
    let first_remove_pass =
        titlebar_mutation_probe(&root, container, title_host, rects[2].position_px[0]);
    assert_close(
        first_remove_pass.container_width_px,
        120.0,
        "first remove pass container width",
    );
    assert_close(
        first_remove_pass.title_width_px,
        88.0,
        "first remove pass title width",
    );
    assert_close(
        first_remove_pass.end_left_px,
        104.0,
        "first remove pass original end left",
    );

    assert_root_dom_update(&mut world, &mut root, 64.0, DomPointerUpdate::Unchanged);
    assert!(
        take_staged_layer_paint_plans(&root).is_empty(),
        "titlebar remove should not shift on the second layout pass",
    );
}

#[test]
// The recent main-axis packing additions must be reachable through stylesheets
// too. Here the container class carries both `justify-content: space-between`
// and `column-gap`, and that packing should survive a later style recompute.
fn stylesheet_justify_content_space_between_and_column_gap_persist_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .toolbar-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: space-between;
                column-gap: 4px;
                width: 90px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "toolbar-row");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        40.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        80.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content third item",
    );
}

#[test]
// CSS Flexbox 9.2 line collection and CSS Align 3 gap handling should keep
// adjacent auto-sized block flex items on the same row when the container has
// definite inline size and positive free space remains. This uses a nested
// block wrapper around one `display:flex` row with `gap: 12px` and two block
// columns containing text rows. Gecko/Blink reach this through
// nsFlexContainerFrame line construction and Blink's flex line breaker plus
// line flexer phases.
fn stylesheet_nested_two_column_row_keeps_auto_sized_flex_items() {
    let scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::Stylesheet { gap_px: Some(12.0) },
        TwoColumnRowWidthMode::Auto,
    );

    assert_two_auto_sized_flex_columns(scenario);
}

#[test]
// This variant removes stylesheet recompute from the equation. If it fails the
// same way, the bug is in core flex item measurement/placement rather than
// class-driven style application.
fn direct_attr_nested_two_column_row_keeps_auto_sized_flex_items() {
    let scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::DirectAttrs { gap_px: Some(12.0) },
        TwoColumnRowWidthMode::Auto,
    );

    assert_two_auto_sized_flex_columns(scenario);
}

#[test]
// This keeps the nested two-column row subtree and auto-sized columns but removes `gap` so
// the only requirement is that the second auto-sized flex item remain present
// and start after the first.
fn stylesheet_nested_two_column_row_keeps_auto_sized_flex_items_without_gap() {
    let scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::Stylesheet { gap_px: None },
        TwoColumnRowWidthMode::Auto,
    );

    assert_two_auto_sized_flex_columns(scenario);
}

#[test]
// This variant pins whether the collapse is specific to auto-sized flex items.
// If explicit widths survive while auto widths collapse, the bug boundary is in
// the browser flex base-size / hypothetical main-size path for block flex
// items, not generic line placement.
fn stylesheet_nested_two_column_row_applies_gap_between_explicit_width_items() {
    let mut scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::Stylesheet { gap_px: Some(12.0) },
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.left_column)
            .expect("left column role should expose a box model")
            .border_box
            .left_px,
        0.0,
        "explicit nested row left column left",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.left_column).width_px,
        96.0,
        "explicit nested row left column width",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.right_column).width_px,
        104.0,
        "explicit nested row right column width",
    );
    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "explicit nested row right column left",
    );
}

#[test]
// This keeps explicit widths but removes the authored gap. If this passes while
// the gap variant fails, the explicit-width path is reaching sibling placement
// but skipping gap insertion.
fn stylesheet_nested_two_column_row_places_explicit_width_items_without_gap() {
    let mut scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::Stylesheet { gap_px: None },
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.left_column)
            .expect("left column role should expose a box model")
            .border_box
            .left_px,
        0.0,
        "explicit no-gap nested row left column left",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.left_column).width_px,
        96.0,
        "explicit no-gap nested row left column width",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.right_column).width_px,
        104.0,
        "explicit no-gap nested row right column width",
    );
    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        96.0,
        "explicit no-gap nested row right column left",
    );
}

#[test]
// This confirms whether the explicit-width + gap miss is independent of
// stylesheet application.
fn direct_attr_nested_two_column_row_applies_gap_between_explicit_width_items() {
    let mut scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::DirectAttrs { gap_px: Some(12.0) },
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        assert_node_layout(&scenario.root, scenario.left_column).width_px,
        96.0,
        "direct explicit nested row left column width",
    );
    assert_close(
        assert_node_layout(&scenario.root, scenario.right_column).width_px,
        104.0,
        "direct explicit nested row right column width",
    );
    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "direct explicit nested row right column left",
    );
}

#[test]
// This is the direct-attr control that matches the passing stylesheet no-gap
// case. It confirms the nested two-column row subtree can place two explicit-width
// flex items adjacently when no gap insertion is required.
fn direct_attr_nested_two_column_row_places_explicit_width_items_without_gap() {
    let mut scenario = build_nested_two_column_row_scenario(
        TwoColumnRowStyleMode::DirectAttrs { gap_px: None },
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        96.0,
        "direct explicit no-gap nested row right column left",
    );
}

#[test]
// This swaps the shorthand out for `column-gap` in the same nested subtree. If
// it fails the same way as `gap`, the issue is in horizontal gap insertion at
// placement time, not in shorthand parsing.
fn direct_attr_nested_two_column_row_applies_column_gap_between_explicit_width_items() {
    let mut scenario = build_nested_two_column_row_scenario_with_gap_attr(
        GapAttrMode::ColumnGap(12.0),
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
        TwoColumnRowContentMode::Rows,
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "direct explicit column-gap nested row right column left",
    );
}

#[test]
// This removes the descendant block rows from both flex items but keeps the
// same nested ancestor wrappers, explicit widths, and gap. If it still
// fails, the gap bug is ancestor/subtree-scoped rather than child-content
// scoped.
fn direct_attr_nested_two_column_row_applies_gap_between_empty_explicit_width_items() {
    let mut scenario = build_nested_two_column_row_scenario_with_gap_attr(
        GapAttrMode::Gap(12.0),
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
        TwoColumnRowContentMode::Empty,
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "direct explicit empty-column nested row right column left",
    );
}

#[test]
// This keeps auto widths but replaces the block-row subtree with direct text
// children inside each flex item. If this passes, the auto-width collapse is
// tied to block descendants inside the flex items rather than all intrinsic
// inline content.
fn direct_attr_nested_two_column_row_keeps_auto_width_items_with_direct_text() {
    let mut scenario = build_nested_two_column_row_scenario_with_gap_attr(
        GapAttrMode::Gap(12.0),
        TwoColumnRowWidthMode::Auto,
        TwoColumnRowContentMode::DirectText,
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert!(
        assert_node_layout(&scenario.root, scenario.left_column).width_px > 0.0,
        "direct-text nested row left column should keep non-zero auto width"
    );
    assert!(
        assert_node_layout(&scenario.root, scenario.right_column).width_px > 0.0,
        "direct-text nested row right column should keep non-zero auto width"
    );
}

#[test]
// This flattens the same explicit-width + gap case to a top-level flex
// container. If this passes while the nested wrapper subtree fails, the gap bug is
// tied to the nested block-wrapper path rather than generic sibling placement.
fn direct_attr_flat_two_column_row_applies_gap_between_explicit_width_items() {
    let mut scenario = build_flat_two_column_row_scenario(
        GapAttrMode::Gap(12.0),
        TwoColumnRowWidthMode::Explicit {
            left_width: "96px",
            right_width: "104px",
        },
        TwoColumnRowContentMode::Rows,
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert_close(
        node_box(&scenario.root, scenario.right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "flat explicit two-column row right column left",
    );
}

#[test]
// This is the flat control for the auto-width case. It uses direct text inside
// each flex item so the measurement path does not have to aggregate descendant
// block rows. If this still collapses, the auto-width bug is broader than the
// nested wrapper subtree.
fn direct_attr_flat_two_column_row_keeps_auto_width_items_with_direct_text() {
    let mut scenario = build_flat_two_column_row_scenario(
        GapAttrMode::Gap(12.0),
        TwoColumnRowWidthMode::Auto,
        TwoColumnRowContentMode::DirectText,
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    assert!(
        assert_node_layout(&scenario.root, scenario.left_column).width_px > 0.0,
        "flat direct-text left column should keep non-zero auto width"
    );
    assert!(
        assert_node_layout(&scenario.root, scenario.right_column).width_px > 0.0,
        "flat direct-text right column should keep non-zero auto width"
    );
}

#[test]
// Existing generic gap proofs use flex-basis-sized items. This flat control
// keeps the same two-column structure but sizes the items through flex-basis
// instead of width. If it passes while the width-sized variant fails, the gap
// boundary is specifically the width/content-sized item path.
fn direct_attr_flat_two_column_row_applies_gap_between_flex_basis_sized_items() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let columns = assert_append_div_to(&mut root, document_root);
    let left_column = assert_append_div_to(&mut root, columns);
    let right_column = assert_append_div_to(&mut root, columns);

    assert_style_set(&mut root, columns, "display", "flex");
    assert_style_set(&mut root, columns, "width", "320px");
    assert_style_set(&mut root, columns, "height", "200px");
    assert_style_set(&mut root, columns, "gap", "12px");
    assert_style_set(&mut root, left_column, "display", "block");
    assert_style_set(&mut root, right_column, "display", "block");
    assert_style_set(&mut root, left_column, "flex-basis", "96px");
    assert_style_set(&mut root, right_column, "flex-basis", "104px");
    assert_style_set(&mut root, left_column, "flex-grow", "0");
    assert_style_set(&mut root, left_column, "flex-shrink", "0");
    assert_style_set(&mut root, right_column, "flex-grow", "0");
    assert_style_set(&mut root, right_column, "flex-shrink", "0");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        node_box(&root, right_column)
            .expect("right column role should expose a box model")
            .border_box
            .left_px,
        108.0,
        "flat flex-basis right column left",
    );
}

#[test]
// Stylesheet coverage should also prove that switching to a conflicting class
// on the same node really changes the packing mode instead of leaving the old
// cached layout in place.
fn switching_stylesheet_justify_content_class_changes_packing_mode() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .space-between-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: space-between;
                width: 90px;
                height: 10px;
            }
            .flex-end-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: flex-end;
                width: 90px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "space-between-row");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[2],
        40.0,
        0.0,
        10.0,
        10.0,
        "space-between second item before class switch",
    );

    assert_set_attr_str(&mut root, container, "class", "flex-end-row");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        60.0,
        0.0,
        10.0,
        10.0,
        "first item after switching to flex-end class",
    );
    assert_rect_op_geometry(
        &rects[2],
        70.0,
        0.0,
        10.0,
        10.0,
        "second item after switching to flex-end class",
    );
    assert_rect_op_geometry(
        &rects[3],
        80.0,
        0.0,
        10.0,
        10.0,
        "third item after switching to flex-end class",
    );
}

fn append_two_column_row_measurement_rows(
    root: &mut RootDom,
    column: DomNodeSlotKey,
    rows: &[(&str, &str)],
) {
    for (label, value) in rows {
        append_two_column_row_measurement_row(root, column, label, value);
    }
}

fn append_two_column_row_measurement_row(
    root: &mut RootDom,
    column: DomNodeSlotKey,
    label: &str,
    value: &str,
) {
    let row = assert_append_div_to(root, column);
    let label_node = assert_append_span_to(root, row);
    let value_node = assert_append_span_to(root, row);
    assert_append_text_to(root, label_node, label);
    assert_append_text_to(root, value_node, value);
    assert_set_attr_str(root, row, "class", "two-column-measure-row");
    assert_set_attr_str(root, label_node, "class", "two-column-measure-label");
    assert_set_attr_str(root, value_node, "class", "two-column-measure-value");
}

#[derive(Clone, Copy)]
enum TwoColumnRowStyleMode {
    Stylesheet { gap_px: Option<f32> },
    DirectAttrs { gap_px: Option<f32> },
}

#[derive(Clone, Copy)]
enum TwoColumnRowWidthMode<'a> {
    Auto,
    Explicit {
        left_width: &'a str,
        right_width: &'a str,
    },
}

#[derive(Clone, Copy)]
enum TwoColumnRowContentMode {
    Rows,
    Empty,
    DirectText,
}

#[derive(Clone, Copy)]
enum GapAttrMode {
    Gap(f32),
    ColumnGap(f32),
}

/// The six element keys one nested two-column row scenario builds, passed as
/// one context object to keep helper signatures inside the workspace
/// pedantic-clippy argument limit.
struct TwoColumnRowElementKeys {
    right: DomNodeSlotKey,
    detail: DomNodeSlotKey,
    points: DomNodeSlotKey,
    columns: DomNodeSlotKey,
    left_column: DomNodeSlotKey,
    right_column: DomNodeSlotKey,
}

struct TwoColumnRowScenario {
    world: bevy::prelude::World,
    root: RootDom,
    left_column: DomNodeSlotKey,
    right_column: DomNodeSlotKey,
    gap_px: f32,
}

fn build_nested_two_column_row_scenario(
    style_mode: TwoColumnRowStyleMode,
    width_mode: TwoColumnRowWidthMode<'_>,
) -> TwoColumnRowScenario {
    build_nested_two_column_row_scenario_inner(
        style_mode,
        width_mode,
        TwoColumnRowContentMode::Rows,
    )
}

fn build_nested_two_column_row_scenario_with_gap_attr(
    gap_attr_mode: GapAttrMode,
    width_mode: TwoColumnRowWidthMode<'_>,
    content_mode: TwoColumnRowContentMode,
) -> TwoColumnRowScenario {
    let gap_px = gap_attr_mode_value(gap_attr_mode);
    let style_mode = TwoColumnRowStyleMode::DirectAttrs { gap_px: None };
    let mut scenario =
        build_nested_two_column_row_scenario_inner(style_mode, width_mode, content_mode);
    apply_gap_attr_mode(
        &mut scenario.root,
        scenario.left_column,
        scenario.right_column,
        gap_attr_mode,
    );
    scenario.gap_px = gap_px;
    scenario
}

fn build_flat_two_column_row_scenario(
    gap_attr_mode: GapAttrMode,
    width_mode: TwoColumnRowWidthMode<'_>,
    content_mode: TwoColumnRowContentMode,
) -> TwoColumnRowScenario {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let columns = assert_append_div_to(&mut root, document_root);
    let left_column = assert_append_div_to(&mut root, columns);
    let right_column = assert_append_div_to(&mut root, columns);
    let gap_px = gap_attr_mode_value(gap_attr_mode);

    assert_style_set(&mut root, columns, "display", "flex");
    assert_style_set(&mut root, columns, "width", "320px");
    assert_style_set(&mut root, columns, "height", "200px");
    match gap_attr_mode {
        GapAttrMode::Gap(value) => {
            assert_style_set(&mut root, columns, "gap", &format!("{value}px"));
        }
        GapAttrMode::ColumnGap(value) => {
            assert_style_set(&mut root, columns, "column-gap", &format!("{value}px"));
        }
    }
    assert_style_set(&mut root, left_column, "display", "block");
    assert_style_set(&mut root, right_column, "display", "block");
    apply_two_column_row_width_mode(&mut root, left_column, right_column, width_mode);
    append_two_column_row_content(&mut root, left_column, right_column, content_mode);

    TwoColumnRowScenario {
        world,
        root,
        left_column,
        right_column,
        gap_px,
    }
}

fn build_nested_two_column_row_scenario_inner(
    style_mode: TwoColumnRowStyleMode,
    width_mode: TwoColumnRowWidthMode<'_>,
    content_mode: TwoColumnRowContentMode,
) -> TwoColumnRowScenario {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let right = assert_append_div_to(&mut root, document_root);
    let detail = assert_append_div_to(&mut root, right);
    let points = assert_append_div_to(&mut root, detail);
    let columns = assert_append_div_to(&mut root, points);
    let left_column = assert_append_div_to(&mut root, columns);
    let right_column = assert_append_div_to(&mut root, columns);
    let gap_px = style_mode_gap_px(&style_mode);

    apply_two_column_row_style_mode(
        &mut root,
        &TwoColumnRowElementKeys {
            right,
            detail,
            points,
            columns,
            left_column,
            right_column,
        },
        &style_mode,
    );
    apply_two_column_row_width_mode(&mut root, left_column, right_column, width_mode);
    append_two_column_row_content(&mut root, left_column, right_column, content_mode);

    TwoColumnRowScenario {
        world,
        root,
        left_column,
        right_column,
        gap_px,
    }
}

fn apply_two_column_row_style_mode(
    root: &mut RootDom,
    keys: &TwoColumnRowElementKeys,
    style_mode: &TwoColumnRowStyleMode,
) {
    match style_mode {
        TwoColumnRowStyleMode::Stylesheet { gap_px } => {
            assert_load_root_stylesheet(
                root,
                theme_stylesheet_origin(),
                &two_column_row_stylesheet_css(*gap_px),
            );
            assert_set_attr_str(root, keys.right, "class", "two-column-host");
            assert_set_attr_str(root, keys.detail, "class", "two-column-detail");
            assert_set_attr_str(root, keys.points, "class", "two-column-block");
            assert_set_attr_str(root, keys.columns, "class", "two-column-row");
            assert_set_attr_str(root, keys.left_column, "class", "two-column-item");
            assert_set_attr_str(root, keys.right_column, "class", "two-column-item");
        }
        TwoColumnRowStyleMode::DirectAttrs { gap_px } => {
            assert_style_set(root, keys.right, "display", "block");
            assert_style_set(root, keys.right, "width", "320px");
            assert_style_set(root, keys.right, "height", "200px");
            assert_style_set(root, keys.right, "overflow", "scroll");
            assert_style_set(root, keys.detail, "display", "block");
            assert_style_set(root, keys.points, "display", "block");
            assert_style_set(root, keys.columns, "display", "flex");
            if let Some(gap_px) = gap_px {
                assert_style_set(root, keys.columns, "gap", &format!("{gap_px}px"));
            }
            assert_style_set(root, keys.left_column, "display", "block");
            assert_style_set(root, keys.right_column, "display", "block");
        }
    }
}

fn append_two_column_row_content(
    root: &mut RootDom,
    left_column: DomNodeSlotKey,
    right_column: DomNodeSlotKey,
    content_mode: TwoColumnRowContentMode,
) {
    match content_mode {
        TwoColumnRowContentMode::Rows => {
            append_two_column_row_measurement_rows(
                root,
                left_column,
                &[
                    ("Mood", "100"),
                    ("Social", "95"),
                    ("Recreation", "90"),
                    ("Hunger", "85"),
                    ("Thirst", "80"),
                    ("Restroom", "75"),
                    ("Exhaustion", "70"),
                    ("Sleep", "65"),
                ],
            );
            append_two_column_row_measurement_rows(
                root,
                right_column,
                &[
                    ("Cleanliness", "60"),
                    ("Oxygen", "55"),
                    ("Temperature", "50"),
                    ("Radiation", "45"),
                ],
            );
        }
        TwoColumnRowContentMode::Empty => {}
        TwoColumnRowContentMode::DirectText => {
            assert_append_text_to(root, left_column, "Mood 100 Social 95");
            assert_append_text_to(root, right_column, "Cleanliness 60 Oxygen 55");
        }
    }
}

fn apply_gap_attr_mode(
    root: &mut RootDom,
    left_column: DomNodeSlotKey,
    right_column: DomNodeSlotKey,
    gap_attr_mode: GapAttrMode,
) {
    let columns = root
        .node(left_column)
        .expect("left column should stay live")
        .parent_key()
        .expect("left column should keep columns parent");
    let right_parent = root
        .node(right_column)
        .expect("right column should stay live")
        .parent_key()
        .expect("right column should keep columns parent");
    assert_eq!(
        columns, right_parent,
        "two-column row siblings should share one parent"
    );
    match gap_attr_mode {
        GapAttrMode::Gap(value) => {
            assert_style_set(root, columns, "gap", &format!("{value}px"));
        }
        GapAttrMode::ColumnGap(value) => {
            assert_style_set(root, columns, "column-gap", &format!("{value}px"));
        }
    }
}

fn gap_attr_mode_value(gap_attr_mode: GapAttrMode) -> f32 {
    match gap_attr_mode {
        GapAttrMode::Gap(value) | GapAttrMode::ColumnGap(value) => value,
    }
}

fn apply_two_column_row_width_mode(
    root: &mut RootDom,
    left_column: DomNodeSlotKey,
    right_column: DomNodeSlotKey,
    width_mode: TwoColumnRowWidthMode<'_>,
) {
    if let TwoColumnRowWidthMode::Explicit {
        left_width,
        right_width,
    } = width_mode
    {
        assert_style_set(root, left_column, "width", left_width);
        assert_style_set(root, right_column, "width", right_width);
    }
}

fn two_column_row_stylesheet_css(gap_px: Option<f32>) -> String {
    let gap_rule = gap_px
        .map(|value| format!("gap: {value}px;"))
        .unwrap_or_default();
    format!(
        r"
            .two-column-host {{
                display: block;
                width: 320px;
                height: 200px;
                overflow: scroll;
            }}
            .two-column-detail {{
                display: block;
                color: #f2f2f2;
            }}
            .two-column-block {{
                display: block;
            }}
            .two-column-row {{
                display: flex;
                {gap_rule}
            }}
            .two-column-item {{
                display: block;
                padding-left: 0px;
                padding-right: 0px;
            }}
            .two-column-measure-row {{
                display: block;
                margin-bottom: 8px;
            }}
            .two-column-measure-label {{
                color: #9fb1c3;
            }}
            .two-column-measure-value {{
                margin-left: 10px;
                color: #f2f2f2;
            }}
        "
    )
}

fn style_mode_gap_px(style_mode: &TwoColumnRowStyleMode) -> f32 {
    match style_mode {
        TwoColumnRowStyleMode::Stylesheet { gap_px }
        | TwoColumnRowStyleMode::DirectAttrs { gap_px } => gap_px.unwrap_or(0.0),
    }
}

fn assert_two_auto_sized_flex_columns(mut scenario: TwoColumnRowScenario) {
    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let left_column_left_px = node_box(&scenario.root, scenario.left_column)
        .expect("left column role should expose a box model")
        .border_box
        .left_px;
    let right_column_left_px = node_box(&scenario.root, scenario.right_column)
        .expect("right column role should expose a box model")
        .border_box
        .left_px;
    let left_column_width_px = assert_node_layout(&scenario.root, scenario.left_column).width_px;
    let right_column_width_px = assert_node_layout(&scenario.root, scenario.right_column).width_px;

    assert_close(left_column_left_px, 0.0, "two-column row left item left");
    assert!(
        left_column_width_px > 0.0,
        "two-column row left item should keep non-zero auto width"
    );
    assert!(
        right_column_width_px > 0.0,
        "two-column row right item should keep non-zero auto width"
    );
    assert!(
        right_column_left_px >= left_column_left_px + left_column_width_px + scenario.gap_px,
        "two-column row right item should appear to the right of the first item with the authored gap; left={left_column_left_px} width={left_column_width_px} right={right_column_left_px} gap={}",
        scenario.gap_px,
    );
}

#[test]
// Logical `start` must stay logical when it comes from a persisted stylesheet
// too. In `direction: rtl` plus `row-reverse`, `justify-content:start` should
// still pin the group to inline-start (physical right) after recompute.
fn stylesheet_direction_rtl_row_reverse_and_justify_content_start_persist_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .rtl-row-reverse-start {
                display: flex;
                direction: rtl;
                flex-direction: row-reverse;
                flex-wrap: nowrap;
                justify-content: start;
                width: 60px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "rtl-row-reverse-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        40.0,
        0.0,
        10.0,
        10.0,
        "stylesheet rtl row-reverse logical-start first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        50.0,
        0.0,
        10.0,
        10.0,
        "stylesheet rtl row-reverse logical-start second item",
    );
}

#[test]
// The reverse-axis flex additions must also be available from selector-driven
// authored style. This class-driven column-reverse container should keep its
// reversed main-axis placement after a later style recompute.
fn stylesheet_column_reverse_and_align_items_flex_end_persist_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .column-reverse-panel {
                display: flex;
                flex-direction: column-reverse;
                flex-wrap: nowrap;
                align-items: flex-end;
                width: 20px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "column-reverse-panel");
    assert_set_inflexible_column_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_column_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        10.0,
        30.0,
        10.0,
        10.0,
        "stylesheet column-reverse first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0,
        10.0,
        10.0,
        "stylesheet column-reverse second item",
    );
}

#[test]
// Wrapped-line reverse stacking and cross-axis packing should remain available
// through stylesheets too. This class-driven case covers `flex-wrap:
// wrap-reverse`, `align-content: space-between`, and `row-gap`.
fn stylesheet_wrap_reverse_row_gap_and_align_content_persist_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .reverse-wrap-stack {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap-reverse;
                align-content: space-between;
                row-gap: 4px;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "reverse-wrap-stack");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        30.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        30.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse third item",
    );
}

#[test]
// The logical/flex-relative split for `align-content` also needs stylesheet
// coverage. In `wrap-reverse`, `align-content:start` should keep the line stack
// at logical cross-start after style recompute.
fn stylesheet_wrap_reverse_align_content_start_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrap-reverse-start-stack {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap-reverse;
                align-content: start;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrap-reverse-start-stack");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        10.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse logical-start first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        10.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse logical-start second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse logical-start third item",
    );
}

#[test]
// `align-self` overrides should stay available through stylesheets too. This
// covers the recent `stretch` support on the selector-driven path and proves it
// survives a later style recompute.
fn stylesheet_align_self_stretch_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .row-start {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                align-items: flex-start;
                width: 60px;
                height: 50px;
            }
            .self-stretch {
                align-self: stretch;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "row-start");
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "height", "10px");
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_set_attr_str(&mut root, second, "class", "self-stretch");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).height_px,
        10.0,
        "stylesheet align-self sibling keeps authored cross size",
    );
    assert_close(
        assert_node_layout(&root, second).height_px,
        50.0,
        "stylesheet align-self stretch item height",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        50.0,
        "stylesheet align-self stretch item",
    );
}

#[test]
// The recent `align-self` surface should also cover non-stretch overrides
// through stylesheets. This class-driven `align-self:center` item should keep
// its cross-axis override after a later style recompute.
fn stylesheet_align_self_center_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .row-start {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                align-items: flex-start;
                width: 60px;
                height: 50px;
            }
            .self-center {
                align-self: center;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "row-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "class", "self-center");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-self center sibling",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        20.0,
        10.0,
        10.0,
        "stylesheet align-self center item",
    );
}

#[test]
// Container-driven cross-axis stretch should remain available through
// stylesheets too. This keeps the recent `align-items: stretch` support on the
// selector path and proves it survives a later style recompute.
fn stylesheet_align_items_stretch_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .row-stretch {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                align-items: stretch;
                width: 60px;
                height: 50px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "row-stretch");
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "flex-shrink", "0");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).height_px,
        50.0,
        "stylesheet align-items stretch first item height",
    );
    assert_close(
        assert_node_layout(&root, second).height_px,
        50.0,
        "stylesheet align-items stretch second item height",
    );
}

#[test]
// `align-self:flex-end` should remain reachable through stylesheets because it
// uses the same recent override path as the direct-attr proof. The override
// must still survive a later style recompute.
fn stylesheet_align_self_flex_end_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .row-start {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                align-items: flex-start;
                width: 60px;
                height: 50px;
            }
            .self-end {
                align-self: flex-end;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "row-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "class", "self-end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-self flex-end sibling",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "stylesheet align-self flex-end item",
    );
}

#[test]
// `align-self:end` needs persisted coverage because browsers resolve it
// logically, not as `flex-end`, when `wrap-reverse` flips the flex cross axis.
fn stylesheet_wrap_reverse_align_self_end_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrap-reverse-start-row {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap-reverse;
                align-items: start;
                width: 30px;
                height: 50px;
            }
            .self-end {
                align-self: end;
            }
        ",
    );

    let tall = assert_append_div_to(&mut root, container);
    let short = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrap-reverse-start-row");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");
    assert_set_attr_str(&mut root, short, "class", "self-end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, tall, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = tall, rects[2] = short.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        20.0,
        "stylesheet wrap-reverse logical-start sibling: the stretched line fills the container cross size, so logical start is the container's block-start",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        40.0,
        10.0,
        10.0,
        "stylesheet wrap-reverse logical-end self override",
    );
}

#[test]
// Gap shorthand should be available through persisted stylesheets, not only
// direct attrs. This keeps the recent item+line spacing support on the class
// path and across a later style recompute.
fn stylesheet_gap_shorthand_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrapped-gap {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap;
                width: 25px;
                gap: 5px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrapped-gap");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[2],
        15.0,
        0.0,
        10.0,
        10.0,
        "stylesheet gap shorthand second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        15.0,
        10.0,
        10.0,
        "stylesheet gap shorthand third item",
    );
}

#[test]
// Main-axis packing variants should also be reachable through stylesheets. This
// class-driven `justify-content: space-evenly` case keeps the equal edge gaps
// after a later style recompute.
fn stylesheet_justify_content_space_evenly_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .space-evenly-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: space-evenly;
                width: 90px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "space-evenly-row");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        15.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-evenly first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        40.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-evenly second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        65.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-evenly third item",
    );
}

#[test]
// The remaining admitted main-axis packing variants should also stay available
// through stylesheets. This `justify-content:flex-end` case should keep its
// right-packed placement after recompute.
fn stylesheet_justify_content_flex_end_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .flex-end-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: flex-end;
                width: 90px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "flex-end-row");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        60.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content flex-end first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        70.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content flex-end second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        80.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content flex-end third item",
    );
}

#[test]
// `justify-content:space-around` is also in the admitted packing surface and
// should stay available on the persisted stylesheet path.
fn stylesheet_justify_content_space_around_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .space-around-row {
                display: flex;
                flex-direction: row;
                flex-wrap: nowrap;
                justify-content: space-around;
                width: 90px;
                height: 10px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "space-around-row");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        10.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-around first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        40.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-around second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        70.0,
        0.0,
        10.0,
        10.0,
        "stylesheet justify-content space-around third item",
    );
}

#[test]
// Wrapped-line packing variants should remain available through stylesheets.
// This class-driven `align-content: space-around` case keeps half-sized edge
// gaps after a later style recompute.
fn stylesheet_align_content_space_around_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrapped-space-around {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap;
                align-content: space-around;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrapped-space-around");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        5.0,
        10.0,
        10.0,
        "stylesheet align-content space-around first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        5.0,
        10.0,
        10.0,
        "stylesheet align-content space-around second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        25.0,
        10.0,
        10.0,
        "stylesheet align-content space-around third item",
    );
}

#[test]
// The remaining admitted wrapped-line packing variant should also remain
// reachable through stylesheets. This `align-content: space-between` case
// should keep the split line packing after recompute.
fn stylesheet_align_content_space_between_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrapped-space-between {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap;
                align-content: space-between;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrapped-space-between");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-content space-between first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-content space-between second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        30.0,
        10.0,
        10.0,
        "stylesheet align-content space-between third item",
    );
}

#[test]
// `align-content:space-evenly` is also admitted and should remain available on
// the persisted stylesheet path across recompute.
fn stylesheet_align_content_space_evenly_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrapped-space-evenly {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap;
                align-content: space-evenly;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrapped-space-evenly");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        6.666_666_5,
        10.0,
        10.0,
        "stylesheet align-content space-evenly first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        6.666_666_5,
        10.0,
        10.0,
        "stylesheet align-content space-evenly second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        23.333_334,
        10.0,
        10.0,
        "stylesheet align-content space-evenly third item",
    );
}

#[test]
// Wrapped-line stretch should also remain reachable through stylesheets. This
// class-driven `align-content: stretch` case keeps the expanded line cross
// sizes after a later style recompute.
fn stylesheet_align_content_stretch_persists_after_recompute() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrapped-stretch {
                display: flex;
                flex-direction: row;
                flex-wrap: wrap;
                align-content: stretch;
                width: 25px;
                height: 40px;
            }
        ",
    );

    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "class", "wrapped-stretch");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_style_set(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second, rects[3] = third.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-content stretch first item",
    );
    assert_rect_op_geometry(
        &rects[2],
        10.0,
        0.0,
        10.0,
        10.0,
        "stylesheet align-content stretch second item",
    );
    assert_rect_op_geometry(
        &rects[3],
        0.0,
        20.0,
        10.0,
        10.0,
        "stylesheet align-content stretch third item",
    );
}

// CSS Flexbox 1 §4 (Flex Items): a contiguous run of inline-level children
// (text nodes, inline/atomic-inline elements) is wrapped into ONE anonymous
// flex item by the box-tree constructor, and whitespace-only runs are
// discarded before flex item collection. Closest WPT oracle:
// `flexbox-anonymous-items-001.html`. Gecko builds anonymous flex items in
// `nsFlexContainerFrame.cpp` (`GenerateFlexLines` walks the flattened item
// list after the box tree has already wrapped inline runs); Blink collects
// them in `flex_layout_algorithm.cc` (`ConstructAndAppendFlexItems`).
//
// ON-SPEC (relabeled from `off_spec_anonymous_inline_runs_are_not_merged` in
// flex arc step 3a-ii): the measurement fold now merges each maximal
// contiguous inline run into ONE virtual item (`measure_inline_run`,
// `flex.rs`), so two adjacent text nodes (and a span + adjacent text) merge
// into one item with no inter-item gap; a whitespace-only run is dropped under
// a collapse white-space mode (no item, no gap) and forms an item under a
// preserve mode. Rows (a)/(b)/(d) flipped to the spec-correct single-item
// shape; row (c) stays the on-spec control (a block breaks the run); rows
// (e)/(g)/(h)/(i) are new on-spec pins; row (f) (fragment-joining min-content)
// is pinned at the item-view/accumulator unit level (`flex.rs`,
// `flex_anonymous_run.rs`) because it pins the fold arithmetic directly, while
// row (i) exercises the run's min-content floor at layout (the run shrinks
// with the CSS initial `flex-shrink: 1` and clamps at that floor). Row (i)(c)
// pins the shrinking-cross half (a shrunk run wraps at its resolved width and
// grows its cross); the row (i) control proves an unshrunk run stays
// single-line.
#[test]
fn anonymous_inline_runs_merge_into_single_flex_items() {
    let rows: [AnonymousRunRow; 10] = [
        (
            "text_plus_text_merges_into_one_item",
            assert_text_plus_text_merged,
        ),
        (
            "span_plus_text_merges_into_one_item",
            assert_span_plus_text_merged,
        ),
        (
            "text_block_text_block_breaks_run_on_spec",
            assert_text_block_text_block_breaks_run,
        ),
        (
            "whitespace_only_text_between_blocks_dropped",
            assert_whitespace_only_text_between_blocks_dropped,
        ),
        (
            "whitespace_only_text_pre_wrap_forms_item",
            assert_whitespace_only_text_pre_wrap_forms_item,
        ),
        (
            "run_order_semantics_ignore_member_order",
            assert_run_order_semantics,
        ),
        ("joint_cross_is_one_strut", assert_joint_cross_is_one_strut),
        (
            "run_shrinks_with_css_initial_flex_shrink",
            assert_anonymous_run_shrinks_with_css_initial,
        ),
        (
            "shrunk_run_wraps_at_resolved_width",
            assert_anonymous_run_wraps_at_resolved_width,
        ),
        (
            "unshrunk_run_stays_single_line",
            assert_anonymous_run_unshrunk_stays_single_line,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named anonymous-inline-run probe: the row name plus a runner that
/// builds the scenario and asserts through the row name.
type AnonymousRunRow = (&'static str, fn(&str));

fn assert_text_plus_text_merged(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let label = assert_append_text_to(&mut root, container, "Left");
    let _value = assert_append_text_to(&mut root, container, "Right");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    assert_eq!(text_runs.len(), 2, "{name}: two text runs");
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: first text left"),
    );
    let label_width = assert_node_layout(&root, label).width_px;
    // On-spec: "Left"+"Right" merge into ONE anonymous item — the second text
    // sits contiguous at the first text's width (no inter-item gap).
    assert_close(
        text_runs[1].position_px[0],
        label_width,
        &format!("{name}: second text contiguous after first"),
    );
}

fn assert_span_plus_text_merged(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let span = assert_append_span_to(&mut root, container);
    let _span_text = assert_append_text_to(&mut root, span, "Foo");
    let _sibling_text = assert_append_text_to(&mut root, container, "bar");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + span rects");
    assert_eq!(
        plans[0].text_runs.len(),
        2,
        "{name}: span text + sibling text"
    );
    let span_width = assert_node_layout(&root, span).width_px;
    // On-spec: the span and the adjacent text merge into ONE anonymous item —
    // the sibling text sits contiguous at the span's width (no inter-item gap).
    assert_close(
        plans[0].text_runs[1].position_px[0],
        span_width,
        &format!("{name}: sibling text contiguous after span"),
    );
}

fn assert_text_block_text_block_breaks_run(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let leading_text = assert_append_text_to(&mut root, container, "A");
    let block = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block, "flex-basis", "30px");
    assert_style_set(&mut root, block, "height", "20px");
    let _trailing_text = assert_append_text_to(&mut root, container, "B");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + block rects");
    assert_eq!(
        plans[0].text_runs.len(),
        2,
        "{name}: leading + trailing text runs"
    );
    let leading_width = assert_node_layout(&root, leading_text).width_px;
    // On-spec control: the block breaks the run, so the leading text, the
    // block, and the trailing text are three items with gaps — the same shape
    // the spec's two anonymous items + block produces.
    assert_close(
        plans[0].rects[1].position_px[0],
        leading_width + 10.0,
        &format!("{name}: block left after gap"),
    );
    assert_close(
        plans[0].text_runs[1].position_px[0],
        leading_width + 10.0 + 30.0 + 10.0,
        &format!("{name}: trailing text left after block and gap"),
    );
}

fn assert_whitespace_only_text_between_blocks_dropped(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "20px");
    let _whitespace = assert_append_text_to(&mut root, container, "   ");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "40px");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(
        plans[0].rects.len(),
        3,
        "{name}: container + two block rects"
    );
    // On-spec: the whitespace-only run is DISCARDED under the normal collapse
    // mode — no phantom ITEM, so the second block sits one gap after the first
    // block (the drop removes the flex-item slot, not the stray paint run).
    assert_close(
        plans[0].rects[2].position_px[0],
        30.0 + 10.0,
        &format!("{name}: second block left after one gap"),
    );
}

fn assert_whitespace_only_text_pre_wrap_forms_item(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "white-space", "pre-wrap");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "20px");
    let whitespace = assert_append_text_to(&mut root, container, "   ");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "40px");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // On-spec: under `pre-wrap` the preserved whitespace keeps its width, so
    // the run FORMS an item and the second block is pushed past it by two gaps.
    let whitespace_width = assert_node_layout(&root, whitespace).width_px;
    assert_close(
        plans[0].rects[2].position_px[0],
        30.0 + 10.0 + whitespace_width + 10.0,
        &format!("{name}: second block left pushed by the preserved whitespace item"),
    );
}

fn assert_run_order_semantics(name: &str) {
    // Pin (i): a span with `order: 1` inside a run of inline children keeps
    // its DOM position — `order` applies to the anonymous run wrapper (the
    // item), not to the inline member, so layout and paint agree on DOM order.
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_text_to(&mut root, container, "A");
    let span = assert_append_span_to(&mut root, container);
    assert_style_set(&mut root, span, "order", "1");
    let _span_text = assert_append_text_to(&mut root, span, "B");
    let _last = assert_append_text_to(&mut root, container, "C");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let first_width = assert_node_layout(&root, first).width_px;
    let plans = take_staged_layer_paint_plans(&root);
    // The span's order:1 does not reorder it out of the run — it sits between
    // the first and last text members in DOM (inline) order.
    assert_close(
        plans[0].rects[1].position_px[0],
        first_width,
        &format!("{name}: span stays in run DOM position despite order: 1"),
    );
    assert_eq!(
        plans[0].text_runs.len(),
        3,
        "{name}: three text runs in DOM order"
    );

    // Pin (ii): a run beside an `order: 1` block item sequences the block
    // last (the run is one unit at order 0).
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let run_text = assert_append_text_to(&mut root, container, "A");
    let block = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block, "order", "1");
    assert_style_set(&mut root, block, "flex-basis", "20px");
    assert_style_set(&mut root, block, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // The order-0 run leads at the start; the order-1 block sequences last.
    assert_close(
        plans[0].text_runs[0].position_px[0],
        0.0,
        &format!("{name}: the order-0 run leads at the start"),
    );
    let run_width = assert_node_layout(&root, run_text).width_px;
    assert_close(
        plans[0].rects[1].position_px[0],
        run_width,
        &format!("{name}: the order-1 block sequences last after the run"),
    );
}

fn assert_joint_cross_is_one_strut(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "width", "100px");
    let first = assert_append_text_to(&mut root, container, "a");
    let _second = assert_append_text_to(&mut root, container, "b");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    // On-spec: "a"+"b" merge into ONE run laid out inline on one line, so the
    // container's cross (height) is ONE strut, not 2× strut (two stacked items).
    let strut_px = assert_node_layout(&root, first).height_px;
    assert_close(
        assert_node_layout(&root, container).height_px,
        strut_px,
        &format!("{name}: joint cross is one strut, not two"),
    );
}

/// Row (i): an anonymous inline run is a flex item carrying the CSS INITIAL
/// flex values — `flex-grow: 0`, `flex-shrink: 1` (an anonymous wrapper
/// inherits nothing, so `flex-shrink` keeps its initial 1). In negative free
/// space the run shrinks by its scaled shrink factor and clamps at its
/// min-content (the widest segment).
///
/// The run has no node of its own, so its resolved main size is observed
/// through the following inflexible block item's left edge (gap 0). With the
/// 3a-ii `flex_shrink: 0.0` error the run froze at max-content and the block
/// stayed at the run's full max-content width, so the shrunk-size assertions
/// here fail — this row discriminates that field.
///
/// The shrinking-cross half of D9(i) ("a shrunk run's cross reflects the
/// wrapping the shrink causes") is pinned by row (i)(c)
/// `shrunk_run_wraps_at_resolved_width`, not here: this row fixes the
/// container height (20px), so the shrunk run's wrapped cross is not the
/// observable. Row (i)(c) drops the fixed height and asserts the run's placed
/// cross directly.
fn assert_anonymous_run_shrinks_with_css_initial(name: &str) {
    // One text node is one run beside an inflexible block item (flex-basis
    // 30px, shrink 0): the run absorbs all of the line's negative free space,
    // so its resolved main size is observable as the block's left edge.
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "400px");
    assert_style_set(&mut root, container, "height", "20px");
    let text = assert_append_text_to(&mut root, container, "one two three four");
    let block = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block, "flex-basis", "30px");
    assert_style_set(&mut root, block, "flex-shrink", "0");
    assert_style_set(&mut root, block, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let max_content_px = assert_text_intrinsic(&root, text).max_content_width_px;
    let min_content_px = assert_text_intrinsic(&root, text).min_content_width_px;

    // (a) scaled-shrink distribution: at a container width equal to the run's
    // max-content, the line's negative free space is the block's 30px basis
    // (the run's shrink factor is 1, the block's 0 freezes it), so the run
    // resolves to max-content − 30px and the block's left edge lands there.
    assert_style_set(
        &mut root,
        container,
        "width",
        &format!("{max_content_px}px"),
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // rects[0] = container, rects[1] = block (the text emits a text run).
    assert_close(
        plans[0].rects[1].position_px[0],
        max_content_px - 30.0,
        &format!("{name}: shrunk run resolves to max-content minus the block basis"),
    );
    assert_close(
        plans[0].rects[1].size_px[0],
        30.0,
        &format!("{name}: the inflexible block keeps its 30px basis"),
    );

    // (b) min-content clamp: a container no wider than the widest word would
    // push the run below its min-content, so it clamps at the widest segment
    // instead and the block's left edge lands at the run's min-content.
    assert_style_set(
        &mut root,
        container,
        "width",
        &format!("{min_content_px}px"),
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_close(
        plans[0].rects[1].position_px[0],
        min_content_px,
        &format!("{name}: run clamps at its min-content (the widest word)"),
    );
}

/// Row (i)(c): a shrunk anonymous run's cross reflects the wrapping the shrink
/// causes (D9(i) "a shrunk run's cross reflects the wrapping the shrink
/// causes", D6). Placement passes the run's resolved content main size down as
/// the available width (`anonymous_inline_run_child_context`), so a run shrunk
/// below its max-content wraps its children at the resolved width and its
/// placed cross (the joint laid-out height) exceeds the single-line strut.
///
/// The run has no node of its own, so its placed cross is observed through its
/// single text child's placed height: four words wrap into one line each at the
/// resolved min-content width, so the text's height is 4 × strut (the wrapped
/// remainder sits on the following lines).
fn assert_anonymous_run_wraps_at_resolved_width(name: &str) {
    // One text node is one run beside an inflexible block item (flex-basis
    // 30px, shrink 0, height 20px): the run absorbs all of the line's negative
    // free space and clamps at its min-content (the widest word), so its four
    // words wrap into one line each at the resolved width.
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "400px");
    let text = assert_append_text_to(&mut root, container, "one two three four");
    let block = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block, "flex-basis", "30px");
    assert_style_set(&mut root, block, "flex-shrink", "0");
    assert_style_set(&mut root, block, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let strut_px = assert_text_intrinsic(&root, text).line_height_px;
    let min_content_px = assert_text_intrinsic(&root, text).min_content_width_px;

    assert_style_set(
        &mut root,
        container,
        "width",
        &format!("{min_content_px}px"),
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);

    // Four words wrap into four lines at the min-content width, so the run's
    // placed cross is 4 × strut: the text child's placed height (the joint
    // laid-out height) spans four lines, exceeding the single-line strut.
    let wrapped_height_px = 4.0 * strut_px;
    assert!(
        assert_node_layout(&root, text).height_px > strut_px,
        "{name}: the shrunk run's cross exceeds the single-line strut",
    );
    assert_close(
        assert_node_layout(&root, text).height_px,
        wrapped_height_px,
        &format!("{name}: the shrunk run's cross wraps into four lines"),
    );
    assert_close(
        plans[0].text_runs[0].size_px[1],
        wrapped_height_px,
        &format!("{name}: the wrapped text run spans four lines"),
    );
}

/// Row (i) control: an UNSHRUNK anonymous run stays single-line. At a container
/// width above the run's max-content the run resolves to its max-content
/// (grow 0, no negative free space), so its children do not wrap and the run's
/// placed cross stays the single-line strut — the flip side of row (i)(c)'s
/// wrap.
fn assert_anonymous_run_unshrunk_stays_single_line(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "400px");
    let text = assert_append_text_to(&mut root, container, "one two three four");
    let block = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block, "flex-basis", "30px");
    assert_style_set(&mut root, block, "flex-shrink", "0");
    assert_style_set(&mut root, block, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let strut_px = assert_text_intrinsic(&root, text).line_height_px;
    let plans = take_staged_layer_paint_plans(&root);

    // No shrink: the text stays on one line (height == strut), unlike row
    // (i)(c)'s wrapped run.
    assert_close(
        assert_node_layout(&root, text).height_px,
        strut_px,
        &format!("{name}: the unshrunk run's text stays on one strut line"),
    );
    assert_close(
        plans[0].text_runs[0].size_px[1],
        strut_px,
        &format!("{name}: the unshrunk run's text run spans one strut line"),
    );
}

#[test]
fn div_children_with_text_content_use_auto_width_and_honor_gap() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let label_div = assert_append_div_to(&mut root, container);
    let _label_text = assert_append_text_to(&mut root, label_div, "Label");
    let value_div = assert_append_div_to(&mut root, container);
    let _value_text = assert_append_text_to(&mut root, value_div, "Some value");

    assert_set_flex_row_container_width(&mut root, container, "300px");
    assert_style_set(&mut root, container, "height", "30px");
    assert_style_set(&mut root, container, "column-gap", "8px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = label_div, rects[2] = value_div
    // (text children emit text runs, not rects).
    assert_close(rects[1].position_px[0], 0.0, "label div left");
    let label_width = assert_node_layout(&root, label_div).width_px;
    assert!(
        label_width > 0.0,
        "label div should auto-size to its text content",
    );
    assert_close(
        rects[2].position_px[0],
        label_width + 8.0,
        "value div left after gap",
    );
}

// CSS Flexbox 1 §9.9 (Intrinsic Sizes): when the flex container lacks a
// definite main size, it stretches to its containing-block width (block-level
// flex containers behave like block boxes). The items are still collected and
// laid out with gap and alignment respected.
#[test]
fn auto_width_flex_container_stretches_to_containing_block_and_respects_gap() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);

    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "row");
    assert_style_set(&mut root, container, "flex-wrap", "nowrap");
    assert_style_set(&mut root, container, "column-gap", "10px");
    assert_set_inflexible_row_item(&mut root, first, "30px", "20px");
    assert_set_inflexible_row_item(&mut root, second, "40px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Block-level flex container stretches to containing block width (viewport 320px).
    assert_close(
        assert_node_layout(&root, container).width_px,
        320.0,
        "container stretches to viewport width as block-level",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, "first item");
    // Gap between items is respected.
    assert_rect_op_geometry(
        &rects[2],
        30.0 + 10.0,
        0.0,
        40.0,
        20.0,
        "second item after gap",
    );
}

// CSS Flexbox 1 §4 (Flex Items): contiguous inline-level children (spans
// without explicit display) merge into ONE anonymous flex item; the inline
// content contributes to the flex algorithm's measurement and placement.
// Moved on-spec in flex arc step 3a-ii: two adjacent spans are now one run
// laid out inline (no inter-item gap) instead of two per-child items.
#[test]
fn span_children_in_flex_row_participate_as_flex_items() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first_span = assert_append_span_to(&mut root, container);
    let _first_text = assert_append_text_to(&mut root, first_span, "First");
    let second_span = assert_append_span_to(&mut root, container);
    let _second_text = assert_append_text_to(&mut root, second_span, "Second");

    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "6px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first_span, rects[2] = second_span
    // (text children emit text runs, not rects).
    assert_close(rects[1].position_px[0], 0.0, "first span left");
    let first_width = assert_node_layout(&root, first_span).width_px;
    assert!(
        first_width > 0.0,
        "first span should have non-zero layout width from its text",
    );
    // On-spec: the two spans merge into one anonymous run, so the second span
    // sits contiguous at the first span's width (no inter-item gap).
    assert_close(
        rects[2].position_px[0],
        first_width,
        "second span contiguous after first",
    );
}

// CSS Flexbox 1 §9.5 (Main-Axis Alignment): `justify-content: space-between`
// distributes remaining space between FLEX ITEMS. Moved on-spec in flex arc
// step 3a-ii: three bare text children now merge into ONE anonymous run item,
// so there are no inter-item gaps to distribute — the run's members flow
// inline contiguously (space-between has no free space over one item).
#[test]
fn text_children_with_justify_content_space_between_and_gap() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_text_to(&mut root, container, "A");
    let second = assert_append_text_to(&mut root, container, "B");
    let _third = assert_append_text_to(&mut root, container, "C");

    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "space-between");
    assert_style_set(&mut root, container, "column-gap", "5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    // text_runs[0] = first, text_runs[1] = second, text_runs[2] = third
    // (text children emit text runs, not rects).
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        "first member left at start",
    );
    let first_width = assert_node_layout(&root, first).width_px;
    let second_width = assert_node_layout(&root, second).width_px;

    // On-spec: one merged run — members flow inline contiguously, no gap and
    // no space-between packing between them.
    assert_close(
        text_runs[1].position_px[0],
        first_width,
        "second member contiguous after first",
    );
    assert_close(
        text_runs[2].position_px[0],
        first_width + second_width,
        "third member contiguous after second",
    );
}

// CSS Flexbox 1 §4 (Flex Items) and §9.2 (Line Breaking): Moved on-spec in
// flex arc step 3a-ii — two text children in a column container now merge
// into ONE anonymous run laid out inline on one line, so the second text
// sits contiguous after the first (horizontally) instead of stacking below.
#[test]
fn text_children_in_flex_column_stack_with_row_gap() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_text_to(&mut root, container, "Top");
    let _second = assert_append_text_to(&mut root, container, "Bottom");

    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "row-gap", "8px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    // text_runs[0] = first, text_runs[1] = second (text children emit text runs, not rects).
    assert_close(text_runs[0].position_px[1], 0.0, "first member top");
    let first_width = assert_node_layout(&root, first).width_px;
    // On-spec: one merged run flows inline — the second text sits contiguous
    // after the first on the SAME line (no row-gap, no vertical stack).
    assert_close(
        text_runs[1].position_px[0],
        first_width,
        "second member contiguous after first on one line",
    );
}

// CSS Flexbox 1 §4 (Flex Items): nested flex containers with auto-basis
// block children participate in the active flex solver.
#[test]
fn nested_flex_auto_basis_block_children_participate_in_flex_layout() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);

    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "column-gap", "10px");

    let first = assert_append_div_to(&mut root, container);
    let _first_text = assert_append_text_to(&mut root, first, "Left");
    let second = assert_append_div_to(&mut root, container);
    let _second_text = assert_append_text_to(&mut root, second, "Right");

    assert_style_set(&mut root, first, "flex-grow", "1");
    assert_style_set(&mut root, first, "flex-shrink", "1");
    assert_style_set(&mut root, first, "flex-basis", "0px");
    assert_style_set(&mut root, second, "flex-grow", "1");
    assert_style_set(&mut root, second, "flex-shrink", "1");
    assert_style_set(&mut root, second, "flex-basis", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, first).width_px,
        95.0,
        "first flex:1 item fills half container minus half gap",
    );
    assert_close(
        assert_node_layout(&root, second).width_px,
        95.0,
        "second flex:1 item fills half container minus half gap",
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = first, rects[2] = second (text children emit text runs).
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 95.0, 40.0, "first item at start");
    assert_rect_op_geometry(
        &rects[2],
        95.0 + 10.0,
        0.0,
        95.0,
        40.0,
        "second item after first + gap",
    );
}

#[test]
// CSS Flexbox 1 §8.1 (aligning with auto margins) — on-spec for the pinned
// case, held by an ACCIDENTAL solver path. Spec: a flex item's `margin: auto`
// absorbs free space in its axis. In a COLUMN container the cross axis is the
// inline axis, so `margin-left: auto; margin-right: auto` on a fixed-width
// block item should center it in the container's cross size.
//
// HOW THIS WORKS TODAY (accidental, §8.1-aligned for this simple case): a
// block-level item in a COLUMN container gets its cross-axis auto margins
// absorbed through the BLOCK §10.3.3 solver — `resolve_block_auto_margins`
// (`src/domains/layout/flow.rs`) is reached via the Vertical branch of the
// flex placed-child context (`src/domains/layout/flex.rs`), which leaves
// `available_width_px` as the container's content width, so the free space is
// non-zero and the pair splits it. Full §8.1 semantics (auto margins vs
// `align-self`, and MAIN-axis absorption — which still computes to 0, pinned by
// `flex_item_auto_margin_computes_to_zero_for_now`) await the named D6
// admission. This pin exists so the accidental cross-axis behavior cannot drift
// silently: it is NOT `off_spec_`-named because it is not divergent.
fn column_flex_block_item_cross_auto_margins_absorb_free_space() {
    // Primary: a 20px-wide block item with both cross-axis margins auto in a
    // 60px column container centers at left (60 − 20) / 2 = 20.
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "60px", "40px");
    assert_set_inflexible_column_item(&mut root, item, "10px", "20px");
    assert_style_set(&mut root, item, "margin-left", "auto");
    assert_style_set(&mut root, item, "margin-right", "auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item.
    assert_rect_op_geometry(
        &rects[1],
        20.0,
        0.0,
        20.0,
        10.0,
        "column item centered by cross-axis auto margins",
    );

    // Control: `margin-left: 0` (right auto) leaves the left edge at the
    // cross-axis start, proving the pair — not the stretch/alignment path —
    // produced the centered placement above.
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);

    assert_set_flex_column_container_size(&mut root, container, "60px", "40px");
    assert_set_inflexible_column_item(&mut root, item, "10px", "20px");
    assert_style_set(&mut root, item, "margin-left", "0");
    assert_style_set(&mut root, item, "margin-right", "auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        10.0,
        "column item left edge stays at cross-start without the auto left margin",
    );
}

fn flex_row_container_setup() -> (bevy::prelude::World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    (world, root, container)
}

fn assert_set_flex_row_container_width(root: &mut RootDom, container: DomNodeSlotKey, width: &str) {
    assert_style_set(root, container, "display", "flex");
    assert_style_set(root, container, "flex-direction", "row");
    assert_style_set(root, container, "flex-wrap", "nowrap");
    assert_style_set(root, container, "width", width);
}

fn assert_set_flex_column_container_size(
    root: &mut RootDom,
    container: DomNodeSlotKey,
    width: &str,
    height: &str,
) {
    assert_style_set(root, container, "display", "flex");
    assert_style_set(root, container, "flex-direction", "column");
    assert_style_set(root, container, "flex-wrap", "nowrap");
    assert_style_set(root, container, "width", width);
    assert_style_set(root, container, "height", height);
}

fn assert_set_flex_basis_and_cross_size(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    height: &str,
) {
    assert_style_set(root, item, "height", height);
    assert_style_set(root, item, "flex-basis", basis);
}

fn assert_set_inflexible_row_item(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    height: &str,
) {
    assert_set_flex_basis_and_cross_size(root, item, basis, height);
    assert_style_set(root, item, "flex-grow", "0");
    assert_style_set(root, item, "flex-shrink", "0");
}

fn auto_width_titlebar_flex_mutation_setup() -> AutoWidthTitlebarFlexMutationScenario {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let container = assert_append_div_to(&mut root, parent);
    let _start = assert_append_inflexible_titlebar_item_to(&mut root, container);
    let title_host = assert_append_div_to(&mut root, container);
    let _end = assert_append_inflexible_titlebar_item_to(&mut root, container);

    assert_style_set(&mut root, parent, "width", "120px");
    assert_style_set(&mut root, parent, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "row");
    assert_style_set(&mut root, container, "flex-wrap", "nowrap");
    assert_style_set(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_style_set(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_style_set(&mut root, title_host, "flex-basis", "0px");
    assert_style_set(&mut root, title_host, "flex-grow", "1");
    assert_style_set(&mut root, title_host, "flex-shrink", "1");
    assert_style_set(&mut root, title_host, "overflow", "hidden");
    let _title_text = assert_append_text_to(&mut root, title_host, "Camera");

    AutoWidthTitlebarFlexMutationScenario {
        world,
        root,
        container,
        title_host,
    }
}

fn assert_append_inflexible_titlebar_item_to(
    root: &mut RootDom,
    container: DomNodeSlotKey,
) -> DomNodeSlotKey {
    let item = assert_append_div_to(root, container);
    assert_set_inflexible_row_item(root, item, TITLEBAR_ICON_BASIS_PX, TITLEBAR_ICON_HEIGHT_PX);
    item
}

fn assert_set_flex_column_basis_and_cross_size(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    width: &str,
) {
    assert_style_set(root, item, "width", width);
    assert_style_set(root, item, "flex-basis", basis);
}

fn assert_set_inflexible_column_item(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    width: &str,
) {
    assert_set_flex_column_basis_and_cross_size(root, item, basis, width);
    assert_style_set(root, item, "flex-grow", "0");
    assert_style_set(root, item, "flex-shrink", "0");
}

fn assert_text_intrinsic(root: &RootDom, text: DomNodeSlotKey) -> ui_dom::TextIntrinsic {
    match root.node(text).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("flex layout proof node should keep text intrinsic state"),
    }
}

fn next_sibling_key(root: &RootDom, key: DomNodeSlotKey) -> Option<DomNodeSlotKey> {
    root.node(key)
        .expect("flex layout proof node should stay in the DOM")
        .next_sibling(|sibling_key| root.node(sibling_key))
        .map(|(sibling_key, _)| sibling_key)
}

/// The retained measurement record for an element node (the flex measure
/// fold's pass output). Elements only: text/image/control nodes carry records
/// too, but these proofs read the flex container's own intrinsic sizes.
fn element_measurement_record(
    root: &RootDom,
    key: DomNodeSlotKey,
) -> ui_dom::domains::layout::MeasurementRecord {
    match root.node(key).expect("flex layout proof node should exist") {
        DomNode::Element(node) => node.base.measurement,
        _ => panic!("flex layout proof node should be an element"),
    }
}

struct AutoWidthTitlebarFlexMutationScenario {
    world: bevy::prelude::World,
    root: RootDom,
    container: DomNodeSlotKey,
    title_host: DomNodeSlotKey,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct TitlebarMutationProbe {
    container_width_px: f32,
    title_width_px: f32,
    end_left_px: f32,
}

fn titlebar_mutation_probe(
    root: &RootDom,
    container: DomNodeSlotKey,
    title_host: DomNodeSlotKey,
    end_left_px: f32,
) -> TitlebarMutationProbe {
    TitlebarMutationProbe {
        container_width_px: assert_node_layout(root, container).width_px,
        title_width_px: assert_node_layout(root, title_host).width_px,
        end_left_px,
    }
}

fn assert_auto_width_titlebar_append_probe(probe: TitlebarMutationProbe) {
    assert_close(
        probe.container_width_px,
        120.0,
        "first auto-width append pass container width",
    );
    assert_close(
        probe.title_width_px,
        72.0,
        "first auto-width append pass title width",
    );
    assert_close(
        probe.end_left_px,
        88.0,
        "first auto-width append pass original end left",
    );
}

fn assert_auto_width_titlebar_remove_probe(probe: TitlebarMutationProbe) {
    assert_close(
        probe.container_width_px,
        120.0,
        "first auto-width remove pass container width",
    );
    assert_close(
        probe.title_width_px,
        88.0,
        "first auto-width remove pass title width",
    );
    assert_close(
        probe.end_left_px,
        104.0,
        "first auto-width remove pass original end left",
    );
}

#[derive(Clone, Copy)]
struct TitlebarFlexCase {
    name: &'static str,
    container_width_px: f32,
    start_icon_count: usize,
    end_icon_count: usize,
    expected_title_width_px: f32,
}

struct TitlebarFlexReport {
    case_name: &'static str,
    container_width_px: f32,
    item_widths_px: Vec<f32>,
    start_icon_widths_px: Vec<f32>,
    start_icon_lefts_px: Vec<f32>,
    title_width_px: f32,
    title_left_px: f32,
    title_right_px: f32,
    title_top_px: f32,
    end_icon_widths_px: Vec<f32>,
    end_icon_lefts_px: Vec<f32>,
    end_icon_tops_px: Vec<f32>,
    start_icon_tops_px: Vec<f32>,
}

fn measured_titlebar_flex_case(case: TitlebarFlexCase) -> TitlebarFlexReport {
    let (mut world, mut root, container) = flex_row_container_setup();
    let mut start_icons = Vec::with_capacity(case.start_icon_count);
    let mut end_icons = Vec::with_capacity(case.end_icon_count);

    assert_set_flex_row_container_width(
        &mut root,
        container,
        &format!("{}px", case.container_width_px),
    );
    assert_style_set(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);

    for _ in 0..case.start_icon_count {
        let icon = assert_append_div_to(&mut root, container);
        assert_set_inflexible_row_item(
            &mut root,
            icon,
            TITLEBAR_ICON_BASIS_PX,
            TITLEBAR_ICON_HEIGHT_PX,
        );
        start_icons.push(icon);
    }

    let title_host = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_style_set(&mut root, title_host, "flex-basis", "0px");
    assert_style_set(&mut root, title_host, "flex-grow", "1");
    assert_style_set(&mut root, title_host, "flex-shrink", "1");
    assert_style_set(&mut root, title_host, "overflow", "hidden");
    let _title_text = assert_append_text_to(&mut root, title_host, "Camera");

    for _ in 0..case.end_icon_count {
        let icon = assert_append_div_to(&mut root, container);
        assert_set_inflexible_row_item(
            &mut root,
            icon,
            TITLEBAR_ICON_BASIS_PX,
            TITLEBAR_ICON_HEIGHT_PX,
        );
        end_icons.push(icon);
    }

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let mut item_keys = start_icons.clone();
    item_keys.push(title_host);
    item_keys.extend(end_icons.iter().copied());
    assert_titlebar_dom_order_in_tree(&root, &item_keys);

    let title_width_px = assert_node_layout(&root, title_host).width_px;
    let title_left_px = root
        .layer_offset_px(title_host)
        .expect("title host role should be a live layer root")[0];
    TitlebarFlexReport {
        case_name: case.name,
        container_width_px: assert_node_layout(&root, container).width_px,
        item_widths_px: titlebar_item_widths(&root, &start_icons, title_host, &end_icons),
        start_icon_widths_px: measured_item_widths(&root, &start_icons),
        start_icon_lefts_px: measured_item_lefts(&root, &start_icons),
        title_width_px,
        title_left_px,
        title_right_px: title_left_px + title_width_px,
        title_top_px: root
            .layer_offset_px(title_host)
            .expect("title host role should be a live layer root")[1],
        end_icon_widths_px: measured_item_widths(&root, &end_icons),
        end_icon_lefts_px: measured_item_lefts(&root, &end_icons),
        end_icon_tops_px: measured_item_tops(&root, &end_icons),
        start_icon_tops_px: measured_item_tops(&root, &start_icons),
    }
}

fn assert_titlebar_icon_positions(case: TitlebarFlexCase, report: &TitlebarFlexReport) {
    for (index, left_px) in report.start_icon_lefts_px.iter().enumerate() {
        assert_close(
            *left_px,
            TITLEBAR_ICON_WIDTH_PX * non_negative_f32_from_usize(index),
            "titlebar canary start icon left",
        );
    }

    let title_right_px = report.title_left_px + report.title_width_px;
    for (index, left_px) in report.end_icon_lefts_px.iter().enumerate() {
        assert_close(
            *left_px,
            title_right_px + (TITLEBAR_ICON_WIDTH_PX * non_negative_f32_from_usize(index)),
            "titlebar canary end icon left",
        );
    }

    assert_eq!(
        report.start_icon_widths_px.len() + 1 + report.end_icon_widths_px.len(),
        case.start_icon_count + 1 + case.end_icon_count,
        "titlebar canary should report one width per rendered flex item",
    );
}

fn assert_titlebar_cross_axis_tops(report: &TitlebarFlexReport) {
    for top_px in &report.start_icon_tops_px {
        assert_close(*top_px, 0.0, "titlebar canary start icon top");
    }
    assert_close(report.title_top_px, 0.0, "titlebar canary title top");
    for top_px in &report.end_icon_tops_px {
        assert_close(*top_px, 0.0, "titlebar canary end icon top");
    }
}

fn assert_titlebar_dom_order_in_tree(root: &RootDom, item_keys: &[DomNodeSlotKey]) {
    for window in item_keys.windows(2) {
        assert_eq!(
            next_sibling_key(root, window[0]),
            Some(window[1]),
            "titlebar canary should keep authored DOM sibling order",
        );
    }
    assert_eq!(
        next_sibling_key(
            root,
            *item_keys
                .last()
                .expect("titlebar canary should keep at least one flex item"),
        ),
        None,
        "titlebar canary should preserve the final DOM sibling terminator",
    );
}

fn measured_item_widths(root: &RootDom, keys: &[DomNodeSlotKey]) -> Vec<f32> {
    keys.iter()
        .map(|key| assert_node_layout(root, *key).width_px)
        .collect()
}

fn measured_item_lefts(root: &RootDom, keys: &[DomNodeSlotKey]) -> Vec<f32> {
    keys.iter()
        .map(|key| {
            node_box(root, *key)
                .expect("icon role should expose a box model")
                .border_box
                .left_px
        })
        .collect()
}

fn measured_item_tops(root: &RootDom, keys: &[DomNodeSlotKey]) -> Vec<f32> {
    keys.iter()
        .map(|key| {
            node_box(root, *key)
                .expect("icon role should expose a box model")
                .border_box
                .top_px
        })
        .collect()
}

fn titlebar_item_widths(
    root: &RootDom,
    start_icons: &[DomNodeSlotKey],
    title_host: DomNodeSlotKey,
    end_icons: &[DomNodeSlotKey],
) -> Vec<f32> {
    let mut widths = measured_item_widths(root, start_icons);
    widths.push(assert_node_layout(root, title_host).width_px);
    widths.extend(measured_item_widths(root, end_icons));
    widths
}

fn assert_widths_close(actual_widths_px: &[f32], expected_widths_px: &[f32], label: &str) {
    assert_eq!(
        actual_widths_px.len(),
        expected_widths_px.len(),
        "{label} length",
    );
    for (index, (actual_width_px, expected_width_px)) in actual_widths_px
        .iter()
        .zip(expected_widths_px.iter())
        .enumerate()
    {
        assert_close(
            *actual_width_px,
            *expected_width_px,
            &format!("{label} index {index}"),
        );
    }
}

fn non_negative_f32_from_usize(value: usize) -> f32 {
    let value_u16 = u16::try_from(value).expect("titlebar flex proof counts should fit within u16");
    f32::from(value_u16)
}

// CSS Flexbox 1 §5.4 (`order`) and §5.4.1 (order-modified painting): flex
// items are laid out and painted in order-modified document order — ascending
// `order` (negatives first), then source order for ties. Closest WPT oracle:
// `css/css-flexbox/order-*` / `flex-order.html` (order-modified layout) and the
// stacking/paint cases of §5.4.1; Gecko and Blink both apply the order-modified
// sequence to painting.
//
// Relabeled from `off_spec_flex_paint_ignores_order_modified_sequence` in PR 3
// step 2: the paint pass now derives the order-modified sequence itself (the
// `order_modified_children` paint helper) instead of walking the DOM tree, so
// every row below is on-spec.
//
// Rows (a)–(c) pin the order-modified layout edge cases on-spec:
// `order_modified_measured_items` (`flex.rs:1520`) uses a stable
// `sort_by_key(|item| item.order)`, so negative orders lead, equal orders keep
// DOM order, and line membership follows the order-modified sequence.
//
// Row (d) pins the paint side: overlapping items (negative margins) stage in
// order-modified order — the lower-order item paints first and the
// higher-order item paints last (on top of it).
#[test]
fn flex_paint_and_layout_follow_order_modified_sequence() {
    let rows: [OrderPaintRow; 4] = [
        (
            "negative_order_values_lead",
            assert_negative_order_values_lead,
        ),
        (
            "equal_orders_keep_dom_stability",
            assert_equal_orders_keep_dom_stability,
        ),
        (
            "order_wrap_line_membership_follows_order_modified_sequence",
            assert_order_wrap_line_membership_follows_order_modified_sequence,
        ),
        (
            "overlapping_items_paint_in_order_modified_sequence",
            assert_overlapping_items_paint_in_order_modified_sequence,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named order-edge-case probe: row name plus a runner.
type OrderPaintRow = (&'static str, fn(&str));

fn assert_negative_order_values_lead(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "50px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "height", "20px");
    assert_style_set(&mut root, first, "order", "-1");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "50px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "height", "20px");
    assert_style_set(&mut root, second, "order", "0");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the −1-order first item leads the 0-order second item.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        50.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        50.0,
        0.0,
        50.0,
        20.0,
        &format!("{name}: second item"),
    );
}

fn assert_equal_orders_keep_dom_stability(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    for basis in ["50px", "50px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", "0");
        assert_style_set(&mut root, item, "height", "20px");
        assert_style_set(&mut root, item, "order", "1");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: equal orders keep DOM (source) order via the stable sort.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        50.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        50.0,
        0.0,
        50.0,
        20.0,
        &format!("{name}: second item"),
    );
}

fn assert_order_wrap_line_membership_follows_order_modified_sequence(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "60px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "40px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "40px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "height", "20px");
    assert_style_set(&mut root, first, "order", "1");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "40px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "height", "20px");
    assert_style_set(&mut root, second, "order", "0");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the 0-order second item takes line 1 and the 1-order first item
    // wraps to line 2, because line membership follows the order-modified
    // sequence (0-order first, then 1-order). The paint walk now stages in the
    // same order-modified sequence, so the 0-order item paints first.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        40.0,
        20.0,
        &format!("{name}: 0-order item line 1"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        20.0,
        40.0,
        20.0,
        &format!("{name}: 1-order item line 2"),
    );
}

fn assert_overlapping_items_paint_in_order_modified_sequence(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "50px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "height", "20px");
    assert_style_set(&mut root, first, "order", "2");
    assert_style_set(&mut root, first, "background-color", "#ff0000");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "50px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "height", "20px");
    assert_style_set(&mut root, second, "order", "1");
    assert_style_set(&mut root, second, "margin-left", "-20px");
    assert_style_set(&mut root, second, "background-color", "#00ff00");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two overlapping items");
    // On-spec: the order-modified sequence stages the green order-1 item
    // (DOM-second) first and the red order-2 item (DOM-first) last, so the
    // higher-order item paints on top.
    assert_rect_op_fill(
        &rects[1],
        [0.0, 1.0, 0.0, 1.0],
        &format!("{name}: green order-1 item paints first"),
    );
    assert_rect_op_fill(
        &rects[2],
        [1.0, 0.0, 0.0, 1.0],
        &format!("{name}: red order-2 item paints last"),
    );
}

// CSS Flexbox 1 §5.4.1 fast path (flex arc step 2): when every flex item
// shares the default `order: 0`, the order-modified sequence IS DOM order and
// the paint walk must not reorder. Pinned so the `order_modified_children`
// fast path (equal orders → DOM order) cannot silently break into the slow
// path or an order permutation.
#[test]
fn flex_paint_all_default_orders_stage_in_dom_order() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, first, "50px", "20px");
    assert_style_set(&mut root, first, "background-color", "#ff0000");
    let second = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, second, "50px", "20px");
    assert_style_set(&mut root, second, "background-color", "#00ff00");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "container + two default-order items");
    // On-spec: equal (default) orders keep DOM order.
    assert_rect_op_fill(
        &rects[1],
        [1.0, 0.0, 0.0, 1.0],
        "DOM-first default-order item",
    );
    assert_rect_op_fill(
        &rects[2],
        [0.0, 1.0, 0.0, 1.0],
        "DOM-second default-order item",
    );
}

// CSS Flexbox 1 §5.4.1 layout-clean path (flex arc step 2 amendment): paint
// derives the order-modified sequence from the children's computed `order`
// values itself, NOT from the update-scoped order lane. The lane resets at
// `RootDom::update`, so a paint-dirty-only update (a color change that moves no
// layout) has an EMPTY lane — if paint consumed the lane, the staged sequence
// would revert to DOM order on exactly this path. This proof mutates only a
// paint-affecting property (background-color) on an order-bearing container and
// asserts the staged op sequence stays order-modified.
#[test]
fn flex_paint_keeps_order_modified_sequence_on_paint_only_update() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, first, "50px", "20px");
    assert_style_set(&mut root, first, "order", "2");
    assert_style_set(&mut root, first, "background-color", "#ff0000");
    let second = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, second, "50px", "20px");
    assert_style_set(&mut root, second, "order", "1");
    assert_style_set(&mut root, second, "margin-left", "-20px");
    assert_style_set(&mut root, second, "background-color", "#00ff00");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    {
        let plans = take_staged_layer_paint_plans(&root);
        let rects = &plans[0].rects;
        assert_eq!(
            rects.len(),
            3,
            "first update: container + two overlapping items"
        );
        assert_rect_op_fill(
            &rects[1],
            [0.0, 1.0, 0.0, 1.0],
            "first update: green order-1 first",
        );
        assert_rect_op_fill(
            &rects[2],
            [1.0, 0.0, 0.0, 1.0],
            "first update: red order-2 last",
        );
    }

    // Paint-affecting-only mutation: the background-color write moves the
    // visual group (A7 group-diff color → paint) without arming layout.
    assert_style_set(&mut root, first, "background-color", "#0000ff");
    assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    {
        let plans = take_staged_layer_paint_plans(&root);
        let rects = &plans[0].rects;
        assert_eq!(
            rects.len(),
            3,
            "paint-only update: container + two overlapping items",
        );
        assert_rect_op_fill(
            &rects[1],
            [0.0, 1.0, 0.0, 1.0],
            "paint-only update: green order-1 still first",
        );
        assert_rect_op_fill(
            &rects[2],
            [0.0, 0.0, 1.0, 1.0],
            "paint-only update: red order-2 (now blue) still last",
        );
    }
}

// CSS Flexbox 1 §7.3.3 (flex-basis): `content` indicates automatic sizing
// based on the item's content — typically the max-content size; `<length>`
// units (em included) resolve against the item's font size; `0` sets a zero
// base. Closest WPT oracle: `css/css-flexbox/flex-basis-*` (the `content`
// keyword resolves to max-content). Gecko (`FlexItem::GetFlexBaseSize`,
// `nsFlexContainerFrame.cpp`) and Blink (`flex_layout_algorithm.cc` flex-basis
// resolution) both size `content` from the content's max-content.
//
// NOT OFF-SPEC (Run A F11 false positive): the coverage matrix suspected
// `flex-basis: content` resolved to an available-width (fit-content) measure
// rather than max-content (`preferred_flex_basis_px`/`measured_flex_base_size_px`,
// `flex.rs:1233`/`flex.rs:1241`). The pinned behavior shows `content` measures
// the max-content size — a 100px container's `content`-basis text item measures
// its full ~140.5px max-content width — so the family lands on-spec, with no
// `off_spec_` name and no registry row. Rows (c)/(d) pin the em and zero unit
// forms alongside.
#[test]
fn flex_basis_content_and_unit_forms() {
    let rows: [FlexBasisRow; 4] = [
        (
            "content_resolves_to_max_content_on_text_item",
            assert_content_resolves_to_max_content_on_text_item,
        ),
        (
            "content_resolves_to_max_content_on_block_item",
            assert_content_resolves_to_max_content_on_block_item,
        ),
        (
            "em_basis_resolves_against_own_font_size",
            assert_em_basis_resolves,
        ),
        (
            "zero_basis_grows_to_fill_free_space",
            assert_zero_basis_grows,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named flex-basis form probe: row name plus a runner.
type FlexBasisRow = (&'static str, fn(&str));

fn assert_content_resolves_to_max_content_on_text_item(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "content");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "height", "20px");
    let text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: `content` measures the max-content width, overflowing the 100px
    // container instead of shrinking to fit it.
    let max_content_px = assert_text_intrinsic(&root, text).max_content_width_px;
    assert_close(
        rects[1].size_px[0],
        max_content_px,
        &format!("{name}: item width equals text max-content"),
    );
}

fn assert_content_resolves_to_max_content_on_block_item(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "content");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "height", "20px");
    let child = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, child, "width", "40px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + item + child");
    // On-spec: `content` measures the block item's max-content (its 40px child).
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 40.0, 20.0, &format!("{name}: item"));
}

fn assert_em_basis_resolves(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "font-size", "10px");
    assert_style_set(&mut root, item, "flex-basis", "3em");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: 3em resolves against the item's own 10px font size → 30px.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, &format!("{name}: item"));
}

fn assert_zero_basis_grows(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "0px");
    assert_style_set(&mut root, item, "flex-grow", "1");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: a zero base plus grow:1 fills the container's 200px free space.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 20.0, &format!("{name}: item"));
}

// CSS Align 3 §5.6 (distributed alignment fallback) × CSS Flexbox 1 §8.2/§8.4:
// with negative free space, `space-between` falls back to flex-start and
// `center`/`space-around`/`space-evenly` clamp to 0 (Blink
// `InitialContentPositionOffset` `ClampNegativeToZero`; Gecko safe-center
// collapses to start at ≤0). Closest WPT oracle:
// `css/css-flexbox/justify-content-*` and `align-content-*` negative free-space
// cases; Blink `flex_layout_algorithm.cc:1678/1684`
// (`(free/2).ClampNegativeToZero()`) and Gecko
// `nsFlexContainerFrame.cpp:3553-3581, 3736-3762` implement the clamp.
//
// ON-SPEC (relabeled from
// `off_spec_space_evenly_negative_free_space_does_not_fall_back_to_center` in
// flex arc step 8): `initial_main_axis_alignment_offset_px` (`flex.rs`) clamps
// negative remaining main space to 0 for `Center`/`SpaceAround`/`SpaceEvenly`
// (`remaining.max(0.0)`), so the initial offset is 0 = flex-start. The cross
// side already clamped via `remaining_cross_axis_line_pack_space_px`'s
// `.max(0.0)` — engine-matching, no change. The former `space-around` "on-spec
// control" (pinned at −10) was itself engine-divergent (Blink clamps center's
// negative to 0) and is re-pinned to 0.
#[test]
fn space_distribution_negative_free_space_clamps_to_flex_start() {
    let rows: [SpaceDistributionRow; 6] = [
        (
            "justify_space_evenly_negative_free_space",
            assert_justify_space_evenly_negative_free_space,
        ),
        (
            "justify_space_around_negative_free_space",
            assert_justify_space_around_negative_free_space,
        ),
        (
            "justify_space_between_negative_free_space",
            assert_justify_space_between_negative_free_space,
        ),
        (
            "justify_center_negative_free_space",
            assert_justify_center_negative_free_space,
        ),
        (
            "align_content_space_evenly_negative_cross_space",
            assert_align_content_space_evenly_negative_cross_space,
        ),
        (
            "align_content_space_around_negative_cross_space",
            assert_align_content_space_around_negative_cross_space,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named negative-free-space distribution probe: row name plus a runner.
type SpaceDistributionRow = (&'static str, fn(&str));

fn assert_justify_space_evenly_negative_free_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "space-evenly");
    for basis in ["60px", "60px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", "0");
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // 0-clamp: −20px free space clamps to 0, so the initial offset is 0
    // (flex-start) instead of the former −6.667px.
    assert_close(
        rects[1].position_px[0],
        0.0,
        &format!("{name}: first item left"),
    );
    assert_close(
        rects[2].position_px[0],
        60.0,
        &format!("{name}: second item left"),
    );
}

fn assert_justify_space_around_negative_free_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "space-around");
    for basis in ["60px", "60px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", "0");
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // 0-clamp (re-pinned from the −10 "on-spec control," which was
    // engine-divergent per Blink's center clamp): −20px free space clamps to 0.
    assert_close(
        rects[1].position_px[0],
        0.0,
        &format!("{name}: first item left"),
    );
    assert_close(
        rects[2].position_px[0],
        60.0,
        &format!("{name}: second item left"),
    );
}

fn assert_justify_space_between_negative_free_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "space-between");
    for basis in ["60px", "60px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", "0");
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // Correct control: space-between falls back to flex-start (0px).
    assert_close(
        rects[1].position_px[0],
        0.0,
        &format!("{name}: first item left"),
    );
    assert_close(
        rects[2].position_px[0],
        60.0,
        &format!("{name}: second item left"),
    );
}

fn assert_justify_center_negative_free_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "center");
    for basis in ["60px", "60px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", "0");
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // 0-clamp: Blink clamps center's negative free space to 0, so the initial
    // offset is 0 (flex-start) instead of the center fallback −10px.
    assert_close(
        rects[1].position_px[0],
        0.0,
        &format!("{name}: first item left"),
    );
    assert_close(
        rects[2].position_px[0],
        60.0,
        &format!("{name}: second item left"),
    );
}

fn assert_align_content_space_evenly_negative_cross_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "30px");
    assert_style_set(&mut root, container, "align-content", "space-evenly");
    for basis in ["30px", "30px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // 0-clamp: the −10px cross space is already clamped to 0 before
    // space-evenly (`remaining_cross_axis_line_pack_space_px`), so the lines
    // pack at 0px — engine-matching, no change.
    assert_close(
        rects[1].position_px[1],
        0.0,
        &format!("{name}: first line top"),
    );
    assert_close(
        rects[2].position_px[1],
        20.0,
        &format!("{name}: second line top"),
    );
}

fn assert_align_content_space_around_negative_cross_space(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "30px");
    assert_style_set(&mut root, container, "align-content", "space-around");
    for basis in ["30px", "30px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // 0-clamp control: the −10px cross space is already clamped to 0
    // (`remaining_cross_axis_line_pack_space_px`), so the lines pack at 0px.
    assert_close(
        rects[1].position_px[1],
        0.0,
        &format!("{name}: first line top"),
    );
    assert_close(
        rects[2].position_px[1],
        20.0,
        &format!("{name}: second line top"),
    );
}

// CSS Flexbox 1 §9.2 (line length determination) + CSS 2.1 §10.7 (minimum and
// maximum heights): the flex container's main and cross sizes used for line
// breaking, justify-content, and align-* distribution are its USED sizes,
// clamped by min/max. Gecko (`FlexLine` construction and the cross-axis tracker
// in `nsFlexContainerFrame.cpp`) and Blink (`flex_layout_algorithm.cc` uses the
// clamped container size) both clamp before distribution. Closest WPT oracle:
// `css/css-flexbox/` container min/max size families; the justify/line-break
// interaction is cited from §9.2 + §10.7 directly.
//
// ON-SPEC (relabeled from `off_spec_flex_container_ignores_min_max_main_cross_sizes`
// in flex arc step 6): `entered_container_main_size` and
// `entered_container_cross_size` (`flex.rs`) now clamp the vertical arm's raw
// `entered.style.height_px` through `clamp_length` against the resolved
// `min_height_px`/`max_height_px` — the same clamp the block machinery applies
// — so justify/line-breaking/align distribution consume the USED (clamped)
// container size, matching the painted box.
#[test]
fn flex_container_used_sizes_are_min_max_clamped() {
    let rows: [ContainerMinMaxRow; 4] = [
        (
            "column_min_height_justify_uses_clamped_main_size",
            assert_column_min_height_justify_uses_clamped_main_size,
        ),
        (
            "column_max_height_justify_uses_clamped_main_size",
            assert_column_max_height_justify_uses_clamped_main_size,
        ),
        (
            "column_wrap_min_height_breaks_at_clamped_main_size",
            assert_column_wrap_min_height_breaks_at_clamped_main_size,
        ),
        (
            "row_min_height_stretch_uses_clamped_cross_size",
            assert_row_min_height_stretch_uses_clamped_cross_size,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named container min/max probe: row name plus a runner.
type ContainerMinMaxRow = (&'static str, fn(&str));

fn assert_column_min_height_justify_uses_clamped_main_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "width", "50px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "min-height", "100px");
    assert_style_set(&mut root, container, "justify-content", "space-between");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "width", "50px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "width", "50px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // The container's USED main size is its clamped 100px min-height, so
    // justify distributes the 80px free space and lands the second item at 90px.
    assert_rect_op_geometry(
        &rects[0],
        0.0,
        0.0,
        50.0,
        100.0,
        &format!("{name}: container"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        90.0,
        50.0,
        10.0,
        &format!("{name}: second item"),
    );
}

fn assert_column_max_height_justify_uses_clamped_main_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "width", "50px");
    assert_style_set(&mut root, container, "height", "100px");
    assert_style_set(&mut root, container, "max-height", "50px");
    assert_style_set(&mut root, container, "justify-content", "space-between");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "width", "50px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "width", "50px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // The container's USED main size is its clamped 50px max-height, so justify
    // distributes the 30px free space and lands the second item at 40px.
    assert_rect_op_geometry(
        &rects[0],
        0.0,
        0.0,
        50.0,
        50.0,
        &format!("{name}: container"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        40.0,
        50.0,
        10.0,
        &format!("{name}: second item"),
    );
}

fn assert_column_wrap_min_height_breaks_at_clamped_main_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "min-height", "100px");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    for basis in ["40px", "40px", "40px"] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "width", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 4, "{name}: container + three items");
    // The container's USED main size is its clamped 100px min-height, so the
    // first two 40px items fit one column and only the third wraps to a second.
    assert_rect_op_geometry(
        &rects[0],
        0.0,
        0.0,
        100.0,
        100.0,
        &format!("{name}: container"),
    );
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        40.0,
        40.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        40.0,
        40.0,
        40.0,
        &format!("{name}: second item"),
    );
    assert_rect_op_geometry(
        &rects[3],
        40.0,
        0.0,
        40.0,
        40.0,
        &format!("{name}: third item"),
    );
}

fn assert_row_min_height_stretch_uses_clamped_cross_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "min-height", "100px");
    assert_style_set(&mut root, container, "align-items", "stretch");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "30px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // The container's USED cross size is its clamped 100px min-height, so the
    // stretch target is 100px and the item stretches to it.
    assert_rect_op_geometry(
        &rects[0],
        0.0,
        0.0,
        200.0,
        100.0,
        &format!("{name}: container"),
    );
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 100.0, &format!("{name}: item"));
}

// CSS Flexbox 1 §8.3/§9.2/§9.4: a flex item's cross size is clamped by its
// min/max cross size (min/max-height for a row container) both in the
// hypothetical cross size that determines the line's cross size and in the
// final aligned/stretched cross size. Closest WPT oracle:
// `css/css-flexbox/flex-minimum-size-*` and the cross-size family; Gecko
// (`FlexItem` cross sizing, `nsFlexContainerFrame.cpp`) and Blink
// (`flex_layout_algorithm.cc` cross-axis step) both clamp.
//
// NOT OFF-SPEC (Run A F6 false positive): the coverage matrix suspected that
// `stretched_flex_item_content_cross_size_px` (`flex.rs:2600`) and the
// line-cross accumulation (`flex.rs:1589`) never clamp by item min/max cross.
// The pinned behavior shows the clamps DO apply — the block measure
// (`resolved_content_size_px`, `flow.rs`) clamps the measured cross size that
// feeds line-cross accumulation, and the block relayout clamps the stretch
// override — so stretch is capped by max-height and floored by min-height, and
// a wrapped line's cross size reflects the item's clamped cross. Pinned
// on-spec, so no `off_spec_` name and no registry row.
#[test]
fn cross_axis_min_max_clamps_apply_via_block_measure_and_relayout() {
    let rows: [CrossClampRow; 4] = [
        (
            "stretch_capped_by_max_height",
            assert_stretch_capped_by_max_height,
        ),
        (
            "stretch_floored_by_min_height",
            assert_stretch_floored_by_min_height,
        ),
        (
            "max_height_item_sets_wrapped_line_cross_size",
            assert_max_height_item_sets_wrapped_line_cross_size,
        ),
        (
            "min_height_item_sets_wrapped_line_cross_size",
            assert_min_height_item_sets_wrapped_line_cross_size,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named cross-axis min/max clamp probe: row name plus a runner.
type CrossClampRow = (&'static str, fn(&str));

fn assert_stretch_capped_by_max_height(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "100px");
    assert_style_set(&mut root, container, "align-items", "stretch");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "30px");
    assert_style_set(&mut root, item, "max-height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: stretch to the 100px line cross is capped by max-height 20px.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, &format!("{name}: item"));
}

fn assert_stretch_floored_by_min_height(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "10px");
    assert_style_set(&mut root, container, "align-items", "stretch");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "30px");
    assert_style_set(&mut root, item, "min-height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: stretch to the 10px line cross is floored by min-height 20px.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, &format!("{name}: item"));
}

fn assert_max_height_item_sets_wrapped_line_cross_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "40px");
    assert_style_set(&mut root, first, "max-height", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "30px");
    assert_style_set(&mut root, second, "height", "10px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the first item's max-height 20px sets line 1's cross size, so
    // the wrapped second item sits at top 20px.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        30.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        20.0,
        30.0,
        10.0,
        &format!("{name}: second item"),
    );
}

fn assert_min_height_item_sets_wrapped_line_cross_size(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "5px");
    assert_style_set(&mut root, first, "min-height", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "30px");
    assert_style_set(&mut root, second, "height", "10px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the first item's min-height 20px sets line 1's cross size, so
    // the wrapped second item sits at top 20px.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        30.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        20.0,
        30.0,
        10.0,
        &format!("{name}: second item"),
    );
}

// CSS Flexbox 1 §9.2 (line length determination) and §4.5 (automatic minimum
// size): a flex item's OUTER hypothetical main size is its flex base clamped by
// min/max AND floored by the used automatic minimum size (content-based for
// visible overflow), and line breaking consumes that floored size. Gecko
// (`FlexItem::GetFlexBaseSize`/`FlexLine` construction in
// `nsFlexContainerFrame.cpp`) and Blink (`FlexItem` sizing in
// `flex_layout_algorithm.cc`) both floor the hypothetical before line
// collection. Closest WPT oracle: `flexbox-min-width-auto-001.html` (the
// automatic-minimum floor); no single WPT pairs the line-break interaction,
// cited from §9.2 directly.
//
// ON-SPEC in TWO PARTS (was `off_spec_hypothetical_main_size_not_floored_by_auto_min`):
// rows (a)(c) land here at step 4 — the outer hypothetical is floored, so a
// wrapping container breaks earlier (a) and a content-min exceeding the
// container overflows from the floored size (c). Row (b) (the INFLEXIBLE item
// staying at base) is the step-5 §9.7 hypothetical-seed switch, the same
// scenario as `off_spec_inflexible_items_skip_hypothetical_clamps`'s
// `assert_auto_min_not_applied_to_inflexible_item` — it does NOT move here.
#[test]
fn flex_hypothetical_main_size_floored_by_auto_min() {
    let rows: [AutoMinFloorRow; 2] = [
        (
            "wrapping_line_break_floors_at_auto_min",
            assert_wrapping_line_break_floors_at_auto_min,
        ),
        (
            "content_min_exceeding_container_overflows_from_floor",
            assert_content_min_exceeding_container_overflows_from_floor,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named automatic-minimum-floor probe: row name plus a runner.
type AutoMinFloorRow = (&'static str, fn(&str));

fn assert_wrapping_line_break_floors_at_auto_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "60px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "overflow", "visible");
    assert_style_set(&mut root, first, "height", "20px");
    let first_text = assert_append_text_to(&mut root, first, "UnbreakableFlexLabel");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let content_min_px = assert_text_intrinsic(&root, first_text).max_content_width_px;
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the first item's outer hypothetical floors at its ~140.5px
    // automatic minimum, which exceeds the 100px container, so it breaks onto
    // its own line; the second item wraps onto line 2 (the container's default
    // `align-content: stretch` stretches both lines to 30px, so line 2 starts
    // at y=30).
    assert_close(
        rects[1].size_px[0],
        content_min_px,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        30.0,
        10.0,
        20.0,
        &format!("{name}: second item wraps to line 2"),
    );
}

fn assert_content_min_exceeding_container_overflows_from_floor(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "10px");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_style_set(&mut root, item, "height", "20px");
    let item_text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let content_min_px = assert_text_intrinsic(&root, item_text).max_content_width_px;
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: the item floors at its ~140.5px content-based minimum, which
    // exceeds the 100px container and overflows from the floored size.
    assert_close(
        rects[1].size_px[0],
        content_min_px,
        &format!("{name}: item overflows from its floored size"),
    );
}

// CSS Flexbox 1 §4.5 automatic minimum size: a visible-overflow flex item
// that is itself a flex container uses its content-based minimum size before
// shrink. The flex arc step 4 derivation reads the item's record intrinsic
// (`h_min_content_px`, the §9.9 main-size fold) for every flex-flow shape —
// no shape gates remain. Gecko computes the automatic minimum in
// `nsFlexContainerFrame.cpp` (`GenerateFlexItemForChild` → content-based
// min-size); Blink in `flex_layout_algorithm.cc` (`ConstructAndAppendFlexItems`
// → `FlexItem::UpdateAutoMinSize`). Closest WPT family:
// `css/css-flexbox/min-width-auto-*`.
//
// ON-SPEC (was `off_spec_flex_auto_min_unsupported_shapes_use_measured_size`):
// every flex-flow shape now computes the true §4.5 `min(min_content, definite)`
// — the column/reverse/wrap/rtl item's widest-child 40px, the grandchild
// recursion's 30px leaf. The measured-size over-estimate died with the
// graceful-degradation gate.
#[test]
fn flex_auto_min_container_shapes_use_content_based_minimum() {
    let rows: [ContainerShapeAutoMinRow; 5] = [
        (
            "column_item_uses_content_min",
            assert_column_item_uses_content_min,
        ),
        ("row_reverse_item_variant", assert_row_reverse_item_variant),
        ("wrap_item_variant", assert_wrap_item_variant),
        ("rtl_row_item_variant", assert_rtl_row_item_variant),
        ("grandchild_recursion", assert_grandchild_recursion),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named flex-flow-shape auto-min probe: row name plus a runner.
type ContainerShapeAutoMinRow = (&'static str, fn(&str));

/// Builds the shared (a)–(d) scenario: an outer row whose first item is a flex
/// container with `configure` shaping it, a 40px inner child, and a 10px
/// inflexible sibling. Asserts the item floors at its content-based minimum
/// (40px) and the sibling is positioned after it.
fn assert_container_shape_uses_content_min(
    name: &str,
    configure: fn(&mut RootDom, DomNodeSlotKey),
) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "flex");
    configure(&mut root, item);
    let inner = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, inner, "width", "40px");
    assert_style_set(&mut root, inner, "height", "10px");
    let sibling = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item, rects[2] = inner, rects[3] = sibling.
    // On-spec: the item floors at its widest-child 40px content-based minimum,
    // so the sibling sits at 40px.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 40.0, 20.0, &format!("{name}: item"));
    assert_rect_op_geometry(
        &rects[3],
        40.0,
        0.0,
        10.0,
        20.0,
        &format!("{name}: sibling after content-min item"),
    );
}

fn assert_column_item_uses_content_min(name: &str) {
    assert_container_shape_uses_content_min(name, |root, item| {
        assert_style_set(root, item, "flex-direction", "column");
    });
}

fn assert_row_reverse_item_variant(name: &str) {
    assert_container_shape_uses_content_min(name, |root, item| {
        assert_style_set(root, item, "flex-direction", "row-reverse");
    });
}

fn assert_wrap_item_variant(name: &str) {
    assert_container_shape_uses_content_min(name, |root, item| {
        assert_style_set(root, item, "flex-wrap", "wrap");
    });
}

fn assert_rtl_row_item_variant(name: &str) {
    assert_container_shape_uses_content_min(name, |root, item| {
        assert_style_set(root, item, "direction", "rtl");
    });
}

fn assert_grandchild_recursion(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "flex");
    let child = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, child, "display", "flex");
    let grandchild = assert_append_div_to(&mut root, child);
    assert_style_set(&mut root, grandchild, "display", "flex");
    assert_style_set(&mut root, grandchild, "flex-direction", "column");
    let leaf = assert_append_div_to(&mut root, grandchild);
    assert_style_set(&mut root, leaf, "width", "30px");
    assert_style_set(&mut root, leaf, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item, rects[2] = child, rects[3] = grandchild, rects[4] = leaf.
    // On-spec: the recursion folds the grandchild's 30px leaf up to the item's
    // content-based minimum (30px); the leaf keeps its own 30px width.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, &format!("{name}: item"));
    assert_close(
        rects[4].size_px[0],
        30.0,
        &format!("{name}: leaf keeps its own width"),
    );
}

// CSS Flexbox 1 §4.5 automatic minimum size: a visible-overflow flex item that
// is itself a flex container uses its content-based minimum size before shrink.
// The flex arc step 4 derivation reads the item's record intrinsic — grow-blind
// (records carry no flex factors) — so a growing child no longer forces the
// measured-size fallback. Gecko computes the automatic minimum in
// `nsFlexContainerFrame.cpp` (`GenerateFlexItemForChild` → content-based
// min-size); Blink in `flex_layout_algorithm.cc` (`ConstructAndAppendFlexItems`
// → `FlexItem::UpdateAutoMinSize`). Closest WPT family:
// `css/css-flexbox/min-width-auto-*`.
//
// ON-SPEC (was `off_spec_flex_auto_min_growing_children_use_measured_size`):
// a growing child no longer forces the measured-size fallback. Row (a): the
// titlebar item's auto-min becomes the factor-blind content minimum — the
// icon's 16px basis plus the growing title's min-content (0px for an empty
// title) = 16px — so the sibling sits at 16px, not the measured 200px. Row (c)
// is the non-growing control: the content-based path is unchanged.
#[test]
fn flex_auto_min_growing_children_use_content_min() {
    let rows: [GrowingChildAutoMinRow; 3] = [
        (
            "titlebar_growing_title",
            assert_titlebar_growing_title_content_min,
        ),
        (
            "nested_supported_flex_growing_grandchild",
            assert_nested_supported_flex_growing_grandchild,
        ),
        (
            "non_growing_control",
            assert_non_growing_control_keeps_content_based_auto_min,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named growing-child auto-min probe: row name plus a runner.
type GrowingChildAutoMinRow = (&'static str, fn(&str));

/// Row (a): the titlebar shape — outer row, item I (row, no width, visible
/// overflow) with a 16px icon and a `flex-grow: 1` title, plus a 10px sibling.
/// Asserts the item floors at its factor-blind content minimum (16px) and the
/// sibling sits after it.
fn assert_titlebar_growing_title_content_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "flex");
    let icon = assert_append_div_to(&mut root, item);
    assert_set_inflexible_row_item(&mut root, icon, "16px", "10px");
    let title = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, title, "flex-grow", "1");
    assert_style_set(&mut root, title, "height", "10px");
    let sibling = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, item).width_px,
        16.0,
        &format!("{name}: item keeps its content minimum"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = item, rects[2] = icon, rects[3] = sibling
    // (the empty growing title emits no paint rect). The item floors at 16px
    // (icon basis + empty title min-content), the icon keeps its 16px basis,
    // and the sibling sits at 16px.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 16.0, 20.0, &format!("{name}: item"));
    assert_rect_op_geometry(&rects[2], 0.0, 0.0, 16.0, 10.0, &format!("{name}: icon"));
    assert_rect_op_geometry(
        &rects[3],
        16.0,
        0.0,
        10.0,
        20.0,
        &format!("{name}: sibling after content-min item"),
    );
}

/// Row (b): the deeper recursion — outer row, item I (row) → child C (row) →
/// growing grandchild G. Asserts no panic and that the growing grandchild's
/// factor-blind content minimum threads through the recursion.
fn assert_nested_supported_flex_growing_grandchild(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "flex");
    let child = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, child, "display", "flex");
    let grandchild = assert_append_div_to(&mut root, child);
    assert_style_set(&mut root, grandchild, "flex-grow", "1");
    assert_style_set(&mut root, grandchild, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_width_px = assert_node_layout(&root, item).width_px;
    // The whole chain is content-empty, so the factor-blind content minimum is
    // 0 (no paint rects are emitted for the empty item/child/grandchild).
    assert_close(item_width_px, 0.0, &format!("{name}: item content minimum"));
}

/// Row (c): non-growing control. A supported row item with a non-growing text
/// child keeps its true content-based auto-min (the text min-content width)
/// instead of the measured-size fallback — the existing behavior is preserved.
fn assert_non_growing_control_keeps_content_based_auto_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "50px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "flex");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "flex-basis", "80px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    let sibling = assert_append_div_to(&mut root, container);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    let child = assert_append_div_to(&mut root, item);
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "flex-basis", "0px");
    assert_style_set(&mut root, child, "flex-grow", "0");
    assert_style_set(&mut root, child, "flex-shrink", "1");
    let text = assert_append_text_to(
        &mut root,
        child,
        "UnbreakableContentUnbreakableContentUnbreakableContentUnbreakableContentUnbreakableContent",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_min_width_px = assert_text_intrinsic(&root, text).max_content_width_px;
    assert!(
        text_min_width_px > 40.0,
        "control requires text min-content wider than the unclamped shrink target; got {text_min_width_px}",
    );
    assert_close(
        assert_node_layout(&root, item).width_px,
        text_min_width_px,
        &format!("{name}: non-growing item keeps content-based auto-min"),
    );
}

// CSS Flexbox 1 §9.9.3: a flex item's intrinsic contribution is floored by its
// flex base size only when the item is inflexible (won't shrink below base).
// The floor applies to the intrinsic MAIN size — for a column container the
// flex base is vertical and never belongs in the container's HORIZONTAL
// intrinsic sizes. A shrink:0 child's `height: 100px; width: 40px` must
// therefore leave the column's h_min/h_max at 40, not inflate them to 100.
#[test]
fn column_container_intrinsic_sizes_ignore_vertical_flex_base_floor() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let column_item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, column_item, "display", "flex");
    assert_style_set(&mut root, column_item, "flex-direction", "column");
    let child = assert_append_div_to(&mut root, column_item);
    assert_style_set(&mut root, child, "width", "40px");
    assert_style_set(&mut root, child, "height", "100px");
    assert_style_set(&mut root, child, "flex-shrink", "0");
    assert_style_set(&mut root, child, "flex-grow", "0");
    let sibling = assert_append_div_to(&mut root, outer);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The column's horizontal intrinsics are the child's width (40), not the
    // child's vertical flex base (100).
    let record = element_measurement_record(&root, column_item);
    assert_close(
        record.h_min_content_px,
        40.0,
        "column item h_min_content_px",
    );
    assert_close(
        record.h_max_content_px,
        40.0,
        "column item h_max_content_px",
    );

    // Derived auto-basis base / auto-min both read the 40px intrinsic, so the
    // column item floors at 40 and the sibling sits immediately after it.
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 40.0, 20.0, "column item at 40px");
    assert_rect_op_geometry(
        &rects[3],
        40.0,
        0.0,
        10.0,
        20.0,
        "sibling after 40px column item",
    );
}

// Row-axis control for the §9.9.3 gate: the floor still applies for ROW
// containers (horizontal main axis). A shrink:0 child whose flex base is 100px
// floors the row item's h_min at 100 even though its empty content contributes
// 0 — the gate removes the floor for column containers only, not row ones.
#[test]
fn row_container_intrinsic_sizes_floor_at_horizontal_flex_base() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let row_item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, row_item, "display", "flex");
    let child = assert_append_div_to(&mut root, row_item);
    assert_style_set(&mut root, child, "flex-basis", "100px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "flex-shrink", "0");
    assert_style_set(&mut root, child, "flex-grow", "0");
    let sibling = assert_append_div_to(&mut root, outer);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The row's h_min/h_max floor at the child's 100px horizontal flex base.
    let record = element_measurement_record(&root, row_item);
    assert_close(record.h_min_content_px, 100.0, "row item h_min_content_px");
    assert_close(record.h_max_content_px, 100.0, "row item h_max_content_px");

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 20.0, "row item at 100px");
    assert_rect_op_geometry(
        &rects[3],
        100.0,
        0.0,
        10.0,
        20.0,
        "sibling after 100px row item",
    );
}

// CSS Flexbox §4.5: the automatic minimum is a CONTENT-BOX size; when applied
// as a shrink floor against the border-box resolved main size it must add the
// item's own border+padding. A shrinkable item with `padding: 20px` (40px
// horizontal) and unbreakable text of min-content ~140 therefore floors at a
// ~180px border box (140 content + 40 padding), keeping the text un-wrapped.
#[test]
fn padded_visible_overflow_flex_item_floors_shrink_at_content_plus_padding() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");

    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_style_set(&mut root, item, "padding", "20px");
    assert_style_set(&mut root, item, "height", "10px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "flex-basis", "auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_min_px = assert_text_intrinsic(&root, text).min_content_width_px;
    let item_layout = assert_node_layout(&root, item);
    let item_box = node_box(&root, item).expect("item box should exist");
    let content_width_px =
        item_box.border_box.width_px - item_box.padding.left_px - item_box.padding.right_px;

    assert!(
        text_min_px > 20.0,
        "padded auto-min proof requires unbreakable text wider than the container; got {text_min_px}",
    );
    // Border-box floor = min-content (content) + 40px horizontal padding.
    assert_close(
        item_layout.width_px,
        text_min_px + 40.0,
        "padded item border-box width",
    );
    // Content box keeps the full min-content — the text does NOT over-wrap.
    assert_close(
        content_width_px,
        text_min_px,
        "padded item content-box width",
    );
}

// CSS Flexbox §4.5/§9.2 (D4 floor): the OUTER hypothetical main size floors at
// the automatic minimum converted to border-box terms (auto-min is content-box).
// With `padding: 20px`, a wrapping item whose unbreakable text min-content
// exceeds the container floors at min-content + 40px padding, so it still breaks
// onto its own line but at the padded border-box width.
#[test]
fn wrapping_line_break_floor_includes_padding() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "60px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "overflow", "visible");
    // Horizontal-only padding: 40px on the MAIN axis (the D4 floor), without
    // changing the item's cross (height) size and muddying the line geometry.
    assert_style_set(&mut root, first, "padding-left", "20px");
    assert_style_set(&mut root, first, "padding-right", "20px");
    assert_style_set(&mut root, first, "height", "20px");
    let first_text = assert_append_text_to(&mut root, first, "UnbreakableFlexLabel");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "height", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let content_min_px = assert_text_intrinsic(&root, first_text).min_content_width_px;
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "container + two items");
    // The first item's floored outer hypothetical is content-min + 40px
    // horizontal padding (still > 100px), so it occupies line 1 at the padded
    // border-box width and the second item wraps to line 2 (y=30, stretched).
    assert_close(
        rects[1].size_px[0],
        content_min_px + 40.0,
        "first item padded border-box width",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        30.0,
        10.0,
        20.0,
        "second item wraps to line 2",
    );
}

// CSS Flexbox 1 §9.7 step 3 (Resolve Flexible Lengths): an inflexible item
// (zero flex factor, or base size on the "wrong" side of its hypothetical)
// freezes at its HYPOTHETICAL main size — the flex base clamped by min/max and
// floored by the used automatic minimum size (§4.5). No single WPT oracle
// pairs the downstream positioning, so this is cited from §9.7 directly;
// Gecko (`FlexLine::ResolveFlexibleLengths`, `nsFlexContainerFrame.cpp`) and
// Blink (`LineFlexer::ResolveFlexibleLengths`, `line_flexer.cc`) both freeze
// inflexible items at the clamped base before the free-space loop.
//
// ON-SPEC (relabeled from `off_spec_inflexible_items_skip_hypothetical_clamps`
// in flex arc step 5): the §9.7 loop now SEEDS at the D1-floored hypothetical
// main size (`initialize_flex_line_resolution`, `flex.rs`) and the constructor
// freeze keeps that hypothetical, so an inflexible item's §9.7 step-3 clamp is
// never skipped — a 10px-base / 20px-min-width item now pushes the next item to
// 20px (row a), a visible item's content-based automatic minimum now applies
// (row b), a max-width clamp freezes at max (row c), and the column min-height
// mirror freezes at min-height (row d). Row (e) is the no-clamp control.
#[test]
fn inflexible_items_freeze_at_hypothetical_main_size() {
    let rows: [InflexibleRow; 5] = [
        (
            "min_width_clamp_freezes_at_min",
            assert_min_width_clamp_freezes_at_min,
        ),
        (
            "visible_inflexible_item_freezes_at_auto_min",
            assert_visible_inflexible_item_freezes_at_auto_min,
        ),
        (
            "max_width_clamp_freezes_at_max",
            assert_max_width_clamp_freezes_at_max,
        ),
        (
            "column_min_height_clamp_freezes_at_min",
            assert_column_min_height_clamp_freezes_at_min,
        ),
        (
            "no_clamp_control_freezes_at_base",
            assert_no_clamp_control_freezes_at_base,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named inflexible-item clamp probe: row name plus a runner.
type InflexibleRow = (&'static str, fn(&str));

fn assert_min_width_clamp_freezes_at_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "min-width", "20px");
    assert_style_set(&mut root, first, "height", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "30px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the first item freezes at its 20px min-width hypothetical, so
    // the second item sits at 20px (not the 10px base).
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        20.0,
        0.0,
        30.0,
        20.0,
        &format!("{name}: second item"),
    );
}

fn assert_visible_inflexible_item_freezes_at_auto_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "10px");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_style_set(&mut root, item, "height", "20px");
    let text = assert_append_text_to(&mut root, item, "UnbreakableFlexLabel");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let content_min_px = assert_text_intrinsic(&root, text).min_content_width_px;
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec: the inflexible visible item freezes at its content-based
    // automatic minimum — the D1 field floor raises the hypothetical above the
    // 10px base, so the item paints at the content min (not the base).
    assert_close(
        rects[1].size_px[0],
        content_min_px,
        &format!("{name}: item"),
    );
}

fn assert_max_width_clamp_freezes_at_max(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "50px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "max-width", "20px");
    assert_style_set(&mut root, first, "height", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "30px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the first item freezes at its 20px max-width hypothetical, so
    // the second item sits at 20px (not the 50px base).
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        20.0,
        0.0,
        30.0,
        20.0,
        &format!("{name}: second item"),
    );
}

fn assert_column_min_height_clamp_freezes_at_min(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_column_container_size(&mut root, container, "200px", "200px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "flex-shrink", "0");
    assert_style_set(&mut root, first, "flex-grow", "0");
    assert_style_set(&mut root, first, "min-height", "20px");
    assert_style_set(&mut root, first, "width", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "30px");
    assert_style_set(&mut root, second, "flex-shrink", "0");
    assert_style_set(&mut root, second, "flex-grow", "0");
    assert_style_set(&mut root, second, "width", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // Column-axis mirror: the first item freezes at its 20px min-height
    // hypothetical, so the second item sits at 20px top (not the 10px base).
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        &format!("{name}: first item"),
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        20.0,
        20.0,
        30.0,
        &format!("{name}: second item"),
    );
}

fn assert_no_clamp_control_freezes_at_base(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "30px");
    assert_style_set(&mut root, item, "flex-shrink", "0");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item");
    // On-spec control: with no min/max/auto-min constraints, the inflexible
    // item freezes at its base size.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 30.0, 20.0, &format!("{name}: item"));
}

// ON-SPEC (relabeled from `off_spec_abspos_and_display_none_children_occupy_flex_slots`
// in flex arc step 3b): the measurement fold now EXCLUDES abs-pos/`display:
// none` children from item formation (`node_is_excluded_flex_child`,
// `flex_anonymous_run.rs`) and lays out each abs-pos child at its D3 static
// position (the 0×0 order-modified insertion model, `flex.rs`). CSS Flexbox 1
// §4.1 (abs-pos children are out of flow; their only flex involvement is
// reordering + the static-position rectangle), CSS 2.1 §9.2 (`display: none`
// generates no box), CSS Flexbox 1 §9.2 (line length counts only real items).
// Closest WPT oracle: `css/css-flexbox/abspos/`; the gap interaction is cited
// from §9.2 directly (no single WPT pairs it). Gecko skips both in
// `GenerateFlexLines` (`nsFlexContainerFrame.cpp`); Blink skips both in
// `ConstructAndAppendFlexItems` (`flex_layout_algorithm.cc`) and resolves the
// static position in `HandleOutOfFlowPositionedItems` (the `AxisEdge` mapping).
#[test]
fn abspos_and_display_none_children_are_not_flex_items() {
    let rows: [AbsposNoneRow; 7] = [
        (
            "abspos_middle_child_occupies_no_slot",
            assert_abspos_middle_child_no_slot,
        ),
        (
            "abspos_only_child_laid_out_at_static_position",
            assert_abspos_only_child_static_position,
        ),
        (
            "display_none_middle_child_occupies_no_slot",
            assert_display_none_middle_child_no_slot,
        ),
        (
            "display_none_only_child_container_sizes_empty",
            assert_display_none_only_child_empty,
        ),
        (
            "run_joins_across_excluded_child",
            assert_run_joins_across_excluded_child,
        ),
        (
            "static_position_centered_for_abspos_only_child",
            assert_static_position_centered,
        ),
        (
            "static_position_at_end_slot",
            assert_static_position_at_end_slot,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named abs-pos/`display:none` slot probe: row name plus a runner.
type AbsposNoneRow = (&'static str, fn(&str));

fn assert_abspos_middle_child_no_slot(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "20px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "width", "10px");
    assert_style_set(&mut root, abs, "height", "20px");
    let third = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, third, "flex-basis", "40px");
    assert_style_set(&mut root, third, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // The abs-pos child occupies NO slot: rects[0]=container, rects[1]=first,
    // rects[2]=third, with the trailing item at 40 (= 30 + one 10px gap), not
    // 50 (the phantom slot + two gaps).
    assert_eq!(
        plans[0].rects.len(),
        3,
        "{name}: container + two visible items"
    );
    assert_close(
        plans[0].rects[2].position_px[0],
        40.0,
        &format!("{name}: trailing item left after one gap"),
    );
    // The abs child gets its D3 static position (after the first item + gap)
    // and PAINTS in its own layer.
    let abs_box = node_box(&root, abs).expect("abs child box model");
    assert_close(
        abs_box.border_box.left_px,
        40.0,
        &format!("{name}: abs child static left"),
    );
    assert_close(
        abs_box.border_box.top_px,
        0.0,
        &format!("{name}: abs child static top"),
    );
    assert_close(
        abs_box.border_box.width_px,
        10.0,
        &format!("{name}: abs child used width"),
    );
    assert_eq!(plans.len(), 2, "{name}: container layer + abs-child layer");
    assert_eq!(plans[1].rects.len(), 1, "{name}: abs child paints");
    assert_rect_op_geometry(
        &plans[1].rects[0],
        0.0,
        0.0,
        10.0,
        20.0,
        &format!("{name}: abs child rect"),
    );
}

fn assert_abspos_only_child_static_position(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "width", "10px");
    assert_style_set(&mut root, abs, "height", "10px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(
        plans[0].rects.len(),
        1,
        "{name}: only the container paints in the root layer"
    );
    // On-spec: the abs-pos-only child is laid out per abs-pos rules at its
    // static position (main-start/cross-start = 0) and PAINTS with its used
    // 10×10 size (today it flattened to 0 width and painted nothing).
    assert_close(
        assert_node_layout(&root, abs).width_px,
        10.0,
        &format!("{name}: abs child used width"),
    );
    let abs_box = node_box(&root, abs).expect("abs child box model");
    assert_close(
        abs_box.border_box.left_px,
        0.0,
        &format!("{name}: abs child static left"),
    );
    assert_close(
        abs_box.border_box.top_px,
        0.0,
        &format!("{name}: abs child static top"),
    );
    assert_close(
        abs_box.border_box.width_px,
        10.0,
        &format!("{name}: abs child border width"),
    );
    assert_close(
        abs_box.border_box.height_px,
        10.0,
        &format!("{name}: abs child border height"),
    );
    assert_eq!(plans.len(), 2, "{name}: container layer + abs-child layer");
    assert_eq!(plans[1].rects.len(), 1, "{name}: abs child paints");
    assert_rect_op_geometry(
        &plans[1].rects[0],
        0.0,
        0.0,
        10.0,
        10.0,
        &format!("{name}: abs child rect"),
    );
}

fn assert_display_none_middle_child_no_slot(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "30px");
    assert_style_set(&mut root, first, "height", "20px");
    let none = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, none, "display", "none");
    let third = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, third, "flex-basis", "40px");
    assert_style_set(&mut root, third, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // The display:none child paints nothing: rects[0]=container, rects[1]=first,
    // rects[2]=third.
    assert_eq!(
        plans[0].rects.len(),
        3,
        "{name}: container + two visible items"
    );
    // On-spec: the display:none child occupies NO slot — the trailing item sits
    // one gap after the first (40), not two gaps (50).
    assert_close(
        plans[0].rects[2].position_px[0],
        40.0,
        &format!("{name}: trailing item left after one gap"),
    );
    assert_eq!(
        plans.len(),
        1,
        "{name}: no extra layer for a display:none child"
    );
}

fn assert_display_none_only_child_empty(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let none = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, none, "display", "none");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // On-spec control: a display:none-only child leaves the container visibly
    // empty (no box, no item, no layout, no paint).
    assert_eq!(plans[0].rects.len(), 1, "{name}: only the container paints");
    assert_close(
        assert_node_layout(&root, container).width_px,
        200.0,
        &format!("{name}: container keeps its explicit width"),
    );
    assert_close(
        assert_node_layout(&root, container).height_px,
        20.0,
        &format!("{name}: container keeps its explicit height"),
    );
}

fn assert_run_joins_across_excluded_child(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let a = assert_append_text_to(&mut root, container, "A");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "width", "10px");
    assert_style_set(&mut root, abs, "height", "20px");
    let _b = assert_append_text_to(&mut root, container, "B");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    // On-spec: the abs child is excluded and NOT a run-breaker, so "A"+"B"
    // merge into ONE run item — B sits contiguous at A's width (no inter-item
    // gap inside the run; a broken run would push B past two 10px gaps).
    assert_eq!(
        plans[0].text_runs.len(),
        2,
        "{name}: two text runs in one run item"
    );
    let a_width = assert_node_layout(&root, a).width_px;
    assert_close(
        plans[0].text_runs[1].position_px[0],
        a_width,
        &format!("{name}: B contiguous after A (no gap inside the run)"),
    );
}

fn assert_static_position_centered(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "justify-content", "center");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "width", "10px");
    assert_style_set(&mut root, abs, "height", "10px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let abs_box = node_box(&root, abs).expect("abs child box model");
    // On-spec: an abs-only child under justify-content:center has static main =
    // content-box center (a 0×0 item centered in the 200px content box → 100),
    // not main-start.
    assert_close(
        abs_box.border_box.left_px,
        100.0,
        &format!("{name}: static main at the content-box center"),
    );
    assert_close(
        abs_box.border_box.top_px,
        0.0,
        &format!("{name}: static cross at cross-start"),
    );
}

fn assert_static_position_at_end_slot(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "height", "20px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "height", "20px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "order", "1");
    assert_style_set(&mut root, abs, "width", "10px");
    assert_style_set(&mut root, abs, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let abs_box = node_box(&root, abs).expect("abs child box model");
    // On-spec: `order: 1` on the abs child puts it after both order-0 items,
    // so its static main is after the last item (10 + 10, no trailing gap).
    assert_close(
        abs_box.border_box.left_px,
        20.0,
        &format!("{name}: static main after the last item (no trailing gap)"),
    );
    assert_close(
        abs_box.border_box.top_px,
        0.0,
        &format!("{name}: static cross at cross-start"),
    );
}

// CSS Flexbox 1 §9.7 (#resolve-flexible-lengths) "Freeze over-flexed items":
// each pass freezes exactly ONE violation class, chosen by the total
// violation's sign — min-violations when the total is ≥ 0, max-violations
// when it is negative — then re-iterates. Gecko mirrors this in
// `FlexLine::ResolveFlexibleLengths` (`nsFlexContainerFrame.cpp`) and Blink in
// `LineFlexer::ResolveFlexibleLengths`/`FreezeItems` (`line_flexer.cc`). No
// single WPT oracle pairs the mixed-violation freeze ORDER with the final
// distribution; the expectation is cited from §9.7 directly (the two
// implementation files confirm the one-class-at-a-time shape).
//
// ON-SPEC (relabeled from `off_spec_mixed_violation_freeze_ordering` in flex
// arc step 5): `freeze_flex_line_violation_class` (`flex.rs`) now records each
// item's violation class and freezes exactly ONE class per iteration by the
// signed total — so a grow pass with a max-clamped A and a min-clamped B
// re-absorbs the freed space into B (190px, row a), the shrink mirror
// re-absorbs into A (70px, row b), and the three-item chain re-distributes
// once more (B 140px, row c).
#[test]
fn mixed_violations_freeze_one_class_per_pass() {
    let rows: [FreezeOrderRow; 3] = [
        (
            "grow_mixed_violations",
            assert_grow_mixed_violation_freeze_order,
        ),
        (
            "shrink_mixed_violations",
            assert_shrink_mixed_violation_freeze_order,
        ),
        ("chained_refreeze", assert_chained_refreeze_freeze_order),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named freeze-order probe: the row name plus a runner.
type FreezeOrderRow = (&'static str, fn(&str));

fn assert_grow_mixed_violation_freeze_order(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "300px");
    assert_style_set(&mut root, container, "height", "20px");
    for (basis, grow, min, max) in [("100px", "1", "", "110px"), ("100px", "1", "160px", "")] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-grow", grow);
        if !min.is_empty() {
            assert_style_set(&mut root, item, "min-width", min);
        }
        if !max.is_empty() {
            assert_style_set(&mut root, item, "max-width", max);
        }
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the total violation is −30 (A −40 max + B +10 min), so the
    // spec freezes only A (max) and lets B re-absorb the 90px freed up, ending
    // at 190px.
    assert_close(rects[1].size_px[0], 110.0, &format!("{name}: A width"));
    assert_close(rects[2].size_px[0], 190.0, &format!("{name}: B width"));
    assert_close(rects[2].position_px[0], 110.0, &format!("{name}: B left"));
}

fn assert_shrink_mixed_violation_freeze_order(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "140px");
    assert_style_set(&mut root, container, "height", "20px");
    for (basis, shrink, min, max) in [("100px", "1", "", "80px"), ("100px", "3", "70px", "")] {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "flex-basis", basis);
        assert_style_set(&mut root, item, "flex-shrink", shrink);
        if !min.is_empty() {
            assert_style_set(&mut root, item, "min-width", min);
        }
        if !max.is_empty() {
            assert_style_set(&mut root, item, "max-width", max);
        }
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    // On-spec: the total violation is +10 (A −5 max + B +15 min), so the spec
    // freezes only B (min) and lets A re-absorb the −30px freed up, ending at
    // 70px.
    assert_close(rects[1].size_px[0], 70.0, &format!("{name}: A width"));
    assert_close(rects[2].size_px[0], 70.0, &format!("{name}: B width"));
    assert_close(rects[2].position_px[0], 70.0, &format!("{name}: B left"));
}

fn assert_chained_refreeze_freeze_order(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "300px");
    assert_style_set(&mut root, container, "height", "20px");
    let a = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, a, "flex-basis", "50px");
    assert_style_set(&mut root, a, "flex-grow", "1");
    assert_style_set(&mut root, a, "max-width", "60px");
    assert_style_set(&mut root, a, "height", "20px");
    let b = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, b, "flex-basis", "50px");
    assert_style_set(&mut root, b, "flex-grow", "1");
    assert_style_set(&mut root, b, "min-width", "120px");
    assert_style_set(&mut root, b, "height", "20px");
    let c = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, c, "flex-basis", "100px");
    assert_style_set(&mut root, c, "flex-grow", "0");
    assert_style_set(&mut root, c, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 4, "{name}: container + three items");
    // On-spec: the total violation is −20 (A −40 max + B +20 min), so the
    // spec freezes A (max) then re-distributes the freed 90px to B (still
    // unfrozen, ending at 140px); the inflexible C stays at its 100px basis
    // either way.
    assert_close(rects[1].size_px[0], 60.0, &format!("{name}: A width"));
    assert_close(rects[2].size_px[0], 140.0, &format!("{name}: B width"));
    assert_close(rects[3].size_px[0], 100.0, &format!("{name}: C width"));
    assert_close(rects[3].position_px[0], 200.0, &format!("{name}: C left"));
}

// CSS Flexbox 1 §9.7 step 3 composed with the all-frozen resolution (flex arc
// step 5 D2.3): when every item in a line freezes in the constructor pass (all
// inflexible), the line resolves to each item's seeded HYPOTHETICAL main size —
// the violation clamp IS the constructor freeze, so nothing is skipped and no
// distribution happens. Cited from §9.7 directly; Gecko
// (`FlexLine::ResolveFlexibleLengths`) and Blink (`LineFlexer` constructor,
// `line_flexer.cc`) both freeze the whole line at hypothetical before the
// free-space loop.
#[test]
fn all_inflexible_line_resolves_to_hypotheticals() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let a = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, a, "flex-basis", "10px");
    assert_style_set(&mut root, a, "flex-shrink", "0");
    assert_style_set(&mut root, a, "flex-grow", "0");
    assert_style_set(&mut root, a, "min-width", "20px");
    assert_style_set(&mut root, a, "height", "20px");
    let b = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, b, "flex-basis", "50px");
    assert_style_set(&mut root, b, "flex-shrink", "0");
    assert_style_set(&mut root, b, "flex-grow", "0");
    assert_style_set(&mut root, b, "max-width", "30px");
    assert_style_set(&mut root, b, "height", "20px");
    let c = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, c, "flex-basis", "40px");
    assert_style_set(&mut root, c, "flex-shrink", "0");
    assert_style_set(&mut root, c, "flex-grow", "0");
    assert_style_set(&mut root, c, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 4, "container + three items");
    // On-spec: all three items are inflexible, so the whole line freezes at
    // their clamped hypotheticals (20, 30, 40) — the 110px leftover free space
    // is never distributed, and the min/max clamps are not skipped.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        20.0,
        20.0,
        "A at min-width hypothetical",
    );
    assert_rect_op_geometry(
        &rects[2],
        20.0,
        0.0,
        30.0,
        20.0,
        "B at max-width hypothetical",
    );
    assert_rect_op_geometry(&rects[3], 50.0, 0.0, 40.0, 20.0, "C at base hypothetical");
}

// CSS Flexbox 1 §9.2 + §4.5 (flex arc step 5 D1): after the hypothetical FIELD
// gains the automatic-minimum floor, the outer hypothetical simplifies to
// `field + margins` (the former outer-site `.max(auto_min + bp)` is a no-op
// once the field floors). This pin re-asserts an existing floor scenario — a
// wrapping row whose first item's content minimum exceeds the container, with
// non-zero main margins — and shows the outer floor (`field + margins`) still
// drives line breaking. It must NOT move under the simplification.
#[test]
fn outer_hypothetical_floor_is_field_plus_margins() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "height", "60px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "flex-basis", "10px");
    assert_style_set(&mut root, first, "overflow", "visible");
    assert_style_set(&mut root, first, "height", "20px");
    assert_style_set(&mut root, first, "margin-left", "5px");
    assert_style_set(&mut root, first, "margin-right", "5px");
    let first_text = assert_append_text_to(&mut root, first, "UnbreakableFlexLabel");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "flex-basis", "10px");
    assert_style_set(&mut root, second, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let content_min_px = assert_text_intrinsic(&root, first_text).min_content_width_px;
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 3, "container + two items");
    // On-spec: the first item's field floors at its content min and its outer
    // hypothetical is field (content min) + 10px margins — still > 100px, so
    // the item occupies line 1 at the floored border-box width (its border-box
    // origin after the 5px leading margin) and the second item wraps to line 2.
    assert_close(
        rects[1].size_px[0],
        content_min_px,
        "first item border-box width",
    );
    assert_close(
        rects[1].position_px[0],
        5.0,
        "first item border-box left after the leading margin",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        30.0,
        10.0,
        20.0,
        "second item wraps to line 2",
    );
}

// CSS Flexbox 1 §9.2/§9.8 + CSS 2.1 §10.1 + CSS Values 4 §percentages: a
// flex item's percentage `width`/`height` resolve against its containing
// block (the flex container's content box) — main-axis width against the
// container content width, cross/main height against the definite container
// height — and percentage `margin`/`padding` resolve against the container's
// inline size (width), per CSS Flexbox 1 §4.2. Gecko resolves these in
// `FlexItem` construction (`nsFlexContainerFrame.cpp`) and Blink in
// `ConstructAndAppendFlexItems` (`flex_layout_algorithm.cc`). Closest WPT
// oracle: `css/css-flexbox/percentage-margins-001.html` plus the
// `flexbox_flex-*-Npercent*` basis family.
//
// ON-SPEC (relabeled from `off_spec_percentage_resolution_against_flex_container`
// in flex arc step 6): `active_flex_resolved_child_context` (`flex.rs`) now
// sets the placement relayout context's GENERAL percentage basis
// (`available_width_px`) to the CONTAINER's content width, so the whole width
// family (`width`, `min-width`, `max-width`, `margin×4`, `padding×4`, `left`,
// `right`) resolves against the containing block — the container — matching
// measure time. The item's own resolved size still arrives via the
// content-size override, and the deferred §8.1 main-axis auto-margin admission
// stays at 0 through the dedicated `auto_margin_reference_width_px` reference.
#[test]
fn flex_item_percentages_resolve_against_flex_container() {
    let rows: [PercentageResolutionRow; 10] = [
        (
            "row_item_width_percent_main_axis",
            assert_row_item_width_percent,
        ),
        (
            "row_item_height_percent_definite_cross",
            assert_row_item_height_percent,
        ),
        (
            "row_item_margin_left_percent",
            assert_row_item_margin_left_percent,
        ),
        (
            "row_item_padding_left_percent",
            assert_row_item_padding_left_percent,
        ),
        (
            "column_item_width_percent_cross_axis",
            assert_column_item_width_percent,
        ),
        (
            "column_item_height_percent_main_axis",
            assert_column_item_height_percent,
        ),
        (
            "wrapped_line_break_uses_resolved_percent",
            assert_wrapped_percent_line_break,
        ),
        (
            "row_item_min_width_percent",
            assert_row_item_min_width_percent,
        ),
        (
            "row_item_max_width_percent",
            assert_row_item_max_width_percent,
        ),
        ("row_item_left_percent", assert_row_item_left_percent),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named percentage-resolution probe: the row name plus a runner.
type PercentageResolutionRow = (&'static str, fn(&str));

fn assert_row_item_width_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50%");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 50% of the 200px container content width = 100px.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 100.0, 20.0, name);
}

fn assert_row_item_height_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "100px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 50% of the 100px definite container height = 50px cross size.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 50.0, 50.0, name);
}

fn assert_row_item_margin_left_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "margin-left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 10% of the 200px container content width = 20px margin.
    assert_rect_op_geometry(&plans[0].rects[1], 20.0, 0.0, 50.0, 20.0, name);
}

fn assert_row_item_padding_left_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "padding-left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 10% of the 200px container content width = 20px padding, so the
    // content-box item's border box is 70px wide.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 70.0, 20.0, name);
}

fn assert_row_item_min_width_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "10px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "min-width", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 50% of the 200px container content width = 100px, flooring the
    // 10px specified width.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 100.0, 20.0, name);
}

fn assert_row_item_max_width_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "200px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "max-width", "25%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 25% of the 200px container content width = 50px, capping the
    // 200px specified width.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 50.0, 20.0, name);
}

fn assert_row_item_left_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "position", "relative");
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let item_box = node_box(&root, item).expect("item box should exist");
    // On-spec: 10% of the 200px container content width = 20px relative offset.
    assert_close(
        item_box.border_box.left_px,
        20.0,
        &format!("{name}: item left"),
    );
    assert_close(
        item_box.border_box.width_px,
        50.0,
        &format!("{name}: item width"),
    );
    assert_close(
        item_box.border_box.height_px,
        20.0,
        &format!("{name}: item height"),
    );
}

fn assert_column_item_width_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_column_container_size(&mut root, container, "200px", "100px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50%");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 50% of the 200px container content width = 100px cross size.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 100.0, 20.0, name);
}

fn assert_column_item_height_percent(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_column_container_size(&mut root, container, "200px", "100px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // On-spec: 50% of the 100px definite container height = 50px main size.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 50.0, 50.0, name);
}

fn assert_wrapped_percent_line_break(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "row");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, container, "height", "60px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "60%");
        assert_style_set(&mut root, item, "height", "20px");
        assert_style_set(&mut root, item, "flex-shrink", "0");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    // On-spec: each 60% width resolves to 120px, so a second item would
    // overflow the 200px line and each item breaks onto its own line.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        0.0,
        120.0,
        20.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        0.0,
        20.0,
        120.0,
        20.0,
        &format!("{name}: item 2"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[3],
        0.0,
        40.0,
        120.0,
        20.0,
        &format!("{name}: item 3"),
    );
}

// CSS Flexbox 1 §5.2 (flex-wrap) + §8.2/§8.4 (alignment) + CSS Box Alignment 3
// (gap): in a column flex container the main axis is vertical and the wrapped
// flex LINES are the columns, so `column-gap` separates the columns along the
// cross axis and `row-gap` separates items along the main axis; reversing the
// main axis (`row-reverse`/`column-reverse`) or the cross axis
// (`wrap-reverse`) moves items/lines to the opposite start without changing
// the gap magnitude. Gecko builds column lines in `GenerateFlexLines`
// (`nsFlexContainerFrame.cpp`) and Blink in `ConstructAndAppendFlexItems`
// (`flex_layout_algorithm.cc`); gap placement is pinned by
// `css/css-flexbox/flexbox-column-row-gap-001.html` (module header).
// ON-SPEC: all four rows land at the spec positions (exact-fit containers so
// the gap geometry is not mixed with negative-free-space fallback, which is
// separately pinned by the space-evenly registry row).
#[test]
fn column_wrap_gap_geometry() {
    let rows: [ColumnWrapGapRow; 4] = [
        ("column_wrap_column_gap", assert_column_wrap_column_gap),
        (
            "column_wrap_reverse_column_gap",
            assert_column_wrap_reverse_column_gap,
        ),
        (
            "row_reverse_wrap_column_gap",
            assert_row_reverse_wrap_column_gap,
        ),
        (
            "column_reverse_wrap_column_gap",
            assert_column_reverse_wrap_column_gap,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named column-wrap gap probe: the row name plus a runner.
type ColumnWrapGapRow = (&'static str, fn(&str));

fn assert_column_wrap_column_gap(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "170px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "50px");
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    // Each 40px item fits one per 60px column; the 10px column-gap separates
    // the three wrapped columns at x = 0, 60, 120.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        60.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 2"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[3],
        120.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 3"),
    );
}

fn assert_column_wrap_reverse_column_gap(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column");
    assert_style_set(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_style_set(&mut root, container, "width", "170px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "50px");
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    // wrap-reverse swaps cross-start/end, so the first column is at the right
    // and the columns run 120, 60, 0 with the same 10px gap.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        120.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        60.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 2"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[3],
        0.0,
        0.0,
        50.0,
        40.0,
        &format!("{name}: item 3"),
    );
}

fn assert_row_reverse_wrap_column_gap(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "row-reverse");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "40px");
        assert_style_set(&mut root, item, "height", "20px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    // Two 40px items + one 10px gap fit per 100px line; the third wraps. The
    // reversed main axis packs the first line from the right: item 1 at 60,
    // item 2 at 10, and the wrapped item 3 at 60 on the next line.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        60.0,
        0.0,
        40.0,
        20.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        10.0,
        0.0,
        40.0,
        20.0,
        &format!("{name}: item 2"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[3],
        60.0,
        20.0,
        40.0,
        20.0,
        &format!("{name}: item 3"),
    );
}

fn assert_column_reverse_wrap_column_gap(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "column-reverse");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "170px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "50px");
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    // column-reverse packs each 40px item to the bottom of its 60px column
    // (y = 20) while the columns still run left-to-right at 0, 60, 120.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        20.0,
        50.0,
        40.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        60.0,
        20.0,
        50.0,
        40.0,
        &format!("{name}: item 2"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[3],
        120.0,
        20.0,
        50.0,
        40.0,
        &format!("{name}: item 3"),
    );
}

// CSS Overflow 3 §3 (overflow clip/scroll) + CSS Flexbox 1 §4.2/§9.2 (item
// border/padding in the outer hypothetical main size) + CSS Box 3 §content
// edges: a flex container with `overflow: scroll`/`hidden` retains the
// overflowing wrapped content as scroll geometry and clips it at its border
// box via the composite `clip_world` (the deliberate layer-clip registry
// decision — NOT re-registered here); an item's `border-*-width` is part of
// its border box and therefore its outer hypothetical main size, so line
// breaking accounts for it; the container's border + padding shift the
// content-box origin for item placement. Gecko applies border/padding in
// `FlexItem` construction (`nsFlexContainerFrame.cpp`) and Blink in
// `ConstructAndAppendFlexItems` (`flex_layout_algorithm.cc`). Closest WPT
// oracle: `css/css-flexbox/flexbox-overflow-horiz-001.html` (overflow) and
// `css/css-flexbox/flexbox-width-overflow.html` (item sizing).
// ON-SPEC: all four rows land at the spec geometry.
#[test]
fn flex_container_overflow_and_borders() {
    let rows: [OverflowBorderRow; 4] = [
        (
            "overflow_scroll_clips_with_scroll_geometry",
            assert_overflow_scroll_scroll_geometry,
        ),
        (
            "overflow_hidden_clips_at_border_box",
            assert_overflow_hidden_border_box_clip,
        ),
        (
            "item_border_width_participates_in_line_breaking",
            assert_item_border_width_line_breaking,
        ),
        (
            "container_border_padding_origin_for_justify",
            assert_container_border_padding_origin,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named overflow/border probe: the row name plus a runner.
type OverflowBorderRow = (&'static str, fn(&str));

fn assert_overflow_scroll_scroll_geometry(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "overflow", "scroll");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "40px");
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let layout = assert_node_layout(&root, container);
    assert_eq!(
        layout.layer_kind,
        DomLayerKind::Clipped,
        "{name}: clipped layer"
    );
    assert_close(layout.width_px, 100.0, &format!("{name}: container width"));
    assert_close(layout.height_px, 60.0, &format!("{name}: container height"));
    // Two wrapped 40px lines overflow the 60px box: the retained scroll
    // geometry keeps the full 80px content extent (clipping is composite).
    assert_close(
        layout.content_height_px,
        80.0,
        &format!("{name}: scroll content height"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 4, "{name}: container + three items");
    assert_rect_op_geometry(
        &plans[0].rects[3],
        0.0,
        40.0,
        40.0,
        40.0,
        &format!("{name}: overflowing item"),
    );
}

fn assert_overflow_hidden_border_box_clip(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "overflow", "hidden");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "40px");
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let layout = assert_node_layout(&root, container);
    assert_eq!(
        layout.layer_kind,
        DomLayerKind::Clipped,
        "{name}: clipped layer"
    );
    assert_close(layout.width_px, 100.0, &format!("{name}: container width"));
    assert_close(layout.height_px, 60.0, &format!("{name}: container height"));
    // The clip is at the container border box (100x60); the retained content
    // extent (80px) is what the composite clip_world cuts off.
    assert_close(
        layout.content_height_px,
        80.0,
        &format!("{name}: clipped content extent"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        100.0,
        60.0,
        &format!("{name}: clip box"),
    );
}

fn assert_item_border_width_line_breaking(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-wrap", "wrap");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "align-content", "flex-start");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "40px");
        assert_style_set(&mut root, item, "height", "20px");
        assert_style_set(&mut root, item, "border-right-width", "15px");
        assert_style_set(&mut root, item, "border-left-width", "0px");
        assert_style_set(&mut root, item, "flex-shrink", "0");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 3, "{name}: container + two items");
    // Each item's border box is 40 + 15 = 55px, so two items (110px) exceed
    // the 100px line and the second wraps onto its own line (y = 20). Without
    // the border, both 40px items would share the first line.
    assert_rect_op_geometry(
        &plans[0].rects[1],
        0.0,
        0.0,
        55.0,
        20.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        0.0,
        20.0,
        55.0,
        20.0,
        &format!("{name}: item 2"),
    );
}

fn assert_container_border_padding_origin(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "flex-direction", "row");
    assert_style_set(&mut root, container, "width", "100px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "border-left-width", "5px");
    assert_style_set(&mut root, container, "border-top-width", "7px");
    assert_style_set(&mut root, container, "padding-left", "3px");
    assert_style_set(&mut root, container, "padding-top", "2px");
    assert_style_set(&mut root, container, "justify-content", "flex-start");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "20px");
        assert_style_set(&mut root, item, "height", "10px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 3, "{name}: container + two items");
    // The border (5,7) + padding (3,2) shift the content-box origin to
    // (8, 9); the container border box is 108 x 49, and the items start at
    // the content edge.
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        108.0,
        49.0,
        &format!("{name}: container"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[1],
        8.0,
        9.0,
        20.0,
        10.0,
        &format!("{name}: item 1"),
    );
    assert_rect_op_geometry(
        &plans[0].rects[2],
        28.0,
        9.0,
        20.0,
        10.0,
        &format!("{name}: item 2"),
    );
}

// CSS Flexbox 1 §8.4 (align-content): "This property has no effect on
// single-line flex containers" — a nowrap container has one line that fills
// the cross size, so `align-content` has no remaining cross space to pack.
// `remaining_cross_axis_line_pack_space_px` (`flex.rs:1750`) early-returns
// 0.0 when the container is not wrapping (`flex.rs:1756`), so the line pack
// offset stays at cross-start for every `align-content` keyword. Gecko skips
// line packing for single-line containers in `nsFlexContainerFrame.cpp`
// (`AlignLines` is only reached with multiple lines); Blink mirrors it in
// `flex_layout_algorithm.cc`. Closest WPT oracle:
// `css/css-flexbox/align-content-001.htm` (align-content is inert without
// wrap). ON-SPEC: every row lands at the flex-start baseline.
#[test]
fn single_line_align_content_is_inert() {
    let rows: [InertAlignContentRow; 3] = [
        ("align_content_center", "center"),
        ("align_content_space_between", "space-between"),
        ("align_content_stretch", "stretch"),
    ];
    for (name, align_content) in rows {
        assert_single_line_align_content_inert(name, align_content);
    }
}

/// A named single-line align-content probe: the row name plus the keyword.
type InertAlignContentRow = (&'static str, &'static str);

fn assert_single_line_align_content_inert(name: &str, align_content: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "flex-wrap", "nowrap");
    assert_style_set(&mut root, container, "align-content", align_content);
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "30px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans[0].rects.len(), 2, "{name}: container + item");
    // Identical to the flex-start baseline: the single line sits at
    // cross-start and the item follows align-items (flex-start), so the
    // align-content keyword has no effect.
    assert_rect_op_geometry(&plans[0].rects[1], 0.0, 0.0, 30.0, 20.0, name);
}

// CSS Text 3 §7.1 (`text-align`) composed with CSS Flexbox 1 §9 (flex item
// sizing/placement): `text-align` inherits through the item subtree, and the
// question is whether the inherited alignment composes with the item's
// resolved main size. Closest WPT oracles:
// `css/css-text/text-align/text-align-center-001.html`,
// `text-align-end-001.html`, `text-align-justify-*`. Gecko resolves the
// physical alignment in `nsTextFrame`/`nsLineLayout` after the flex item's
// used size is fixed (`nsFlexContainerFrame.cpp`); Blink in
// `NGInlineLayoutAlgorithm` line alignment over the fragment's resolved inline
// size (`flex_layout_algorithm.cc`).
//
// ON-SPEC: item SIZING never reads `text-align`, so an auto-width item still
// shrink-wraps to its text; the text run then fills the item's RESOLVED
// content width (the text box is the block's line box, stretched to the
// content box), and `text-align` is carried as the run's `alignment` field for
// cosmic-text to center/end/justify the glyphs within that full-width box. The
// staged-op contract therefore encodes the composition as `{ size_px = resolved
// content width, position_px = content-box start, alignment = inherited }`.
// Row (c) pins the watched case: the grown item centers text in its GROWN
// width (the text run size is the resolved 200px, not the measured width).
#[test]
fn flex_item_text_alignment_stacking() {
    let rows: [TextAlignStackingRow; 7] = [
        (
            "a_auto_width_center_short_text_shrink_wraps",
            assert_text_align_auto_width_center,
        ),
        (
            "b_definite_width_center_fills_content_box",
            assert_text_align_definite_width_center,
        ),
        (
            "b_definite_width_end_fills_content_box",
            assert_text_align_definite_width_end,
        ),
        (
            "c_grown_item_center_uses_resolved_width",
            assert_text_align_grown_center,
        ),
        (
            "d_container_center_inherited_by_item_text",
            assert_text_align_container_inherited,
        ),
        (
            "e_auto_width_justify_shrink_context",
            assert_text_align_justify_auto_width,
        ),
        (
            "f_column_definite_width_end_fills_content_box",
            assert_text_align_column_definite_end,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named text-align-in-flex probe: the row name plus a runner.
type TextAlignStackingRow = (&'static str, fn(&str));

/// Row (a): an auto-width item with `text-align: center` and a short text
/// child shrink-wraps in the main axis (text fills the shrunk box), so the
/// alignment is inert for geometry; the inherited alignment field is still
/// center.
fn assert_text_align_auto_width_center(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "text-align", "center");
    let text = assert_append_text_to(&mut root, item, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    // rects[0] = container, rects[1] = item; the text child emits a text run.
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_eq!(text_runs.len(), 1, "{name}: one text run");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        20.0,
        &format!("{name}: item shrink-wraps to text"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Center,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: text left"),
    );
}

/// Row (b, center): a definite-width item centers its text in the content box.
/// The text run fills the 120px content box and carries the center alignment.
fn assert_text_align_definite_width_center(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "120px");
    assert_style_set(&mut root, item, "text-align", "center");
    let _text = assert_append_text_to(&mut root, item, "Mid");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        120.0,
        20.0,
        &format!("{name}: definite-width item"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Center,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].size_px[0],
        120.0,
        &format!("{name}: text run fills the content box"),
    );
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: text left"),
    );
}

/// Row (b, end): the `end` keyword resolves to the physical right in LTR; the
/// text run fills the 120px content box and carries the right alignment.
fn assert_text_align_definite_width_end(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "120px");
    assert_style_set(&mut root, item, "text-align", "end");
    let _text = assert_append_text_to(&mut root, item, "Mid");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        120.0,
        20.0,
        &format!("{name}: definite-width item"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Right,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].size_px[0],
        120.0,
        &format!("{name}: text run fills the content box"),
    );
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: text left"),
    );
}

/// Row (c): a grown item centers text in the RESOLVED (grown) width, not the
/// measured width. A single `flex-grow: 1` item fills the 200px container and
/// the text run fills that grown width (the watched composition case).
fn assert_text_align_grown_center(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "20px");
    assert_style_set(&mut root, item, "flex-grow", "1");
    assert_style_set(&mut root, item, "text-align", "center");
    let _text = assert_append_text_to(&mut root, item, "Mid");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        200.0,
        20.0,
        &format!("{name}: grown item fills the container"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Center,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].size_px[0],
        200.0,
        &format!("{name}: text run fills the grown width"),
    );
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: text left"),
    );
}

/// Row (d): `text-align: center` authored on the CONTAINER inherits into the
/// item's text child.
fn assert_text_align_container_inherited(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "text-align", "center");
    let item = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, item, "Inherit");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        20.0,
        &format!("{name}: item shrink-wraps"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Center,
        "{name}: inherited alignment"
    );
}

/// Row (e): `text-align: justify` on an auto-width item keeps shrink-wrap
/// sizing; the justify alignment is carried on the run and inert for a single
/// unbroken line.
fn assert_text_align_justify_auto_width(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "text-align", "justify");
    let text = assert_append_text_to(&mut root, item, "Justify me");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        20.0,
        &format!("{name}: item shrink-wraps"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Justified,
        "{name}: alignment"
    );
}

/// Row (f): a column container with a definite-width item and `text-align:
/// end` resolves the alignment to the physical right; the text run fills the
/// 120px content box and carries the right alignment.
fn assert_text_align_column_definite_end(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_column_container_size(&mut root, container, "200px", "80px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "120px");
    assert_style_set(&mut root, item, "height", "20px");
    assert_style_set(&mut root, item, "text-align", "end");
    let _text = assert_append_text_to(&mut root, item, "End me");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        120.0,
        20.0,
        &format!("{name}: definite-width column item"),
    );
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Right,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].size_px[0],
        120.0,
        &format!("{name}: text run fills the content box"),
    );
    assert_close(
        text_runs[0].position_px[0],
        0.0,
        &format!("{name}: text left"),
    );
}

// CSS Text 4 §white-space composed with CSS Flexbox 1 §4.5 (automatic minimum
// size) and §9.2 (line collection): white-space mode changes the text's
// intrinsic min/max content, and the question is whether the flex item's
// measurement and auto-min floor honor that mode. Closest WPT oracles:
// `css/css-text/white-space/white-space-*` and `css/css-flexbox/flexbox-min-
// width-auto-*`. Gecko (`nsTextFrame` intrinsic width per white-space mode,
// `nsFlexContainerFrame` content-based min) and Blink (`NGInlineNode` intrinsic
// per `EWhiteSpace`, `FlexItem::UpdateAutoMinSize`) both resolve the automatic
// minimum from the text's per-white-space min-content.
#[test]
fn flex_item_text_white_space_stacking() {
    let rows: [TextWhiteSpaceStackingRow; 4] = [
        (
            "a_pre_wrap_newline_two_lines",
            assert_white_space_pre_wrap_newline,
        ),
        (
            "b_pre_line_shrink_context_auto_min",
            assert_white_space_pre_line_shrink,
        ),
        (
            "c_break_spaces_lone_trailing_space_fold",
            assert_white_space_break_spaces_trailing,
        ),
        (
            "d_normal_wrap_item_wraps_at_resolved_width",
            assert_white_space_normal_wrap_item,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named white-space-in-flex probe: the row name plus a runner.
type TextWhiteSpaceStackingRow = (&'static str, fn(&str));

/// Row (a): `white-space: pre-wrap` preserves the `\n` segment break, so the
/// text box is two lines and the item's cross size grows to two line boxes.
fn assert_white_space_pre_wrap_newline(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "40px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "white-space", "pre-wrap");
    let text = assert_append_text_to(&mut root, item, "Alpha\nBeta");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let text_layout = assert_node_layout(&root, text);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        text_intrinsic.line_height_px * 2.0,
        &format!("{name}: item two lines tall"),
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px * 2.0,
        &format!("{name}: text box two lines"),
    );
}

/// Row (b): `white-space: pre-line` collapses spaces but preserves segment
/// breaks, so its min-content (widest word) matches `normal`'s — the shrink
/// floor lands on the widest word, on-spec.
fn assert_white_space_pre_line_shrink(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "20px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "white-space", "pre-line");
    assert_style_set(&mut root, item, "flex-basis", "300px");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    let text = assert_append_text_to(&mut root, item, "one two three four");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let item_width = assert_node_layout(&root, item).width_px;
    assert_close(
        item_width,
        text_intrinsic.min_content_width_px,
        &format!("{name}: pre-line shrink floor = min-content (widest word)"),
    );
}

/// Row (c): `white-space: break-spaces` inside a flex item composes the
/// registered `off_spec_break_spaces_approximated_as_pre_wrap` divergence. The
/// shrink-wrap WIDTH side is on-spec: the lone trailing space takes advance in
/// max-content (the item is ~3.3px wider); the divergent side is the line-end
/// COLLAPSE (the space does not wrap/hang onto a second line), which is pinned
/// by the `admitted_gaps` row and shares the same root cause
/// (`text_layout_input_for_white_space` maps `BreakSpaces` to the pre-wrap
/// transform).
fn assert_white_space_break_spaces_trailing(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let no_trailing = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, no_trailing, "white-space", "break-spaces");
    let _no_trailing_text = assert_append_text_to(&mut root, no_trailing, "Alpha Beta");
    let lone_trailing = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, lone_trailing, "white-space", "break-spaces");
    let _lone_trailing_text = assert_append_text_to(&mut root, lone_trailing, "Alpha Beta ");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let no_trailing_width = assert_node_layout(&root, no_trailing).width_px;
    let lone_trailing_width = assert_node_layout(&root, lone_trailing).width_px;
    assert!(
        lone_trailing_width > no_trailing_width,
        "{name}: trailing space takes advance in shrink-wrap width ({lone_trailing_width} > {no_trailing_width})",
    );
}

/// Row (d): `white-space: normal` in a definite-width item wraps the long text
/// at the RESOLVED item width (100px), so the text run fills 100px and spans
/// multiple lines.
fn assert_white_space_normal_wrap_item(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "60px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "100px");
    assert_style_set(&mut root, item, "white-space", "normal");
    let text = assert_append_text_to(
        &mut root,
        item,
        "The quick brown fox jumps over the lazy dog",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    let text_runs = &plans[0].text_runs;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        100.0,
        text_runs[0].size_px[1],
        &format!("{name}: item wraps at 100px"),
    );
    assert_close(
        text_runs[0].size_px[0],
        100.0,
        &format!("{name}: text run fills the 100px content box"),
    );
    assert!(
        text_runs[0].size_px[1] > text_intrinsic.line_height_px,
        "{name}: wrapped text should span more than one line; got {}",
        text_runs[0].size_px[1],
    );
}

// CSS Flexbox 1 §4.5 automatic minimum size composed with CSS Text 4
// `white-space`: the content-based automatic minimum for a visible-overflow
// flex item is the item's min-content size in the main axis, resolved per the
// text's white-space mode. `white-space: nowrap` forbids wrapping, so its
// min-content is the FULL run (max-content); `white-space: pre` shares that
// shape. No single WPT oracle pairs a nowrap text with the flex auto-min floor,
// so this is cited from §4.5 directly; closest family is
// `css/css-flexbox/min-width-auto-*`. Gecko (`nsTextFrame` per-white-space
// intrinsic width, `nsFlexContainerFrame` content-based min) and Blink
// (`NGInlineNode` intrinsic per `EWhiteSpace`, `FlexItem::UpdateAutoMinSize`)
// both floor a nowrap item at its full-run min-content.
//
// ON-SPEC (was `off_spec_flex_auto_min_nowrap_text_floors_at_widest_word`):
// the record's min-content is mode-aware (the 3a-i intrinsic), so the nowrap
// item floors at its full-run min-content (115.5px), not the widest word
// (33.3px) the manual whitespace-splitting leaf used to return.
#[test]
fn flex_auto_min_nowrap_text_floors_at_full_run_width() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "20px");
    assert_style_set(&mut root, container, "height", "20px");

    let nowrap_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, nowrap_item, "white-space", "nowrap");
    assert_style_set(&mut root, nowrap_item, "flex-basis", "300px");
    assert_style_set(&mut root, nowrap_item, "flex-shrink", "1");
    let nowrap_text = assert_append_text_to(&mut root, nowrap_item, "one two three four");

    let normal_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, normal_item, "white-space", "normal");
    assert_style_set(&mut root, normal_item, "flex-basis", "300px");
    assert_style_set(&mut root, normal_item, "flex-shrink", "1");
    let _normal_text = assert_append_text_to(&mut root, normal_item, "one two three four");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let nowrap_text_intrinsic = assert_text_intrinsic(&root, nowrap_text);
    let nowrap_width = assert_node_layout(&root, nowrap_item).width_px;
    let normal_width = assert_node_layout(&root, normal_item).width_px;
    // On-spec: the nowrap item floors at its full-run min-content (==
    // max-content for nowrap), while the normal item floors at its widest word.
    assert_close(
        nowrap_width,
        nowrap_text_intrinsic.min_content_width_px,
        "nowrap floor equals its full-run min-content",
    );
    assert!(
        nowrap_width > normal_width,
        "nowrap floor should exceed normal's widest-word floor; nowrap {nowrap_width} vs normal {normal_width}",
    );
}

// CSS 2.1 §10.3.2/§10.6.2 (replaced-element width/height transfer through the
// intrinsic ratio) composed with CSS Flexbox 1 §4.5 (automatic minimum): an
// `img` is a replaced flex item whose main-axis base is its intrinsic width and
// whose used cross size follows the intrinsic ratio. The 16:9 placeholder is
// the crate-owned policy (see `layout/replaced_elements.rs` and the registry);
// the transfer math is the browser-backed subset (CSS 2.1 §10.3.2/§10.6.2).
// Gecko (`nsImageFrame::GetIntrinsicRatio`, `nsFlexContainerFrame`
// replaced-item measurement) and Blink (`LayoutReplaced` intrinsic ratio,
// `flex_layout_algorithm.cc` replaced-item measurement) both size the replaced
// item from its intrinsic ratio. Closest WPT oracles:
// `css/css-flexbox/flexbox-replaced-*`, `css/replaced-element-*`. The
// cross-axis STRETCH case keeps the ratio (moved on-spec in flex arc step 7)
// and is pinned separately by `flex_stretch_keeps_replaced_img_ratio`.
#[test]
fn flex_item_replaced_images() {
    let rows: [ReplacedImageRow; 4] = [
        ("a_intrinsic_size_row_base", assert_img_intrinsic_row_base),
        ("c_shrink_context", assert_img_shrink_context),
        (
            "d_percent_width_in_definite_item",
            assert_img_percent_in_definite_item,
        ),
        ("f_img_text_sibling_gap", assert_img_text_sibling_gap),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named replaced-image-in-flex probe: the row name plus a runner.
type ReplacedImageRow = (&'static str, fn(&str));

const IMG_PLACEHOLDER_WIDTH_PX: f32 = 160.0;
const IMG_PLACEHOLDER_HEIGHT_PX: f32 = 90.0;

fn assert_img_intrinsic_row_base(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "100px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let img = assert_append_img_to(&mut root, container);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, img).width_px,
        IMG_PLACEHOLDER_WIDTH_PX,
        &format!("{name}: img intrinsic base width"),
    );
    assert_close(
        assert_node_layout(&root, img).height_px,
        IMG_PLACEHOLDER_HEIGHT_PX,
        &format!("{name}: img intrinsic height"),
    );
}

// CSS Flexbox 1 §8.3 (align-items/align-self stretch) composed with CSS 2.1
// §10.6.2 (replaced-element cross size through the intrinsic ratio): stretch
// applies only to an item whose cross size is `auto`. A replaced `img` with an
// intrinsic ratio has a NON-auto cross size (its used height follows the ratio
// from the used width), so stretch must not distort it. Gecko exempts replaced
// items in `nsFlexContainerFrame::GetFlexBaseSizeAndHypotheticalMainSize` /
// the `Stretch` branch of `AlignSelf` resolution; Blink in
// `FlexItem::CrossAxisIntrinsicSize` (a replaced item carries a definite cross
// intrinsic size, so `Stretch` is a no-op). Closest WPT oracles:
// `css/css-flexbox/flexbox_stretch-image.html`.
//
// ON-SPEC (moved in flex arc step 7, FLEX_STEP7_DESIGN.md D5): the cross-axis
// stretch path no longer overrides a replaced-ratio item's cross size — the
// stretched-content override returns `None` for `replaced_ratio`, so the img's
// height stays the ratio-derived 90px from its resolved main 160px instead of
// the line cross size.
#[test]
fn flex_stretch_keeps_replaced_img_ratio() {
    let rows: [ReplacedStretchRow; 2] = [
        ("direct_stretch", assert_replaced_img_direct_stretch),
        (
            "nested_flex_stretch",
            assert_replaced_img_nested_flex_stretch,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named replaced-img stretch probe: the row name plus a runner.
type ReplacedStretchRow = (&'static str, fn(&str));

fn assert_replaced_img_direct_stretch(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "120px");
    // default align-items: stretch.
    let img = assert_append_img_to(&mut root, container);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, img).width_px,
        IMG_PLACEHOLDER_WIDTH_PX,
        &format!("{name}: img width"),
    );
    // On-spec: stretch does not distort — the img's height follows the ratio.
    assert_close(
        assert_node_layout(&root, img).height_px,
        IMG_PLACEHOLDER_HEIGHT_PX,
        &format!("{name}: img height keeps the ratio instead of the line cross"),
    );
}

fn assert_replaced_img_nested_flex_stretch(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "100px");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    let img = assert_append_img_to(&mut root, inner);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, img).width_px,
        IMG_PLACEHOLDER_WIDTH_PX,
        &format!("{name}: nested img width"),
    );
    // On-spec: stretch through the nested flex does not distort the ratio.
    assert_close(
        assert_node_layout(&root, img).height_px,
        IMG_PLACEHOLDER_HEIGHT_PX,
        &format!("{name}: nested img height keeps the ratio instead of 100px"),
    );
}

#[test]
// CSS Flexbox §8.3: the replaced-ratio exemption must not leak to non-replaced
// items. A block item with an auto cross size still stretches to the line cross
// size (its cross IS `auto`), while a replaced-ratio img keeps its ratio.
fn flex_stretch_applies_to_non_replaced_auto_cross_items() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "120px");
    // default align-items: stretch.
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "10px");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "0");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, item).height_px,
        120.0,
        "non-replaced auto-cross block item stretches to the line cross size",
    );
}

#[test]
// CSS Flexbox §9.4 + §9.7: line cross sizes resolve AFTER main resolution. A
// text-bearing block item that shrinks re-wraps at the resolved width (taller),
// and the line cross reflects that resolved cross rather than the unshrunk
// measured cross. Gecko flags this `CanMainSizeInfluenceCrossSize`; Blink
// re-lays-out items at the flexed size for the cross determination.
fn flex_line_cross_reflects_resolved_text_wrap_height() {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "40px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "auto");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "overflow", "visible");
    let text = assert_append_text_to(&mut root, item, "aa bb cc dd ee");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The item shrinks to its min-content (the widest word) and the text wraps
    // into multiple lines: the container's cross size is the resolved wrapped
    // height, taller than a single line.
    let container_height_px = assert_node_layout(&root, container).height_px;
    let single_line_height_px = assert_text_intrinsic(&root, text).fragment_line_height_max_px;
    assert!(
        container_height_px > single_line_height_px,
        "line cross should reflect the resolved wrapped height ({container_height_px}), \
         not the single-line measured height ({single_line_height_px})",
    );
}

#[test]
// Flex arc step 7, D4: `FlexNodesRefolded` counts one per flagged-and-width-
// changed re-measure. A shrinking text-bearing item refolds once; a width-
// unchanged text-bearing item does not refold.
fn flex_nodes_refolded_counts_flagged_width_changed_items() {
    // Changed: a text-bearing block item that shrinks (flagged + width changed).
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "40px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "auto");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_append_text_to(&mut root, item, "aa bb cc dd ee");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.metrics().count(RootDomMetricName::FlexNodesRefolded),
        1,
        "a shrinking text-bearing item refolds once",
    );

    // Unchanged: a text-bearing block item whose resolved main equals its
    // measured main (no shrink) never refolds.
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "400px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "flex-basis", "auto");
    assert_style_set(&mut root, item, "flex-grow", "0");
    assert_style_set(&mut root, item, "flex-shrink", "1");
    assert_style_set(&mut root, item, "overflow", "visible");
    assert_append_text_to(&mut root, item, "aa bb cc dd ee");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.metrics().count(RootDomMetricName::FlexNodesRefolded),
        0,
        "a width-unchanged text-bearing item does not refold",
    );
}

fn assert_img_shrink_context(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "100px");
    assert_style_set(&mut root, container, "height", "100px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    let img = assert_append_img_to(&mut root, container);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_style_set(&mut root, img, "flex-shrink", "1");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let img_width = assert_node_layout(&root, img).width_px;
    let img_height = assert_node_layout(&root, img).height_px;
    // The replaced item's automatic minimum is its content size suggestion
    // (intrinsic 160px); shrinking below that is divergence.
    assert_close(
        img_width,
        IMG_PLACEHOLDER_WIDTH_PX,
        &format!("{name}: img floors at intrinsic width"),
    );
    assert_close(
        img_height,
        IMG_PLACEHOLDER_HEIGHT_PX,
        &format!("{name}: img height follows ratio"),
    );
}

fn assert_img_percent_in_definite_item(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "100px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "200px");
    let img = assert_append_img_to(&mut root, item);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_style_set(&mut root, img, "width", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // 50% of the 200px item content width = 100px; height transfers through
    // the 16:9 ratio.
    assert_close(
        assert_node_layout(&root, img).width_px,
        100.0,
        &format!("{name}: img 50% width"),
    );
    assert_close(
        assert_node_layout(&root, img).height_px,
        100.0 / (IMG_PLACEHOLDER_WIDTH_PX / IMG_PLACEHOLDER_HEIGHT_PX),
        &format!("{name}: img height through ratio"),
    );
}

fn assert_img_text_sibling_gap(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "100px");
    assert_style_set(&mut root, container, "align-items", "flex-start");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let img = assert_append_img_to(&mut root, container);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    let label = assert_append_div_to(&mut root, container);
    let _label_text = assert_append_text_to(&mut root, label, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let label_box = node_box(&root, label).expect("label role should expose a box model");
    assert_close(
        label_box.border_box.left_px,
        IMG_PLACEHOLDER_WIDTH_PX + 10.0,
        &format!("{name}: label after img + gap"),
    );
    assert!(
        label_box.border_box.width_px > 0.0,
        "{name}: label keeps its measured width",
    );
}

// CSS Flexbox 1 §9.9 (intrinsic sizes) and §9.7 (flexible length resolution)
// composed across a NESTED flex boundary with real content: an inner flex
// container whose items carry text/images is measured for its content, then
// resolved through the outer line. This is the measurement-recursion path the
// flex arc step 4 §9.9 intrinsic fold (D2a) now computes — the two former
// graceful gates (`flex_container_content_based_auto_min_is_supported` and
// the grow-factor boundary in `horizontal_flex_container_content_based_auto_min_content_width_px`,
// both `flex.rs`) died with the walk. Gecko (`GenerateFlexItemForChild` →
// `FlexLine::ResolveFlexibleLengths` → final reflow, `nsFlexContainerFrame.cpp`)
// and Blink (`ConstructAndAppendFlexItems` → `LineFlexer::ResolveFlexibleLengths`
// → `BuildSpaceForLayout`, `flex_layout_algorithm.cc`) measure the inner
// container from its content before resolving the outer line.
#[test]
fn nested_flex_with_content() {
    let rows: [NestedFlexContentRow; 5] = [
        (
            "a_inner_row_text_items_with_gap",
            assert_nested_row_text_items_gap,
        ),
        (
            "b_inner_column_text_items_measured_size",
            assert_nested_column_text_items_measured,
        ),
        (
            "c_inner_row_img_text_auto_min",
            assert_nested_row_img_text_auto_min,
        ),
        (
            "d_inner_justify_center_uses_outer_grown_width",
            assert_nested_justify_center_grown,
        ),
        (
            "e_inner_item_text_align_center",
            assert_nested_item_text_align_center,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named nested-flex-with-content probe: the row name plus a runner.
type NestedFlexContentRow = (&'static str, fn(&str));

/// Row (a): outer row → inner row (no width) → two text children. A
/// block-level inner flex container used to stretch to the outer's content
/// width (200px); its auto-basis base now reads the record's max-content (§9.2
/// base consequence, flex arc step 4), so it shrink-wraps to the merged
/// anonymous run's max-content sum. The two text nodes still merge into ONE
/// anonymous run (no inter-item gap), so the second text sits contiguous after
/// the first.
fn assert_nested_row_text_items_gap(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    assert_style_set(&mut root, inner, "column-gap", "10px");
    let left_text = assert_append_text_to(&mut root, inner, "Left");
    let right_text = assert_append_text_to(&mut root, inner, "Right");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let left_width = assert_node_layout(&root, left_text).width_px;
    let right_width = assert_node_layout(&root, right_text).width_px;
    let inner_width = assert_node_layout(&root, inner).width_px;
    assert_close(
        inner_width,
        left_width + right_width,
        &format!("{name}: inner shrink-wraps to its max-content sum"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    assert_close(
        text_runs[1].position_px[0],
        left_width,
        &format!("{name}: second text contiguous after first (one run)"),
    );
}

/// Row (b): outer row → inner COLUMN (no width) → text items. The inner's
/// auto-basis base now reads the record's max-content (§9.2 base consequence,
/// flex arc step 4), so it shrink-wraps to its widest item rather than the
/// former measured 200px.
fn assert_nested_column_text_items_measured(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    assert_style_set(&mut root, inner, "flex-direction", "column");
    let first = assert_append_div_to(&mut root, inner);
    let first_text = assert_append_text_to(&mut root, first, "Alpha");
    let second = assert_append_div_to(&mut root, inner);
    let second_text = assert_append_text_to(&mut root, second, "Beta");
    let sibling = assert_append_div_to(&mut root, outer);
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let widest_text_px = assert_text_intrinsic(&root, first_text)
        .max_content_width_px
        .max(assert_text_intrinsic(&root, second_text).max_content_width_px);
    let inner_width = assert_node_layout(&root, inner).width_px;
    assert_close(
        inner_width,
        widest_text_px,
        &format!("{name}: inner column shrink-wraps to its widest item"),
    );
    let sibling_box = node_box(&root, sibling).expect("sibling role should expose a box model");
    assert_close(
        sibling_box.border_box.left_px,
        widest_text_px,
        &format!("{name}: sibling after max-content inner"),
    );
}

/// Row (c): outer row shrink context → inner row with img + text items. The
/// content-based auto-min walk over mixed replaced + text content sums the img
/// intrinsic width and the text min-content.
fn assert_nested_row_img_text_auto_min(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "50px");
    assert_style_set(&mut root, outer, "height", "100px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    assert_style_set(&mut root, inner, "flex-basis", "300px");
    assert_style_set(&mut root, inner, "flex-shrink", "1");
    let img = assert_append_img_to(&mut root, inner);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    let text = assert_append_text_to(&mut root, inner, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let inner_width = assert_node_layout(&root, inner).width_px;
    let expected_floor = IMG_PLACEHOLDER_WIDTH_PX + text_intrinsic.min_content_width_px;
    assert_close(
        inner_width,
        expected_floor,
        &format!("{name}: inner auto-min = img + text min-content"),
    );
}

/// Row (d): inner row with `justify-content: center` and a text item, the
/// inner grown by the outer flex-grow. The inner justify must use the
/// OUTER-resolved (grown) width, centering the text item in the grown box.
fn assert_nested_justify_center_grown(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    assert_style_set(&mut root, inner, "justify-content", "center");
    assert_style_set(&mut root, inner, "height", "20px");
    assert_style_set(&mut root, inner, "flex-basis", "20px");
    assert_style_set(&mut root, inner, "flex-grow", "1");
    assert_style_set(&mut root, inner, "flex-shrink", "1");
    let text = assert_append_text_to(&mut root, inner, "Mid");
    let sibling = assert_append_div_to(&mut root, outer);
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_width = assert_node_layout(&root, text).width_px;
    // Inner grew to 200 − 20 (sibling) = 180; the text item centers at
    // (180 − text_width) / 2.
    let inner_grown = 180.0;
    assert_close(
        assert_node_layout(&root, inner).width_px,
        inner_grown,
        &format!("{name}: inner grown width"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    assert_close(
        text_runs[0].position_px[0],
        (inner_grown - text_width) / 2.0,
        &format!("{name}: text centered in grown inner"),
    );
}

/// Row (e): a nested item with `text-align: center` composes the C1 alignment
/// into the nested structure — the text run fills the item's content box and
/// carries the center alignment.
fn assert_nested_item_text_align_center(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    let item = assert_append_div_to(&mut root, inner);
    assert_style_set(&mut root, item, "width", "120px");
    assert_style_set(&mut root, item, "text-align", "center");
    let _text = assert_append_text_to(&mut root, item, "Mid");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let text_runs = &plans[0].text_runs;
    assert_eq!(
        text_runs[0].alignment,
        TextAlign::Center,
        "{name}: alignment"
    );
    assert_close(
        text_runs[0].size_px[0],
        120.0,
        &format!("{name}: text run fills the item content box"),
    );
}

// CSS Flexbox 1 §4.5/§9.9 measurement recursion through 3–4 level content
// trees under flex items: an item whose subtree is nested blocks (or nested
// flex containers) measures its intrinsic width by walking the whole subtree,
// and content mutation at depth must re-measure on the next update (the
// phantom-dirty lesson in the visitor README). Gecko/Blink measure the item's
// content contribution through the full descendant tree before final reflow.
// Rows (c)/(d)/(e) additionally pin the deep-recursion measurement path — the
// two former graceful gates (`flex_container_content_based_auto_min_is_supported`
// and the grow-factor boundary, `flex.rs`) died in flex arc step 4, so a nested
// unsupported shape now folds its true content minimum instead of degrading to
// the measured size.
#[test]
fn deep_hierarchy_mixed_content() {
    let rows: [DeepHierarchyRow; 5] = [
        (
            "a_three_level_block_text_measurement",
            assert_deep_block_text_measurement,
        ),
        ("b_mixed_img_text_at_depth", assert_deep_mixed_img_text),
        (
            "c_flex_in_flex_in_flex_text_leaf",
            assert_deep_flex_text_leaf,
        ),
        (
            "d_append_text_at_depth_remeasures",
            assert_deep_append_text_remeasures,
        ),
        (
            "e_remove_deep_child_retracts",
            assert_deep_remove_child_retracts,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named deep-hierarchy probe: the row name plus a runner.
type DeepHierarchyRow = (&'static str, fn(&str));

/// Row (a): flex item → div → div → text (three levels). Measurement propagates
/// the text max-content up through the block levels to the item's auto width.
fn assert_deep_block_text_measurement(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    let mid = assert_append_div_to(&mut root, item);
    let deep = assert_append_div_to(&mut root, mid);
    let text = assert_append_text_to(&mut root, deep, "Deep text");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    assert_close(
        assert_node_layout(&root, item).width_px,
        text_intrinsic.max_content_width_px,
        &format!("{name}: item width propagates text through three levels"),
    );
}

/// Row (b): flex item → div → img + div → text (mixed content at depth). The
/// block's width is the widest child (the img intrinsic), propagated up.
fn assert_deep_mixed_img_text(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "100px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    let mid = assert_append_div_to(&mut root, item);
    let img = assert_append_img_to(&mut root, mid);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    let deep = assert_append_div_to(&mut root, mid);
    let _text = assert_append_text_to(&mut root, deep, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, item).width_px,
        IMG_PLACEHOLDER_WIDTH_PX,
        &format!("{name}: item width = widest deep child (img intrinsic)"),
    );
}

/// Row (c): flex → flex → flex → text (three flex levels). Measurement recurses
/// through two supported flex boundaries without panicking; the leaf text keeps
/// its own width.
fn assert_deep_flex_text_leaf(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let inner = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, inner, "display", "flex");
    let middle = assert_append_div_to(&mut root, inner);
    assert_style_set(&mut root, middle, "display", "flex");
    let leaf = assert_append_div_to(&mut root, middle);
    assert_style_set(&mut root, leaf, "display", "flex");
    let text = assert_append_text_to(&mut root, leaf, "Leaf");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    assert_close(
        assert_node_layout(&root, text).width_px,
        text_intrinsic.max_content_width_px,
        &format!("{name}: leaf text keeps its own width (no panic)"),
    );
}

/// Row (d): append a longer text node at depth 3 and update; the item must
/// re-measure and shift on the FIRST update (no delayed phantom-dirty shift).
fn assert_deep_append_text_remeasures(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    let mid = assert_append_div_to(&mut root, item);
    let deep = assert_append_div_to(&mut root, mid);
    let short = assert_append_text_to(&mut root, deep, "A");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let short_width = assert_node_layout(&root, item).width_px;

    root.set_text_content(short, "MuchLongerText")
        .expect("text mutation");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let grown_width = assert_node_layout(&root, item).width_px;
    assert!(
        grown_width > short_width,
        "{name}: item should re-measure wider after append ({grown_width} > {short_width})",
    );
}

/// Row (e): remove one of two deep children and update; the item's width
/// retracts to the remaining child on the first update.
fn assert_deep_remove_child_retracts(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    let mid = assert_append_div_to(&mut root, item);
    let wide = assert_append_div_to(&mut root, mid);
    let _wide_text = assert_append_text_to(&mut root, wide, "Deep text");
    let narrow = assert_append_div_to(&mut root, mid);
    let _narrow_text = assert_append_text_to(&mut root, narrow, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let before_width = assert_node_layout(&root, item).width_px;

    root.remove_node(wide).expect("deep child removal");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let after_width = assert_node_layout(&root, item).width_px;
    assert!(
        after_width < before_width,
        "{name}: item should retract after deep removal ({after_width} < {before_width})",
    );
}

// CSS Values 4 §percentages + CSS 2.1 §10.1 composed with a flex item's block
// SUBTREE: children INSIDE the item resolve box-feature percentages against
// their own containing block (the item's block child, whose content width is
// the item's content width), never against the flex container. This is a
// DIFFERENT path from `flex_item_percentages_resolve_against_flex_container`
// (which covers the item's OWN width-family percentages resolving against the
// container).
// Gecko/Blink resolve descendant percentages against the nearest block
// container's content box during the descendant reflow that follows flex item
// final layout.
#[test]
fn subtree_box_features_in_flex_items() {
    let rows: [SubtreeBoxFeatureRow; 4] = [
        (
            "a_grandchild_margin_left_percent",
            assert_subtree_grandchild_margin_left_percent,
        ),
        (
            "b_grandchild_padding_left_percent",
            assert_subtree_grandchild_padding_left_percent,
        ),
        (
            "c_grandchild_width_percent_of_definite_item",
            assert_subtree_grandchild_width_percent,
        ),
        (
            "e_item_border_padding_subtree_percent_basis",
            assert_subtree_percent_against_content_basis,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named subtree-box-feature probe: the row name plus a runner.
type SubtreeBoxFeatureRow = (&'static str, fn(&str));

fn assert_subtree_grandchild_margin_left_percent(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "margin-left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grandchild_box = node_box(&root, grandchild).expect("grandchild box");
    // On-spec: 10% of the block child's content width (= item content 100px).
    assert_close(
        grandchild_box.margin.left_px,
        10.0,
        &format!("{name}: grandchild margin-left resolves against item content"),
    );
}

fn assert_subtree_grandchild_padding_left_percent(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "padding-left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grandchild_box = node_box(&root, grandchild).expect("grandchild box");
    assert_close(
        grandchild_box.padding.left_px,
        10.0,
        &format!("{name}: grandchild padding-left resolves against item content"),
    );
}

fn assert_subtree_grandchild_width_percent(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "width", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, grandchild).width_px,
        50.0,
        &format!("{name}: grandchild 50% of item content width"),
    );
}

// CSS 2.1 §8.3.1 (margin collapsing) composed with CSS Flexbox 1 §4 (flex
// items) — ON-SPEC (relabeled from
// off_spec_flex_item_subtree_first_child_margin_containment in flex arc step 9).
// Blink reads only the item's OWN margins (`FlexItem::initial_margins`,
// flex_layout_algorithm.cc) and Gecko likewise (`FlexItem::mMargin`,
// nsFlexContainerFrame.cpp) — the flex cross-axis placement never sees a
// descendant's margin, so the ITEM must NOT offset (row a). CSS 2.1 §8.3.1: a
// box establishing a new block formatting context does NOT collapse margins with
// its in-flow children, so the grandchild's 20px first-child top margin is
// CONTAINED inside the item — the grandchild sits 20px below the item's
// content-box top (row b, now on-spec). Row (c) is the plain-block control
// (collapse-out is correct there). Closest WPT: the CSS21 margin-collapse suite
// (`css/CSS2/normal-flow/margin-collapse-*.xht`).
//
// The flex item's entered flag is now false (its flex-container parent marks it
// as an independent formatting context), so the leading-margin-chain walk
// (`collapsed_leading_margin_chain_px`, flow.rs) contains the chain as the first
// in-flow child's offset inside the item.
#[test]
fn flex_item_subtree_first_child_margin_containment() {
    let rows: [FlexItemSubtreeFirstChildMarginRow; 3] = [
        (
            "a_item_stays_at_flex_cross_start",
            assert_item_stays_at_flex_cross_start,
        ),
        (
            "b_grandchild_contained_at_item_content_top_plus_20",
            assert_grandchild_contained_at_item_content_top_plus_20,
        ),
        (
            "c_block_control_positive_margin_collapses_out",
            assert_block_control_positive_margin_collapses_out,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named flex-item subtree first-child margin probe: the row name plus a runner.
type FlexItemSubtreeFirstChildMarginRow = (&'static str, fn(&str));

/// Row (a): a flex item → empty block child → grandchild with `margin-top: 20px`
/// in a `flex-start`-aligned row. CSS Flexbox §4 gives the item an independent
/// formatting context and both engines read only the item's OWN margins
/// (Blink `FlexItem::initial_margins`, Gecko `FlexItem::mMargin`), so the item
/// border box stays at the flex cross-start (0) — the descendant's margin must
/// NOT offset the item.
fn assert_item_stays_at_flex_cross_start(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "40px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "margin-top", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = node_box(&root, item).expect("item box");
    assert_close(
        item_box.border_box.top_px,
        0.0,
        &format!("{name}: item border box stays at flex cross-start"),
    );
}

/// Row (b): the internal disposition of the same flex scenario. The grandchild's
/// border box sits 20px below the item's CONTENT-box top — the 20px margin is
/// CONTAINED inside the item (its independent formatting context) as a real
/// first-in-flow offset, not swallowed into phantom height.
fn assert_grandchild_contained_at_item_content_top_plus_20(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "40px");
    assert_style_set(&mut root, outer, "align-items", "flex-start");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "margin-top", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grandchild_box = node_box(&root, grandchild).expect("grandchild box");
    let item_box = node_box(&root, item).expect("item box");
    let item_content_top_px =
        item_box.border_box.top_px + item_box.border.top_px + item_box.padding.top_px;
    assert_close(
        grandchild_box.border_box.top_px,
        item_content_top_px + 20.0,
        &format!("{name}: grandchild sits 20px below the item content top (contained)"),
    );
    assert_close(
        assert_node_layout(&root, item).content_height_px,
        20.0,
        &format!("{name}: the contained 20px is a real first-in-flow offset"),
    );
}

/// Row (c): the same subtree shape in a plain block (non-flex) container — the
/// positive flex-free reference. Normal flow collapses the positive first-child
/// margin out through the empty intermediary and the container's top boundary
/// (CSS 2.1 §8.3.1), so the grandchild sits at the container's content top.
/// This is the collapse-out behavior the flex item's independent formatting
/// context must NOT follow (row b's corrected claim is containment instead).
fn assert_block_control_positive_margin_collapses_out(name: &str) {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block_child = assert_append_div_to(&mut root, container);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, grandchild, "margin-top", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grandchild_box = node_box(&root, grandchild).expect("grandchild box");
    let container_box = node_box(&root, container).expect("container box");
    let container_content_top_px = container_box.border_box.top_px
        + container_box.border.top_px
        + container_box.padding.top_px;
    assert_close(
        grandchild_box.border_box.top_px,
        container_content_top_px,
        &format!("{name}: grandchild sits at the block container content top (collapsed out)"),
    );
}

fn assert_subtree_percent_against_content_basis(name: &str) {
    let (mut world, mut root, outer) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, outer, "200px");
    assert_style_set(&mut root, outer, "height", "20px");
    let item = assert_append_div_to(&mut root, outer);
    assert_style_set(&mut root, item, "width", "100px");
    assert_style_set(&mut root, item, "padding-left", "10px");
    assert_style_set(&mut root, item, "border-left-width", "2px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "width", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The grandchild's 50% resolves against the item's CONTENT width (100 −
    // padding-left 10 − border-left 2 = 88), not the border-box 100.
    let grandchild_width = assert_node_layout(&root, grandchild).width_px;
    let item_box = node_box(&root, item).expect("item box");
    let content_width = item_box.border_box.width_px
        - item_box.padding.left_px
        - item_box.padding.right_px
        - item_box.border.left_px
        - item_box.border.right_px;
    assert_close(
        grandchild_width,
        content_width * 0.5,
        &format!("{name}: grandchild 50% of item content width (after insets)"),
    );
}

// CSS Flexbox 1 §4 (anonymous items) composed with CSS Display 3 §2.7
// (blockification) and CSS Align 3 §5.1.3 (`normal` behaves as `stretch`):
// per both engines, a lone `display: inline` element in a flex container is
// NOT a flex item and is NOT blockified — it joins a length-1 anonymous run
// wrapped in an anonymous BLOCK box (Blink: anonymous `LayoutBlock` at
// `LayoutBlock::AddChild`, `layout_block.cc:147-171`; Gecko:
// `-moz-anonymous-item` `nsBlockFrame` in `CreateNeededAnonFlexOrGridItems`,
// `nsCSSFrameConstructor.cpp:8134`). The anonymous block participates as the
// item — it STRETCHES to the line cross size under `align-items`/`align-self:
// stretch` when the cross size is auto; the inline content inside it renders
// at its own inline height, positioned inside the stretched block at the line's
// cross-start. No single WPT oracle pairs `display: inline`/`inline-block` flex
// items with cross-axis stretch, so this is cited from §4/§2.7 directly;
// closest family is `css/css-flexbox/flex-item-blockify-*`.
//
// ON-SPEC (re-adjudicated from `off_spec_flex_item_inline_level_display_blockification`
// in flex arc step 3c): the T1 pin's spec-correct expectation ("blockified →
// the item stretches to the line cross size") was written against a
// blockification model the engines do not use. The engine-correct observable is
// the run item's resolved cross == the line cross (the anonymous wrapper
// stretches — pinned at the unit level by
// `anonymous_inline_run_resolves_cross_to_line_cross_under_stretch` in
// `flex.rs`), while the inline content keeps its line-height cross size at the
// line's cross-start. Rows (a)/(c) therefore flip from "inline does not
// stretch" (divergent) to "the wrapper stretches, the inline content keeps its
// inline height" (on-spec); rows (b)/(d) stay on-spec.
#[test]
fn flex_item_inline_level_display_participation() {
    let rows: [InlineLevelDisplayParticipationRow; 4] = [
        (
            "a_inline_item_forms_run_and_keeps_inline_height",
            assert_inline_item_forms_run_and_keeps_inline_height,
        ),
        (
            "b_inline_block_item_block_measures_and_stretches",
            assert_inline_block_item_block_measures_and_stretches,
        ),
        (
            "c_inline_item_under_stretch_keeps_inline_height_vs_block_control",
            assert_inline_item_under_stretch_vs_block_control,
        ),
        (
            "d_block_control_shrink_wraps_and_stretches",
            assert_block_control_shrink_wraps_and_stretches,
        ),
    ];
    for (name, run) in rows {
        run(name);
    }
}

/// A named inline-level display participation probe: the row name plus a runner.
type InlineLevelDisplayParticipationRow = (&'static str, fn(&str));

/// Row (a): a lone `display: inline` child with short text in a definite 200×20
/// row. The child forms a length-1 anonymous run: the run item (the anonymous
/// block) resolves its cross to the 20px line cross under the stretch-behaving
/// `normal` initial (pinned at the unit level), while the inline content renders
/// at its own inline height (the text line height, ≈14px) at the line's
/// cross-start — the engine model, not blockification.
fn assert_inline_item_forms_run_and_keeps_inline_height(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "inline");
    let text = assert_append_text_to(&mut root, item, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let item_layout = assert_node_layout(&root, item);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = the inline run member; the text child
    // emits a text run.
    assert_eq!(rects.len(), 2, "{name}: container + inline element rects");
    assert_close(
        item_layout.width_px,
        text_intrinsic.max_content_width_px,
        &format!("{name}: inline run member main size shrink-wraps to text"),
    );
    // On-spec: the inline content keeps its inline height (≈14px) at the line's
    // cross-start — the anonymous wrapper stretches, the inline content does not.
    assert_close(
        item_layout.height_px,
        text_intrinsic.line_height_px,
        &format!("{name}: inline content keeps its inline height, not the 20px line cross"),
    );
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        text_intrinsic.line_height_px,
        &format!("{name}: inline element geometry at the line's cross-start"),
    );
    // The rendered text rect keeps its inline height at the line's cross-start.
    assert_eq!(plans[0].text_runs.len(), 1, "{name}: one text run");
    assert_close(
        plans[0].text_runs[0].size_px[1],
        text_intrinsic.line_height_px,
        &format!("{name}: rendered text rect height stays at its inline height"),
    );
    assert_close(
        plans[0].text_runs[0].position_px[1],
        0.0,
        &format!("{name}: rendered text rect sits at the line's cross-start"),
    );
}

/// Row (b): a `display: inline-block` item, same shape — UNCHANGED on-spec
/// (T1's original outcome). `inline-block` is a run-breaker (not an inline run
/// member), so it block-measures and stretches its auto cross size to the line
/// cross size exactly like the block control.
fn assert_inline_block_item_block_measures_and_stretches(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "inline-block");
    let text = assert_append_text_to(&mut root, item, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let item_layout = assert_node_layout(&root, item);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_close(
        item_layout.width_px,
        text_intrinsic.max_content_width_px,
        &format!("{name}: inline-block item main size shrink-wraps to text"),
    );
    assert_close(
        item_layout.height_px,
        20.0,
        &format!("{name}: inline-block item stretches to the line cross size"),
    );
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        20.0,
        &format!("{name}: inline-block item geometry"),
    );
}

/// Row (c): a `display: inline` child under explicit `align-items: stretch` next
/// to a `display: block` control. Both STRETCH to the 50px line cross size (the
/// run item's resolved cross == 50, pinned at the unit level; the block
/// control's content box == 50) — the inline child's rendered content keeps its
/// line-height cross size (≈14px) inside the stretched wrapper, which is the
/// engine model, not a failure to blockify `inline` → `block`.
fn assert_inline_item_under_stretch_vs_block_control(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "50px");
    assert_style_set(&mut root, container, "align-items", "stretch");
    let inline_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, inline_item, "display", "inline");
    let inline_text = assert_append_text_to(&mut root, inline_item, "Hi");
    let block_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, block_item, "display", "block");
    let _block_text = assert_append_text_to(&mut root, block_item, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, inline_text);
    let inline_layout = assert_node_layout(&root, inline_item);
    let block_layout = assert_node_layout(&root, block_item);
    // On-spec: the inline child's rendered content keeps its line-height cross
    // size inside the stretched wrapper (the wrapper stretches, not the content).
    assert_close(
        inline_layout.height_px,
        text_intrinsic.line_height_px,
        &format!("{name}: inline content keeps line-height cross size under stretch"),
    );
    // Spec-correct control: a block item stretches to the 50px line cross size.
    assert_close(
        block_layout.height_px,
        50.0,
        &format!("{name}: block control stretches to the line cross size"),
    );
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = inline run member, rects[2] = block control.
    assert_eq!(rects.len(), 3, "{name}: container + two items");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        text_intrinsic.line_height_px,
        &format!("{name}: inline run member rect"),
    );
    assert_rect_op_geometry(
        &rects[2],
        text_intrinsic.max_content_width_px,
        0.0,
        text_intrinsic.max_content_width_px,
        50.0,
        &format!("{name}: block control rect"),
    );
}

/// Row (d): a `display: block` control item, the known-good reference geometry
/// — UNCHANGED on-spec. It shrink-wraps its main size to the text and stretches
/// its auto cross size to the line cross size.
fn assert_block_control_shrink_wraps_and_stretches(name: &str) {
    let (mut world, mut root, container) = flex_row_container_setup();
    assert_set_flex_row_container_width(&mut root, container, "200px");
    assert_style_set(&mut root, container, "height", "20px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "display", "block");
    let text = assert_append_text_to(&mut root, item, "Hi");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_intrinsic = assert_text_intrinsic(&root, text);
    let item_layout = assert_node_layout(&root, item);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 2, "{name}: container + item rects");
    assert_close(
        item_layout.width_px,
        text_intrinsic.max_content_width_px,
        &format!("{name}: block item main size shrink-wraps to text"),
    );
    assert_close(
        item_layout.height_px,
        20.0,
        &format!("{name}: block item stretches to the line cross size"),
    );
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        text_intrinsic.max_content_width_px,
        20.0,
        &format!("{name}: block item geometry"),
    );
}
