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

use ui_dom::{DomPointerUpdate, node_box};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_close, assert_node_layout, assert_rect_op_geometry,
    assert_root_dom_update, assert_set_attr_str, container_with_two_parents_setup,
    nested_parent_child_setup, parent_with_two_children_setup, take_staged_layer_paint_plans,
};

#[test]
// CSS 2.1 9.4.3 (relative positioning): a relatively positioned box keeps its
// normal-flow slot; the offset is applied later and does not change siblings.
fn relative_child_stays_in_normal_flow_during_root_update() {
    let (mut world, mut root, container, parent, child) = nested_parent_child_setup();

    assert_set_attr_str(&mut root, container, "width", "80px");
    assert_set_attr_str(&mut root, child, "position", "relative");
    assert_set_attr_str(&mut root, child, "width", "10px");
    assert_set_attr_str(&mut root, child, "height", "12px");
    assert_set_attr_str(&mut root, child, "margin-left", "2px");
    assert_set_attr_str(&mut root, child, "margin-right", "3px");
    assert_set_attr_str(&mut root, child, "margin-top", "4px");
    assert_set_attr_str(&mut root, child, "margin-bottom", "5px");

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

    assert_set_attr_str(&mut root, container, "width", "80px");
    assert_set_attr_str(&mut root, child, "position", "absolute");
    assert_set_attr_str(&mut root, child, "width", "40px");
    assert_set_attr_str(&mut root, child, "height", "12px");

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

    assert_set_attr_str(&mut root, scroll_root, "width", "80px");
    assert_set_attr_str(&mut root, scroll_root, "height", "40px");
    assert_set_attr_str(&mut root, first_item, "width", "80px");
    assert_set_attr_str(&mut root, first_item, "height", "40px");
    assert_set_attr_str(&mut root, second_item, "width", "80px");
    assert_set_attr_str(&mut root, second_item, "height", "40px");

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

    assert_set_attr_str(&mut root, container, "width", "200px");
    assert_set_attr_str(&mut root, parent, "margin-left", "6px");
    assert_set_attr_str(&mut root, parent, "margin-right", "8px");
    assert_set_attr_str(&mut root, parent, "border-left", "2px");
    assert_set_attr_str(&mut root, parent, "border-right", "3px");
    assert_set_attr_str(&mut root, parent, "padding-left", "4px");
    assert_set_attr_str(&mut root, parent, "padding-right", "5px");
    assert_set_attr_str(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
    let child_snapshot = root
        .node(child)
        .expect("child should remain readable after layout")
        .snapshot();

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

    assert_set_attr_str(&mut root, container, "width", "200px");
    assert_set_attr_str(&mut root, parent, "min-width", "150px");
    assert_set_attr_str(&mut root, parent, "max-width", "160px");
    assert_set_attr_str(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
    let child_snapshot = root
        .node(child)
        .expect("child should remain readable after layout")
        .snapshot();

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

    assert_set_attr_str(&mut root, container, "width", "200px");
    assert_set_attr_str(&mut root, parent, "width", "10px");
    assert_set_attr_str(&mut root, parent, "min-width", "20px");
    assert_set_attr_str(&mut root, parent, "max-width", "30px");
    assert_set_attr_str(&mut root, parent, "border-left", "2px");
    assert_set_attr_str(&mut root, parent, "border-right", "3px");
    assert_set_attr_str(&mut root, parent, "padding-left", "4px");
    assert_set_attr_str(&mut root, parent, "padding-right", "5px");
    assert_set_attr_str(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
    let child_snapshot = root
        .node(child)
        .expect("child should remain readable after layout")
        .snapshot();

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

    assert_set_attr_str(&mut root, container, "width", "200px");
    assert_set_attr_str(&mut root, parent, "height", "40px");
    assert_set_attr_str(&mut root, parent, "border-top", "2px");
    assert_set_attr_str(&mut root, parent, "border-bottom", "3px");
    assert_set_attr_str(&mut root, parent, "padding-top", "4px");
    assert_set_attr_str(&mut root, parent, "padding-bottom", "5px");
    assert_set_attr_str(&mut root, child, "height", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
    let child_snapshot = root
        .node(child)
        .expect("child should remain readable after layout")
        .snapshot();

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

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, first_child, "width", "180px");
    assert_set_attr_str(&mut root, first_child, "height", "10px");
    assert_set_attr_str(&mut root, first_child, "margin-bottom", "12px");
    assert_set_attr_str(&mut root, second_child, "width", "180px");
    assert_set_attr_str(&mut root, second_child, "height", "10px");
    assert_set_attr_str(&mut root, second_child, "margin-top", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
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

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, parent, "border-bottom", "1px");
    assert_set_attr_str(&mut root, child, "width", "180px");
    assert_set_attr_str(&mut root, child, "height", "10px");
    assert_set_attr_str(&mut root, child, "margin-top", "20px");
    assert_set_attr_str(&mut root, child, "margin-bottom", "5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
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

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, parent, "border-top", "1px");
    assert_set_attr_str(&mut root, child, "width", "180px");
    assert_set_attr_str(&mut root, child, "height", "10px");
    assert_set_attr_str(&mut root, child, "margin-top", "5px");
    assert_set_attr_str(&mut root, child, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();
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

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, parent, "border-top", "1px");
    assert_set_attr_str(&mut root, parent, "border-bottom", "1px");
    assert_set_attr_str(&mut root, empty_child, "width", "180px");
    assert_set_attr_str(&mut root, empty_child, "margin-top", "10px");
    assert_set_attr_str(&mut root, empty_child, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let empty_child_snapshot = root
        .node(empty_child)
        .expect("empty child should remain readable after layout")
        .snapshot();
    let parent_snapshot = root
        .node(parent)
        .expect("parent should remain readable after layout")
        .snapshot();

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

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, outer_parent, "width", "180px");
    assert_set_attr_str(&mut root, outer_parent, "border-top", "1px");
    assert_set_attr_str(&mut root, outer_parent, "border-bottom", "1px");
    assert_set_attr_str(&mut root, empty_middle, "width", "180px");
    assert_set_attr_str(&mut root, grandchild, "width", "180px");
    assert_set_attr_str(&mut root, grandchild, "height", "10px");
    assert_set_attr_str(&mut root, grandchild, "margin-top", "10px");
    assert_set_attr_str(&mut root, grandchild, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let outer_snapshot = root
        .node(outer_parent)
        .expect("outer parent should remain readable after layout")
        .snapshot();
    let middle_snapshot = root
        .node(empty_middle)
        .expect("empty middle should remain readable after layout")
        .snapshot();
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

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, outer_parent, "width", "180px");
    assert_set_attr_str(&mut root, outer_parent, "border-top", "1px");
    assert_set_attr_str(&mut root, outer_parent, "border-bottom", "1px");
    assert_set_attr_str(&mut root, empty_middle, "width", "180px");
    assert_set_attr_str(&mut root, empty_middle, "margin-top", "14px");
    assert_set_attr_str(&mut root, empty_middle, "margin-bottom", "7px");
    assert_set_attr_str(&mut root, grandchild, "width", "180px");
    assert_set_attr_str(&mut root, grandchild, "height", "10px");
    assert_set_attr_str(&mut root, grandchild, "margin-top", "10px");
    assert_set_attr_str(&mut root, grandchild, "margin-bottom", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let middle_snapshot = root
        .node(empty_middle)
        .expect("empty middle should remain readable after layout")
        .snapshot();
    let outer_snapshot = root
        .node(outer_parent)
        .expect("outer parent should remain readable after layout")
        .snapshot();

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
// OFF-SPEC (divergent): CSS 2.1 §8.3.1 collapsing margins, final paragraph —
// <https://www.w3.org/TR/CSS21/box.html#collapsing-margins>
// Closest WPT: the CSS21 margin-collapse suite
// (css/CSS2/normal-flow/margin-collapse-*.xht).
// Gecko: nsBlockFrame.cpp margin-collapse path.
// Blink: third_party/blink/renderer/core/layout/ng/ng_margin_collapse.cc.
//
// CURRENT (asserted below): the adjoining -10px bottom and -5px top margins
// SUM to -15px, so `lower`'s border box lands at top -4px and the parent
// border-box height is 7px.
// SPEC-CORRECT: with no positive adjoining margins, the collapsed margin is
// the most negative adjoining margin (-10px): `lower` at top 1px, parent
// border-box height 12px.
// ROOT CAUSE: collapsed_sibling_margin_overlap_px in
// src/domains/layout/flow.rs clamps the collapse overlap to >= 0, so signed
// negative-margin collapse is not admitted.
// RECLASSIFY: if this proof fails, the divergence moved — see the off-spec
// registry in tests/layout_proofs/README.md.
fn off_spec_adjacent_negative_block_margins_collapse_to_most_negative() {
    let (mut world, mut root) =
        crate::layout_proofs::support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let upper = assert_append_div_to(&mut root, parent);
    let lower = assert_append_div_to(&mut root, parent);

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, parent, "border-top", "1px");
    assert_set_attr_str(&mut root, parent, "border-bottom", "1px");
    assert_set_attr_str(&mut root, upper, "width", "180px");
    assert_set_attr_str(&mut root, upper, "height", "10px");
    assert_set_attr_str(&mut root, upper, "margin-bottom", "-10px");
    assert_set_attr_str(&mut root, lower, "width", "180px");
    assert_set_attr_str(&mut root, lower, "height", "10px");
    assert_set_attr_str(&mut root, lower, "margin-top", "-5px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    let rects = &plans[0].rects;
    // rects[0] = parent, rects[1] = upper, rects[2] = lower. Off-spec pinning:
    // the summed -15px margin places `lower`'s border box at top -4px past
    // `upper`'s top edge and leaves the parent content height at 5px
    // (border box 7px). Spec-correct values are named in the OFF-SPEC header.
    assert_rect_op_geometry(&rects[1], 0.0, 1.0, 180.0, 10.0, "upper block");
    assert_rect_op_geometry(
        &rects[2],
        0.0,
        -4.0,
        180.0,
        10.0,
        "lower block after off-spec summed negative margins",
    );
    assert_rect_op_geometry(&rects[0], 0.0, 0.0, 180.0, 7.0, "parent border box");
}
