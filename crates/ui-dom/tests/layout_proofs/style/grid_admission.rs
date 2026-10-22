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
fn grid_line_name_tokens_admit_and_round_trip_brackets() {
    // The T3 gate inversion: `[name]` line-name brackets are ADMITTED and
    // re-serialize with their brackets (the wire round-trip).
    assert_grid_wire_value(
        "grid-template-columns",
        "[main] 100px [aside] 1fr",
        "[main] 100px [aside] 1fr",
    );
    assert_grid_wire_value(
        "grid-template-rows",
        "[header] 40px [footer]",
        "[header] 40px [footer]",
    );
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

#[test]
fn grid_placement_longhands_parse_to_typed_grid_lines() {
    // The four longhands re-serialize their `<grid-line>` canonically.
    for (property, value, expected) in [
        ("grid-column-start", "auto", "auto"),
        ("grid-column-start", "2", "2"),
        ("grid-column-start", "-1", "-1"),
        ("grid-column-start", "span 2", "span 2"),
        ("grid-column-end", "span 2", "span 2"),
        ("grid-row-start", "1", "1"),
        ("grid-row-end", "auto", "auto"),
    ] {
        assert_grid_wire_value(property, value, expected);
    }
}

#[test]
fn grid_column_and_row_shorthands_expand_to_longhands() {
    // `grid-column: span 2` → start auto / end span 2; `2 / 4` → start 2 / end 4.
    for (property, value, start_expected, end_expected) in [
        ("grid-column", "span 2", "auto", "span 2"),
        ("grid-column", "2 / 4", "2", "4"),
        ("grid-row", "1 / 3", "1", "3"),
    ] {
        let (mut world, mut root, target) = attached_target();
        assert_style_set(&mut root, target, property, value);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        let map = root.compute_style_map(target).expect("style map");
        let start_property = PresentationProperty::from_name(match property {
            "grid-column" => "grid-column-start",
            _ => "grid-row-start",
        })
        .expect("start longhand");
        let end_property = PresentationProperty::from_name(match property {
            "grid-column" => "grid-column-end",
            _ => "grid-row-end",
        })
        .expect("end longhand");
        assert_eq!(
            map.get(&start_property),
            Some(&DomAttributeValue::String(start_expected.to_string())),
            "{property}: {value} start",
        );
        assert_eq!(
            map.get(&end_property),
            Some(&DomAttributeValue::String(end_expected.to_string())),
            "{property}: {value} end",
        );
    }
}

#[test]
fn grid_justify_items_and_self_parse_to_keywords() {
    for (property, value, expected) in [
        ("justify-items", "normal", "normal"),
        ("justify-items", "stretch", "stretch"),
        ("justify-items", "start", "start"),
        ("justify-items", "end", "end"),
        ("justify-items", "center", "center"),
        ("justify-self", "auto", "auto"),
        ("justify-self", "center", "center"),
    ] {
        assert_grid_wire_value(property, value, expected);
    }
}

#[test]
fn grid_justify_items_wider_self_position_values_drop() {
    // The closed `justify-items` vocabulary (`normal|stretch|start|end|center`)
    // admits no wider `<self-position>` values. `flex-start`/`flex-end`/
    // `baseline` (and `safe`/`unsafe`) are spec-valid `<self-position>` but
    // unadmitted per D6, so each drops + counts and the grid group stays at its
    // `Normal` initial. `place-items: flex-start` drops the WHOLE declaration —
    // the align side is NOT applied either, because the G-rule whole-declaration
    // drop is the behavior for an unadmitted side of the two-sided shorthand.
    for value in ["flex-start", "flex-end", "baseline"] {
        assert_decl_drop(&format!(".x {{ justify-items: {value} }}"), 0, 1);
    }
    assert_decl_drop(".x { place-items: flex-start }", 0, 1);

    // The group stays initial: the strict write rejects the unadmitted value,
    // and `place-items: flex-start` applies neither side.
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "display", "grid");
    assert!(
        root.style_mut(target)
            .set("justify-items", "flex-start")
            .is_err(),
        "justify-items: flex-start must be rejected by the strict write"
    );
    assert!(
        root.style_mut(target)
            .set("place-items", "flex-start")
            .is_err(),
        "place-items: flex-start must be rejected by the strict write"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let map = root.compute_style_map(target).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::JustifyItems),
        None,
        "unadmitted justify-items value must not be declared (group stays initial)"
    );
    assert_eq!(
        map.get(&PresentationProperty::AlignItems),
        None,
        "place-items: flex-start must not apply the align side either"
    );

    // CONTROL: `place-items: center` still applies both sides.
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "display", "grid");
    assert_style_set(&mut root, target, "place-items", "center");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(target).expect("style map");
    assert_eq!(
        map.get(&PresentationProperty::JustifyItems),
        Some(&DomAttributeValue::String("center".to_string())),
        "place-items: center applies the justify side"
    );
    assert_eq!(
        map.get(&PresentationProperty::AlignItems),
        Some(&DomAttributeValue::String("center".to_string())),
        "place-items: center applies the align side"
    );
}

#[test]
fn grid_span_zero_and_span_negative_drop() {
    // `span <integer>` must be positive: 0 and negatives drop + count.
    assert_decl_drop(".x { grid-column-start: span 0 }", 0, 1);
    assert_decl_drop(".x { grid-column-start: span -1 }", 0, 1);
    assert_decl_drop(".x { grid-column: span 0 }", 0, 1);
    assert_decl_drop(".x { grid-row: span -1 }", 0, 1);
}

#[test]
fn grid_line_zero_drops() {
    // Zero is not a line (CSS Grid §7.1): `grid-column-start: 0` drops + counts.
    assert_decl_drop(".x { grid-column-start: 0 }", 0, 1);
    assert_decl_drop(".x { grid-column: 0 / 2 }", 0, 1);
}

#[test]
fn grid_named_lines_admitted() {
    // The T3 gate inversion: `<custom-ident>` and `span <custom-ident>`
    // `<grid-line>` forms are ADMITTED (named lines).
    for (property, value, expected) in [
        ("grid-column-start", "main", "main"),
        ("grid-column-start", "span main", "span main"),
        ("grid-column-start", "span main 3", "span main 3"),
        ("grid-row-start", "header-end", "header-end"),
    ] {
        assert_grid_wire_value(property, value, expected);
    }
}

#[test]
fn grid_template_areas_parse_and_round_trip() {
    assert_grid_wire_value(
        "grid-template-areas",
        "\"header header\" \"sidebar main\" \"footer footer\"",
        "\"header header\" \"sidebar main\" \"footer footer\"",
    );
    assert_grid_wire_value("grid-template-areas", "\". main main\"", "\". main main\"");
}

#[test]
fn grid_template_areas_none_resets_to_the_empty_map() {
    // `grid-template-areas: none` is the CSS initial value: it must be ADMITTED
    // (not dropped) and write the empty areas map, so a later `none` clears any
    // prior areas. CSS 2.1 §4.1.3 makes the `none` keyword ASCII
    // case-insensitive, so `NONE`/`None` admit identically (the spec-correct
    // entry point; the rest of the grammar stays case-sensitive — see the
    // `off_spec_keyword_grammars_are_case_sensitive` registry row). CONTROL:
    // the `<string>+` form still parses (pinned by
    // `grid_template_areas_parse_and_round_trip`).
    for reset in ["none", "NONE", "None"] {
        assert_grid_wire_value("grid-template-areas", reset, "none");
    }

    // Reset: `"a a"` then each reset keyword clears the areas — the named area
    // no longer resolves, so `grid-area: a` falls back to auto placement.
    for reset in ["none", "NONE", "None"] {
        let (mut world, mut root, container) = attached_target();
        assert_style_set(&mut root, container, "display", "grid");
        assert_style_set(&mut root, container, "grid-template-columns", "100px 100px");
        assert_style_set(&mut root, container, "width", "200px");
        assert_style_set(&mut root, container, "grid-template-areas", "\"a a\"");
        let item = assert_append_div_to(&mut root, container);
        assert_style_set(&mut root, item, "grid-area", "a");
        assert_style_set(&mut root, item, "height", "40px");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_close(
            node_box(&root, item).expect("item").border_box.width_px,
            200.0,
            "before the reset the `a` area spans both columns",
        );

        assert_style_set(&mut root, container, "grid-template-areas", reset);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_close(
            node_box(&root, item).expect("item").border_box.width_px,
            100.0,
            &format!("after `{reset}` the `a` area no longer resolves — auto placement"),
        );
    }
}

#[test]
fn grid_template_areas_non_rectangular_name_drops() {
    // `a` does not tile a rectangle (cells at (0,0),(0,1),(1,0)) → the WHOLE
    // declaration drops + counts, the group stays initial.
    assert_decl_drop(".x { grid-template-areas: \"a a\" \"a b\" }", 0, 1);
}

#[test]
fn grid_template_areas_non_uniform_columns_drops() {
    assert_decl_drop(".x { grid-template-areas: \"a a\" \"b\" }", 0, 1);
}

#[test]
fn grid_template_areas_capacity_gates_drop() {
    // > 8 rows.
    let nine_rows = (0..9).map(|_| "\"a a\"").collect::<Vec<_>>().join(" ");
    assert_decl_drop(&format!(".x {{ grid-template-areas: {nine_rows} }}"), 0, 1);
    // > 12 columns.
    let thirteen_cols = (0..13).map(|_| "a").collect::<Vec<_>>().join(" ");
    assert_decl_drop(
        &format!(".x {{ grid-template-areas: \"{thirteen_cols}\" }}"),
        0,
        1,
    );
    // > 12 distinct names (13 distinct names in one row).
    let thirteen_names = (0..13)
        .map(|i| format!("a{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_decl_drop(
        &format!(".x {{ grid-template-areas: \"{thirteen_names}\" }}"),
        0,
        1,
    );
}

#[test]
fn grid_area_expands_per_d2() {
    // A lone <custom-ident> names the area: it expands to the four placement
    // longhands as `Named(name)` (the margin-shorthand model, adjudicated
    // 2026-10-22), serializing each as the bare name — not a separate
    // `GridArea` area-name longhand.
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "grid-area", "header");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(target).expect("style map");
    for property_name in [
        "grid-row-start",
        "grid-column-start",
        "grid-row-end",
        "grid-column-end",
    ] {
        let property = PresentationProperty::from_name(property_name).expect("longhand");
        assert_eq!(
            map.get(&property),
            Some(&DomAttributeValue::String("header".to_string())),
            "grid-area: header → {property_name} = header",
        );
    }
    assert_eq!(
        map.get(&PresentationProperty::GridArea),
        None,
        "grid-area: header must not write a separate area-name longhand",
    );

    // 1–4 <grid-line> values write the four placement longhands per D2.
    for (value, row_start, column_start, row_end, column_end) in [
        ("2", "2", "auto", "auto", "auto"),
        ("2 / 3", "2", "3", "auto", "auto"),
        ("2 / 3 / 4", "2", "3", "4", "auto"),
        ("2 / 3 / 4 / 5", "2", "3", "4", "5"),
        ("span 2", "span 2", "auto", "auto", "auto"),
    ] {
        let (mut world, mut root, target) = attached_target();
        assert_style_set(&mut root, target, "grid-area", value);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let map = root.compute_style_map(target).expect("style map");
        for (property_name, expected) in [
            ("grid-row-start", row_start),
            ("grid-column-start", column_start),
            ("grid-row-end", row_end),
            ("grid-column-end", column_end),
        ] {
            let property = PresentationProperty::from_name(property_name).expect("longhand");
            assert_eq!(
                map.get(&property),
                Some(&DomAttributeValue::String(expected.to_string())),
                "grid-area: {value} → {property_name}",
            );
        }
    }
}

#[test]
fn grid_area_auto_resets_to_auto_placement() {
    // `grid-area: auto` is the shorthand's keyword, not a name: it writes
    // `GridLine::Auto` to all four longhands (the lone-ident branch rejects
    // `auto`, so it falls through to the grid-line path's defaulting table).
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "grid-area", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(target).expect("style map");
    for property_name in [
        "grid-row-start",
        "grid-column-start",
        "grid-row-end",
        "grid-column-end",
    ] {
        let property = PresentationProperty::from_name(property_name).expect("longhand");
        assert_eq!(
            map.get(&property),
            Some(&DomAttributeValue::String("auto".to_string())),
            "grid-area: auto → {property_name} = auto",
        );
    }

    // Re-setting between forms: `grid-area: header` then `grid-area: auto`
    // resets to full auto placement (the header name does not linger).
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "grid-area", "header");
    assert_style_set(&mut root, target, "grid-area", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(target).expect("style map");
    for property_name in [
        "grid-row-start",
        "grid-column-start",
        "grid-row-end",
        "grid-column-end",
    ] {
        let property = PresentationProperty::from_name(property_name).expect("longhand");
        assert_eq!(
            map.get(&property),
            Some(&DomAttributeValue::String("auto".to_string())),
            "grid-area: header then auto → {property_name} = auto",
        );
    }
}

#[test]
fn grid_custom_ident_positions_reject_reserved_keywords() {
    // CSS Values 4 §3.2 excludes the CSS-wide keywords from <custom-ident>;
    // CSS Grid §7.1 additionally excludes `span`; `auto` is the shorthand's
    // keyword, not a name. Each reserved keyword in a name position drops +
    // counts (never becomes a name).
    assert_decl_drop(".x { grid-template-areas: \"auto auto\" }", 0, 1);
    assert_decl_drop(".x { grid-template-columns: [auto] 1fr }", 0, 1);

    // `span` is already excluded and stays excluded (control).
    assert_decl_drop(".x { grid-area: span }", 0, 1);

    // `grid-area: inherit` is the css-wide keyword, not a name: the strict
    // write rejects it and the group stays initial (no placement longhands).
    let (mut world, mut root, target) = attached_target();
    assert_style_set(&mut root, target, "display", "grid");
    assert!(
        root.style_mut(target).set("grid-area", "inherit").is_err(),
        "grid-area: inherit must be rejected by the strict write"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let map = root.compute_style_map(target).expect("style map");
    for property_name in [
        "grid-row-start",
        "grid-column-start",
        "grid-row-end",
        "grid-column-end",
    ] {
        let property = PresentationProperty::from_name(property_name).expect("longhand");
        assert_eq!(
            map.get(&property),
            None,
            "grid-area: inherit must not become {property_name} area state",
        );
    }
}

#[test]
fn grid_line_name_capacity_gates_drop() {
    // A name labeling more than four lines drops + counts.
    assert_decl_drop(
        ".x { grid-template-columns: [m] 1fr [m] 1fr [m] 1fr [m] 1fr [m] 1fr }",
        0,
        1,
    );
    // More than eight name registrations drops + counts.
    assert_decl_drop(
        ".x { grid-template-columns: [a] 1fr [b] 1fr [c] 1fr [d] 1fr [e] 1fr [f] 1fr [g] 1fr [h] 1fr [i] 1fr }",
        0,
        1,
    );
}

#[test]
fn grid_template_and_grid_shorthands_still_drop() {
    // The T4 shorthands stay gated (unchanged).
    assert_decl_drop(".x { grid-template: \"a b\" 1fr / 1fr 1fr }", 0, 1);
    assert_decl_drop(".x { grid: \"a b\" 1fr / 1fr 1fr }", 0, 1);
}
