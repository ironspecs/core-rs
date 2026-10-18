#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals and the
// computed groups' initial values are bit-exact, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Box-shorthand pins for the style system (PLAN.md B3).
//!
//! Purpose: pin the gate behavior for unimplemented shorthand NAMES — the
//! box-model shorthands CSS admits as one expansion family (`margin`,
//! `padding`, `border`, `border-width`, `inset`, `outline`) whose expansion the
//! engine has not yet admitted. Each pin asserts BOTH sides of the gate: the
//! declaration is dropped + counted (drop-and-continue, CSS 2.1 §4.1.7) AND
//! the corresponding longhand group fields stay at their group initial. A later
//! admission flips exactly these rows on-spec (see each per-test
//! "SPEC-CORRECT" note).
//!
//! Class note: every row in this file is `unsupported_*` = unimplemented —
//! policy-gated absence per the D6 admission policy in `MIGRATION.md` (a named
//! consumer need + spec citation + Blink/Gecko algorithm reference + proof).
//! The names are absent from the `property_shape` name table (`decl_parse.rs`),
//! so they drop at the name level before any value grammar runs — that is the
//! whole-declaration-drop behavior this file pins.
//!
//! This file will later also hold B10's family (`font`, `background`,
//! `text-decoration`, `all`, `overflow-x`/`overflow-y`) as a separate table
//! family. B3's rows assert "dropped+counted AND the longhand group fields stay
//! at initial" for the box shorthands only.

use bevy::prelude::World;
use ui_dom::{DomNodeSlotKey, DomPointerUpdate, RootDom, StylesheetLoadReport};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_decl_drop, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
};

/// Returns `(world, root, target, report)`: a fresh root with a single
/// class-"x" div target under the document root, `css` loaded (not yet
/// resolved — the caller runs the update), and the stylesheet load report.
fn styled_target_load(css: &str) -> (World, RootDom, DomNodeSlotKey, StylesheetLoadReport) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load");
    (world, root, target, report)
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Box 3 §4.2 `margin` is a shorthand for the four `margin-*`
//   longhands, taking 1–4 values with the corner mapping (1 = all sides,
//   2 = vertical/horizontal, 3 = top/horizontal/bottom, 4 = top/right/bottom/
//   left). <https://drafts.csswg.org/css-box-3/#margin-shorthand>
// - CURRENT (asserted): `margin: 8px`, `margin: 1px 2px`, and
//   `margin: 1px 2px 3px 4px` each drop + count (`declarations_dropped == 1`)
//   — the name is absent from `property_shape` (`decl_parse.rs`); after a
//   four-value load the computed `margin` stays `[0.0; 4]` (the group initial).
// - SPEC-CORRECT: expand to the four `margin-*` longhands via the 1–4 corner
//   map (the `expand_border_radius` template in `decl_parse.rs`).
// - D6: admission requires a named consumer need + spec citation + Blink/Gecko
//   algorithm reference. NOTE: `margin`/`padding` are the #1 admission
//   candidates by popularity (PLAN.md B3).
fn unsupported_margin_shorthand_drops() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "margin_one_value",
            body: "margin: 8px",
        },
        Case {
            name: "margin_two_values",
            body: "margin: 1px 2px",
        },
        Case {
            name: "margin_four_values",
            body: "margin: 1px 2px 3px 4px",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) =
        styled_target_load(".x { margin: 1px 2px 3px 4px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().margin,
        [0.0; 4],
        "margin shorthand should stay at its [0.0; 4] group initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Box 3 §4.3 `padding` is a shorthand for the four `padding-*`
//   longhands, taking 1–4 `<length-percentage [0,∞]>` values with the same
//   corner mapping as `margin`.
//   <https://drafts.csswg.org/css-box-3/#padding-shorthand>
// - CURRENT (asserted): `padding: 8px`, `padding: 1px 2px`, and
//   `padding: 1px 2px 3px 4px` each drop + count; after a four-value load the
//   computed `padding` stays `[0.0; 4]` (the group initial).
// - SPEC-CORRECT: expand to the four `padding-*` longhands via the corner map.
// - D6: admission-gated.
fn unsupported_padding_shorthand_drops() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "padding_one_value",
            body: "padding: 8px",
        },
        Case {
            name: "padding_two_values",
            body: "padding: 1px 2px",
        },
        Case {
            name: "padding_four_values",
            body: "padding: 1px 2px 3px 4px",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) =
        styled_target_load(".x { padding: 1px 2px 3px 4px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .padding,
        [0.0; 4],
        "padding shorthand should stay at its [0.0; 4] group initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Backgrounds 3 §4.2 `border` is a full shorthand (width, style,
//   color); §4.3 `border-width` is its width subset with the 1–4 corner map.
//   <https://drafts.csswg.org/css-backgrounds-3/#the-border-shorthands>
// - CURRENT (asserted): `border: 1px`, `border-width: 1px`, and
//   `border-width: 1px 2px` each drop + count. `border` is a FULL shorthand
//   whose style/color components are ALSO unadmitted — the whole declaration
//   drops as ONE (no partial width application): after a `border: 1px` load the
//   computed `border_width` stays `[0.0; 4]` (the group initial).
// - SPEC-CORRECT: expand `border` to its width/style/color longhands (the
//   unadmitted style/color components dropped with a counted note at admission)
//   and `border-width` to the four `border-*-width` longhands.
// - D6: admission-gated.
fn unsupported_border_and_border_width_shorthands_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "border_one_value",
            body: "border: 1px",
        },
        Case {
            name: "border_width_one_value",
            body: "border-width: 1px",
        },
        Case {
            name: "border_width_two_values",
            body: "border-width: 1px 2px",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) = styled_target_load(".x { border: 1px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .border_width,
        [0.0; 4],
        "border shorthand should stay at its [0.0; 4] group initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Position 3 §6 `inset` is a shorthand for `top`/`right`/`bottom`/
//   `left`, taking 1–4 values with the same corner map as `margin`.
//   <https://drafts.csswg.org/css-position-3/#inset-shorthand>
// - CURRENT (asserted): `inset: 0` and `inset: 1px 2px` each drop + count — the
//   NAME is absent from `property_shape`, so the value grammar's unitless-zero
//   arm never runs; after an `inset: 1px 2px` load the computed
//   `top`/`right`/`bottom`/`left` all stay `None` (the group initial).
// - SPEC-CORRECT: expand to the four inset longhands via the corner map.
// - D6: admission-gated.
fn unsupported_inset_shorthand_drops() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "inset_one_value",
            body: "inset: 0",
        },
        Case {
            name: "inset_two_values",
            body: "inset: 1px 2px",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) = styled_target_load(".x { inset: 1px 2px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target").as_box();
    assert_eq!(view.top, None, "inset top should stay at its initial None");
    assert_eq!(
        view.right, None,
        "inset right should stay at its initial None",
    );
    assert_eq!(
        view.bottom, None,
        "inset bottom should stay at its initial None",
    );
    assert_eq!(
        view.left, None,
        "inset left should stay at its initial None",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS UI 4 §4.3 `outline` is a shorthand for `outline-width`/
//   `outline-style`/`outline-color`.
//   <https://drafts.csswg.org/css-ui-4/#outline>
// - CURRENT (asserted): `outline: 2px` drops + counts (the NAME is absent, so
//   the whole declaration drops as one — no width is partially applied); after
//   the load the computed `outline_width` stays `0.0` and `outline_rgba` stays
//   the group initial (transparent `[0.0; 4]`).
// - SPEC-CORRECT: expand `outline` to its width/style/color longhands (the
//   unadmitted style/color components handled at admission).
// - D6: admission-gated.
fn unsupported_outline_shorthand_drops() {
    assert_decl_drop(".x { outline: 2px }", 0, 1);

    let (mut world, mut root, target, _report) = styled_target_load(".x { outline: 2px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target").as_visual();
    assert_eq!(
        view.outline_width, 0.0,
        "outline shorthand should stay at its 0.0 initial",
    );
    assert_eq!(
        view.outline_rgba, [0.0; 4],
        "outline shorthand should stay at its transparent [0.0; 4] initial",
    );
}
