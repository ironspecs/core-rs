#![allow(rustdoc::bare_urls)]

//! Content-height proofs for normal flow.
//!
//! Spec references:
//! - CSS 2.1 9.4.1 normal flow:
//!   <https://www.w3.org/TR/CSS21/visuren.html#normal-flow>
//! - CSS 2.1 10.6.3 block formatting context height calculation:
//!   <https://www.w3.org/TR/CSS21/visudet.html#normal-block>

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_append_div_to, assert_close, assert_load_root_stylesheet, assert_node_layout,
    assert_root_dom_update, assert_set_attr_str, container_with_two_parents_setup,
    nested_parent_child_setup, parent_with_two_children_setup, theme_stylesheet_origin,
};

const RELATIVE_OFFSET_STYLESHEET: &str = "
    .container { width: 80px; }
    .relative-baseline { position: relative; height: 12px; }
    .relative-offset {
        position: relative;
        height: 12px;
        top: 40px;
        left: 15px;
    }
";

#[test]
// CSS 2.1 10.6.3: in-flow block children contribute to parent auto height.
fn auto_height_sums_in_flow_block_children_during_root_update() {
    let (mut world, mut root, parent, first_child, second_child) = parent_with_two_children_setup();

    assert_set_attr_str(&mut root, parent, "width", "80px");
    assert_set_attr_str(&mut root, first_child, "height", "30px");
    assert_set_attr_str(&mut root, second_child, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);

    assert_close(parent_layout.height_px, 70.0, "parent auto height");
    assert_close(
        parent_layout.content_height_px,
        70.0,
        "parent content height",
    );
}

#[test]
// CSS 2.1 9.2.2: display:none removes a child from layout participation.
fn display_none_child_does_not_change_parent_retained_height() {
    let (mut world, mut root, container, empty_parent, hidden_parent) =
        container_with_two_parents_setup();
    let hidden_child = assert_append_div_to(&mut root, hidden_parent);

    assert_set_attr_str(&mut root, container, "width", "80px");
    assert_set_attr_str(&mut root, hidden_child, "display", "none");
    assert_set_attr_str(&mut root, hidden_child, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let empty_parent_layout = assert_node_layout(&root, empty_parent);
    let hidden_parent_layout = assert_node_layout(&root, hidden_parent);

    assert_close(
        hidden_parent_layout.height_px,
        empty_parent_layout.height_px,
        "display:none parent height",
    );
    assert_close(
        hidden_parent_layout.content_height_px,
        empty_parent_layout.content_height_px,
        "display:none parent content height",
    );
}

#[test]
// CSS 2.1 9.4.1: overflow changes clipping behavior, not normal-flow status.
fn overflow_hidden_child_still_contributes_to_parent_height() {
    let (mut world, mut root, _container, parent, child) = nested_parent_child_setup();

    assert_set_attr_str(&mut root, parent, "width", "80px");
    assert_set_attr_str(&mut root, child, "overflow", "hidden");
    assert_set_attr_str(&mut root, child, "width", "24px");
    assert_set_attr_str(&mut root, child, "height", "12px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    let child_layout = assert_node_layout(&root, child);

    assert_close(child_layout.height_px, 12.0, "overflow:hidden child height");
    assert_close(parent_layout.height_px, 12.0, "parent auto height");
    assert_close(
        parent_layout.content_height_px,
        12.0,
        "parent content height",
    );
}

#[test]
// CSS 2.1 9.4.3: relative positioning shifts visual position without
// changing the box's normal-flow participation.
fn relative_offsets_do_not_change_parent_content_height() {
    let (mut world, mut root, container, baseline_parent, offset_parent) =
        container_with_two_parents_setup();
    let baseline_child = assert_append_div_to(&mut root, baseline_parent);
    let offset_child = assert_append_div_to(&mut root, offset_parent);

    assert_set_attr_str(&mut root, container, "width", "80px");
    assert_set_attr_str(&mut root, baseline_child, "position", "relative");
    assert_set_attr_str(&mut root, baseline_child, "height", "12px");
    assert_set_attr_str(&mut root, offset_child, "position", "relative");
    assert_set_attr_str(&mut root, offset_child, "height", "12px");
    assert_set_attr_str(&mut root, offset_child, "top", "40px");
    assert_set_attr_str(&mut root, offset_child, "left", "15px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_parent_layout = assert_node_layout(&root, baseline_parent);
    let offset_parent_layout = assert_node_layout(&root, offset_parent);

    assert_close(
        offset_parent_layout.height_px,
        baseline_parent_layout.height_px,
        "relative offset parent height",
    );
    assert_close(
        offset_parent_layout.content_height_px,
        baseline_parent_layout.content_height_px,
        "relative offset parent content height",
    );
}

#[test]
// CSS 2.1 9.4.3: relative positioning from stylesheet rules should shift
// visual position without changing normal-flow participation.
fn stylesheet_relative_offsets_do_not_change_parent_content_height() {
    let (mut world, mut root, container, baseline_parent, offset_parent) =
        container_with_two_parents_setup();
    let baseline_child = assert_append_div_to(&mut root, baseline_parent);
    let offset_child = assert_append_div_to(&mut root, offset_parent);

    assert_set_attr_str(&mut root, container, "class", "container");
    assert_set_attr_str(&mut root, baseline_child, "class", "relative-baseline");
    assert_set_attr_str(&mut root, offset_child, "class", "relative-offset");
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), RELATIVE_OFFSET_STYLESHEET);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_parent_layout = assert_node_layout(&root, baseline_parent);
    let offset_parent_layout = assert_node_layout(&root, offset_parent);

    assert_close(
        offset_parent_layout.height_px,
        baseline_parent_layout.height_px,
        "stylesheet relative offset parent height",
    );
    assert_close(
        offset_parent_layout.content_height_px,
        baseline_parent_layout.content_height_px,
        "stylesheet relative offset parent content height",
    );
}

#[test]
// CSS 2.1 10.6.3: an empty auto-height nested block should collapse to zero
// content height when it has no in-flow children and no authored height.
fn empty_nested_auto_height_block_collapses_to_zero_height() {
    let (mut world, mut root, container, empty_parent, _other_parent) =
        container_with_two_parents_setup();

    assert_set_attr_str(&mut root, container, "width", "80px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let empty_parent_layout = assert_node_layout(&root, empty_parent);

    assert_close(empty_parent_layout.height_px, 0.0, "empty parent height");
    assert_close(
        empty_parent_layout.content_height_px,
        0.0,
        "empty parent content height",
    );
}

#[test]
// CSS 2.1 10.6.3: when the last in-flow child is removed, the parent's
// auto height must be recomputed from the now-empty in-flow contents.
fn removing_last_in_flow_child_collapses_parent_back_to_zero_height() {
    let (mut world, mut root, _container, parent, child) = nested_parent_child_setup();

    assert_set_attr_str(&mut root, parent, "width", "80px");
    assert_set_attr_str(&mut root, child, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let populated_parent_layout = assert_node_layout(&root, parent);
    assert_close(
        populated_parent_layout.height_px,
        40.0,
        "populated parent height",
    );

    root.remove_node(child)
        .expect("content-height proof should remove the last in-flow child");

    assert_root_dom_update(&mut world, &mut root, 16.0, DomPointerUpdate::Unchanged);

    let empty_parent_layout = assert_node_layout(&root, parent);
    assert_close(
        empty_parent_layout.height_px,
        0.0,
        "parent height after removing the last in-flow child",
    );
    assert_close(
        empty_parent_layout.content_height_px,
        0.0,
        "parent content height after removing the last in-flow child",
    );
}
