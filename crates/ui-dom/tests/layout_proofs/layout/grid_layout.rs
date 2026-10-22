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
    DomAttributeValue, DomNode, DomNodeSlotKey, DomPointerUpdate, DomRect, PresentationProperty,
    RootDom, RootDomMetricName, domains::layout::MeasurementRecord, node_box,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_img_to, assert_append_text_to, assert_close,
    assert_load_root_stylesheet, assert_node_layout, assert_rect_op_geometry,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, take_staged_layer_paint_plans,
    theme_stylesheet_origin,
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
// CSS Grid §8.4: an abs-pos child with BOTH axes definite resolves its static
// position to the resolved grid-area rectangle (content box + track offset),
// not the padding box. `grid-column: 2 / 3; grid-row: 2 / 3` in a
// `100px 100px / 40px 40px` grid with `padding: 10px` → the area rect at
// content (10, 10) + track offset (100, 40) = (110, 50).
fn grid_abs_pos_child_static_position_uses_the_grid_area_rect() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "grid-template-rows", "40px 40px");
    assert_style_set(&mut root, container, "padding", "10px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "grid-column", "2 / 3");
    assert_style_set(&mut root, abs, "grid-row", "2 / 3");
    assert_style_set(&mut root, abs, "width", "20px");
    assert_style_set(&mut root, abs, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The abs child sits at the grid-area rectangle: content box (10, 10) plus
    // the track offset (100, 40) = (110, 50), not the padding box (10, 10).
    let abs_box = border_box_of(&root, abs);
    assert_close(
        abs_box.left_px,
        110.0,
        "abs child static left (grid area rect)",
    );
    assert_close(
        abs_box.top_px,
        50.0,
        "abs child static top (grid area rect)",
    );
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

#[test]
// CSS Grid §7.1 + §8.5: `grid-column: span 2` occupies two consecutive columns,
// sized by both tracks plus the gap.
fn grid_column_span_places_across_two_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "column-gap", "10px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "span 2");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let box_ = border_box_of(&root, item);
    assert_close(box_.left_px, 0.0, "span-2 item starts at column 1");
    assert_close(box_.width_px, 210.0, "item spans 100px + 10px gap + 100px");
    assert_close(box_.height_px, 40.0, "item height");
}

#[test]
// CSS Grid §7.1 + §8.5: `grid-row: span 2` occupies two consecutive rows.
fn grid_row_span_places_across_two_rows() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px");
    assert_style_set(&mut root, container, "grid-template-rows", "40px 40px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-row", "span 2");
    assert_style_set(&mut root, item, "width", "50px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let box_ = border_box_of(&root, item);
    assert_close(box_.top_px, 0.0, "span-2 item starts at row 1");
    assert_close(box_.height_px, 80.0, "item spans 40px + 40px");
}

#[test]
// CSS Grid §8.5 step 3: a span larger than the explicit grid grows the implicit
// grid to match. `grid-column: span 3` in a 2-track grid gets a third implicit
// column (sized by `grid-auto-columns`), and the trailing auto item wraps to
// the next row. The unfixed solver looped forever here — `col + col_span <=
// grid_width` could never hold — so this pin doubles as the termination proof
// (the update must COMPLETE).
fn grid_column_span_exceeding_tracks_expands_the_implicit_grid() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, container, "grid-auto-columns", "100px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "grid-column", "span 3");
    assert_style_set(&mut root, wide, "height", "40px");
    let auto_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, auto_item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let wide_box = border_box_of(&root, wide);
    assert_close(wide_box.left_px, 0.0, "span-3 item starts at column 1");
    assert_close(
        wide_box.width_px,
        300.0,
        "span-3 item spans the implicit third column",
    );
    let auto_box = border_box_of(&root, auto_item);
    assert_close(auto_box.left_px, 0.0, "auto item wraps to the next row");
    assert_close(
        auto_box.top_px,
        40.0,
        "auto item sits below the span-3 item",
    );
}

#[test]
// CSS Grid §8.5 step 3 (symmetric): `grid-row: span 3` in a 2-row grid under
// column flow grows the implicit grid to three rows. The unfixed solver looped
// forever in `find_auto_auto_col` (`row + row_span <= grid_height` could never
// hold), so this pin doubles as the termination proof.
fn grid_row_span_exceeding_tracks_expands_the_implicit_grid() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-rows", "40px 40px");
    assert_style_set(&mut root, container, "grid-auto-flow", "column");
    assert_style_set(&mut root, container, "grid-auto-rows", "40px");
    let tall = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, tall, "grid-row", "span 3");
    assert_style_set(&mut root, tall, "width", "50px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let tall_box = border_box_of(&root, tall);
    assert_close(tall_box.top_px, 0.0, "span-3 item starts at row 1");
    assert_close(tall_box.height_px, 120.0, "span-3 item spans three rows");
}

#[test]
// CSS Grid §8.3: `grid-column: 2 / 4` places the item in line-index columns
// 2–3, and auto items flow around it.
fn grid_column_start_end_integer_lines() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(3, 100px)", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "grid-column", "2 / 4");
    assert_style_set(&mut root, first, "height", "40px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_box = border_box_of(&root, first);
    assert_close(first_box.left_px, 100.0, "definite item at column 2");
    assert_close(first_box.width_px, 200.0, "definite item spans columns 2-3");
    let second_box = border_box_of(&root, second);
    assert_close(second_box.left_px, 0.0, "auto item flows to column 1");
    assert_close(second_box.width_px, 100.0, "auto item width");
}

#[test]
// CSS Grid §8.5 step 3.2: a negative definite line creates left-implicit
// tracks via the container-wide explicit-origin offset (D2), and the auto
// placement wrap bound must cover those tracks. `grid-column: -5 / span 1` in
// a 2-track grid resolves to 0-based line −2 (offset `1 − min(start)` = 2), so
// the item lands in the first of TWO left-implicit tracks; the two auto items
// then fill the second left-implicit track and the first EXPLICIT track — the
// wrap point covers left-implicit + explicit columns, not just the explicit
// count (Blink `column_start_offset`, Gecko `mExplicitGridOffsetCol`).
fn grid_left_implicit_tracks_join_auto_placement_at_the_offset() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "100px 100px", "200px");
    assert_style_set(&mut root, container, "grid-auto-columns", "100px");
    let definite = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, definite, "grid-column", "-5 / span 1");
    assert_style_set(&mut root, definite, "height", "40px");
    let first_auto = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first_auto, "height", "40px");
    let second_auto = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second_auto, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The four columns' worth of layout (two left-implicit + two explicit) is
    // shown by the three positions: the definite item at the first left-implicit
    // track, the first auto at the second, and the second auto at the first
    // EXPLICIT track (the row the unfixed wrap bound breaks).
    let definite_box = border_box_of(&root, definite);
    assert_close(
        definite_box.left_px,
        0.0,
        "definite item in the first left-implicit track",
    );
    assert_close(definite_box.width_px, 100.0, "definite item width");
    let first_box = border_box_of(&root, first_auto);
    assert_close(
        first_box.left_px,
        100.0,
        "first auto item in the second left-implicit track",
    );
    assert_close(first_box.width_px, 100.0, "first auto item width");
    let second_box = border_box_of(&root, second_auto);
    assert_close(
        second_box.left_px,
        200.0,
        "second auto item in the first explicit track",
    );
    assert_close(second_box.width_px, 100.0, "second auto item width");
}

#[test]
// CSS Grid §8.3 negative lines: `grid-column: -1 / -3` in a 3-track grid
// resolves from the explicit end to lines 4/2 (spanning tracks 2–3).
fn grid_negative_lines_translate_from_the_explicit_end() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(3, 100px)", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "-1 / -3");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let box_ = border_box_of(&root, item);
    assert_close(box_.left_px, 100.0, "negative lines resolve to track 2");
    assert_close(box_.width_px, 200.0, "item spans tracks 2-3");
}

#[test]
// CSS Grid §8.5: `grid-auto-flow: column` fills down the first column before
// starting the second (column-major).
fn grid_auto_flow_column_fills_column_major() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(2, 100px)",
    );
    assert_style_set(
        &mut root,
        container,
        "grid-template-rows",
        "repeat(2, 40px)",
    );
    assert_style_set(&mut root, container, "grid-auto-flow", "column");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "item 0 (col 1, row 1)");
    assert_rect_op_geometry(&rects[2], 0.0, 40.0, 100.0, 40.0, "item 1 (col 1, row 2)");
    assert_rect_op_geometry(&rects[3], 100.0, 0.0, 100.0, 40.0, "item 2 (col 2, row 1)");
}

#[test]
// CSS Grid §8.5: dense packing backfills a hole that sparse leaves empty.
fn grid_auto_flow_dense_backfills_holes() {
    // Sparse: the span-2 auto item cannot fit beside the definite item, so it
    // wraps; the trailing span-1 item cannot return to (0,0).
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(2, 100px)",
    );
    let definite = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, definite, "grid-column", "2");
    assert_style_set(&mut root, definite, "height", "40px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "grid-column", "span 2");
    assert_style_set(&mut root, wide, "height", "40px");
    let last = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, last, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let sparse_last_box = border_box_of(&root, last);
    assert_close(
        sparse_last_box.top_px,
        80.0,
        "sparse: last auto item at row 3",
    );

    // Dense: the trailing span-1 item backfills the (0,0) hole.
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(2, 100px)",
    );
    assert_style_set(&mut root, container, "grid-auto-flow", "dense");
    let definite = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, definite, "grid-column", "2");
    assert_style_set(&mut root, definite, "height", "40px");
    let wide = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, wide, "grid-column", "span 2");
    assert_style_set(&mut root, wide, "height", "40px");
    let last = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, last, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let dense_last_box = border_box_of(&root, last);
    assert_close(
        dense_last_box.top_px,
        0.0,
        "dense: last auto item backfills (0,0)",
    );
    assert_close(dense_last_box.left_px, 0.0, "dense backfill at column 1");
}

#[test]
// CSS Grid §12.7: `grid-template-rows: 40px 1fr 2fr` against a definite height
// sizes rows 40 / 20 / 40.
fn grid_template_rows_fr_and_fixed() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px");
    assert_style_set(&mut root, container, "grid-template-rows", "40px 1fr 2fr");
    assert_style_set(&mut root, container, "height", "100px");
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "width", "50px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 50.0, 40.0, "row 1 = 40px");
    assert_rect_op_geometry(&rects[2], 0.0, 40.0, 50.0, 20.0, "row 2 = 1fr (20px)");
    assert_rect_op_geometry(&rects[3], 0.0, 60.0, 50.0, 40.0, "row 3 = 2fr (40px)");
}

#[test]
// CSS Grid §12.6: `grid-template-rows: minmax(60px, max-content)` with content
// floors the row at 60px.
fn grid_template_rows_minmax_and_content() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px");
    assert_style_set(
        &mut root,
        container,
        "grid-template-rows",
        "minmax(60px, max-content)",
    );
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.content_height_px,
        60.0,
        "row floors at 60px",
    );
}

#[test]
// CSS Grid §8.5 + §12.7: items past the explicit rows size by `grid-auto-rows`.
fn grid_auto_rows_beyond_explicit_count() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px");
    assert_style_set(&mut root, container, "grid-template-rows", "40px");
    assert_style_set(&mut root, container, "grid-auto-rows", "minmax(80px, auto)");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "25px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.content_height_px,
        120.0,
        "explicit 40px row + implicit 80px auto-row",
    );
}

#[test]
// CSS Box Alignment §10.2: `justify-items` aligns a smaller item on the inline
// axis within its track.
fn grid_justify_items_start_end_center_in_cell() {
    // center
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "justify-items", "center");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, item).left_px,
        75.0,
        "center: (200-50)/2",
    );

    // end
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "justify-items", "end");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(border_box_of(&root, item).left_px, 150.0, "end: 200-50");

    // start
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "justify-items", "start");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, item).left_px,
        0.0,
        "start: inline-start edge",
    );
}

#[test]
// CSS Box Alignment §10.2: `justify-self` overrides the container's
// `justify-items`.
fn grid_justify_self_overrides_container() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "justify-items", "center");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "justify-self", "end");
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        border_box_of(&root, item).left_px,
        150.0,
        "justify-self end beats container center",
    );
}

#[test]
// CSS Box Alignment §10.2: the flex group's `align-items` applies to the grid
// item's block axis (bottom-aligns items in their rows).
fn grid_align_items_block_axis_in_rows() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "grid-template-rows", "100px");
    assert_style_set(&mut root, container, "align-items", "end");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        border_box_of(&root, item).top_px,
        60.0,
        "align-items end bottom-aligns in the row",
    );
}

#[test]
// CSS Box Alignment §10.2: `place-items: center` expands to `align-items` AND
// `justify-items` (the D1 expansion correction).
fn grid_place_items_expands_to_justify_items() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "grid-template-rows", "100px");
    assert_style_set(&mut root, container, "place-items", "center");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let box_ = border_box_of(&root, item);
    assert_close(
        box_.left_px,
        75.0,
        "place-items center: inline-axis centering",
    );
    assert_close(
        box_.top_px,
        30.0,
        "place-items center: block-axis centering",
    );
}

#[test]
// CSS Box Alignment §10.2: `place-self: center` expands to `align-self` AND
// `justify-self`.
fn grid_place_self_expands_to_justify_self() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "grid-template-rows", "100px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "place-self", "center");
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let box_ = border_box_of(&root, item);
    assert_close(box_.left_px, 75.0, "place-self center: inline axis");
    assert_close(box_.top_px, 30.0, "place-self center: block axis");
}

#[test]
// CSS Box Alignment §8.1: absorbed auto margins take precedence over
// justify-items.
fn grid_auto_margins_still_beat_justify() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "200px", "200px");
    assert_style_set(&mut root, container, "justify-items", "start");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "margin", "auto");
    assert_style_set(&mut root, item, "width", "50px");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_close(
        border_box_of(&root, item).left_px,
        75.0,
        "auto margins center regardless of justify-items: start",
    );
}

#[test]
// CSS Grid §8.5 order: definite items resolve first, then items locked to the
// major axis, then remaining autos.
fn grid_definite_then_locked_then_auto_order() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(2, 100px)", "200px");
    // First in document order: an auto item.
    let auto_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, auto_item, "height", "40px");
    // Second: an item locked to row 1 (definite row, auto column).
    let locked = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, locked, "grid-row", "1");
    assert_style_set(&mut root, locked, "height", "40px");
    // Third: a both-axes-definite item.
    let definite = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, definite, "grid-column", "2");
    assert_style_set(&mut root, definite, "grid-row", "2");
    assert_style_set(&mut root, definite, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The locked item claims (row 1, col 1) before the auto item, so the auto
    // item flows to (row 1, col 2).
    let locked_box = border_box_of(&root, locked);
    assert_close(locked_box.left_px, 0.0, "locked item at column 1");
    assert_close(locked_box.top_px, 0.0, "locked item at row 1");
    let auto_box = border_box_of(&root, auto_item);
    assert_close(
        auto_box.left_px,
        100.0,
        "auto item flows past the locked item",
    );
    assert_close(auto_box.top_px, 0.0, "auto item at row 1");
    let definite_box = border_box_of(&root, definite);
    assert_close(definite_box.left_px, 100.0, "definite item at column 2");
    assert_close(definite_box.top_px, 40.0, "definite item at row 2");
}

#[test]
// CSS Grid §7.3: `grid-template-areas` + explicit columns build the page shell
// (`GRID_STEP4_DESIGN.md` D5 pin `grid_template_areas_page_shell`). The areas
// define three rows; the items named via `grid-area` fill their rects.
fn grid_template_areas_page_shell() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\" \"sidebar main\" \"footer footer\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 1fr");
    assert_style_set(&mut root, container, "width", "300px");
    let header = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, header, "grid-area", "header");
    assert_style_set(&mut root, header, "height", "40px");
    let sidebar = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, sidebar, "grid-area", "sidebar");
    assert_style_set(&mut root, sidebar, "height", "40px");
    let main = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, main, "grid-area", "main");
    assert_style_set(&mut root, main, "height", "40px");
    let footer = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, footer, "grid-area", "footer");
    assert_style_set(&mut root, footer, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Header spans both columns (300px) at row 0.
    let header_box = border_box_of(&root, header);
    assert_close(header_box.left_px, 0.0, "header left");
    assert_close(header_box.width_px, 300.0, "header spans both columns");
    assert_close(header_box.top_px, 0.0, "header top");
    // Sidebar in column 1 (100px), main in column 2 (200px), both row 1.
    let sidebar_box = border_box_of(&root, sidebar);
    assert_close(sidebar_box.left_px, 0.0, "sidebar left");
    assert_close(sidebar_box.width_px, 100.0, "sidebar width");
    assert_close(sidebar_box.top_px, 40.0, "sidebar top");
    let main_box = border_box_of(&root, main);
    assert_close(main_box.left_px, 100.0, "main left");
    assert_close(main_box.width_px, 200.0, "main width");
    assert_close(main_box.top_px, 40.0, "main top");
    // Footer spans both columns at row 2.
    let footer_box = border_box_of(&root, footer);
    assert_close(footer_box.left_px, 0.0, "footer left");
    assert_close(footer_box.width_px, 300.0, "footer spans both columns");
    assert_close(footer_box.top_px, 80.0, "footer top");
}

#[test]
// CSS Grid §7.3.1: a dot-run names an empty cell (`grid_template_areas_empty_dot_cells`).
fn grid_template_areas_empty_dot_cells() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\". main main\"",
    );
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "100px 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let main = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, main, "grid-area", "main");
    assert_style_set(&mut root, main, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // `main` occupies columns 2–3 (the dot-run left column 1 empty).
    let main_box = border_box_of(&root, main);
    assert_close(main_box.left_px, 100.0, "main starts at column 2");
    assert_close(main_box.width_px, 200.0, "main spans columns 2-3");
}

#[test]
// CSS Grid §7.4: `grid-area: <name>` fills the area's rect with stretch
// (`grid_area_name_fills_its_area_rect`).
fn grid_area_name_fills_its_area_rect() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "width", "200px");
    let header = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, header, "grid-area", "header");
    assert_style_set(&mut root, header, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let header_box = border_box_of(&root, header);
    assert_close(header_box.left_px, 0.0, "header start");
    assert_close(
        header_box.width_px,
        200.0,
        "header stretches across the rect",
    );
    assert_close(header_box.top_px, 0.0, "header top");
    assert_close(header_box.height_px, 40.0, "header height");
}

#[test]
// CSS Cascade: `grid-area: header` expands to the four placement longhands as
// `Named(header)`, so a later placement longhand wins its own slot — the
// margin-shorthand model (`margin: 0 auto` then `margin-left: 5px`). Here
// `grid-column-start: 2` overrides the header's column-start edge while the
// other three longhands still come from the header area
// (`grid_area_longhand_after_shorthand_wins_its_slot`).
fn grid_area_longhand_after_shorthand_wins_its_slot() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\" \"sidebar main\" \"footer footer\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 1fr");
    assert_style_set(&mut root, container, "width", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-area", "header");
    assert_style_set(&mut root, item, "grid-column-start", "2");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The column-start longhand wins its slot: the item starts at column 2
    // (100px), while row-start/end come from the header area (top 0, height 40)
    // and column-end comes from header (span to the right edge at 300px). This
    // is NOT the whole header rect (left 0, width 300).
    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        100.0,
        "column-start longhand wins: column 2",
    );
    assert_close(
        item_box.width_px,
        200.0,
        "column-end from header spans to the right edge",
    );
    assert_close(item_box.top_px, 0.0, "row-start from header (top 0)");
    assert_close(item_box.height_px, 40.0, "row-end from header (height 40)");
}

#[test]
// CSS Cascade: re-setting `grid-area` between forms clears the previous form's
// longhands (the disjoint name/longhand slots no longer exist). `grid-area:
// header` then `grid-area: 2 / 3` must place per `2 / 3`, not the stale header
// rect (`grid_area_reset_between_forms_clears_the_name`).
fn grid_area_reset_between_forms_clears_the_name() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\"",
    );
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "100px 100px 100px",
    );
    assert_style_set(&mut root, container, "grid-template-rows", "40px 40px");
    assert_style_set(&mut root, container, "width", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-area", "header");
    assert_style_set(&mut root, item, "grid-area", "2 / 3");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // `2 / 3` → row-start 2, column-start 3 (row 1, column 2); not the header
    // rect (left 0, top 0).
    let item_box = border_box_of(&root, item);
    assert_close(item_box.left_px, 200.0, "column-start 3 (no stale name)");
    assert_close(item_box.top_px, 40.0, "row-start 2 (no stale name)");
    assert_close(item_box.width_px, 100.0, "single column width");
    assert_close(item_box.height_px, 40.0, "single row height");
}

#[test]
// CSS Grid §7.4: `grid-area: nosuch` (no matching area or line) falls back to
// auto placement — documented, NOT a drop (`grid_area_unknown_name_becomes_auto`).
fn grid_area_unknown_name_becomes_auto() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "width", "200px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-area", "nosuch");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        0.0,
        "unknown name auto-places at column 1",
    );
    assert_close(item_box.width_px, 100.0, "unknown name auto width");
    assert_close(item_box.top_px, 0.0, "unknown name auto row");
}

#[test]
// CSS Grid §7.1: explicit `[name]` lines resolve `Named` references
// (`grid_named_lines_in_track_lists`).
fn grid_named_lines_in_track_lists() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [aside] 1fr",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "grid-column", "main");
    assert_style_set(&mut root, first, "height", "40px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "grid-column", "aside");
    assert_style_set(&mut root, second, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_box = border_box_of(&root, first);
    assert_close(first_box.left_px, 0.0, "main starts at line 1");
    assert_close(first_box.width_px, 100.0, "main width");
    let second_box = border_box_of(&root, second);
    assert_close(second_box.left_px, 100.0, "aside starts at line 2");
}

#[test]
// CSS Grid §7.3.1: the area-implied `<name>-end` line resolves
// (`grid_area_implied_start_end_lines`).
fn grid_area_implied_start_end_lines() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"header header\" \"footer footer\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "width", "200px");
    let header = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, header, "grid-area", "header");
    assert_style_set(&mut root, header, "height", "40px");
    let after = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, after, "grid-row-start", "header-end");
    assert_style_set(&mut root, after, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let after_box = border_box_of(&root, after);
    assert_close(
        after_box.top_px,
        40.0,
        "header-end starts after the header area",
    );
}

#[test]
// CSS Grid §8.1.3 + §8.3: multiple same-name lines form a named set indexed by
// filtering the placement by name — a bare `<custom-ident>` is "treated as if
// the integer 1 had been specified" (the FIRST match), and `span
// <custom-ident>` with no integer is replaced with a span of 1. CORRECTED
// 2026-10-22: the landed T3 "treated as Span(count) across the matches" rule
// does not exist — `main`/`span main` occupy the FIRST track (width 100), not
// span 2 (width 200). (`grid_multiple_same_name_lines_span_count`.)
fn grid_multiple_same_name_lines_span_count() {
    // `grid-column: main` with two `main` lines → the FIRST match (line 0):
    // one track, width 100 (the landed wrong value was span 2 / width 200).
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let bare = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, bare, "grid-column", "main");
    assert_style_set(&mut root, bare, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, bare).left_px,
        0.0,
        "bare name starts at the first `main` line",
    );
    assert_close(
        border_box_of(&root, bare).width_px,
        100.0,
        "bare name occupies the first track (NOT span 2 / width 200)",
    );

    // `grid-column: span main` → span 1 from the first match (§8.3): width 100
    // (the landed wrong value was span 2 / width 200).
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let span = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, span, "grid-column", "span main");
    assert_style_set(&mut root, span, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, span).left_px,
        0.0,
        "span main starts at the first `main` line",
    );
    assert_close(
        border_box_of(&root, span).width_px,
        100.0,
        "span main is span 1 from the first match (NOT span 2 / width 200)",
    );

    // `grid-column: span main 3` → span 3 from the first match: width 300
    // (already correct — unchanged).
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let span_n = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, span_n, "grid-column", "span main 3");
    assert_style_set(&mut root, span_n, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, span_n).width_px,
        300.0,
        "span main 3 spans 3 from the first match",
    );
}

#[test]
// CSS Grid §8.1.3 + §8.3.1: `name / <integer>` resolves the name to the FIRST
// match and lets the explicit end line win. `main / 4` → start at the first
// `main` (line 0), end at line 4 (the far edge after three 100px tracks) →
// three tracks. The landed wrong rule (name → span 2) ignored the `4` and
// produced width 200.
// (`grid_column_main_slash_4_resolves_first_match_plus_explicit_end`.)
fn grid_column_main_slash_4_resolves_first_match_plus_explicit_end() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "main / 4");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        0.0,
        "the name resolves to the first `main` (line 0), not a span",
    );
    assert_close(
        item_box.width_px,
        300.0,
        "the explicit end 4 wins: three tracks (NOT the name's span 2 / 200)",
    );
}

#[test]
// CSS Grid §8.3.1: `integer / name` where the definite start exceeds the
// name's first-match end applies the placement-conflict swap. `3 / main` →
// start line 3 (0-based 2), end at the first `main` (line 0) → start > end →
// swapped to (0, 2) → two tracks at left 0. The landed wrong rule short-
// circuited the name to a `Range(0, 2)` and ignored the swap.
// (`grid_column_3_slash_main_resolves_end_first_match_with_swap`.)
fn grid_column_3_slash_main_resolves_end_first_match_with_swap() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "3 / main");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        0.0,
        "start 3 > end (first `main` at 0) swaps to the first `main`",
    );
    assert_close(
        item_box.width_px,
        200.0,
        "swapped definite range spans two tracks",
    );
}

#[test]
// CSS Grid §8.3: `name / span <integer>` uses the span from the FIRST match of
// the name. `main / span 2` → start at the first `main` (line 0), span 2 →
// two tracks at left 0. (`grid_column_main_slash_span2_uses_span_from_first_match`.)
fn grid_column_main_slash_span2_uses_span_from_first_match() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px [main] 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "main / span 2");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        0.0,
        "the name resolves to the first `main` (line 0)",
    );
    assert_close(
        item_box.width_px,
        200.0,
        "span 2 from the first match spans two tracks",
    );
}

#[test]
// CSS Grid §7.1: `grid-column: nosuch` (no matching line) falls back to auto
// (`grid_named_line_unknown_becomes_auto`).
fn grid_named_line_unknown_becomes_auto() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "width", "200px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "nosuch");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        0.0,
        "unknown line auto-places at column 1",
    );
    assert_close(item_box.width_px, 100.0, "unknown line auto width");
}

#[test]
// CSS Grid §7.3: areas naming a subset of a larger explicit template — the
// remaining tracks size and place normally (`grid_areas_and_explicit_template_tracks`).
fn grid_areas_and_explicit_template_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-areas", "\"a a\"");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "100px 100px 100px",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let area_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, area_item, "grid-area", "a");
    assert_style_set(&mut root, area_item, "height", "40px");
    let auto_item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, auto_item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let area_box = border_box_of(&root, area_item);
    assert_close(area_box.width_px, 200.0, "area spans its two columns");
    let auto_box = border_box_of(&root, auto_item);
    assert_close(
        auto_box.left_px,
        200.0,
        "auto item places in the third column",
    );
    assert_close(auto_box.width_px, 100.0, "auto item width");
}

#[test]
// CSS Grid §8.4: an abs-pos child with `grid-area: <name>` resolves its static
// position to the area rect (`grid_areas_definite_items_abs_pos_area_rect`).
fn grid_areas_definite_items_abs_pos_area_rect() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"a a\" \"b b\"",
    );
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "grid-template-rows", "40px 40px");
    let abs = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, abs, "position", "absolute");
    assert_style_set(&mut root, abs, "grid-area", "b");
    assert_style_set(&mut root, abs, "width", "20px");
    assert_style_set(&mut root, abs, "height", "20px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let abs_box = border_box_of(&root, abs);
    assert_close(abs_box.left_px, 0.0, "abs static left (area b rect)");
    assert_close(abs_box.top_px, 40.0, "abs static top (area b rect)");
}

#[test]
// CSS Grid §7: the canonical style snapshot round-trips areas, named lines, and
// `Named`/`NamedSpan` longhands (`grid_area_wire_round_trip`).
fn grid_area_wire_round_trip() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-areas",
        "\"a a\" \"b b\"",
    );
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "[main] 100px 100px",
    );
    assert_style_set(&mut root, container, "grid-column-start", "main");
    assert_style_set(&mut root, container, "grid-column-end", "span main 2");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let snapshot = root.canonical_style_snapshot(container);
    let replayed = snapshot
        .styles
        .iter()
        .map(|(name, value)| {
            let ui_dom::DomAttributeValue::String(text) = value else {
                panic!("grid styles serialize to string values only");
            };
            format!("{name}: {text}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    assert_set_attr_str(&mut root, container, "style", &replayed);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(root.canonical_style_snapshot(container), snapshot);
}

// — Grid T3.5 auto-fill/auto-fit repetition (`GRID_STEP5_DESIGN.md` D4) — //

#[test]
// CSS Grid §7.2.3.2: `repeat(auto-fit, minmax(240px, 1fr))` in 600px, 2 items
// → N = 2, each track 300 (240 base + (600−480)/2 = 60 each); both filled.
fn grid_repeat_auto_fit_minmax_fr_card_wall() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fit, minmax(240px, 1fr))",
        "600px",
    );
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_eq!(rects.len(), 3, "container + 2 items");
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 300.0, 40.0, "first card");
    assert_rect_op_geometry(&rects[2], 300.0, 0.0, 300.0, 40.0, "second card");
}

#[test]
// CSS Grid §7.2.3.2: auto-fit collapses empty tracks. 600px, 1 item → N=2
// created, the empty second track collapses to 0, and the fr distribution
// re-runs over the surviving track → the item's track gets the full 600.
fn grid_repeat_auto_fit_collapses_empty_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fit, minmax(240px, 1fr))",
        "600px",
    );
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.width_px,
        600.0,
        "the surviving track absorbs the collapsed track's space",
    );
}

#[test]
// CSS Grid §7.2.3.2 (the stylesheet→inline arena reintern path): a matched
// stylesheet rule (not an inline `style`) authors
// `repeat(auto-fit, minmax(240px, 1fr))`. The `52d7ac3` reintern fix keeps the
// auto-repeat span across the arena reintern, so the stylesheet-authored path
// matches the inline pin `grid_repeat_auto_fit_collapses_empty_tracks`: at
// 600px with 1 item, the empty track collapses and the surviving track gets
// the full 600.
//
// The layout outcome alone cannot distinguish a dropped span from a collapsed
// empty track (a single `minmax(240px, 1fr)` track also resolves to 600), so
// the pin also asserts the computed `grid-template-columns` still serializes
// to the `repeat(auto-fit, …)` form — the span marker's canonical re-emission,
// which is the reintern's job to preserve.
fn grid_repeat_survives_stylesheet_reintern() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".grid-wall { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); width: 600px; }",
    );
    assert_set_attr_str(&mut root, container, "class", "grid-wall");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.width_px,
        600.0,
        "the surviving track absorbs the collapsed track's space",
    );

    // The auto-repeat span survives the reintern: the computed wire value
    // re-emits the `repeat(auto-fit, …)` form, not the bare inner track.
    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String(
            "repeat(auto-fit, minmax(240px, 1fr))".to_string()
        )),
        "the reinterned track list keeps its auto-fit span",
    );
}

#[test]
// CSS Grid §7.4 `none` form through the stylesheet→inline reintern path: a
// matched stylesheet rule authors `grid-template: none`, which expands to the
// null-key sentinel for all three template longhands. The sentinel is
// store-independent (it resolves to no arena entry BY DESIGN), so the reintern
// must pass it through unchanged rather than resolve it. Layout outcomes for
// `none` are non-discriminating, so this pin asserts the WIRE form via
// `compute_style_map` (the `grid_repeat_survives_stylesheet_reintern` precedent).
fn grid_template_none_survives_stylesheet_reintern() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { display: grid; grid-template: none }",
    );
    assert_set_attr_str(&mut root, container, "class", "x");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateRows),
        Some(&DomAttributeValue::String("none".to_string())),
        "rows none",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String("none".to_string())),
        "columns none",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateAreas),
        Some(&DomAttributeValue::String("none".to_string())),
        "areas none",
    );
}

#[test]
// CSS Grid §7.3 `none` longhand through the stylesheet→inline reintern path: a
// matched stylesheet rule authors `grid-template-columns: none` directly (not
// via a shorthand), storing the null-key sentinel. The sentinel must pass
// through the reintern unchanged; the pin asserts the WIRE form (`none`).
fn grid_track_list_none_survives_stylesheet_reintern() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { display: grid; grid-template-columns: none }",
    );
    assert_set_attr_str(&mut root, container, "class", "x");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String("none".to_string())),
        "columns none",
    );
}

#[test]
// CSS Grid §7.7 row-flow auto-flow form through the stylesheet→inline reintern
// path: `grid: auto-flow 60px / 100px` emits rows as the null-key sentinel
// (columns 100px, auto-flow row, auto-rows 60px). The rows sentinel must pass
// through the reintern unchanged; the pin asserts the WIRE form.
fn grid_shorthand_null_axis_survives_stylesheet_reintern() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { display: grid; grid: auto-flow 60px / 100px }",
    );
    assert_set_attr_str(&mut root, container, "class", "x");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateRows),
        Some(&DomAttributeValue::String("none".to_string())),
        "rows none",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridAutoFlow),
        Some(&DomAttributeValue::String("row".to_string())),
        "auto-flow row",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridAutoRows),
        Some(&DomAttributeValue::String("60px".to_string())),
        "auto-rows 60px",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String("100px".to_string())),
        "columns 100px",
    );
}

#[test]
// CSS Grid §7.2.3.2: auto-FILL never collapses. 600px, 1 item → N=2, no
// collapse, fr-expand verbatim: both tracks 300 (the empty "ghost" track 300).
fn grid_repeat_auto_fill_keeps_empty_tracks_flexed() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fill, minmax(240px, 1fr))",
        "600px",
    );
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.width_px,
        300.0,
        "auto-fill: the item's track is 300 (no collapse of the ghost track)",
    );
}

#[test]
// CSS Grid §7.2.3.2: the count recomputes on container resize (the dirty-bit
// doctrine re-runs the whole solve). Auto-FILL never collapses, so each track
// flexes to S/N: 600 → 2 cols (300 each); 400 → 1 col (400); 800 → 3 cols
// (800/3 each).
fn grid_repeat_recount_on_container_resize() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fill, minmax(240px, 1fr))",
        "600px",
    );
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, item).width_px,
        300.0,
        "600px: 2 columns of 300",
    );

    assert_style_set(&mut root, container, "width", "400px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, item).width_px,
        400.0,
        "400px: 1 column of 400",
    );

    assert_style_set(&mut root, container, "width", "800px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_close(
        border_box_of(&root, item).width_px,
        800.0 / 3.0,
        "800px: 3 columns of 800/3",
    );
}

#[test]
// CSS Grid §7.2.3.2 + §8.2: column-gap joins the count formula's gutter term.
// `repeat(auto-fit, minmax(240px, 1fr))` + 10px gap at 600 → N = floor(610/250)
// = 2, tracks (600−10)/2 = 295 each.
fn grid_repeat_count_with_gutters() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fit, minmax(240px, 1fr))",
        "600px",
    );
    assert_style_set(&mut root, container, "column-gap", "10px");
    for _ in 0..2 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 295.0, 40.0, "first card");
    assert_rect_op_geometry(&rects[2], 305.0, 0.0, 295.0, 40.0, "second card");
}

#[test]
// CSS Grid §7.2.3.2: a fixed multi-track list's repetition basis is the SUM of
// the list's bases. `repeat(auto-fit, 100px 200px)` in 600 → rep_size 300, N =
// floor(600/300) = 2 → four tracks (100,200,100,200).
fn grid_repeat_multi_track_list_span() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fit, 100px 200px)",
        "600px",
    );
    for _ in 0..4 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 100.0, 40.0, "track 1");
    assert_rect_op_geometry(&rects[2], 100.0, 0.0, 200.0, 40.0, "track 2");
    assert_rect_op_geometry(&rects[3], 300.0, 0.0, 100.0, 40.0, "track 3");
    assert_rect_op_geometry(&rects[4], 400.0, 0.0, 200.0, 40.0, "track 4");
}

#[test]
// CSS Grid §7.2.3.2 + the 12-track lane capacity: `repeat(auto-fill, 10px)` in
// 600px → the fill formula wants 60 repetitions, the count clamps to 12, and
// the `grid_repetition_count_capped` metric increments.
fn grid_repeat_count_capped_at_lane_capacity() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(&mut root, container, "repeat(auto-fill, 10px)", "600px");
    root.reset_metrics();
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.count(RootDomMetricName::GridRepetitionCountCapped),
        1,
        "the repetition count clamped to lane capacity once"
    );
}

#[test]
// CSS Grid §7.2.3.2 + B2: the capacity clamp compares in f32 BEFORE the f32→u32
// conversion. An astronomical definite size (`width: 2000000000000px`) would
// push the raw quotient past u32's range; the count clamps to 12 (no panic),
// and the `grid_repetition_count_capped` metric fires.
fn grid_repeat_count_clamps_before_conversion() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fill, 10px)",
        "2000000000000px",
    );
    for _ in 0..12 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    root.reset_metrics();
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.count(RootDomMetricName::GridRepetitionCountCapped),
        1,
        "the astronomical width's count clamped to lane capacity once"
    );
    // N = 12: twelve 10px tracks, the twelfth item sits at x = 110.
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_eq!(rects.len(), 13, "container + 12 items");
    assert_rect_op_geometry(&rects[12], 110.0, 0.0, 10.0, 40.0, "12th track");
}

#[test]
// CSS Grid §7.2.3.2 + N1: finite ~3e38 lengths (width, column-gap, and the span
// track) whose `s + gap` and `rep_size + gap` sums overflow f32 to INF make the
// fill-formula quotient INF/INF = NaN. The clamp still engages — the count
// lands on the 12-track lane capacity (no panic) — and
// `grid_repetition_count_capped` must count it, because a NaN quotient is a
// capped clamp, not an in-range count.
fn grid_repeat_nan_quotient_counts_as_capped() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(auto-fill, 300000000000000000000000000000000000000px)",
    );
    assert_style_set(
        &mut root,
        container,
        "column-gap",
        "300000000000000000000000000000000000000px",
    );
    assert_style_set(
        &mut root,
        container,
        "width",
        "300000000000000000000000000000000000000px",
    );
    for _ in 0..12 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    root.reset_metrics();
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.count(RootDomMetricName::GridRepetitionCountCapped),
        1,
        "the NaN quotient's count clamped to lane capacity once"
    );
    // N = 12 (no panic): twelve span tracks hold the twelve items.
    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_eq!(rects.len(), 13, "container + 12 items");
}

#[test]
// CSS Grid §7.1 + §8.1.3: named lines inside an auto-repeat span repeat with
// their tracks; `grid-column: main` resolves to the FIRST match (one track,
// not a span across both `main` lines).
fn grid_repeat_named_lines_repeat_with_tracks() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(auto-fit, [main] minmax(240px, 1fr))",
    );
    assert_style_set(&mut root, container, "width", "600px");
    let named = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, named, "grid-column", "main");
    assert_style_set(&mut root, named, "height", "40px");
    let auto = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, auto, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // `main` resolves to the FIRST match (line 0): the named item occupies one
    // 300px track, and the auto item flows to the second track (both filled, so
    // auto-fit does not collapse).
    let named_box = border_box_of(&root, named);
    assert_close(named_box.left_px, 0.0, "main resolves to the first line");
    assert_close(
        named_box.width_px,
        300.0,
        "the first-match track is ONE 300px column, not a span",
    );
    let auto_box = border_box_of(&root, auto);
    assert_close(
        auto_box.left_px,
        300.0,
        "auto item flows to the second track",
    );
    assert_close(auto_box.width_px, 300.0, "auto item width");
}

#[test]
// CSS Grid §7.2.3.2 + §8.1.3 (B3): a line name immediately after the close
// paren (`repeat(auto-fill, 100px) [after] 200px`) is suffix-owned, distinct
// from a trailing name inside the span. It resolves at the expanded boundary
// line (N=6 → after line 6, left 600) and round-trips with `after` emitted
// exactly once, in the suffix.
fn grid_repeat_boundary_line_name_ownership() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(auto-fill, 100px) [after] 200px",
    );
    assert_style_set(&mut root, container, "width", "600px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "after");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        600.0,
        "after resolves at the expanded boundary line (after the 6th 100px track)",
    );
    assert_close(
        item_box.width_px,
        200.0,
        "the suffix 200px track follows the boundary line",
    );

    // Round-trip: `after` is emitted exactly once, in the suffix section.
    let snapshot = root.canonical_style_snapshot(container);
    let value = snapshot
        .styles
        .iter()
        .find(|(name, _)| name == "grid-template-columns")
        .map(|(_, value)| value)
        .expect("grid-template-columns should be declared");
    let DomAttributeValue::String(text) = value else {
        panic!("grid-template-columns serializes to a string value");
    };
    assert_eq!(text, "repeat(auto-fill, 100px) [after] 200px");
    assert_eq!(
        text.matches("after").count(),
        1,
        "after is emitted exactly once (in the suffix, not duplicated in the span)"
    );
}

#[test]
// CSS Grid §7.2.3.2 + §8.1.3 (F1 — the B3 span-START mirror): a line name
// authored between the last prefix track and the `repeat(` token
// (`50px [pre] repeat(auto-fill, 100px) 200px`) is prefix-owned and sits on
// the span-START line. It resolves at line 1 (left 50, width 100) and
// round-trips with `[pre]` emitted exactly once, BEFORE the `repeat(` wrapper.
fn grid_repeat_prefix_boundary_name_round_trips() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "50px [pre] repeat(auto-fill, 100px) 200px",
    );
    assert_style_set(&mut root, container, "width", "600px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "grid-column", "pre");
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.left_px,
        50.0,
        "pre resolves at line 1 (after the 50px prefix track)",
    );
    assert_close(
        item_box.width_px,
        100.0,
        "the pre-named start is one 100px span track",
    );

    // Round-trip: `pre` is emitted exactly once, before the `repeat(`.
    let snapshot = root.canonical_style_snapshot(container);
    let value = snapshot
        .styles
        .iter()
        .find(|(name, _)| name == "grid-template-columns")
        .map(|(_, value)| value)
        .expect("grid-template-columns should be declared");
    let DomAttributeValue::String(text) = value else {
        panic!("grid-template-columns serializes to a string value");
    };
    assert_eq!(text, "50px [pre] repeat(auto-fill, 100px) 200px");
    assert_eq!(
        text.matches("pre").count(),
        1,
        "pre is emitted exactly once (in the prefix, not duplicated in the span)"
    );
    let pre_idx = text.find("[pre]").expect("the prefix bracket is present");
    let repeat_idx = text.find("repeat(").expect("the repeat wrapper is present");
    assert!(
        pre_idx < repeat_idx,
        "the prefix name is emitted before the repeat wrapper"
    );
}

#[test]
// CSS Grid §7.2.3.2 is per-axis (B4): auto-fit collapse applies to the ROW
// axis too. `height: 600px; grid-template-rows: repeat(auto-fit,
// minmax(40px, 1fr))` with 1 item → the count clamps to 12 (15 would exceed
// the lane capacity), 11 empty rows collapse, the fr distribution re-runs over
// the surviving row → the item stretches to the full 600px height.
fn grid_repeat_auto_fit_collapses_rows() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-rows",
        "repeat(auto-fit, minmax(40px, 1fr))",
    );
    assert_style_set(&mut root, container, "height", "600px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.height_px,
        600.0,
        "the surviving row absorbs the collapsed rows' space",
    );
}

#[test]
// CSS Grid §7.2.2.2: a collapsed auto-fit track's gutters on either side
// collapse. `repeat(auto-fit, minmax(240px, 1fr))` + 10px column-gap at 600px
// with 1 item → N=2, the empty second track collapses, and its adjacent gutter
// collapses, so the survivor absorbs the full 600 (not 600 − 10 = 590).
fn grid_repeat_auto_fit_collapses_gutters_columns() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fit, minmax(240px, 1fr))",
        "600px",
    );
    assert_style_set(&mut root, container, "column-gap", "10px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.width_px,
        600.0,
        "the survivor absorbs the collapsed track AND its gutter",
    );
}

#[test]
// CSS Grid §7.2.2.2 (row axis): `height: 600px; grid-template-rows:
// repeat(auto-fit, minmax(40px, 1fr))` + 10px row-gap with 1 item → 11 empty
// rows collapse, so every row gutter (each touches a collapsed row) collapses
// and the container's content height is the survivor's 600, not 600 + 11×10.
fn grid_repeat_auto_fit_collapses_gutters_rows() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-rows",
        "repeat(auto-fit, minmax(40px, 1fr))",
    );
    assert_style_set(&mut root, container, "height", "600px");
    assert_style_set(&mut root, container, "row-gap", "10px");
    let item = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, item, "width", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The survivor row keeps the full 600 (the item stretches to it), and the
    // container's content height drops the collapsed rows' gutters entirely.
    let item_box = border_box_of(&root, item);
    assert_close(
        item_box.height_px,
        600.0,
        "the surviving row keeps the full 600",
    );
    let container_layout = assert_node_layout(&root, container);
    assert_close(
        container_layout.content_height_px,
        600.0,
        "the collapsed rows' gutters collapse (content height 600, not 710)",
    );
}

#[test]
// CSS Grid §7.2.2.2: a collapsed auto-fit track's gutters on EITHER side
// collapse. Items explicitly placed in tracks 1 and 3 of a
// `repeat(auto-fit, 100px)` grid leave track 2 empty; track 2 collapses, so
// BOTH of its adjacent gutters collapse and the track-3 item's left edge sits
// flush after the track-1 item's right edge (no phantom 10px gap in the offset
// accumulation).
fn grid_repeat_auto_fit_middle_collapse_places_flush() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template-columns",
        "repeat(auto-fit, 100px)",
    );
    assert_style_set(&mut root, container, "column-gap", "10px");
    assert_style_set(&mut root, container, "width", "320px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "grid-column", "1");
    assert_style_set(&mut root, first, "height", "40px");
    let third = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, third, "grid-column", "3");
    assert_style_set(&mut root, third, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_box = border_box_of(&root, first);
    let third_box = border_box_of(&root, third);
    assert_close(first_box.width_px, 100.0, "track 1 is a fixed 100px track");
    assert_close(
        third_box.left_px,
        first_box.left_px + first_box.width_px,
        "track 3 item is flush after the track 1 item (track 2's gutters collapse)",
    );
}

#[test]
// CSS Grid §7.2.3.2 (NB2): a minmax is "treated as its max track sizing
// function if that is definite". `repeat(auto-fill, minmax(100px, 200px))` at
// 600px → basis 200 → N = floor(600/200) = 3 (three 200px tracks).
fn grid_repeat_minmax_definite_max_basis() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_grid_container(
        &mut root,
        container,
        "repeat(auto-fill, minmax(100px, 200px))",
        "600px",
    );
    for _ in 0..3 {
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "height", "40px");
    }
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let rects = &take_staged_layer_paint_plans(&root)[0].rects;
    assert_eq!(rects.len(), 4, "container + 3 items");
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 200.0, 40.0, "track 1 (basis 200)");
    assert_rect_op_geometry(&rects[2], 200.0, 0.0, 200.0, 40.0, "track 2");
    assert_rect_op_geometry(&rects[3], 400.0, 0.0, 200.0, 40.0, "track 3");
}

// — Grid T4: the `grid-template` and `grid` shorthands (`GRID_STEP6_DESIGN.md` D4) — //

/// Builds a `display: grid` container (width 300px) with `set_template` applied
/// (a shorthand or its equivalent longhands), appends the items authored by
/// `author_items`, runs one update, and returns each item's border box in order.
fn grid_item_boxes<F>(
    set_template: impl FnOnce(&mut RootDom, DomNodeSlotKey),
    author_items: F,
) -> Vec<DomRect>
where
    F: Fn(&mut RootDom, DomNodeSlotKey) -> Vec<DomNodeSlotKey>,
{
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    set_template(&mut root, container);
    assert_style_set(&mut root, container, "width", "300px");
    let keys = author_items(&mut root, container);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    keys.into_iter()
        .map(|key| border_box_of(&root, key))
        .collect()
}

/// Asserts two layout scenarios' item border boxes are pairwise identical
/// (field-wise, within the file's `assert_close` tolerance).
fn assert_boxes_match(left: &[DomRect], right: &[DomRect], label: &str) {
    assert_eq!(left.len(), right.len(), "{label}: item count");
    for (index, (a, b)) in left.iter().zip(right).enumerate() {
        assert_close(a.left_px, b.left_px, &format!("{label} item {index} left"));
        assert_close(a.top_px, b.top_px, &format!("{label} item {index} top"));
        assert_close(
            a.width_px,
            b.width_px,
            &format!("{label} item {index} width"),
        );
        assert_close(
            a.height_px,
            b.height_px,
            &format!("{label} item {index} height"),
        );
    }
}

#[test]
// CSS Grid §7.4: `grid-template: 40px 1fr / 100px 1fr` expands to the three
// template longhands; layout is identical to the longhand control
// (`grid_template_admits_and_expands`).
fn grid_template_admits_and_expands() {
    let items = |root: &mut RootDom, container: DomNodeSlotKey| -> Vec<DomNodeSlotKey> {
        let first = assert_append_div_to(root, container);
        assert_style_set(root, first, "height", "40px");
        let second = assert_append_div_to(root, container);
        assert_style_set(root, second, "height", "40px");
        vec![first, second]
    };
    let shorthand = grid_item_boxes(
        |root, c| assert_style_set(root, c, "grid-template", "40px 1fr / 100px 1fr"),
        items,
    );
    let longhand = grid_item_boxes(
        |root, c| {
            assert_style_set(root, c, "grid-template-rows", "40px 1fr");
            assert_style_set(root, c, "grid-template-columns", "100px 1fr");
            assert_style_set(root, c, "grid-template-areas", "none");
        },
        items,
    );
    assert_boxes_match(&shorthand, &longhand, "grid-template");

    // The shorthand emits all three template longhands on the wire.
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template",
        "40px 1fr / 100px 1fr",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateRows),
        Some(&DomAttributeValue::String("40px 1fr".to_string())),
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String("100px 1fr".to_string())),
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateAreas),
        Some(&DomAttributeValue::String("none".to_string())),
    );
}

#[test]
// CSS Grid §7.4 `none` form: prior `grid-template-columns: 100px` is reset by
// `grid-template: none` — items stack in implicit auto columns (columns none
// wins) (`grid_template_none_resets_all_three`).
fn grid_template_none_resets_all_three() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
    assert_style_set(&mut root, container, "grid-template", "none");
    assert_style_set(&mut root, container, "width", "300px");
    let first = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, first, "height", "40px");
    let second = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, second, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_box = border_box_of(&root, first);
    let second_box = border_box_of(&root, second);
    assert_close(
        first_box.width_px,
        300.0,
        "columns none: one implicit auto column",
    );
    assert_close(
        second_box.top_px,
        40.0,
        "items stack in implicit auto columns",
    );
    assert_close(second_box.left_px, 0.0, "second item in the same column");
}

#[test]
// CSS Grid §7.4 areas form: `grid-template: [top] "a b" 40px [middle] "c d" 1fr
// [bottom] / 100px 1fr` sets areas, rows (40/1fr with row line names), and
// columns (100/1fr); `grid-area: b` and `grid-row: middle` resolve; layout
// matches the longhand control (`grid_template_areas_form_sets_all_three`).
fn grid_template_areas_form_sets_all_three() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(
        &mut root,
        container,
        "grid-template",
        "[top] \"a b\" 40px [middle] \"c d\" 1fr [bottom] / 100px 1fr",
    );
    assert_style_set(&mut root, container, "width", "300px");
    let b = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, b, "grid-area", "b");
    assert_style_set(&mut root, b, "height", "40px");
    let middle = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, middle, "grid-row", "middle");
    assert_style_set(&mut root, middle, "height", "40px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let b_box = border_box_of(&root, b);
    assert_close(b_box.left_px, 100.0, "area b lands in column 2");
    assert_close(b_box.top_px, 0.0, "area b in row 1");
    assert_close(b_box.width_px, 200.0, "area b spans column 2 (1fr = 200px)");

    let middle_box = border_box_of(&root, middle);
    assert_close(
        middle_box.top_px,
        40.0,
        "grid-row: middle starts at the middle line",
    );

    // Longhand control produces the identical layout.
    let (mut world2, mut root2, container2) = grid_container_setup();
    assert_style_set(&mut root2, container2, "display", "grid");
    assert_style_set(
        &mut root2,
        container2,
        "grid-template-rows",
        "[top] 40px [middle] 1fr [bottom]",
    );
    assert_style_set(&mut root2, container2, "grid-template-columns", "100px 1fr");
    assert_style_set(
        &mut root2,
        container2,
        "grid-template-areas",
        "\"a b\" \"c d\"",
    );
    assert_style_set(&mut root2, container2, "width", "300px");
    let b2 = assert_append_div_to(&mut root2, container2);
    assert_style_set(&mut root2, b2, "grid-area", "b");
    assert_style_set(&mut root2, b2, "height", "40px");
    let middle2 = assert_append_div_to(&mut root2, container2);
    assert_style_set(&mut root2, middle2, "grid-row", "middle");
    assert_style_set(&mut root2, middle2, "height", "40px");
    assert_root_dom_update(&mut world2, &mut root2, 0.0, DomPointerUpdate::Unchanged);
    let control_b_box = border_box_of(&root2, b2);
    let control_middle_box = border_box_of(&root2, middle2);
    assert_close(
        b_box.left_px,
        control_b_box.left_px,
        "b left matches the longhand control",
    );
    assert_close(
        b_box.width_px,
        control_b_box.width_px,
        "b width matches the longhand control",
    );
    assert_close(
        middle_box.top_px,
        control_middle_box.top_px,
        "middle top matches the longhand control",
    );
}

#[test]
// CSS Grid §7.4 areas form without `/ columns`: `grid-template: "a b" 40px` →
// columns none; the areas map widens to 2 implicit columns; `grid-area: b`
// lands in column 2 (`grid_template_areas_form_without_columns`).
fn grid_template_areas_form_without_columns() {
    let items = |root: &mut RootDom, container: DomNodeSlotKey| -> Vec<DomNodeSlotKey> {
        let a = assert_append_div_to(root, container);
        assert_style_set(root, a, "grid-area", "a");
        assert_style_set(root, a, "height", "40px");
        let b = assert_append_div_to(root, container);
        assert_style_set(root, b, "grid-area", "b");
        assert_style_set(root, b, "height", "40px");
        vec![a, b]
    };
    let shorthand = grid_item_boxes(
        |root, c| assert_style_set(root, c, "grid-template", "\"a b\" 40px"),
        items,
    );
    let longhand = grid_item_boxes(
        |root, c| {
            assert_style_set(root, c, "grid-template-rows", "40px");
            assert_style_set(root, c, "grid-template-columns", "none");
            assert_style_set(root, c, "grid-template-areas", "\"a b\"");
        },
        items,
    );
    assert_boxes_match(&shorthand, &longhand, "grid-template areas without columns");

    // Two implicit auto columns split the 300px width; `b` sits in column 2.
    assert_close(shorthand[0].left_px, 0.0, "a in column 1");
    assert_close(shorthand[1].left_px, 150.0, "b lands in column 2");
    assert_close(
        shorthand[1].width_px,
        150.0,
        "b spans the second implicit column",
    );
}

#[test]
// CSS Grid §7.4 areas form: a missing `<track-size>` defaults to `auto`, so
// `grid-template: "a" 40px "b" / 100px` sizes the second row to content
// (`grid_template_areas_form_default_row_size_auto`).
fn grid_template_areas_form_default_row_size_auto() {
    let items = |root: &mut RootDom, container: DomNodeSlotKey| -> Vec<DomNodeSlotKey> {
        let a = assert_append_div_to(root, container);
        assert_style_set(root, a, "grid-area", "a");
        assert_style_set(root, a, "height", "40px");
        let b = assert_append_div_to(root, container);
        assert_style_set(root, b, "grid-area", "b");
        assert_style_set(root, b, "height", "25px");
        vec![a, b]
    };
    let shorthand = grid_item_boxes(
        |root, c| assert_style_set(root, c, "grid-template", "\"a\" 40px \"b\" / 100px"),
        items,
    );
    let longhand = grid_item_boxes(
        |root, c| {
            assert_style_set(root, c, "grid-template-rows", "40px auto");
            assert_style_set(root, c, "grid-template-columns", "100px");
            assert_style_set(root, c, "grid-template-areas", "\"a\" \"b\"");
        },
        items,
    );
    assert_boxes_match(&shorthand, &longhand, "grid-template default row size");

    // The second (missing → auto) row sizes to the item's 25px content.
    assert_close(shorthand[1].top_px, 40.0, "b below the 40px first row");
    assert_close(shorthand[1].height_px, 25.0, "auto row sizes to content");
}

#[test]
// CSS Grid §7.4 areas form wider than the explicit columns: `grid-template:
// "a b c" 40px / 100px 100px` — the third areas column creates an implicit
// column and `grid-area: c` lands there (`grid_template_areas_wider_than_columns_widens`).
fn grid_template_areas_wider_than_columns_widens() {
    let items = |root: &mut RootDom, container: DomNodeSlotKey| -> Vec<DomNodeSlotKey> {
        let c = assert_append_div_to(root, container);
        assert_style_set(root, c, "grid-area", "c");
        assert_style_set(root, c, "height", "40px");
        vec![c]
    };
    let shorthand = grid_item_boxes(
        |root, c| assert_style_set(root, c, "grid-template", "\"a b c\" 40px / 100px 100px"),
        items,
    );
    let longhand = grid_item_boxes(
        |root, c| {
            assert_style_set(root, c, "grid-template-rows", "40px");
            assert_style_set(root, c, "grid-template-columns", "100px 100px");
            assert_style_set(root, c, "grid-template-areas", "\"a b c\"");
        },
        items,
    );
    assert_boxes_match(
        &shorthand,
        &longhand,
        "grid-template areas wider than columns",
    );

    // The third (implicit) column absorbs the leftover 100px; `c` lands there.
    assert_close(
        shorthand[0].left_px,
        200.0,
        "c in the third (implicit) column",
    );
    assert_close(shorthand[0].width_px, 100.0, "c spans the implicit column");
}

#[test]
// CSS Grid §7.7 column-flow form: `grid: 40px 1fr / auto-flow dense 80px` →
// rows 40/1fr, auto-flow column dense, auto-columns 80px, columns/areas none;
// an overflow item sizes its implicit column to 80px
// (`grid_shorthand_auto_flow_column_form`).
fn grid_shorthand_auto_flow_column_form() {
    let items = |root: &mut RootDom, container: DomNodeSlotKey| -> Vec<DomNodeSlotKey> {
        (0..3)
            .map(|_| {
                let item = assert_append_div_to(root, container);
                assert_style_set(root, item, "height", "40px");
                item
            })
            .collect()
    };
    let shorthand = grid_item_boxes(
        |root, c| assert_style_set(root, c, "grid", "40px 1fr / auto-flow dense 80px"),
        items,
    );
    let longhand = grid_item_boxes(
        |root, c| {
            assert_style_set(root, c, "grid-template-rows", "40px 1fr");
            assert_style_set(root, c, "grid-template-columns", "none");
            assert_style_set(root, c, "grid-template-areas", "none");
            assert_style_set(root, c, "grid-auto-flow", "column dense");
            assert_style_set(root, c, "grid-auto-rows", "auto");
            assert_style_set(root, c, "grid-auto-columns", "80px");
        },
        items,
    );
    assert_boxes_match(&shorthand, &longhand, "grid auto-flow column form");

    // The third item overflows into the second (implicit) column sized 80px.
    assert_close(
        shorthand[2].width_px,
        80.0,
        "overflow item's implicit column is 80px",
    );
}

#[test]
// CSS Cascade: `grid-auto-rows: 99px; grid-template-rows: 5px` then `grid:
// auto-flow 60px / 100px` — the shorthand resets both priors (auto-rows 60,
// rows none) (`grid_shorthand_resets_implicit_props`).
fn grid_shorthand_resets_implicit_props() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "grid-auto-rows", "99px");
    assert_style_set(&mut root, container, "grid-template-rows", "5px");
    assert_style_set(&mut root, container, "grid", "auto-flow 60px / 100px");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridAutoRows),
        Some(&DomAttributeValue::String("60px".to_string())),
        "auto-rows reset to 60px",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateRows),
        Some(&DomAttributeValue::String("none".to_string())),
        "rows reset to none",
    );
}

#[test]
// CSS Cascade §3: `grid-template: 40px / 100px; grid-template-rows: 1fr` in one
// style — the later longhand wins its slot over the shorthand's expansion
// (`grid_shorthand_longhand_override_order`).
fn grid_shorthand_longhand_override_order() {
    let (mut world, mut root, container) = grid_container_setup();
    assert_style_set(&mut root, container, "display", "grid");
    assert_set_attr_str(&mut root, container, "class", "x");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { grid-template: 40px / 100px; grid-template-rows: 1fr }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(container).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateRows),
        Some(&DomAttributeValue::String("1fr".to_string())),
        "the later grid-template-rows longhand overrides the shorthand's rows",
    );
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        Some(&DomAttributeValue::String("100px".to_string())),
        "the shorthand's columns survive",
    );
}
