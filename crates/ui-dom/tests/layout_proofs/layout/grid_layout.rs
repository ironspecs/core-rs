#![allow(rustdoc::bare_urls)]

//! Grid layout proofs for the T1 card-wall solver (`GRID_STEP2_DESIGN.md` D8).
//!
//! Spec references:
//! - CSS Grid Layout Module Level 1 §5 (auto-placement), §7 (sizing), §8.5
//!   (auto-placement algorithm), §12 (track sizing):
//!   <https://drafts.csswg.org/css-grid-1/>
//! - CSS Box Alignment Module Level 3 §8.1 (auto margins):
//!   <https://drafts.csswg.org/css-align-3/#auto-margins>
//!
//! Browser implementation references:
//! - Blink `grid_track_sizing_algorithm.cc` (track sizing phases)
//! - Gecko `nsGridContainerFrame.cpp` `CalculateSizes` / `FindUsedFlexFraction`
//!
//! These proofs pin the D8 family: the card wall (fr wall, fixed+fr mix,
//! minmax clamps, min/max-content, the grow-sum<1 cap, overflow-uses-bases),
//! gaps, auto-rows, order re-ordering, stretch-to-cell with the definite-size
//! and replaced-ratio exemptions, auto-margin absorption, the abs-pos model,
//! the nested-grid intrinsic contribution, inline-grid shrink-wrap, and the
//! D5-interim's on-spec move.

use bevy::prelude::World;
use ui_dom::{
    DomNode, DomNodeSlotKey, DomPointerUpdate, DomRect, RootDom,
    domains::layout::MeasurementRecord, node_box,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_img_to, assert_append_text_to, assert_close,
    assert_node_layout, assert_rect_op_geometry, assert_root_dom_update, assert_set_attr_str,
    assert_style_set, attached_gpu_root_dom_default_viewport_setup, take_staged_layer_paint_plans,
};

/// Returns `(world, root, container)`: a fresh root with one div container.
fn grid_container_setup() -> (World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    (world, root, container)
}

fn assert_grid_container(
    root: &mut RootDom,
    container: DomNodeSlotKey,
    columns: &str,
    width: &str,
) {
    assert_style_set(root, container, "display", "grid");
    assert_style_set(root, container, "grid-template-columns", columns);
    assert_style_set(root, container, "width", width);
}

fn border_box_of(root: &RootDom, key: DomNodeSlotKey) -> DomRect {
    node_box(root, key)
        .expect("grid proof node should exist")
        .border_box
}

fn element_measurement_record(root: &RootDom, key: DomNodeSlotKey) -> MeasurementRecord {
    match root.node(key).expect("grid proof node should exist") {
        DomNode::Element(node) => node.base.measurement,
        _ => panic!("grid proof node should be an element"),
    }
}

#[test]
// CSS Grid §12.7 + WPT `grid-template-columns-1.html`: `repeat(3, 1fr)` in a
// 300px container divides the width into three equal 100px tracks; six items
// fill row-major into two rows.
fn grid_card_wall_three_fr_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(3, 1fr)", "300px");
    for _ in 0..6 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_eq!(rects.len(), 7, "container + 6 items");
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 300.0, 80.0, "container");
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "item 0");
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 100.0, 40.0, "item 1");
    assert_rect_op_geometry(&rects[3], 200.0, 0.0, 100.0, 40.0, "item 2");
    assert_rect_op_geometry(&rects[4], 0.0, 40.0, 100.0, 40.0, "item 3");
    assert_rect_op_geometry(&rects[5], 100.0, 40.0, 100.0, 40.0, "item 4");
    assert_rect_op_geometry(&rects[6], 200.0, 40.0, 100.0, 40.0, "item 5");
}

#[test]
// CSS Grid §12.7: fr distribution happens after the fixed base, in factor
// ratio — `100px 1fr 2fr` in 300px → 100 / 66.67 / 133.33.
fn grid_fixed_and_fr_track_mix() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "100px 1fr 2fr", "300px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "fixed track item");
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 200.0 / 3.0, 40.0, "1fr item");
    assert_rect_op_geometry(
        &rects[3],
        100.0 + 200.0 / 3.0,
        0.0,
        400.0 / 3.0,
        40.0,
        "2fr item",
    );
}

#[test]
// CSS Grid §12.6: `minmax(100px, 200px) auto` grows the minmax track to its
// limit (200) and the auto track absorbs the rest.
fn grid_minmax_track_clamps() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "minmax(100px, 200px) auto", "300px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 40.0, "minmax track item");
    assert_rect_op_geometry(&rects[2], 200.0, 0.0, 100.0, 40.0, "auto track item");
}

#[test]
// CSS Grid §12.7 + correction 1: `0.25fr 0.25fr` (Σ=0.5 < 1) scales the free
// space by the sum — each track resolves 100px in a 400px container (total 200,
// NOT 400 — the cap's observable).
fn grid_fr_sum_below_one_scales_free_space() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "0.25fr 0.25fr", "400px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "first 0.25fr item");
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 100.0, 40.0, "second 0.25fr item");
}

#[test]
// CSS Grid §12.6 zero-size edge: fixed bases exceeding the container overflow
// at their bases, and the fr track gets its content base.
fn grid_track_overflow_uses_bases() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px 200px 1fr", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "height", "40px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "height", "40px");
    let fr_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, fr_item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    // Two fixed rects; the fr item paints nothing (zero width).
    assert_eq!(
        rects.len(),
        3,
        "container + 2 fixed items (fr item is 0-width)"
    );
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 40.0, "first fixed item");
    assert_rect_op_geometry(&rects[2], 200.0, 0.0, 200.0, 40.0, "second fixed item");
    // The fr item sits at x=400 with its content base (0 width).
    let fr_box = border_box_of(&root, fr_item);
    assert_close(
        fr_box.left_px,
        400.0,
        "fr item at the overflowing base offset",
    );
    assert_close(fr_box.width_px, 0.0, "fr item gets its content base");
}

#[test]
// CSS Grid §8.2: column-gap separates tracks and row-gap separates rows.
fn grid_gap_separates_tracks_and_rows() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(3, 1fr)", "300px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    assert_style_set(&mut root, container, "row-gap", "10px");
    for _ in 0..6 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Column tracks: (300 − 2×10) / 3 = 93.33 each. Row 1 top = 40 + 10 = 50.
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 280.0 / 3.0, 40.0, "item 0");
    assert_rect_op_geometry(
        &rects[2],
        280.0 / 3.0 + 10.0,
        0.0,
        280.0 / 3.0,
        40.0,
        "item 1",
    );
    assert_rect_op_geometry(&rects[4], 0.0, 50.0, 280.0 / 3.0, 40.0, "item 3 (row 1)");
}

#[test]
// CSS Grid §8.5 + §5.4: the `order` property applies to auto-placement — an
// `order: 1` card places last in the row-major fill.
fn grid_order_property_reorders_auto_placement() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(3, 1fr)", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "order", "1");
    assert_style_set(&mut root, first, "height", "40px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The first DOM child (order: 1) places last, at the third column.
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(
        &rects[1],
        200.0,
        0.0,
        100.0,
        40.0,
        "order:1 item places last",
    );
}

#[test]
// CSS Grid §6.1/§7.1: `normal` stretches items with auto size in the axis; a
// definite size is used as-is and a replaced-ratio img derives its cross from
// its ratio instead of stretching.
fn grid_item_stretches_to_its_cell_by_default() {
    // Stretch + definite exemptions (fixed tracks so the widths are exact).
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "100px 100px", "200px");
    let tall = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, tall, "height", "40px");
    let definite = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, definite, "width", "50px");
    assert_style_set(&mut root, definite, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    // The tall item stretches to its 100px track (width auto → stretch).
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "stretched item");
    // The definite item keeps its 50×20 size, start-aligned in its cell.
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 50.0, 20.0, "definite item");

    // Replaced-ratio img: width stretches to the track, height follows ratio.
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "100px", "100px");
    let img = assert_append_img_to(&mut root, container);
    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The 16:9 placeholder: width 100, height 100 × 9/16 = 56.25 (not the
    // row height, which is the intrinsic 90).
    let img_layout = assert_node_layout(&root, img);
    assert_close(img_layout.width_px, 100.0, "img stretched width");
    assert_close(img_layout.height_px, 56.25, "img ratio-derived height");
}

#[test]
// CSS Box Alignment §8.1: `margin: auto` on a card absorbs the cell's free
// space and centers it in both axes.
fn grid_item_auto_margins_absorb_cell_free_space() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px 200px", "400px");
    let card = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, card, "width", "50px");
    assert_style_set(&mut root, card, "height", "20px");
    assert_style_set(&mut root, card, "margin", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The 50×20 card centers in its 200×20 cell: (200−50)/2 = 75 left/top.
    let box_ = border_box_of(&root, card);
    assert_close(box_.left_px, 75.0, "card centered horizontally");
    assert_close(
        box_.top_px,
        0.0,
        "card centered vertically (row = card height)",
    );
    assert_close(box_.width_px, 50.0, "card width (not stretched)");
}

#[test]
// CSS Grid §8.4: an abs-pos child's static position uses the padding box (auto
// lines fall back to the padding-box edges).
fn grid_abspos_child_static_position_uses_padding_box() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(2, 1fr)", "200px");
    assert_style_set(&mut root, container, "padding", "10px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The abs child sits at the container's content origin (padding-box top-
    // left + padding) = (10, 10).
    let abs_box = border_box_of(&root, abs);
    assert_close(abs_box.left_px, 10.0, "abs child static left");
    assert_close(abs_box.top_px, 10.0, "abs child static top");
}

#[test]
// CSS Grid §6.6 / D7: a grid container with an empty template has one auto
// column, so cards stack.
fn grid_empty_template_is_one_auto_column() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "width", "300px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    // One auto column fills the 300px width; cards stack at rows 0 and 40.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 300.0, 40.0, "first card");
    assert_rect_op_geometry(&rects[2], 0.0, 40.0, 300.0, 40.0, "second card");
}

#[test]
// CSS Grid §6.6 / D7: `inline-grid` sizes to its tracks' total width (shrink-
// wrap), not the containing block.
fn grid_inline_grid_shrink_wraps() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "inline-grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.width_px,
        200.0,
        "inline-grid shrink-wraps to tracks",
    );
}

#[test]
// CSS Grid §8.5: `grid-auto-rows` sizes implicit rows to content, and a
// specified `minmax(80px, auto)` floors them at 80px.
fn grid_auto_rows_size_to_content_and_specified_track_size() {
    // Content-sized (auto) rows.
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(2, 1fr)", "200px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "25px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.content_height_px,
        25.0,
        "content-sized row",
    );

    // minmax(80px, auto) rows floor at 80px.
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(2, 1fr)", "200px");
    assert_style_set(&mut root, container, "grid-auto-rows", "minmax(80px, auto)");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "25px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.content_height_px,
        80.0,
        "auto-rows floor at 80px",
    );
}

#[test]
// CSS Grid §4 + CSS 2.1 §8.3.1: the T0 formatting-context flag holds under the
// real solver — a grid item's first-child margin is contained, not collapsed.
fn grid_item_margins_contained_at_the_boundary() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(2, 1fr)", "200px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "100px");
    let block_child = assert_append_div_to(&mut root, item);
    let grandchild = assert_append_div_to(&mut root, block_child);
    assert_style_set(&mut root, grandchild, "margin-top", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grandchild_box = border_box_of(&root, grandchild);
    let item_box = border_box_of(&root, item);
    let item_content_top = item_box.top_px + 0.0 + 0.0; // no border/padding
    assert_close(
        grandchild_box.top_px,
        item_content_top + 20.0,
        "grandchild sits 20px below the item content top (contained)",
    );
}

#[test]
// CSS Grid §6.6 / D7: a nested grid's intrinsic sizes derive from its tracks —
// both as a block child and as a flex item.
fn grid_container_intrinsic_sizes_derive_from_tracks() {
    // As a block child: the grid fills the containing block (block-outer), and
    // its record carries the D7 track-derived intrinsic (two 100px tracks).
    let (mut world, mut root, container) = grid_container_setup();
    let nested = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, nested, "display", "grid");
    assert_style_set(&mut root, nested, "grid-template-columns", "100px 100px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, nested);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let record = element_measurement_record(&root, nested);
    assert_close(
        record.h_min_content_px,
        200.0,
        "nested grid h_min from tracks",
    );
    assert_close(
        record.h_max_content_px,
        200.0,
        "nested grid h_max from tracks",
    );

    // As a flex item: the flex solver reads the grid's record intrinsic sizes,
    // so the item resolves to its 200px max-content.
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "flex");
    assert_style_set(&mut root, container, "width", "400px");
    let nested = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, nested, "display", "grid");
    assert_style_set(&mut root, nested, "grid-template-columns", "100px 100px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, nested);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let nested_layout = assert_node_layout(&root, nested);
    assert_close(
        nested_layout.width_px,
        200.0,
        "nested grid as flex item width from tracks",
    );
}

#[test]
// CSS Grid §4/§5: the D5 interim scenario now produces the card wall — a grid
// container with `grid-template-columns` solves as a grid (the interim row
// moved on-spec).
fn grid_t0_block_interim_is_replaced() {
    let (mut world, mut root, target) = grid_container_setup();
    assert_style_set(&mut root, target, "display", "grid");
    assert_style_set(&mut root, target, "grid-template-columns", "100px 1fr");
    assert_style_set(&mut root, target, "width", "300px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, target);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Two columns side by side (100px + 200px fr), not stacked block flow.
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "first column item");
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 200.0, 40.0, "second column item");
}

#[test]
// CSS Grid §12: `min-content max-content 20%` sizes tracks from item
// contributions (records) and the percent track against the container width.
fn grid_min_max_content_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "min-content max-content 20%", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_append_text_to(&mut root, first, "aa bb");
    let second = assert_append_div_to(&mut root, container);
    assert_append_text_to(&mut root, second, "cccc");
    let third = assert_append_div_to(&mut root, container);
    assert_append_text_to(&mut root, third, "dddd");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The percent track is 20% of 300 = 60px; the min-content/max-content
    // tracks are content-derived and ordered min ≤ max. The third item sits at
    // the percent track's left edge = min-content + max-content widths.
    let third_box = border_box_of(&root, third);
    let first_box = border_box_of(&root, first);
    let second_box = border_box_of(&root, second);
    let min_width = first_box.width_px;
    let max_width = second_box.width_px;
    assert_close(third_box.width_px, 60.0, "percent track width");
    assert_close(
        third_box.left_px,
        min_width + max_width,
        "percent track left edge = min-content + max-content",
    );
    // The min-content track is no wider than the max-content track.
    assert!(min_width <= max_width, "min-content ≤ max-content");
    assert!(min_width > 0.0, "min-content track from item content");
}

#[test]
// CSS Grid §7.2.4 (Gecko `FindUsedFlexFraction`, Blink `FindFrSize`): a
// flexible track whose content base exceeds its fr share is frozen at its base
// and the remaining space redistributes. `1fr 1fr` in a 400px container with a
// 300px-wide item in column 0 → 300 / 100 (the old closed form left 50px
// unfilled at 300/50).
fn grid_fr_tracks_freeze_over_base_and_redistribute() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "1fr 1fr", "400px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "width", "300px");
    assert_style_set(&mut root, wide, "height", "40px");
    let narrow = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, narrow, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Column 0 freezes at its 300px base; column 1 absorbs the full 100px
    // remainder (the container fills).
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 300.0, 40.0, "frozen 300px item");
    assert_rect_op_geometry(&rects[2], 300.0, 0.0, 100.0, 40.0, "item fills the rest");
}

#[test]
// CSS Grid §7.2.4 restart recomputes the used flex fraction over the REMAINING
// tracks, so the sum<1 cap re-applies per iteration. `0.25fr 0.25fr` in a 400px
// container with a 150px-wide item in column 0 → 150 / 62.5: the freeze drops
// column 0 at its 150px base, and the restart scales the 250px remainder by the
// remaining 0.25 factor sum (250 × 0.25 / 0.25 → 0.25 × 250 = 62.5). The old
// single up-front cap gave 150 / 50.
fn grid_fr_sum_below_one_freeze_recomputes_cap_per_restart() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "0.25fr 0.25fr", "400px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "width", "150px");
    assert_style_set(&mut root, wide, "height", "40px");
    let narrow = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, narrow, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 150.0, 40.0, "frozen 150px item");
    assert_rect_op_geometry(&rects[2], 150.0, 0.0, 62.5, 40.0, "second item gets 62.5");
}

#[test]
// No-freeze control: `1fr 1fr` in a 400px container with two zero-content items
// divides the width 200/200 (unchanged by the freeze loop).
fn grid_fr_tracks_with_zero_content_stay_even() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "1fr 1fr", "400px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 40.0, "first item");
    assert_rect_op_geometry(&rects[2], 200.0, 0.0, 200.0, 40.0, "second item");
}

#[test]
// Partial freeze: `1fr 1fr 1fr` in a 400px container, one 200px-wide item in
// column 0 → the over-base track at 200, the other two at 100 each
// (400 = 200 + 100 + 100).
fn grid_fr_tracks_partial_freeze_redistributes_among_the_rest() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "1fr 1fr 1fr", "400px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "width", "200px");
    assert_style_set(&mut root, wide, "height", "40px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 40.0, "frozen 200px item");
    assert_rect_op_geometry(&rects[2], 200.0, 0.0, 100.0, 40.0, "second 100px item");
    assert_rect_op_geometry(&rects[3], 300.0, 0.0, 100.0, 40.0, "third 100px item");
}
