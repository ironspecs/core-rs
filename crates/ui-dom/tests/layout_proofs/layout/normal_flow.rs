#![allow(rustdoc::bare_urls)]

//! Normal flow layout proofs.
//!
//! Spec references:
//! - CSS 2.1 visual formatting model, normal flow:
//!   <https://www.w3.org/TR/CSS21/visuren.html#normal-flow>
//! - CSS 2.1 positioning schemes:
//!   <https://www.w3.org/TR/CSS21/visuren.html#positioning-scheme>
//! - CSS 2.1 heights of block formatting context roots and normal blocks:
//!   <https://www.w3.org/TR/CSS21/visudet.html#normal-block>
//! - CSS 2.1 10.3.3 block-level, non-replaced elements in normal flow:
//!   <https://www.w3.org/TR/CSS21/visudet.html#blockwidth>
//!
//! Browser implementation references for the normal-flow block contract pinned
//! here:
//! - Gecko:
//!   `layout/generic/nsBlockFrame.cpp`,
//!   `layout/generic/ReflowInput.cpp`
//! - Blink:
//!   `third_party/blink/renderer/core/layout/block_layout_algorithm.cc`,
//!   `third_party/blink/renderer/core/layout/ng/ng_constraint_space_builder.cc`

use ui_dom::{DomLayerKind, DomPointerUpdate, node_box};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_close, assert_node_layout, assert_rect_op_geometry,
    assert_root_dom_update, assert_style_set, container_with_two_parents_setup,
    nested_parent_child_setup, parent_with_two_children_setup, take_staged_layer_paint_plans,
};

#[test]
// CSS 2.1 9.4.3 (relative positioning): a relatively positioned box keeps its
// normal-flow slot; the offset is applied later and does not change siblings.
fn relative_child_stays_in_normal_flow_during_root_update() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "80px");
    assert_style_set(&mut root, child, "position", "relative");
    assert_style_set(&mut root, child, "width", "10px");
    assert_style_set(&mut root, child, "height", "12px");
    assert_style_set(&mut root, child, "margin-left", "2px");
    assert_style_set(&mut root, child, "margin-right", "3px");
    assert_style_set(&mut root, child, "margin-top", "4px");
    assert_style_set(&mut root, child, "margin-bottom", "5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let child_layout = assert_node_layout(&root, child);

    assert_close(
        child_layout.height_px,
        12.0,
        "relative child border-box height",
    );
    assert_close(parent_layout.height_px, 21.0, "parent auto height");
    assert_close(
        parent_layout.content_height_px,
        21.0,
        "parent content height",
    );
}

#[test]
// CSS 2.1 9.3.1: absolutely positioned boxes are removed from normal flow.
fn absolute_child_does_not_extend_parent_auto_height_during_root_update() {
    let (mut world, mut root, container, empty_parent, parent_with_absolute_child) =
        container_with_two_parents_setup();
    let child = assert_append_div_to(&mut root, parent_with_absolute_child);

    assert_style_set(&mut root, container, "width", "80px");
    assert_style_set(&mut root, child, "position", "absolute");
    assert_style_set(&mut root, child, "width", "40px");
    assert_style_set(&mut root, child, "height", "12px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let empty_parent_layout = assert_node_layout(&root, empty_parent);
    let parent_layout = assert_node_layout(&root, parent_with_absolute_child);
    let child_layout = assert_node_layout(&root, child);

    assert_close(child_layout.height_px, 12.0, "absolute child height");
    assert_close(
        parent_layout.height_px,
        empty_parent_layout.height_px,
        "absolute child parent height",
    );
    assert_close(
        parent_layout.content_height_px,
        empty_parent_layout.content_height_px,
        "absolute child parent content height",
    );
}

#[test]
// CSS 2.1 10.6.3 plus ui-dom invariant: used height may clamp while
// retained content height still tracks child content for scroll bounds.
fn explicit_height_keeps_larger_child_content_extent_for_scroll_bounds() {
    let (mut world, mut root, scroll_root, first_item, second_item) =
        parent_with_two_children_setup();

    assert_style_set(&mut root, scroll_root, "width", "80px");
    assert_style_set(&mut root, scroll_root, "height", "40px");
    assert_style_set(&mut root, first_item, "width", "80px");
    assert_style_set(&mut root, first_item, "height", "40px");
    assert_style_set(&mut root, second_item, "width", "80px");
    assert_style_set(&mut root, second_item, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let scroll_layout = assert_node_layout(&root, scroll_root);

    assert_close(scroll_layout.height_px, 40.0, "scroll root viewport height");
    assert_close(
        scroll_layout.content_height_px,
        80.0,
        "scroll root retained content height",
    );
}

#[test]
// CSS 2.1 10.3.3: a block-level non-replaced auto-width box uses the
// containing block width minus its horizontal non-content edges, and its block
// child receives that resolved content width as available width.
fn block_auto_width_uses_containing_block_width_minus_horizontal_edges() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, parent, "margin-left", "6px");
    assert_style_set(&mut root, parent, "margin-right", "8px");
    assert_style_set(&mut root, parent, "border-left", "2px");
    assert_style_set(&mut root, parent, "border-right", "3px");
    assert_style_set(&mut root, parent, "padding-left", "4px");
    assert_style_set(&mut root, parent, "padding-right", "5px");
    assert_style_set(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let child_snapshot = root
        .canonical_snapshot(child)
        .expect("child should remain readable after layout");

    assert_close(
        parent_snapshot.layout.available_width_px,
        200.0,
        "parent available width from containing block",
    );
    assert_close(
        parent_snapshot.layout.used_width_px,
        186.0,
        "parent used border-box width",
    );
    assert_close(
        child_snapshot.layout.available_width_px,
        172.0,
        "child available width from parent content box",
    );
    assert_close(
        child_snapshot.layout.used_width_px,
        172.0,
        "child used width fills parent content box",
    );
}

#[test]
// CSS 2.1 10.4 plus 10.3.3: min-width and max-width clamp the used width of a
// non-replaced block in normal flow after auto-width resolution, and the child
// receives the clamped parent content width as its available width.
fn block_auto_width_respects_min_and_max_width_clamps() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, parent, "min-width", "150px");
    assert_style_set(&mut root, parent, "max-width", "160px");
    assert_style_set(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let child_snapshot = root
        .canonical_snapshot(child)
        .expect("child should remain readable after layout");

    assert_close(
        parent_snapshot.layout.available_width_px,
        200.0,
        "parent available width before clamping",
    );
    assert_close(
        parent_snapshot.layout.used_width_px,
        160.0,
        "parent used width clamped by max-width",
    );
    assert_close(
        child_snapshot.layout.available_width_px,
        160.0,
        "child available width from clamped parent content box",
    );
    assert_close(
        child_snapshot.layout.used_width_px,
        160.0,
        "child used width fills clamped parent content box",
    );
}

#[test]
// CSS 2.1 10.4 plus 10.3.3: width/min-width/max-width clamp the content box,
// and only then do border and padding expand the used border-box width seen by
// descendants and paint.
fn explicit_width_clamps_content_box_before_border_and_padding() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, parent, "width", "10px");
    assert_style_set(&mut root, parent, "min-width", "20px");
    assert_style_set(&mut root, parent, "max-width", "30px");
    assert_style_set(&mut root, parent, "border-left", "2px");
    assert_style_set(&mut root, parent, "border-right", "3px");
    assert_style_set(&mut root, parent, "padding-left", "4px");
    assert_style_set(&mut root, parent, "padding-right", "5px");
    assert_style_set(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let child_snapshot = root
        .canonical_snapshot(child)
        .expect("child should remain readable after layout");

    assert_close(
        parent_snapshot.layout.used_width_px,
        34.0,
        "parent used border-box width from clamped content plus horizontal edges",
    );
    assert_close(
        child_snapshot.layout.available_width_px,
        20.0,
        "child available width from clamped parent content box",
    );
    assert_close(
        child_snapshot.layout.used_width_px,
        20.0,
        "child used width fills clamped parent content box",
    );
}

#[test]
// CSS 2.1 10.6.3 plus the Gecko/Blink block reflow contract: when a normal
// flow block has a definite content-box height, its child block should inherit
// that content-box height as available height, not the ancestor viewport
// height.
fn child_available_height_uses_parent_content_box_height() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "200px");
    assert_style_set(&mut root, parent, "height", "40px");
    assert_style_set(&mut root, parent, "border-top", "2px");
    assert_style_set(&mut root, parent, "border-bottom", "3px");
    assert_style_set(&mut root, parent, "padding-top", "4px");
    assert_style_set(&mut root, parent, "padding-bottom", "5px");
    assert_style_set(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let child_snapshot = root
        .canonical_snapshot(child)
        .expect("child should remain readable after layout");

    assert_close(
        parent_snapshot.layout.used_height_px,
        54.0,
        "parent used border-box height from content plus vertical edges",
    );
    assert_close(
        child_snapshot.layout.available_height_px,
        40.0,
        "child available height from parent content box",
    );
    assert_close(
        child_snapshot.layout.used_height_px,
        10.0,
        "child used height remains its authored block height",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: adjacent in-flow block margins
// collapse, so one previous block's bottom margin and the next block's top
// margin contribute only their maximum vertical separation.
fn adjacent_block_margins_collapse_between_normal_flow_siblings() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let first_child = assert_append_div_to(&mut root, parent);
    let second_child = assert_append_div_to(&mut root, parent);

    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, first_child, "width", "180px");
    assert_style_set(&mut root, first_child, "height", "10px");
    assert_style_set(&mut root, first_child, "margin-bottom", "12px");
    assert_style_set(&mut root, second_child, "width", "180px");
    assert_style_set(&mut root, second_child, "height", "10px");
    assert_style_set(&mut root, second_child, "margin-top", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;

    // rects[0] = parent, rects[1] = first child, rects[2] = second child.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 180.0, 10.0, "first block child");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        30.0,
        180.0,
        10.0,
        "second block child after sibling margin collapse",
    );
    assert_close(
        parent_snapshot.layout.used_height_px,
        40.0,
        "parent auto height after collapsed sibling margins",
    );
    assert_close(
        parent_layout.content_height_px,
        40.0,
        "parent retained content height after collapsed sibling margins",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: a parent's top boundary and its
// first in-flow block child's top margin collapse when the parent has no top
// border or padding. A bottom border prevents the separate last-child bottom
// collapse from changing this proof's expected height.
fn first_block_child_top_margin_collapses_with_parent_top_boundary() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, child, "width", "180px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "margin-top", "20px");
    assert_style_set(&mut root, child, "margin-bottom", "5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;

    // rects[0] = container, rects[1] = parent (both 180x16 at the origin;
    // DFS pre-order keeps container first), rects[2] = child.
    assert_rect_op_geometry(&rects[1], 0.0, 0.0, 180.0, 16.0, "parent border box");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        0.0,
        180.0,
        10.0,
        "first block child after parent-child top margin collapse",
    );
    assert_close(
        parent_snapshot.layout.used_height_px,
        16.0,
        "parent border-box height after collapsed top margin",
    );
    assert_close(
        parent_layout.content_height_px,
        15.0,
        "parent retained content height after collapsed top margin",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: an auto-height parent's bottom
// boundary and its last in-flow block child's bottom margin collapse when the
// parent has no bottom border or padding. A top border prevents the separate
// first-child top collapse from changing this proof's expected height.
fn last_block_child_bottom_margin_collapses_with_parent_bottom_boundary() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, child, "width", "180px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "margin-top", "5px");
    assert_style_set(&mut root, child, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;

    // rects[0] = container, rects[1] = parent, rects[2] = child.
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        6.0,
        180.0,
        10.0,
        "last block child when parent top border prevents top collapse",
    );
    assert_close(
        parent_snapshot.layout.used_height_px,
        16.0,
        "parent border-box height after collapsed bottom margin",
    );
    assert_close(
        parent_layout.content_height_px,
        15.0,
        "parent retained content height after collapsed bottom margin",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: an empty in-flow block with no
// border, padding, inline content, height, or min-height collapses its own top
// and bottom margins together, contributing only their maximum vertical
// separation to normal flow.
fn empty_block_self_collapses_top_and_bottom_margins() {
    let (mut world, mut root, container, parent, empty_child) = nested_parent_child_setup();

    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, empty_child, "width", "180px");
    assert_style_set(&mut root, empty_child, "margin-top", "10px");
    assert_style_set(&mut root, empty_child, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let empty_child_snapshot = root
        .canonical_snapshot(empty_child)
        .expect("empty child should remain readable after layout");
    let parent_snapshot = root
        .canonical_snapshot(parent)
        .expect("parent should remain readable after layout");

    assert_close(
        node_box(&root, empty_child)
            .expect("empty child should expose a box model")
            .border_box
            .top_px,
        11.0,
        "empty block top when parent borders prevent parent-child collapse",
    );
    assert_close(
        empty_child_snapshot.layout.used_height_px,
        0.0,
        "empty block border-box height after self-collapsing margins",
    );
    assert_close(
        parent_snapshot.layout.used_height_px,
        22.0,
        "parent border-box height after empty block self-collapse",
    );
    assert_close(
        parent_layout.content_height_px,
        20.0,
        "parent retained content height after empty block self-collapse",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: when an intermediate block has
// no border, padding, inline content, height, min-height, or own margins, its
// first and last child boundary margins must propagate upward without being
// counted again at the intermediate generation.
fn empty_intermediate_block_propagates_collapsed_margins_without_double_counting() {
    let (mut world, mut root, container, outer_parent, empty_middle) = nested_parent_child_setup();
    let grandchild = assert_append_div_to(&mut root, empty_middle);

    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, outer_parent, "width", "180px");
    assert_style_set(&mut root, outer_parent, "border-top", "1px");
    assert_style_set(&mut root, outer_parent, "border-bottom", "1px");
    assert_style_set(&mut root, empty_middle, "width", "180px");
    assert_style_set(&mut root, grandchild, "width", "180px");
    assert_style_set(&mut root, grandchild, "height", "10px");
    assert_style_set(&mut root, grandchild, "margin-top", "10px");
    assert_style_set(&mut root, grandchild, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let outer_snapshot = root
        .canonical_snapshot(outer_parent)
        .expect("outer parent should remain readable after layout");
    let middle_snapshot = root
        .canonical_snapshot(empty_middle)
        .expect("empty middle should remain readable after layout");
    assert_close(
        middle_snapshot.layout.used_height_px,
        10.0,
        "empty middle keeps only the child border-box height as its used height",
    );
    assert_close(
        outer_snapshot.layout.used_height_px,
        42.0,
        "outer parent counts the propagated child margins only once across the empty middle",
    );
}

#[test]
// CSS 2.1 8.3.1 and Gecko/Blink block reflow: when an empty intermediary has
// its own margins, the propagated boundary margins collapse by `max` with the
// descendant boundary margins instead of being added on top.
fn empty_intermediate_block_collapses_own_margins_with_descendant_boundaries_by_max() {
    let (mut world, mut root, container, outer_parent, empty_middle) = nested_parent_child_setup();
    let grandchild = assert_append_div_to(&mut root, empty_middle);

    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, outer_parent, "width", "180px");
    assert_style_set(&mut root, outer_parent, "border-top", "1px");
    assert_style_set(&mut root, outer_parent, "border-bottom", "1px");
    assert_style_set(&mut root, empty_middle, "width", "180px");
    assert_style_set(&mut root, empty_middle, "margin-top", "14px");
    assert_style_set(&mut root, empty_middle, "margin-bottom", "7px");
    assert_style_set(&mut root, grandchild, "width", "180px");
    assert_style_set(&mut root, grandchild, "height", "10px");
    assert_style_set(&mut root, grandchild, "margin-top", "10px");
    assert_style_set(&mut root, grandchild, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let middle_snapshot = root
        .canonical_snapshot(empty_middle)
        .expect("empty middle should remain readable after layout");
    let outer_snapshot = root
        .canonical_snapshot(outer_parent)
        .expect("outer parent should remain readable after layout");

    assert_close(
        middle_snapshot.layout.used_height_px,
        10.0,
        "empty middle keeps only the child border-box height as its used height",
    );
    assert_close(
        outer_snapshot.layout.used_height_px,
        46.0,
        "outer parent uses max-collapsed propagated boundaries instead of summing both generations",
    );
}

#[test]
// CSS 2.1 §8.3.1 collapsing margins, final paragraph —
// <https://www.w3.org/TR/CSS21/box.html#collapsing-margins>
// Closest WPT: the CSS21 margin-collapse suite
// (css/CSS2/normal-flow/margin-collapse-*.xht).
// Gecko: nsBlockFrame.cpp margin-collapse path.
// Blink: third_party/blink/renderer/core/layout/ng/ng_margin_collapse.cc.
//
// Adjoining negative margins collapse to the most negative: the -10px bottom
// margin and -5px top margin collapse to -10px, so `lower`'s border box lands
// at top 1px (overlapping `upper`) and the parent border-box height is 12px.
//
// RELABEL: relabeled from
// off_spec_adjacent_negative_block_margins_collapse_to_most_negative in PR 1
// item 2 after the three-branch used-margin rule landed in
// collapsed_sibling_margin_overlap_px (src/domains/layout/flow.rs).
fn adjacent_negative_block_margins_collapse_to_most_negative() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let upper = assert_append_div_to(&mut root, parent);
    let lower = assert_append_div_to(&mut root, parent);

    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, upper, "width", "180px");
    assert_style_set(&mut root, upper, "height", "10px");
    assert_style_set(&mut root, upper, "margin-bottom", "-10px");
    assert_style_set(&mut root, lower, "width", "180px");
    assert_style_set(&mut root, lower, "height", "10px");
    assert_style_set(&mut root, lower, "margin-top", "-5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = parent, rects[1] = upper, rects[2] = lower. The -10px and
    // -5px adjoining margins collapse to the most negative (-10px), so
    // `lower` overlaps `upper` (both border boxes at top 1px) and the parent
    // border box is 12px tall.
    assert_rect_op_geometry(&rects[1], 0.0, 1.0, 180.0, 10.0, "upper block");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        1.0,
        180.0,
        10.0,
        "lower block after negative margins collapse to the most negative",
    );
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 180.0, 12.0, "parent border box");
}

#[test]
// CSS 2.1 §8.3.1 collapsing margins: one positive and one negative adjoining
// margin SUM to their used gap (no min/max branch), matching the engines'
// mixed-sign branch in nsBlockFrame.cpp / ng_margin_collapse.cc. The 10px
// bottom margin and -5px top margin sum to 5px, so `lower`'s border box lands
// at top 16px and the parent border-box height is 27px. This guards the
// mixed-sign branch of the three-branch used-margin rule, which was
// spec-correct before the PR 1 item 2 change and is pinned so it cannot regress.
fn mixed_sign_adjacent_block_margins_sum_to_used_gap() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let upper = assert_append_div_to(&mut root, parent);
    let lower = assert_append_div_to(&mut root, parent);

    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, upper, "width", "180px");
    assert_style_set(&mut root, upper, "height", "10px");
    assert_style_set(&mut root, upper, "margin-bottom", "10px");
    assert_style_set(&mut root, lower, "width", "180px");
    assert_style_set(&mut root, lower, "height", "10px");
    assert_style_set(&mut root, lower, "margin-top", "-5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = parent, rects[1] = upper, rects[2] = lower. The 10px and
    // -5px adjoining margins sum to a 5px used gap, so `lower` sits 5px below
    // `upper`'s bottom edge (top 16px) and the parent border box is 27px tall.
    assert_rect_op_geometry(&rects[1], 0.0, 1.0, 180.0, 10.0, "upper block");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        16.0,
        180.0,
        10.0,
        "lower block after mixed-sign margins sum",
    );
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 180.0, 27.0, "parent border box");
}

#[test]
// DELIBERATE (the 2026-10-18 layer-spillover decision, PLAN.md decision log):
// a clipped layer's bounds clip content that spills outside them — negative
// margins, `overflow: visible` children, transforms — where a browser would
// paint it. Rationale: bounded layer surfaces ARE the render cache (scroll,
// animation, targeted repaint); surface expansion defeats the cache geometry
// and the damage-rect model. The escape hatch is an unclipped (Normal) layer —
// see `normal_layer_paints_spillover_escape_hatch`.
//
// Split pin: this proof guards the integration-visible half — the container is
// classified as a Clipped layer (`layer_kind_for_node`), its border box is the
// bounded surface (100×100), and the negative-margin child's rect is staged
// ABOVE that bound (top −20px). The actual clip is the composite stage's
// `clip_world` (the layer's border box for Clipped layers), which is pub(crate)
// and pinned in `src/domains/composite/mod.rs::
// nested_clipped_layer_clip_world_uses_layer_tree_world_offset`. The staged
// paint plan is NOT itself clipped: it carries the spilled rect at −20px, the
// same as the Normal-layer escape hatch below — the clip is applied at draw
// time, never by the paint plan.
fn layer_bounds_clip_spillover_by_design() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let clipped = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, clipped);

    assert_style_set(&mut root, clipped, "width", "100px");
    assert_style_set(&mut root, clipped, "height", "100px");
    assert_style_set(&mut root, clipped, "overflow", "hidden");
    assert_style_set(&mut root, child, "width", "100px");
    assert_style_set(&mut root, child, "height", "20px");
    assert_style_set(&mut root, child, "margin-top", "-20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // The container is a Clipped layer: overflow:hidden arms the composite
    // clip_world (border box), which is what trims the spill below.
    assert_eq!(
        assert_node_layout(&root, clipped).layer_kind,
        DomLayerKind::Clipped,
        "overflow:hidden container should classify as a Clipped layer",
    );
    let clipped_box = node_box(&root, clipped).expect("clipped layer should expose a box");
    assert_close(clipped_box.border_box.top_px, 0.0, "clipped layer top");
    assert_close(
        clipped_box.border_box.width_px,
        100.0,
        "clipped layer border-box width (the bounded surface)",
    );
    assert_close(
        clipped_box.border_box.height_px,
        100.0,
        "clipped layer border-box height (the bounded surface)",
    );

    // The staged paint plan carries the child's border box at its negative
    // offset (top −20px), above the layer's top bound; the composite clip_world
    // trims that spilled region at draw time.
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 100.0, 100.0, "clipped layer container");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        -20.0,
        100.0,
        20.0,
        "negative-margin child staged above the clipped layer bounds",
    );
}

#[test]
// CSS 2.1 §9.4.3 plus the layer-spillover escape hatch: an unclipped (Normal)
// layer paints content that spills outside its border box. A `position:
// relative` container is a Normal layer (`node_has_unclipped_layer`), so its
// −20px-top child's border box is staged at its negative offset and paints —
// the deliberate clipped-layer divergence (see `layer_bounds_clip_spillover_by_design`)
// does NOT apply here. This is on-spec: a browser paints the spill.
fn normal_layer_paints_spillover_escape_hatch() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let normal = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, normal);

    assert_style_set(&mut root, normal, "position", "relative");
    assert_style_set(&mut root, normal, "width", "100px");
    assert_style_set(&mut root, normal, "height", "100px");
    assert_style_set(&mut root, child, "width", "100px");
    assert_style_set(&mut root, child, "height", "20px");
    assert_style_set(&mut root, child, "margin-top", "-20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        assert_node_layout(&root, normal).layer_kind,
        DomLayerKind::Normal,
        "relative container should classify as a Normal (unclipped) layer",
    );

    // The child's rect op exists at its negative-offset position: the spill
    // paints through the unclipped escape hatch.
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 100.0, 100.0, "normal layer container");
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        -20.0,
        100.0,
        20.0,
        "negative-margin child paints at its negative offset",
    );
}

#[test]
// CSS 2.1 §8.3.1 (collapsing margins) — OFF-SPEC divergent pin. Closest WPT:
// the CSS21 margin-collapse suite (css/CSS2/normal-flow/margin-collapse-*.xht);
// no single WPT covers the parent-child NEGATIVE-margin boundary shape, so this
// pins the exact current divergence against the spec's three-branch rule.
// Gecko: layout/generic/nsBlockFrame.cpp margin-collapse path. Blink:
// third_party/blink/renderer/core/layout/ng/ng_margin_collapse.cc /
// third_party/blink/renderer/core/layout/block_layout_algorithm.cc.
//
// SPEC: a first in-flow child's top margin (last in-flow child's bottom margin)
// adjoins the parent's and collapses by the SAME three-branch rule as siblings
// — a negative child margin passes through the boundary (parent 0 + child −20
// → −20), so the child pulls the parent's boundary up by the collapsed value.
//
// CURRENT (pinned): `collapsed_parent_top_margin_px` /
// `collapsed_parent_bottom_margin_px` (src/domains/layout/flow.rs) clamp the
// child margin with `.max(0.0)`, and
// `propagated_empty_intermediary_block_margin_height_px` clamps the
// intermediary's own and child boundary margins the same way — a negative
// margin is treated as 0 and the boundary does not move.
//
// Spec-correct per row: (a) parent top −20 (child flush at −20); (b) parent
// border-box height 11 (content 10); (c) outer border-box height 11 (content
// 10). The proof asserts the CURRENT values, so it fails if behavior moves in
// EITHER direction.
fn off_spec_parent_boundary_negative_margins_clamp() {
    // (a) first child negative top margin: the adjoined pair should collapse to
    // the negative value and pull the parent's top boundary up by 20 — instead
    // the clamp keeps the parent at top 0 and the child overflows above it.
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();
    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "height", "40px");
    assert_style_set(&mut root, child, "width", "180px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "margin-top", "-20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = parent, rects[2] = child.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        180.0,
        40.0,
        "(a) parent stays at top 0",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        -20.0,
        180.0,
        10.0,
        "(a) child overflows above the un-moved parent boundary",
    );

    // (b) last child negative bottom margin: the adjoined pair should collapse
    // to −20 and pull the parent's bottom boundary up (auto height 10 + border
    // 1 = 11) — instead the clamp keeps the parent height at just its border.
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();
    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, child, "width", "180px");
    assert_style_set(&mut root, child, "height", "10px");
    assert_style_set(&mut root, child, "margin-bottom", "-20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = parent, rects[2] = child.
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        180.0,
        1.0,
        "(b) parent collapses to its border",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        1.0,
        180.0,
        10.0,
        "(b) child overflows below the un-moved parent bottom boundary",
    );
    assert_close(
        parent_layout.height_px,
        1.0,
        "(b) parent border-box height after the bottom-boundary clamp",
    );

    // (c) negative margin propagated through an empty intermediary: the
    // grandchild's −20 bottom margin should propagate through the empty middle
    // to the outer's bottom boundary (outer auto height 10 + border 1 = 11) —
    // instead `propagated_empty_intermediary_block_margin_height_px` clamps the
    // propagated bottom margin with `.max(0.0)`.
    let (mut world, mut root, container, outer, middle) = nested_parent_child_setup();
    let grandchild = assert_append_div_to(&mut root, middle);
    assert_style_set(&mut root, container, "width", "180px");
    assert_style_set(&mut root, outer, "width", "180px");
    assert_style_set(&mut root, outer, "border-top", "1px");
    assert_style_set(&mut root, middle, "width", "180px");
    assert_style_set(&mut root, grandchild, "width", "180px");
    assert_style_set(&mut root, grandchild, "height", "10px");
    assert_style_set(&mut root, grandchild, "margin-bottom", "-20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let outer_layout = assert_node_layout(&root, outer);
    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = container, rects[1] = outer, rects[2] = grandchild (the empty
    // middle is zero-height and emits no rect).
    assert_rect_op_geometry(
        &rects[1],
        0.0,
        0.0,
        180.0,
        1.0,
        "(c) outer collapses to its border",
    );
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        1.0,
        180.0,
        10.0,
        "(c) grandchild overflows below the un-moved outer boundary",
    );
    assert_close(
        outer_layout.height_px,
        1.0,
        "(c) outer border-box height after the empty-intermediary clamp",
    );
}

#[test]
// CSS 2.1 §8.3.1 (collapsing margins) — OFF-SPEC divergent pin. Closest WPT:
// the CSS21 margin-collapse suite (css/CSS2/normal-flow/margin-collapse-*.xht);
// no single WPT covers an empty block whose own margins are BOTH negative, so
// this pins the exact current divergence against the spec's three-branch rule.
// Gecko: layout/generic/nsBlockFrame.cpp margin-collapse path. Blink:
// third_party/blink/renderer/core/layout/ng/ng_margin_collapse.cc.
//
// SPEC: an empty block's own adjoining top and bottom margins collapse by the
// same three-branch rule — two negative margins collapse to the MOST NEGATIVE
// (−10, −5 → −10), and a mixed pair sums (10, −5 → 5).
//
// CURRENT (pinned): the empty-block self-collapse in `layout_flow_size`
// (src/domains/layout/flow.rs) predates PR 1 item 2's three-branch rule and
// uses plain `max` — (−10, −5 → −5) and (10, −5 → 10).
//
// The proof asserts the CURRENT values, so it fails if behavior moves in EITHER
// direction.
fn off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative() {
    // (both_negative) empty block `margin: -10px 0 -5px`: current `max` flow
    // height −5 pulls the following sibling up by 5 (top −4) and leaves the
    // parent content at 5 (border box 7); spec −10 would pull it to −9 and
    // leave content 0 (border box 2).
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let empty = assert_append_div_to(&mut root, parent);
    let after = assert_append_div_to(&mut root, parent);

    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, empty, "width", "180px");
    assert_style_set(&mut root, empty, "margin-top", "-10px");
    assert_style_set(&mut root, empty, "margin-bottom", "-5px");
    assert_style_set(&mut root, after, "width", "180px");
    assert_style_set(&mut root, after, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let after_box = node_box(&root, after).expect("after block should expose a box model");
    assert_close(
        parent_layout.height_px,
        7.0,
        "(both_negative) parent border-box height after `max` self-collapse",
    );
    assert_close(
        after_box.border_box.top_px,
        -4.0,
        "(both_negative) following sibling pulled up by the −5 `max` flow height",
    );

    // (mixed_sign_control) empty block `margin: 10px 0 -5px`: current `max`
    // flow height 10 (parent content 10, border box 12); spec mixed-sign sum 5
    // (content 5, border box 7).
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let empty = assert_append_div_to(&mut root, parent);

    assert_style_set(&mut root, parent, "width", "180px");
    assert_style_set(&mut root, parent, "border-top", "1px");
    assert_style_set(&mut root, parent, "border-bottom", "1px");
    assert_style_set(&mut root, empty, "width", "180px");
    assert_style_set(&mut root, empty, "margin-top", "10px");
    assert_style_set(&mut root, empty, "margin-bottom", "-5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    assert_close(
        parent_layout.height_px,
        12.0,
        "(mixed_sign_control) parent border-box height after `max` self-collapse",
    );
}
