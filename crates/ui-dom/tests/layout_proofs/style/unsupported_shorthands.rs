#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals and the
// computed groups' initial values are bit-exact, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Shorthand pins for the style system (PLAN.md B3 + B10).
//!
//! Purpose: pin the gate behavior for unimplemented shorthand NAMES — the
//! box-model shorthands CSS admits as one expansion family (`border`,
//! `border-width`, `inset`, `outline`) whose expansion the engine has not yet
//! admitted (B3), and the other-shorthand + overflow-axes family (`font`,
//! `background`, `all`, `overflow-x`/`overflow-y`) (B10). Each pin asserts BOTH
//! sides of the gate: the declaration is dropped + counted (drop-and-continue,
//! CSS 2.1 §4.1.7) AND the corresponding longhand group fields stay at their
//! group initial. A later admission flips exactly these rows on-spec (see each
//! per-test "SPEC-CORRECT" note).
//!
//! The `margin` and `padding` rows of the original B3 box-model family were
//! admitted in PR 1 item 1 and relabeled on-spec here
//! (`margin_shorthand_expands_to_longhands`, `padding_shorthand_expands_to_longhands`):
//! they now assert the 1–4 corner expansion to the four `*-top`/`*-right`/
//! `*-bottom`/`*-left` longhands, not the drop gate.
//!
//! Class note: every `unsupported_*` row in this file is unimplemented —
//! policy-gated absence per the D6 admission policy in `MIGRATION.md` (a named
//! consumer need + spec citation + Blink/Gecko algorithm reference + proof).
//! The names are absent from the `property_shape` name table (`decl_parse.rs`),
//! so they drop at the name level before any value grammar runs — that is the
//! whole-declaration-drop behavior this file pins. The one value-grammar row
//! (B10's two-token `overflow: scroll auto`) drops because the admitted
//! `overflow` keyword grammar matches the whole value string, so the
//! whole declaration drops as ONE (never two partial drops).

use bevy::prelude::World;
use ui_dom::{
    BoxLength, DisplayKw, DomNodeSlotKey, DomPointerUpdate, Keyword, OverflowKw, RootDom,
    StylesheetLoadReport,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_decl_drop, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, box_px, theme_stylesheet_origin,
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
// ON-SPEC (relabeled from `unsupported_margin_shorthand_drops` in PR 1 item 1):
// - SPEC: CSS Box 3 §4.2 `margin` is a shorthand for the four `margin-*`
//   longhands, taking 1–4 values with the corner mapping (1 = all sides,
//   2 = vertical/horizontal, 3 = top/horizontal/bottom, 4 = top/right/bottom/
//   left). <https://drafts.csswg.org/css-box-3/#margin-shorthand>
// - CURRENT (asserted): `margin: 8px`, `margin: 1px 2px`, and
//   `margin: 1px 2px 3px 4px` each expand to the four `margin-*` longhands via
//   the corner map with no drops; the computed `margin` is TRBL
//   `[8.0; 4]` / `[1.0, 2.0, 1.0, 2.0]` / `[1.0, 2.0, 3.0, 4.0]`.
//   `margin: 0 auto` (2-value form) expands to `[0.0, Auto, 0.0, Auto]` —
//   the horizontal `auto` pair is admitted on the shorthand (PR 1 follow-up).
fn margin_shorthand_expands_to_longhands() {
    struct Case {
        name: &'static str,
        body: &'static str,
        margin: [BoxLength; 4],
    }

    let cases = [
        Case {
            name: "margin_one_value",
            body: "margin: 8px",
            margin: [box_px(8.0); 4],
        },
        Case {
            name: "margin_two_values",
            body: "margin: 1px 2px",
            margin: [box_px(1.0), box_px(2.0), box_px(1.0), box_px(2.0)],
        },
        Case {
            name: "margin_four_values",
            body: "margin: 1px 2px 3px 4px",
            margin: [box_px(1.0), box_px(2.0), box_px(3.0), box_px(4.0)],
        },
        Case {
            name: "margin_auto_two_values",
            body: "margin: 0 auto",
            margin: [box_px(0.0), BoxLength::Auto, box_px(0.0), BoxLength::Auto],
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_eq!(report.declarations_applied, 1, "{} should apply", case.name);
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should not drop",
            case.name,
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target).expect("target").as_box().margin,
            case.margin,
            "{} should expand to TRBL {:?}",
            case.name,
            case.margin,
        );
    }
}

#[test]
// ON-SPEC (relabeled from `unsupported_padding_shorthand_drops` in PR 1 item 1):
// - SPEC: CSS Box 3 §4.3 `padding` is a shorthand for the four `padding-*`
//   longhands, taking 1–4 `<length-percentage [0,∞]>` values with the same
//   corner mapping as `margin`.
//   <https://drafts.csswg.org/css-box-3/#padding-shorthand>
// - CURRENT (asserted): `padding: 8px`, `padding: 1px 2px`, and
//   `padding: 1px 2px 3px 4px` each expand to the four `padding-*` longhands
//   via the corner map with no drops; the computed `padding` is TRBL
//   `[8.0; 4]` / `[1.0, 2.0, 1.0, 2.0]` / `[1.0, 2.0, 3.0, 4.0]`.
fn padding_shorthand_expands_to_longhands() {
    struct Case {
        name: &'static str,
        body: &'static str,
        padding: [BoxLength; 4],
    }

    let cases = [
        Case {
            name: "padding_one_value",
            body: "padding: 8px",
            padding: [box_px(8.0); 4],
        },
        Case {
            name: "padding_two_values",
            body: "padding: 1px 2px",
            padding: [box_px(1.0), box_px(2.0), box_px(1.0), box_px(2.0)],
        },
        Case {
            name: "padding_four_values",
            body: "padding: 1px 2px 3px 4px",
            padding: [box_px(1.0), box_px(2.0), box_px(3.0), box_px(4.0)],
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_eq!(report.declarations_applied, 1, "{} should apply", case.name);
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should not drop",
            case.name,
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target)
                .expect("target")
                .as_box()
                .padding,
            case.padding,
            "{} should expand to TRBL {:?}",
            case.name,
            case.padding,
        );
    }
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
    assert_eq!(
        view.top,
        BoxLength::Auto,
        "inset top should stay at its initial Auto"
    );
    assert_eq!(
        view.right,
        BoxLength::Auto,
        "inset right should stay at its initial Auto",
    );
    assert_eq!(
        view.bottom,
        BoxLength::Auto,
        "inset bottom should stay at its initial Auto",
    );
    assert_eq!(
        view.left,
        BoxLength::Auto,
        "inset left should stay at its initial Auto",
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

// — B10: other shorthands & overflow axes — //

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Fonts 4 §font shorthand — `font` is a full shorthand for the
//   font longhands (style/weight/size/line-height/family); per the shorthand
//   grammar, `font-size` and `font-family` are the required components and the
//   rest reset to their initial values.
//   <https://drafts.csswg.org/css-fonts-4/#font-shorthand>
// - CURRENT (asserted): `font: italic 12px serif` and `font: bold 14px
//   sans-serif` each drop + count (`declarations_dropped == 1`) — the NAME is
//   absent from `property_shape` (`decl_parse.rs`), so the whole declaration
//   drops as one before any value grammar runs; after a load the computed font
//   group stays at its group initial (size 14.0px, style Normal, weight 400).
// - SPEC-CORRECT: expand to the five font longhands via the CSS Fonts 4
//   shorthand rules (size/family required, style/weight/line-height initial).
// - D6: admission-gated.
fn unsupported_font_shorthand_drops() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "font_italic_12px_serif",
            body: "font: italic 12px serif",
        },
        Case {
            name: "font_bold_14px_sans_serif",
            body: "font: bold 14px sans-serif",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) =
        styled_target_load(".x { font: italic 12px serif }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let font = root.computed_style(target).expect("target").as_font();
    assert_eq!(
        font.size_px, 14.0,
        "font shorthand should stay at the 14.0 default size seed",
    );
    assert_eq!(
        font.style,
        Keyword::Normal,
        "font shorthand should stay at its Normal style initial",
    );
    assert_eq!(
        font.weight, 400,
        "font shorthand should stay at its 400 weight initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Backgrounds 3 §background shorthand — `background` is a full
//   shorthand for the background longhands (`background-color`, `-image`, …).
//   <https://drafts.csswg.org/css-backgrounds-3/#background>
// - CURRENT (asserted): `background: #112233` drops + counts — the NAME is
//   absent from `property_shape`, so the whole declaration drops as one; after
//   the load the computed `background_rgba` stays `[0.0; 4]` (the transparent
//   group initial). No partial color application despite the value being a
//   parseable hex color.
// - SPEC-CORRECT: expand to the background longhands (`background-color` for
//   this single-color form).
// - D6: admission-gated.
fn unsupported_background_shorthand_drops() {
    assert_decl_drop(".x { background: #112233 }", 0, 1);

    let (mut world, mut root, target, _report) = styled_target_load(".x { background: #112233 }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_visual()
            .background_rgba,
        [0.0; 4],
        "background shorthand should stay at its transparent [0.0; 4] initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Cascade 5 §all shorthand — `all` resets every property except
//   `direction`/`unicode-bidi` to a CSS-wide keyword (`initial`, `inherit`,
//   `unset`, `revert`, …). <https://drafts.csswg.org/css-cascade-5/#all-shorthand>
// - CURRENT (asserted): `all: initial` drops + counts — the NAME is absent from
//   `property_shape`, so the whole declaration drops as one; after the load the
//   computed groups stay at their group initials (width `None`, the near-white
//   color initial, display `Block`).
// - SPEC-CORRECT: reset every longhand to its initial value.
// - D6: admission-gated.
fn unsupported_all_shorthand_drops() {
    assert_decl_drop(".x { all: initial }", 0, 1);

    let (mut world, mut root, target, _report) = styled_target_load(".x { all: initial }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let style = root.computed_style(target).expect("target");
    assert_eq!(
        style.as_box().width,
        BoxLength::Auto,
        "all shorthand should leave width at its initial Auto",
    );
    assert_eq!(
        style.as_color().color_rgba,
        [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0],
        "all shorthand should leave color at its near-white initial",
    );
    assert_eq!(
        style.as_box().display,
        DisplayKw::Block,
        "all shorthand should leave display at its Block initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Overflow 3 §overflow-x/§overflow-y — the per-axis longhands are
//   `overflow-x`/`overflow-y`; `overflow` is a two-value shorthand (x then y).
//   <https://drafts.csswg.org/css-overflow-3/#overflow-x>
// - CURRENT (asserted): `overflow: scroll auto` (the two-token shorthand form),
//   `overflow-x: scroll`, and `overflow-y: scroll` each drop + count. The
//   two-token form drops as ONE declaration — the admitted `overflow` keyword
//   grammar matches the whole value string (`Keyword::from_name_in`), so
//   `scroll auto` fails and the whole declaration drops (never two partial
//   drops). `overflow-x`/`overflow-y` are NAME-absent, so they drop at name
//   lookup. After a load the computed `overflow` stays `OverflowKw::Visible`
//   (the group initial).
// - CONTROL (must not over-reach): `overflow: scroll` (single-token, admitted)
//   still applies — the computed `overflow` becomes `OverflowKw::Scroll`.
// - SPEC-CORRECT: expand `overflow` to `overflow-x`/`overflow-y` and split the
//   box group's single `overflow` field into `[OverflowKw; 2]` (x, y).
// - D6: admission-gated.
fn unsupported_overflow_axes_and_two_token_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "overflow_two_tokens",
            body: "overflow: scroll auto",
        },
        Case {
            name: "overflow_x_scroll",
            body: "overflow-x: scroll",
        },
        Case {
            name: "overflow_y_scroll",
            body: "overflow-y: scroll",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }

    let (mut world, mut root, target, _report) = styled_target_load(".x { overflow-x: scroll }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .overflow,
        OverflowKw::Visible,
        "overflow-x should stay at its Visible initial",
    );

    let (mut world, mut root, target, _report) = styled_target_load(".x { overflow: scroll }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .overflow,
        OverflowKw::Scroll,
        "overflow: scroll (single-token, admitted) should apply",
    );
}
