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
//! These proofs are activated batch-by-batch as `ui-dom`'s staged flex solver
//! matches the researched browser contract. Remaining ignored proofs pin the
//! external contract ahead of implementation; active proofs lock the admitted
//! batch behavior in place.

use ui_dom::{DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, RootDom};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_close, assert_load_root_stylesheet,
    assert_node_border_box_screen_left_px, assert_node_border_box_screen_top_px,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, first, "10px", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "1");
    assert_set_flex_basis_and_cross_size(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "1");
    assert_set_attr_str(&mut root, second, "flex-shrink", "1");
    assert_set_flex_basis_and_cross_size(&mut root, third, "10px", "10px");
    assert_set_attr_str(&mut root, third, "flex-grow", "2");
    assert_set_attr_str(&mut root, third, "flex-shrink", "1");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first grow item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first grow item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second grow item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second grow item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        40.0,
        "third grow item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "third grow item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "flex-direction", "row-reverse");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        80.0,
        "first reversed-row item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first reversed-row item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        70.0,
        "second reversed-row item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second reversed-row item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        60.0,
        "third reversed-row item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "third reversed-row item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "direction", "rtl");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        80.0,
        "first rtl row item should start at inline-start",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        70.0,
        "second rtl row item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        60.0,
        "third rtl row item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "direction", "rtl");
    assert_set_attr_str(&mut root, container, "flex-direction", "row-reverse");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first rtl row-reverse item should start at physical left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second rtl row-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        20.0,
        "third rtl row-reverse item left",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "flex-direction", "row-reverse");
    assert_set_attr_str(&mut root, container, "justify-content", "start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        10.0,
        "first row-reverse start-aligned item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "second row-reverse start-aligned item left",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "flex-direction", "row-reverse");
    assert_set_attr_str(&mut root, container, "justify-content", "end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        50.0,
        "first row-reverse end-aligned item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        40.0,
        "second row-reverse end-aligned item left",
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "second column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "second column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        20.0,
        "third column item top",
    );
}

#[test]
// CSS Flexbox 5.2 and WPT flex-wrap-005.html: wrap-reverse keeps each line's
// item order intact while reversing the cross-axis stacking order of flex
// lines. Gecko models this as a reversed cross axis in nsFlexContainerFrame.cpp
// and Blink applies it as reversed line order plus flipped cross-axis placement
// in flex_layout_algorithm.cc and ng_flex_layout_algorithm.cc.
fn wrap_reverse_stacks_wrapped_rows_from_the_opposite_cross_start() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_set_attr_str(&mut root, container, "height", "25px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        15.0,
        "first wrap-reverse item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        15.0,
        "second wrap-reverse item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        5.0,
        "third wrap-reverse item top",
    );
}

#[test]
// CSS Align 3 `start`/`end` are logical, while `flex-start`/`flex-end` are
// flex-relative. In `wrap-reverse`, those differ on the cross axis even for a
// single-line row container. WebKit added a WPT for this exact boundary in
// `self-align-start-end-flex-001.html`.
fn align_items_start_ignores_wrap_reverse_cross_flip() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let tall = assert_append_div_to(&mut root, container);
    let short = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "30px");
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_attr_str(&mut root, container, "align-items", "start");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, tall),
        30.0,
        "tall sibling should anchor the wrap-reverse line at cross-end",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, short),
        30.0,
        "logical start should stay at the line's block-start even when wrap-reverse flips flex-start",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_attr_str(&mut root, container, "align-items", "end");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, tall),
        30.0,
        "tall sibling should still occupy the wrap-reverse line anchor",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, short),
        40.0,
        "logical end should stay at the line's block-end even when wrap-reverse flips flex-end",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_attr_str(&mut root, container, "align-items", "start");
    assert_set_inflexible_row_item(&mut root, tall, "10px", "20px");
    assert_set_inflexible_row_item(&mut root, short, "10px", "10px");
    assert_set_attr_str(&mut root, short, "align-self", "end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, tall),
        30.0,
        "logical-start sibling should stay at the line's block-start",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, short),
        40.0,
        "logical end override should ignore wrap-reverse flex-end flipping",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_attr_str(&mut root, container, "align-content", "start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "logical cross-start should place the startmost wrap-reversed line at the physical top edge",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        10.0,
        "second wrap-reversed line should follow directly after the first logical-start line",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "second wrap-reversed line should preserve the line pair top",
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
    assert_set_attr_str(&mut root, container, "flex-direction", "column-reverse");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first reversed-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        80.0,
        "first reversed-column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "second reversed-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        70.0,
        "second reversed-column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third reversed-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        60.0,
        "third reversed-column item top",
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
    assert_set_attr_str(&mut root, container, "direction", "rtl");
    assert_set_attr_str(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_column_item(&mut root, child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, child),
        40.0,
        "rtl column cross-start should be the physical right edge",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, first, "40px", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "2");
    assert_set_flex_basis_and_cross_size(&mut root, second, "40px", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "0");
    assert_set_attr_str(&mut root, second, "flex-shrink", "3");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first shrink item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first shrink item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        28.0,
        "second shrink item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second shrink item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, growing, "flex", "1 1 10px");
    assert_set_attr_str(&mut root, growing, "height", "10px");
    assert_set_attr_str(&mut root, fixed, "flex", "0 0 20px");
    assert_set_attr_str(&mut root, fixed, "height", "10px");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, fixed),
        80.0,
        "flex shorthand fixed item left",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    for child in [first, second] {
        assert_set_attr_str(&mut root, child, "flex-basis", "10px");
        assert_set_attr_str(&mut root, child, "flex-grow", "1");
        assert_set_attr_str(&mut root, child, "flex-shrink", "1");
        assert_set_attr_str(&mut root, child, "height", "10px");
    }
    assert_set_attr_str(&mut root, first, "max-width", "20px");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        20.0,
        "sibling left after max-width freeze",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    for child in [first, second] {
        assert_set_attr_str(&mut root, child, "flex-basis", "20px");
        assert_set_attr_str(&mut root, child, "flex-grow", "0");
        assert_set_attr_str(&mut root, child, "flex-shrink", "1");
        assert_set_attr_str(&mut root, child, "height", "10px");
    }
    assert_set_attr_str(&mut root, first, "min-width", "15px");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "sibling left after min-width freeze",
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
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first wrapped item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first wrapped item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "second wrapped item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "second wrapped item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        30.0,
        "third wrapped item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        10.0,
        "third wrapped item top",
    );
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

    assert_set_attr_str(&mut root, container, "display", "flex");
    assert_set_attr_str(&mut root, container, "flex-flow", "column wrap");
    assert_set_attr_str(&mut root, container, "width", "40px");
    assert_set_attr_str(&mut root, container, "height", "25px");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first wrapped-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first wrapped-column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "second wrapped-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "second wrapped-column item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        10.0,
        "third wrapped-column item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "third wrapped-column item top",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_inflexible_row_item(&mut root, dom_first, "10px", "10px");
    assert_set_attr_str(&mut root, dom_first, "order", "3");
    assert_set_inflexible_row_item(&mut root, dom_second, "10px", "10px");
    assert_set_attr_str(&mut root, dom_second, "order", "1");
    assert_set_inflexible_row_item(&mut root, dom_third, "10px", "10px");
    assert_set_attr_str(&mut root, dom_third, "order", "2");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, dom_second),
        0.0,
        "order 1 item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, dom_third),
        10.0,
        "order 2 item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, dom_first),
        20.0,
        "order 3 item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "justify-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "justify-content should not rewrite DOM sibling links",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first centered item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        40.0,
        "first centered item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second centered item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        50.0,
        "second centered item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "justify-content", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        80.0,
        "first flex-end item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        90.0,
        "second flex-end item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "justify-content", "space-between");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first space-between item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        45.0,
        "second space-between item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        90.0,
        "third space-between item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "justify-content", "space-around");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        35.0 / 3.0,
        "first space-around item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        45.0,
        "second space-around item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        235.0 / 3.0,
        "third space-around item left",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "justify-content", "space-evenly");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        17.5,
        "first space-evenly item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        45.0,
        "second space-evenly item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        72.5,
        "third space-evenly item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "padding-left", "4px");
    assert_set_attr_str(&mut root, container, "padding-right", "4px");
    assert_set_attr_str(&mut root, container, "column-gap", "5px");
    assert_set_attr_str(&mut root, container, "justify-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        31.5,
        "first centered padded-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        46.5,
        "second centered padded-gap item left",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "center");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first align-items center left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        20.0,
        "first align-items center top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second align-items center left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "second align-items center top",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        40.0,
        "first align-items flex-end top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        40.0,
        "second align-items flex-end top",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "stretch");
    assert_set_attr_str(&mut root, first, "flex-basis", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "0");
    assert_set_attr_str(&mut root, second, "flex-basis", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "0");
    assert_set_attr_str(&mut root, second, "flex-shrink", "0");

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
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first align-items stretch item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second align-items stretch item top",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "align-self", "center");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "align-self should not rewrite DOM sibling links",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "flex-start item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "flex-start item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "align-self center item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "align-self center item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "flex-start");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "align-self", "flex-end");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "flex-start item top when sibling uses align-self flex-end",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        40.0,
        "align-self flex-end item top",
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "align-items", "flex-start");
    assert_set_attr_str(&mut root, first, "flex-basis", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "0");
    assert_set_attr_str(&mut root, first, "height", "10px");
    assert_set_attr_str(&mut root, second, "flex-basis", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "0");
    assert_set_attr_str(&mut root, second, "flex-shrink", "0");
    assert_set_attr_str(&mut root, second, "align-self", "stretch");

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
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "align-self stretch item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "50px");
    assert_set_attr_str(&mut root, container, "place-items", "end center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "place-self", "center auto");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        40.0,
        "place-items first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "place-self overriding item top",
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
    assert_set_attr_str(&mut root, container, "height", "30px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(
        &mut root,
        container,
        "place-content",
        "center space-between",
    );
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "place-content first item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        5.0,
        "place-content first item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "place-content second item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        5.0,
        "place-content second item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "place-content third item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        15.0,
        "place-content third item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "center");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first align-content center item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        10.0,
        "first align-content center item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second align-content center item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "second align-content center item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third align-content center item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        20.0,
        "third align-content center item top",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "flex-end");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        20.0,
        "first align-content flex-end item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "second align-content flex-end item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        30.0,
        "third align-content flex-end item top",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "space-between");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first align-content space-between item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second align-content space-between item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        30.0,
        "third align-content space-between item top",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "space-around");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        5.0,
        "first align-content space-around item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        5.0,
        "second align-content space-around item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        25.0,
        "third align-content space-around item top",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "space-evenly");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        20.0 / 3.0,
        "first align-content space-evenly item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0 / 3.0,
        "second align-content space-evenly item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        70.0 / 3.0,
        "third align-content space-evenly item top",
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
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "align-content", "stretch");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first align-content stretch item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second align-content stretch item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        20.0,
        "third align-content stretch item top",
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

    assert_set_attr_str(&mut root, parent, "width", "90px");
    assert_set_attr_str(&mut root, parent, "height", "10px");
    assert_set_attr_str(&mut root, container, "display", "flex");
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_flex_basis_and_cross_size(&mut root, item, "10px", "10px");
    assert_set_attr_str(&mut root, item, "flex-grow", "1");
    assert_set_attr_str(&mut root, item, "flex-shrink", "1");
    assert_set_attr_str(&mut root, item, "overflow", "scroll");

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
    let first_append_pass = titlebar_mutation_probe(
        &scenario.root,
        scenario.container,
        scenario.title_host,
        scenario.end,
    );
    assert_auto_width_titlebar_append_probe(first_append_pass);
    assert_close(
        assert_node_border_box_screen_left_px(&scenario.root, inserted_end),
        104.0,
        "first auto-width append pass inserted end left",
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        32.0,
        DomPointerUpdate::Unchanged,
    );
    assert_eq!(
        first_append_pass,
        titlebar_mutation_probe(
            &scenario.root,
            scenario.container,
            scenario.title_host,
            scenario.end,
        ),
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
    let first_remove_pass = titlebar_mutation_probe(
        &scenario.root,
        scenario.container,
        scenario.title_host,
        scenario.end,
    );
    assert_auto_width_titlebar_remove_probe(first_remove_pass);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        64.0,
        DomPointerUpdate::Unchanged,
    );
    assert_eq!(
        first_remove_pass,
        titlebar_mutation_probe(
            &scenario.root,
            scenario.container,
            scenario.title_host,
            scenario.end,
        ),
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "1");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_attr_str(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_attr_str(&mut root, sibling, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");
    assert_set_attr_str(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "grown nested flex item width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, sibling),
        80.0,
        "sibling after grown nested item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "1");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "default-overflow nested flex item width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
        35.0,
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "0");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "0px", "20px");
    assert_set_attr_str(&mut root, sibling, "overflow", "scroll");
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "width", "20px");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "1");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "auto-basis nested flex item final width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
        35.0,
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "0.5");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        50.0,
        "fractional-grow nested flex item width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, sibling),
        50.0,
        "fractional-grow sibling starts after partially grown nested item",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
        20.0,
        "fractional-grow nested child centered in partially grown item",
    );
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "80px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "0");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    assert_set_attr_str(&mut root, nested_child, "height", "10px");
    assert_set_attr_str(&mut root, nested_child, "flex-basis", "0px");
    assert_set_attr_str(&mut root, nested_child, "flex-grow", "0");
    assert_set_attr_str(&mut root, nested_child, "flex-shrink", "1");

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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "80px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "0");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_attr_str(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "10px", "20px");
    assert_set_attr_str(&mut root, sibling, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "10px", "10px");
    assert_set_attr_str(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        40.0,
        "shrunk nested flex item width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, sibling),
        40.0,
        "sibling after shrunk nested item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_inflexible_row_item(&mut root, start, "16px", "20px");
    assert_set_inflexible_row_item(&mut root, end, "16px", "20px");
    assert_set_attr_str(&mut root, start, "overflow", "scroll");
    assert_set_attr_str(&mut root, end, "overflow", "scroll");

    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "0px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "1");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_attr_str(&mut root, nested, "padding-left", "4px");
    assert_set_attr_str(&mut root, nested, "padding-right", "4px");
    assert_set_attr_str(&mut root, nested, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, nested_child, "20px", "10px");
    assert_set_attr_str(&mut root, nested_child, "overflow", "scroll");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_layout(&root, nested).width_px,
        88.0,
        "padded nested flex item outer width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, end),
        104.0,
        "end item after padded nested flex item",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, nested_child),
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
// - Blink `LineFlexer::FreezeViolations`:
//   https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/flex/line_flexer.cc
// - Gecko `FlexLine::ResolveFlexibleLengths`:
//   https://searchfox.org/firefox-main/source/layout/generic/nsFlexContainerFrame.cpp
fn padded_flex_item_floors_content_box_at_zero_after_shrink() {
    let (mut world, mut root, outer) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, outer);

    assert_set_flex_row_container_width(&mut root, outer, "4px");
    assert_set_attr_str(&mut root, outer, "height", "10px");
    assert_set_attr_str(&mut root, item, "height", "10px");
    assert_set_attr_str(&mut root, item, "flex-basis", "0px");
    assert_set_attr_str(&mut root, item, "flex-grow", "0");
    assert_set_attr_str(&mut root, item, "flex-shrink", "1");
    assert_set_attr_str(&mut root, item, "padding-left", "4px");
    assert_set_attr_str(&mut root, item, "padding-right", "4px");
    assert_set_attr_str(&mut root, item, "overflow", "scroll");

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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, middle, "display", "flex");
    assert_set_attr_str(&mut root, middle, "height", "20px");
    assert_set_attr_str(&mut root, middle, "flex-basis", "20px");
    assert_set_attr_str(&mut root, middle, "flex-grow", "1");
    assert_set_attr_str(&mut root, middle, "flex-shrink", "1");
    assert_set_attr_str(&mut root, middle, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_attr_str(&mut root, sibling, "overflow", "scroll");

    assert_set_attr_str(&mut root, inner, "display", "flex");
    assert_set_attr_str(&mut root, inner, "justify-content", "center");
    assert_set_attr_str(&mut root, inner, "height", "20px");
    assert_set_attr_str(&mut root, inner, "flex-basis", "20px");
    assert_set_attr_str(&mut root, inner, "flex-grow", "1");
    assert_set_attr_str(&mut root, inner, "flex-shrink", "1");
    assert_set_attr_str(&mut root, inner, "overflow", "scroll");
    assert_set_inflexible_row_item(&mut root, leaf, "10px", "10px");
    assert_set_attr_str(&mut root, leaf, "overflow", "scroll");

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
        assert_node_border_box_screen_left_px(&root, leaf),
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
    assert_set_attr_str(&mut root, outer, "height", "20px");
    assert_set_attr_str(&mut root, nested, "display", "flex");
    assert_set_attr_str(&mut root, nested, "justify-content", "center");
    assert_set_attr_str(&mut root, nested, "height", "20px");
    assert_set_attr_str(&mut root, nested, "flex-basis", "20px");
    assert_set_attr_str(&mut root, nested, "flex-grow", "1");
    assert_set_attr_str(&mut root, nested, "flex-shrink", "1");
    assert_set_inflexible_row_item(&mut root, sibling, "20px", "20px");
    assert_set_inflexible_row_item(&mut root, first_nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        assert_node_layout(&root, nested).width_px,
        80.0,
        "warm nested flex width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, first_nested_child),
        35.0,
        "warm single nested child centered",
    );

    let second_nested_child = assert_append_div_to(&mut root, nested);
    assert_set_inflexible_row_item(&mut root, second_nested_child, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 16.0, DomPointerUpdate::Unchanged);
    let first_append_pass = nested_flex_mutation_probe(&root, nested, first_nested_child, sibling);
    assert_close(
        first_append_pass.nested_width_px,
        80.0,
        "first append pass nested width",
    );
    assert_close(
        first_append_pass.first_child_left_px,
        30.0,
        "first append pass first child left",
    );
    assert_close(
        first_append_pass.sibling_left_px,
        80.0,
        "first append pass sibling left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second_nested_child),
        40.0,
        "first append pass second child left",
    );

    assert_root_dom_update(&mut world, &mut root, 32.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        first_append_pass,
        nested_flex_mutation_probe(&root, nested, first_nested_child, sibling),
        "append should not need a second layout pass to reach stable flex geometry",
    );

    root.remove_node(second_nested_child)
        .expect("dynamic flex proof should remove appended child");

    assert_root_dom_update(&mut world, &mut root, 48.0, DomPointerUpdate::Unchanged);
    let first_remove_pass = nested_flex_mutation_probe(&root, nested, first_nested_child, sibling);
    assert_close(
        first_remove_pass.nested_width_px,
        80.0,
        "first remove pass nested width",
    );
    assert_close(
        first_remove_pass.first_child_left_px,
        35.0,
        "first remove pass first child left",
    );
    assert_close(
        first_remove_pass.sibling_left_px,
        80.0,
        "first remove pass sibling left",
    );

    assert_root_dom_update(&mut world, &mut root, 64.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        first_remove_pass,
        nested_flex_mutation_probe(&root, nested, first_nested_child, sibling),
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "column-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        next_sibling_key(&root, first),
        Some(second),
        "column-gap should not rewrite DOM sibling links",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first same-line gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first same-line gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second same-line gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "second same-line gap item left",
    );
}

#[test]
// CSS Align 3 gap properties and WPT flexbox-column-row-gap-001.html:
// row-gap inserts fixed space between wrapped flex lines. When a third 10px
// item wraps below the first line in a 25px-wide container, a 5px row gap puts
// the second line at a top offset of 15px instead of 10px.
fn row_gap_separates_wrapped_flex_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "row-gap", "5px");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first wrapped-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first wrapped-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "second wrapped-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second wrapped-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third wrapped-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        15.0,
        "third wrapped-gap item top",
    );
}

#[test]
// CSS Align 3 gap shorthand and WPT flexbox-column-row-gap-001.html: the gap
// shorthand sets both row-gap and column-gap for flex containers. In this
// wrapped row case, `gap: 5px` produces the same geometry as `column-gap: 5px`
// plus `row-gap: 5px`.
fn gap_shorthand_applies_row_and_column_spacing_to_wrapped_flex_lines() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "25px");
    assert_set_attr_str(&mut root, container, "height", "40px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "gap", "5px");
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first shorthand-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first shorthand-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "second shorthand-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second shorthand-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third shorthand-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        15.0,
        "third shorthand-gap item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "25px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, fourth, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first phantom-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "first phantom-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "second phantom-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "second phantom-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        0.0,
        "third phantom-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        15.0,
        "third phantom-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, fourth),
        15.0,
        "fourth phantom-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, fourth),
        15.0,
        "fourth phantom-gap item top",
    );
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
    assert_set_attr_str(&mut root, container, "height", "31px");
    assert_set_attr_str(&mut root, container, "padding-left", "4px");
    assert_set_attr_str(&mut root, container, "padding-right", "4px");
    assert_set_attr_str(&mut root, container, "padding-top", "3px");
    assert_set_attr_str(&mut root, container, "padding-bottom", "3px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap-reverse");
    assert_set_attr_str(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, fourth, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        4.0,
        "first padded wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        24.0,
        "first padded wrap-reverse item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        19.0,
        "second padded wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        24.0,
        "second padded wrap-reverse item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        4.0,
        "third padded wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        9.0,
        "third padded wrap-reverse item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, fourth),
        19.0,
        "fourth padded wrap-reverse item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, fourth),
        9.0,
        "fourth padded wrap-reverse item top",
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
    assert_set_attr_str(&mut root, container, "height", "31px");
    assert_set_attr_str(&mut root, container, "padding-left", "4px");
    assert_set_attr_str(&mut root, container, "padding-right", "4px");
    assert_set_attr_str(&mut root, container, "padding-top", "3px");
    assert_set_attr_str(&mut root, container, "padding-bottom", "3px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        4.0,
        "first padded-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        3.0,
        "first padded-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        19.0,
        "second padded-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        3.0,
        "second padded-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        4.0,
        "third padded-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        18.0,
        "third padded-gap item top",
    );
}

#[test]
// CSS Align 3 gap properties and CSS Box 3 padding edges: row-gap is measured
// between wrapped flex lines inside the content box. Container padding shifts
// both wrapped rows while preserving the authored line gap.
fn row_gap_uses_the_padded_content_box_for_wrapped_line_offsets() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let first = assert_append_div_to(&mut root, container);
    let second = assert_append_div_to(&mut root, container);
    let third = assert_append_div_to(&mut root, container);

    assert_set_flex_row_container_width(&mut root, container, "28px");
    assert_set_attr_str(&mut root, container, "height", "31px");
    assert_set_attr_str(&mut root, container, "padding-left", "4px");
    assert_set_attr_str(&mut root, container, "padding-right", "4px");
    assert_set_attr_str(&mut root, container, "padding-top", "3px");
    assert_set_attr_str(&mut root, container, "padding-bottom", "3px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    assert_set_attr_str(&mut root, container, "row-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_inflexible_row_item(&mut root, third, "10px", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        3.0,
        "first padded row-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        3.0,
        "second padded row-gap item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        18.0,
        "third padded row-gap item top",
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "column-gap", "5px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_attr_str(&mut root, first, "padding-left", "4px");
    assert_set_attr_str(&mut root, first, "padding-right", "4px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "padding-left", "4px");
    assert_set_attr_str(&mut root, second, "padding-right", "4px");

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
    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "first padded-gap item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        23.0,
        "second padded-gap item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "10px");
    assert_set_attr_str(&mut root, container, "column-gap", "7px");
    assert_set_inflexible_row_item(&mut root, first, "10px", "10px");
    assert_set_attr_str(&mut root, first, "margin-right", "5px");
    assert_set_inflexible_row_item(&mut root, second, "10px", "10px");
    assert_set_attr_str(&mut root, second, "margin-left", "3px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "margin+gap first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        25.0,
        "margin+gap second item left",
    );
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
    assert_set_attr_str(&mut root, container, "height", "20px");
    assert_set_attr_str(&mut root, container, "flex-wrap", "wrap");
    for child in [first, second] {
        assert_set_attr_str(&mut root, child, "flex-basis", "auto");
        assert_set_attr_str(&mut root, child, "flex-grow", "0");
        assert_set_attr_str(&mut root, child, "flex-shrink", "1");
        assert_set_attr_str(&mut root, child, "width", "40px");
        assert_set_attr_str(&mut root, child, "height", "10px");
    }

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "wrapped auto-basis first item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "wrapped auto-basis first item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        0.0,
        "wrapped auto-basis second item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "wrapped auto-basis second item top",
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
        assert_set_attr_str(&mut root, child, "flex-basis", "auto");
        assert_set_attr_str(&mut root, child, "flex-grow", "0");
        assert_set_attr_str(&mut root, child, "flex-shrink", "1");
        assert_set_attr_str(&mut root, child, "width", "20px");
        assert_set_attr_str(&mut root, child, "height", height);
    }

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "column auto-basis first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "column auto-basis second item top",
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
    assert_set_attr_str(&mut root, item, "overflow", "scroll");
    assert_set_attr_str(&mut root, item, "height", "10px");
    assert_set_attr_str(&mut root, item, "flex-grow", "0");
    assert_set_attr_str(&mut root, item, "flex-shrink", "1");
    assert_set_attr_str(&mut root, item, "flex-basis", "auto");

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
        assert_node_border_box_screen_top_px(&root, item),
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
    assert_set_attr_str(&mut root, item, "overflow", "visible");
    assert_set_attr_str(&mut root, item, "height", "10px");
    assert_set_attr_str(&mut root, item, "flex-grow", "0");
    assert_set_attr_str(&mut root, item, "flex-shrink", "1");
    assert_set_attr_str(&mut root, item, "flex-basis", "auto");

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
    assert_close(
        assert_node_border_box_screen_top_px(&root, item),
        0.0,
        "visible-overflow flex item top",
    );
}

#[test]
// CSS Flexbox automatic minimum size and the Gecko/Blink block-axis content
// size suggestion path: in a column flex container, a visible-overflow item
// keeps its measured intrinsic block size as the automatic minimum main size.
// A 30px-tall item with flex-basis: 30px inside a 20px-tall column therefore
// stays 30px tall instead of shrinking to the container's free space.
fn visible_overflow_column_flex_item_uses_intrinsic_block_size_before_shrink() {
    let (mut world, mut root, container) = flex_row_container_setup();
    let item = assert_append_div_to(&mut root, container);
    let nested = assert_append_div_to(&mut root, item);

    assert_set_flex_column_container_size(&mut root, container, "20px", "20px");
    assert_set_attr_str(&mut root, item, "overflow", "visible");
    assert_set_flex_column_basis_and_cross_size(&mut root, item, "30px", "20px");
    assert_set_attr_str(&mut root, item, "flex-grow", "0");
    assert_set_attr_str(&mut root, item, "flex-shrink", "1");
    assert_set_attr_str(&mut root, nested, "width", "20px");
    assert_set_attr_str(&mut root, nested, "height", "30px");

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
    assert_close(
        assert_node_layout(&root, item).height_px,
        30.0,
        "column visible-overflow flex item height",
    );
    assert_close(
        assert_node_layout(&root, item).width_px,
        20.0,
        "column visible-overflow flex item width",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, item),
        0.0,
        "column visible-overflow flex item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, item),
        0.0,
        "column visible-overflow flex item top",
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
    assert_set_attr_str(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_inflexible_row_item(
        &mut root,
        start,
        TITLEBAR_ICON_BASIS_PX,
        TITLEBAR_ICON_HEIGHT_PX,
    );
    assert_set_attr_str(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_attr_str(&mut root, title_host, "flex-basis", "0px");
    assert_set_attr_str(&mut root, title_host, "flex-grow", "1");
    assert_set_attr_str(&mut root, title_host, "flex-shrink", "1");
    assert_set_attr_str(&mut root, title_host, "overflow", "hidden");
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
    let first_append_pass = titlebar_mutation_probe(&root, container, title_host, end);
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
    assert_close(
        assert_node_border_box_screen_left_px(&root, inserted_end),
        104.0,
        "first append pass inserted end left",
    );

    assert_root_dom_update(&mut world, &mut root, 32.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        first_append_pass,
        titlebar_mutation_probe(&root, container, title_host, end),
        "titlebar append should not shift on the second layout pass",
    );

    root.remove_node(inserted_end)
        .expect("dynamic titlebar proof should remove appended icon");

    assert_root_dom_update(&mut world, &mut root, 48.0, DomPointerUpdate::Unchanged);
    let first_remove_pass = titlebar_mutation_probe(&root, container, title_host, end);
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
    assert_eq!(
        first_remove_pass,
        titlebar_mutation_probe(&root, container, title_host, end),
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

    assert_set_attr_str(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        0.0,
        "stylesheet justify-content first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        40.0,
        "stylesheet justify-content second item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        80.0,
        "stylesheet justify-content third item left",
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

    let before_second_left = assert_node_border_box_screen_left_px(&root, second);
    assert_close(
        before_second_left,
        40.0,
        "space-between second item left before class switch",
    );

    assert_set_attr_str(&mut root, container, "class", "flex-end-row");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        60.0,
        "first item left after switching to flex-end class",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        70.0,
        "second item left after switching to flex-end class",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        80.0,
        "third item left after switching to flex-end class",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        40.0,
        "stylesheet rtl row-reverse logical-start first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        50.0,
        "stylesheet rtl row-reverse logical-start second item left",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        10.0,
        "stylesheet column-reverse first item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        30.0,
        "stylesheet column-reverse first item top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        10.0,
        "stylesheet column-reverse second item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "stylesheet column-reverse second item top",
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

    assert_set_attr_str(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        30.0,
        "stylesheet wrap-reverse first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        30.0,
        "stylesheet wrap-reverse second item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "stylesheet wrap-reverse third item top",
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

    assert_set_attr_str(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        0.0,
        "stylesheet wrap-reverse logical-start third item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        10.0,
        "stylesheet wrap-reverse logical-start first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        10.0,
        "stylesheet wrap-reverse logical-start second item top",
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
    assert_set_attr_str(&mut root, first, "flex-basis", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "0");
    assert_set_attr_str(&mut root, first, "height", "10px");
    assert_set_attr_str(&mut root, second, "flex-basis", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "0");
    assert_set_attr_str(&mut root, second, "flex-shrink", "0");
    assert_set_attr_str(&mut root, second, "class", "self-stretch");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
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
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "stylesheet align-self stretch item top",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "stylesheet align-self center sibling top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        20.0,
        "stylesheet align-self center item top",
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
    assert_set_attr_str(&mut root, first, "flex-basis", "10px");
    assert_set_attr_str(&mut root, first, "flex-grow", "0");
    assert_set_attr_str(&mut root, first, "flex-shrink", "0");
    assert_set_attr_str(&mut root, second, "flex-basis", "10px");
    assert_set_attr_str(&mut root, second, "flex-grow", "0");
    assert_set_attr_str(&mut root, second, "flex-shrink", "0");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "stylesheet align-self flex-end sibling top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        40.0,
        "stylesheet align-self flex-end item top",
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

    assert_set_attr_str(&mut root, tall, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, tall),
        30.0,
        "stylesheet wrap-reverse logical-start sibling top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, short),
        40.0,
        "stylesheet wrap-reverse logical-end self override top",
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

    assert_set_attr_str(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        15.0,
        "stylesheet gap shorthand second item left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        15.0,
        "stylesheet gap shorthand third item top",
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

    assert_set_attr_str(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        15.0,
        "stylesheet justify-content space-evenly first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        40.0,
        "stylesheet justify-content space-evenly second item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        65.0,
        "stylesheet justify-content space-evenly third item left",
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

    assert_set_attr_str(&mut root, first, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        60.0,
        "stylesheet justify-content flex-end first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        70.0,
        "stylesheet justify-content flex-end second item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        80.0,
        "stylesheet justify-content flex-end third item left",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_left_px(&root, first),
        10.0,
        "stylesheet justify-content space-around first item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second),
        40.0,
        "stylesheet justify-content space-around second item left",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, third),
        70.0,
        "stylesheet justify-content space-around third item left",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        5.0,
        "stylesheet align-content space-around first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        5.0,
        "stylesheet align-content space-around second item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        25.0,
        "stylesheet align-content space-around third item top",
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

    assert_set_attr_str(&mut root, second, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "stylesheet align-content space-between first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "stylesheet align-content space-between second item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        30.0,
        "stylesheet align-content space-between third item top",
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

    assert_set_attr_str(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        6.666_666_5,
        "stylesheet align-content space-evenly first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        6.666_666_5,
        "stylesheet align-content space-evenly second item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        23.333_334,
        "stylesheet align-content space-evenly third item top",
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

    assert_set_attr_str(&mut root, third, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    assert_close(
        assert_node_border_box_screen_top_px(&root, first),
        0.0,
        "stylesheet align-content stretch first item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second),
        0.0,
        "stylesheet align-content stretch second item top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, third),
        20.0,
        "stylesheet align-content stretch third item top",
    );
}

fn flex_row_container_setup() -> (bevy::prelude::World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    (world, root, container)
}

fn assert_set_flex_row_container_width(root: &mut RootDom, container: DomNodeSlotKey, width: &str) {
    assert_set_attr_str(root, container, "display", "flex");
    assert_set_attr_str(root, container, "flex-direction", "row");
    assert_set_attr_str(root, container, "flex-wrap", "nowrap");
    assert_set_attr_str(root, container, "width", width);
}

fn assert_set_flex_column_container_size(
    root: &mut RootDom,
    container: DomNodeSlotKey,
    width: &str,
    height: &str,
) {
    assert_set_attr_str(root, container, "display", "flex");
    assert_set_attr_str(root, container, "flex-direction", "column");
    assert_set_attr_str(root, container, "flex-wrap", "nowrap");
    assert_set_attr_str(root, container, "width", width);
    assert_set_attr_str(root, container, "height", height);
}

fn assert_set_flex_basis_and_cross_size(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    height: &str,
) {
    assert_set_attr_str(root, item, "height", height);
    assert_set_attr_str(root, item, "flex-basis", basis);
}

fn assert_set_inflexible_row_item(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    height: &str,
) {
    assert_set_flex_basis_and_cross_size(root, item, basis, height);
    assert_set_attr_str(root, item, "flex-grow", "0");
    assert_set_attr_str(root, item, "flex-shrink", "0");
}

fn auto_width_titlebar_flex_mutation_setup() -> AutoWidthTitlebarFlexMutationScenario {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let container = assert_append_div_to(&mut root, parent);
    let _start = assert_append_inflexible_titlebar_item_to(&mut root, container);
    let title_host = assert_append_div_to(&mut root, container);
    let end = assert_append_inflexible_titlebar_item_to(&mut root, container);

    assert_set_attr_str(&mut root, parent, "width", "120px");
    assert_set_attr_str(&mut root, parent, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_attr_str(&mut root, container, "display", "flex");
    assert_set_attr_str(&mut root, container, "flex-direction", "row");
    assert_set_attr_str(&mut root, container, "flex-wrap", "nowrap");
    assert_set_attr_str(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_attr_str(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_attr_str(&mut root, title_host, "flex-basis", "0px");
    assert_set_attr_str(&mut root, title_host, "flex-grow", "1");
    assert_set_attr_str(&mut root, title_host, "flex-shrink", "1");
    assert_set_attr_str(&mut root, title_host, "overflow", "hidden");
    let _title_text = assert_append_text_to(&mut root, title_host, "Camera");

    AutoWidthTitlebarFlexMutationScenario {
        world,
        root,
        container,
        title_host,
        end,
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
    assert_set_attr_str(root, item, "width", width);
    assert_set_attr_str(root, item, "flex-basis", basis);
}

fn assert_set_inflexible_column_item(
    root: &mut RootDom,
    item: DomNodeSlotKey,
    basis: &str,
    width: &str,
) {
    assert_set_flex_column_basis_and_cross_size(root, item, basis, width);
    assert_set_attr_str(root, item, "flex-grow", "0");
    assert_set_attr_str(root, item, "flex-shrink", "0");
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

struct AutoWidthTitlebarFlexMutationScenario {
    world: bevy::prelude::World,
    root: RootDom,
    container: DomNodeSlotKey,
    title_host: DomNodeSlotKey,
    end: DomNodeSlotKey,
}

#[allow(clippy::struct_field_names)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct NestedFlexMutationProbe {
    nested_width_px: f32,
    first_child_left_px: f32,
    sibling_left_px: f32,
}

fn nested_flex_mutation_probe(
    root: &RootDom,
    nested: DomNodeSlotKey,
    first_nested_child: DomNodeSlotKey,
    sibling: DomNodeSlotKey,
) -> NestedFlexMutationProbe {
    NestedFlexMutationProbe {
        nested_width_px: assert_node_layout(root, nested).width_px,
        first_child_left_px: assert_node_border_box_screen_left_px(root, first_nested_child),
        sibling_left_px: assert_node_border_box_screen_left_px(root, sibling),
    }
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
    end: DomNodeSlotKey,
) -> TitlebarMutationProbe {
    TitlebarMutationProbe {
        container_width_px: assert_node_layout(root, container).width_px,
        title_width_px: assert_node_layout(root, title_host).width_px,
        end_left_px: assert_node_border_box_screen_left_px(root, end),
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
    assert_set_attr_str(&mut root, container, "height", TITLEBAR_HOST_HEIGHT_PX);

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
    assert_set_attr_str(&mut root, title_host, "height", TITLEBAR_HOST_HEIGHT_PX);
    assert_set_attr_str(&mut root, title_host, "flex-basis", "0px");
    assert_set_attr_str(&mut root, title_host, "flex-grow", "1");
    assert_set_attr_str(&mut root, title_host, "flex-shrink", "1");
    assert_set_attr_str(&mut root, title_host, "overflow", "hidden");
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
    let title_left_px = assert_node_border_box_screen_left_px(&root, title_host);
    TitlebarFlexReport {
        case_name: case.name,
        container_width_px: assert_node_layout(&root, container).width_px,
        item_widths_px: titlebar_item_widths(&root, &start_icons, title_host, &end_icons),
        start_icon_widths_px: measured_item_widths(&root, &start_icons),
        start_icon_lefts_px: measured_item_lefts(&root, &start_icons),
        title_width_px,
        title_left_px,
        title_right_px: title_left_px + title_width_px,
        title_top_px: assert_node_border_box_screen_top_px(&root, title_host),
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
        .map(|key| assert_node_border_box_screen_left_px(root, *key))
        .collect()
}

fn measured_item_tops(root: &RootDom, keys: &[DomNodeSlotKey]) -> Vec<f32> {
    keys.iter()
        .map(|key| assert_node_border_box_screen_top_px(root, *key))
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
