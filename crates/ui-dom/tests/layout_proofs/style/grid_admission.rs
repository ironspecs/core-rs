#![allow(rustdoc::bare_urls)]

//! Grid T0 admission proofs (`GRID_STEP1_DESIGN.md` D6).
//!
//! These are the public-surface pins for the grid T0 landing: `display:
//! grid`/`inline-grid` routing, the T1 property set (`grid-template-columns`,
//! `grid-template-rows`, `grid-auto-flow`, `grid-auto-columns`,
//! `grid-auto-rows`) admitted parse-and-hold, the counted-drop gates (the >12
//! capacity gate, the `[` line-name gate, the auto-fill/auto-fit gate), and the
//! two layout pins (the D5 block interim, and the formatting-context flag
//! generalization that contains a grid item's first-child margin).
//!
//! The computed grid group is observed through the public wire projection
//! (`compute_style_map`): an admitted track list re-serializes to its canonical
//! token sequence, so "exact `GridTrackSize` sequences in the arena" is
//! asserted as the exact re-serialized string. The value-dedup interning and the
//! resolver's group writes are pinned by unit tests in `decl_store.rs`,
//! `decl_parse.rs`, and `project.rs` (the owning files).

use bevy::prelude::World;
use ui_dom::{
    DisplayKw, DomAttributeValue, DomNodeSlotKey, DomPointerUpdate, PresentationProperty, RootDom,
    node_box,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_close, assert_decl_drop, assert_node_layout,
    assert_root_dom_update, assert_style_set, attached_gpu_root_dom_default_viewport_setup,
};

/// Returns `(world, root, target)`: a fresh root with one div target under the
/// document root.
fn attached_target() -> (World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    (world, root, target)
}

/// Asserts the inline-declared grid property resolves to `expected` through the
/// public wire projection.
fn assert_grid_wire_value(property: &str, value: &str, expected: &str) {
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, property, value);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(target).expect("style map");
    let property = PresentationProperty::from_name(property).expect("admitted property");
    assert_eq!(
        map.get(&property),
        Some(&DomAttributeValue::String(expected.to_string())),
        "{property:?}: {value:?} should project to {expected:?}",
    );
}

#[test]
fn display_grid_and_inline_grid_are_admitted() {
    for (value, expected) in [
        ("grid", DisplayKw::Grid),
        ("inline-grid", DisplayKw::InlineGrid),
    ] {
        let (mut world, mut root, target) = attached_target();
        assert_style_set(&mut root, target, "display", value);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target)
                .expect("target")
                .as_box()
                .display,
            expected,
            "display: {value} should resolve to {expected:?}",
        );
    }
}

#[test]
fn grid_template_columns_parse_to_typed_track_lists() {
    for (value, expected) in [
        ("repeat(3, 1fr)", "1fr 1fr 1fr"),
        ("100px 1fr 2fr", "100px 1fr 2fr"),
        ("minmax(100px, 200px) auto", "minmax(100px, 200px) auto"),
        ("min-content max-content 20%", "min-content max-content 20%"),
    ] {
        assert_grid_wire_value("grid-template-columns", value, expected);
    }
}

#[test]
fn grid_template_rows_parse_to_typed_track_lists() {
    for (value, expected) in [
        ("repeat(3, 1fr)", "1fr 1fr 1fr"),
        ("100px 1fr 2fr", "100px 1fr 2fr"),
        ("minmax(100px, 200px) auto", "minmax(100px, 200px) auto"),
        ("min-content max-content 20%", "min-content max-content 20%"),
    ] {
        assert_grid_wire_value("grid-template-rows", value, expected);
    }
}

#[test]
fn grid_auto_flow_parses_row_column_dense() {
    for (value, expected) in [
        ("row", "row"),
        ("column", "column"),
        ("row dense", "row dense"),
        ("column dense", "column dense"),
    ] {
        assert_grid_wire_value("grid-auto-flow", value, expected);
    }
}

#[test]
fn grid_auto_columns_and_rows_parse_single_track_size() {
    for (property, value, expected) in [
        ("grid-auto-columns", "auto", "auto"),
        ("grid-auto-columns", "1fr", "1fr"),
        (
            "grid-auto-columns",
            "minmax(80px, max-content)",
            "minmax(80px, max-content)",
        ),
        ("grid-auto-rows", "auto", "auto"),
        ("grid-auto-rows", "1fr", "1fr"),
        (
            "grid-auto-rows",
            "minmax(80px, max-content)",
            "minmax(80px, max-content)",
        ),
    ] {
        assert_grid_wire_value(property, value, expected);
    }
}

#[test]
fn grid_track_list_over_capacity_drops() {
    // Thirteen explicit tracks and `repeat(13, 1fr)` both expand past the
    // capacity-12 gate: drop + count, no crash.
    let thirteen = (0..13).map(|_| "1fr").collect::<Vec<_>>().join(" ");
    assert_decl_drop(&format!(".x {{ grid-template-columns: {thirteen} }}"), 0, 1);
    assert_decl_drop(".x { grid-template-columns: repeat(13, 1fr) }", 0, 1);

    // The capacity-12 boundary IS admitted.
    let twelve = (0..12).map(|_| "1fr").collect::<Vec<_>>().join(" ");
    assert_decl_drop(&format!(".x {{ grid-template-columns: {twelve} }}"), 1, 0);
}

#[test]
fn grid_line_name_tokens_drop_for_now() {
    assert_decl_drop(".x { grid-template-columns: [main] 1fr [aside] }", 0, 1);
}

#[test]
fn grid_shorthand_and_area_names_still_drop() {
    // `subgrid` is a still-gated VALUE of the admitted template longhands (the
    // track-size leaf has no subgrid arm), so it drops + counts. The still-gated
    // NAMES are pinned by `unsupported_grid_property_names_drop` in
    // `unsupported_properties.rs`.
    assert_decl_drop(".x { grid-template-columns: subgrid }", 0, 1);
    assert_decl_drop(".x { grid-template-rows: subgrid }", 0, 1);
}

#[test]
fn grid_auto_fill_and_auto_fit_repeat_drops_for_now() {
    assert_decl_drop(
        ".x { grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)) }",
        0,
        1,
    );
    assert_decl_drop(
        ".x { grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)) }",
        0,
        1,
    );
}

#[test]
fn grid_negative_track_sizes_drop() {
    // CSS Grid §7.1 rejects negative track sizes. The four sign-check sites —
    // the leaf `fr` arm, the leaf `%` arm, the minmax min-bound `%` arm, and
    // the minmax max-bound `%` arm — each drop the whole declaration with a
    // counted report (never `Percent(-10.0)`/`Fr(-1.0)` interned).
    assert_decl_drop(".x { grid-template-columns: -10% }", 0, 1);
    assert_decl_drop(".x { grid-template-columns: repeat(2, -1fr) }", 0, 1);
    assert_decl_drop(".x { grid-template-columns: minmax(-5%, 100px) }", 0, 1);
    assert_decl_drop(".x { grid-template-columns: minmax(10%, -50%) }", 0, 1);
}

#[test]
fn grid_negative_track_size_leaves_the_group_initial() {
    // The strict single-declaration path rejects the negative track size, so
    // the grid group's `template_columns` stays at its empty initial. Observed
    // through the public wire projection (`compute_style_map`): no
    // `grid-template-columns` winner is declared.
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "display", "grid");
    assert!(
        root.style_mut(target)
            .set("grid-template-columns", "-10%")
            .is_err(),
        "negative track size must be rejected by the strict write"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(target).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::GridTemplateColumns),
        None,
        "a rejected track size must not be declared (group stays initial)"
    );
}

#[test]
fn grid_track_size_positive_controls_admitted() {
    // Positive fr/percent controls (and zero, which `<flex>` admits — non-
    // negative, zero inclusive) still intern and re-serialize canonically.
    for (value, expected) in [
        ("50%", "50%"),
        ("1.5fr", "1.5fr"),
        ("minmax(10%, 50%)", "minmax(10%, 50%)"),
        ("0fr", "0fr"),
    ] {
        assert_grid_wire_value("grid-template-columns", value, expected);
    }
}

#[test]
fn grid_item_margins_are_contained_at_the_boundary() {
    // CSS Grid 1 §4 + CSS 2.1 §8.3.1: a grid item establishes an independent
    // formatting context, so its first in-flow child's top margin is CONTAINED
    // inside the item rather than collapsing out through it (the flex-arc step-9
    // rule generalized by the formatting-context flag). At T0 the grid container
    // lays out as block (D5), but the flag is false for its children, so the
    // grandchild sits 20px below the item's content-box top.
    let (mut world, mut root, container) = attached_target();
    assert_style_set(&mut root, container, "display", "grid");
    assert_style_set(&mut root, container, "width", "200px");
    let item = assert_append_div_to(&mut root, container);
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
        "grid item: grandchild sits 20px below the item content top (contained)",
    );
    assert_close(
        assert_node_layout(&root, item).content_height_px,
        20.0,
        "grid item: the contained 20px is a real first-in-flow offset",
    );
}
