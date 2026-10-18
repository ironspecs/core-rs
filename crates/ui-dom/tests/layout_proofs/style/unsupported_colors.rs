#![allow(rustdoc::bare_urls)]
// The color grammar normalizes every admitted form to bit-exact `[f32; 4]`
// channels at the declaration boundary (`parse_css_color_rgba` divides by
// 255.0/100.0), and the computed group initial is a const of the same shape,
// so the typed computed-group comparisons in this module are deliberate exact
// checks.
#![allow(clippy::float_cmp)]

//! Color-grammar boundary pins for the style system (PLAN.md B7).
//!
//! Purpose: pin the boundary of the admitted CSS color grammar — which color
//! forms are accepted into `DeclValue::Color([f32; 4])` and which are dropped +
//! counted as drop-and-continue (CSS 2.1 §4.1.7). The admitted subset is
//! exactly `transparent`, 6/8-digit hex, `rgb()`/`rgba()` (comma- OR
//! space-separated channels; no slash-alpha), and the five named colors
//! `white`/`black`/`red`/`green`/`blue` — all normalized at the declaration
//! boundary by `parse_css_color_rgba` (`dom_attr_parse.rs:136`). Everything
//! outside that subset (3/4-digit hex, the wider CSS named-color set,
//! `currentColor`, and modern `rgb()` slash-alpha) is dropped + counted with the
//! computed `color` staying at its group initial (the near-white
//! `[240/255; 3, 1.0]` design-system default, `computed_color_group.rs:23`).
//!
//! Class notes: the `unsupported_*` rows are unimplemented (D6 admission-gated
//! drop-and-continue — the value grammar form is not admitted). The admitted
//! rows inside `modern_rgb_syntax_is_partially_admitted` (space-separated,
//! comma-separated, `rgba` alpha, and percent channels) are on-spec keeps that
//! pin what the parser already admits today so a later widening cannot silently
//! change them; the slash-alpha rows in that same family are unimplemented (the
//! `/` is not a separator the token splitter understands). The admitted
//! named-color CONTROL row (`color: blue`) proves the named-color pin does not
//! over-reach the admitted set.

use bevy::prelude::World;
use ui_dom::{DomNodeSlotKey, DomPointerUpdate, RootDom, StylesheetLoadReport};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_decl_drop, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
};

/// The absent-`color` group initial (near-white), as pinned in
/// `style/admitted_gaps.rs::initial_color_is_near_white_by_design`
/// (`DEFAULT_TEXT_COLOR_RGBA`, `computed_color_group.rs:23`).
const NEAR_WHITE: [f32; 4] = [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0];

/// The admitted named color `blue` (`parse_named_color_rgba`,
/// `dom_attr_parse.rs:169-171`).
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

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

/// Loads `.x { color: <declaration> }`, runs one update, and returns the
/// target's computed `color_rgba`.
fn computed_color_after_load(declaration: &str) -> [f32; 4] {
    let (mut world, mut root, target, _report) =
        styled_target_load(&format!(".x {{ color: {declaration} }}"));
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.computed_style(target)
        .expect("target")
        .as_color()
        .color_rgba
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Color 4 §4.2 admits the 3-digit (`#RGB`) and 4-digit (`#RGBA`)
//   hex shorthands, each digit duplicated to form the 6/8-digit equivalent
//   (`#abc` → `#aabbcc`, `#abcd` → `#aabbccdd`).
//   <https://drafts.csswg.org/css-color-4/#hex-notation>
// - CURRENT (asserted): `color: #abc` and `color: #abcd` each drop + count
//   (`declarations_dropped == 1`) — `parse_hex_color_rgba` (`dom_attr_parse.rs:
//   245-261`) admits only `hex.len() == 6` or `8`, so 3/4-digit hex returns
//   `Err`; after the load the computed `color` stays at its near-white group
//   initial.
// - SPEC-CORRECT: admit 3/4-digit hex by digit-doubling (one branch in
//   `parse_hex_color_rgba`).
// - D6: admission-gated.
fn unsupported_short_hex_colors_drop() {
    struct Case {
        name: &'static str,
        declaration: &'static str,
    }

    let cases = [
        Case {
            name: "hex3",
            declaration: "color: #abc",
        },
        Case {
            name: "hex4",
            declaration: "color: #abcd",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.declaration), 0, 1);
    }

    assert_eq!(
        computed_color_after_load("#abc"),
        NEAR_WHITE,
        "a dropped short hex color should stay at the near-white group initial",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated) + one admitted
// CONTROL:
// - SPEC: CSS Color 4 §6.1 admits the full named-color set (~148 names, e.g.
//   `rebeccapurple`, `gray`, `teal`, `orange`).
//   <https://drafts.csswg.org/css-color-4/#named-colors>
// - CURRENT (asserted): `rebeccapurple`, `gray`, `teal`, and `orange` each drop
//   + count — `parse_named_color_rgba` (`dom_attr_parse.rs:156-173`) admits
//   exactly `white`/`black`/`red`/`green`/`blue` (plus `transparent`, handled
//   before the named table at `dom_attr_parse.rs:141-143`); every other name
//   returns `None`. After the load the computed `color` stays near-white.
// - CONTROL (asserted): `color: blue` applies (`declarations_applied == 1`,
//   `declarations_dropped == 0`) and computes `[0, 0, 1, 1]`, proving the pin
//   does not over-reach into the admitted set.
// - SPEC-CORRECT: admit the full named-color table (a static sorted
//   `&[(&str, [f32; 4])]`, binary search).
// - D6: admission-gated.
fn unsupported_named_colors_drop() {
    struct Case {
        name: &'static str,
        declaration: &'static str,
    }

    let cases = [
        Case {
            name: "rebeccapurple",
            declaration: "color: rebeccapurple",
        },
        Case {
            name: "gray",
            declaration: "color: gray",
        },
        Case {
            name: "teal",
            declaration: "color: teal",
        },
        Case {
            name: "orange",
            declaration: "color: orange",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.declaration), 0, 1);
    }

    assert_eq!(
        computed_color_after_load("gray"),
        NEAR_WHITE,
        "a dropped named color should stay at the near-white group initial",
    );

    // Admitted CONTROL: the named set is exactly the five colors, and `blue`
    // applies rather than dropping — the pin above must not over-reach.
    let (mut world, mut root, target, report) = styled_target_load(".x { color: blue }");
    assert_eq!(
        report.declarations_applied, 1,
        "color: blue should apply (admitted CONTROL)",
    );
    assert_eq!(
        report.declarations_dropped, 0,
        "color: blue should not drop (admitted CONTROL)",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_color()
            .color_rgba,
        BLUE,
        "color: blue should compute the admitted [0, 0, 1, 1]",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Color 4 §`currentColor` is a value REFERENCE, not a color — it
//   computes to the element's own `color` value, so any property accepting a
//   `<color>` accepts `currentColor`.
//   <https://drafts.csswg.org/css-color-4/#currentcolor-color>
// - CURRENT (asserted): `color: currentColor` drops + counts —
//   `parse_css_color_rgba` has no `currentColor` arm: it is not `transparent`,
//   not hex, not an `rgb()`/`rgba()` function, and not in the five-name table,
//   so it returns `Err`; after the load the computed `color` stays near-white.
// - SPEC-CORRECT: store `Keyword(CurrentColor)` at the declaration and resolve
//   it from the color group at style time (Servo's computed-value shape; see
//   PLAN.md B7). NOTE: `currentColor` on the `color` longhand itself is the
//   cyclic reference — spec computes it to the inherited `color`, not its own.
// - D6: admission-gated.
fn unsupported_current_color_drops() {
    assert_decl_drop(".x { color: currentColor }", 0, 1);

    assert_eq!(
        computed_color_after_load("currentColor"),
        NEAR_WHITE,
        "a dropped currentColor should stay at the near-white group initial",
    );
}

#[test]
// PARTIALLY ADMITTED (verify-and-pin-actual, PLAN.md B7):
// - SPEC: CSS Color 4 §4.2 modern `rgb()` admits space-separated channels and
//   a slash-separated alpha (`rgb(255 0 0 / 50%)`, `rgb(100% 0% 0%)`); the
//   legacy comma form (`rgb(255, 0, 0)`, `rgba(255, 0, 0, 0.5)`) is also
//   admitted. <https://drafts.csswg.org/css-color-4/#rgb-functions>
// - VERIFIED ACTUAL (per `parse_css_color_rgba`, `dom_attr_parse.rs:139-154`):
//   the channel splitter `rgb_function_tokens` (`dom_attr_parse.rs:202-207`)
//   splits on commas OR whitespace, but it has no slash handling — the `/`
//   becomes an extra token, so `parse_rgb_function_args` (`dom_attr_parse.rs:
//   184-200`) rejects the token count (`tokens.len() != required_len`,
//   `dom_attr_parse.rs:187`). Percent channels are admitted through
//   `parse_percent_channel` (`dom_attr_parse.rs:235-243`). The accept/drop map
//   pinned below:
//     accepted  `rgb(255 0 0)`        → [1, 0, 0, 1]   (space-separated)
//     accepted  `rgb(255, 0, 0)`      → [1, 0, 0, 1]   (legacy comma)
//     accepted  `rgba(255, 0, 0, 0.5)` → [1, 0, 0, 0.5] (legacy rgba alpha)
//     dropped   `rgb(255 0 0 / 50%)`  (slash-alpha → 5 tokens, count mismatch)
//     dropped   `rgb(255 0 0 / 0.5)`  (slash-alpha → 5 tokens, count mismatch)
//     accepted  `rgb(100% 0% 0%)`     → [1, 0, 0, 1]   (percent channels)
// - SPEC-CORRECT: admit slash-alpha by treating `/` as the alpha separator in
//   `rgb_function_tokens` (the Phase 2 grammar widening in PLAN.md B7).
// - D6: the accepted forms are on-spec keeps; the dropped slash-alpha forms are
//   admission-gated.
fn modern_rgb_syntax_is_partially_admitted() {
    enum Expected {
        Channels([f32; 4]),
        Dropped,
    }

    struct Case {
        name: &'static str,
        declaration: &'static str,
        expected: Expected,
    }

    let cases = [
        Case {
            name: "space_separated",
            declaration: "rgb(255 0 0)",
            expected: Expected::Channels([1.0, 0.0, 0.0, 1.0]),
        },
        Case {
            name: "legacy_comma",
            declaration: "rgb(255, 0, 0)",
            expected: Expected::Channels([1.0, 0.0, 0.0, 1.0]),
        },
        Case {
            name: "legacy_rgba_alpha",
            declaration: "rgba(255, 0, 0, 0.5)",
            expected: Expected::Channels([1.0, 0.0, 0.0, 0.5]),
        },
        Case {
            name: "slash_alpha_percent",
            declaration: "rgb(255 0 0 / 50%)",
            expected: Expected::Dropped,
        },
        Case {
            name: "slash_alpha_float",
            declaration: "rgb(255 0 0 / 0.5)",
            expected: Expected::Dropped,
        },
        Case {
            name: "percent_channels",
            declaration: "rgb(100% 0% 0%)",
            expected: Expected::Channels([1.0, 0.0, 0.0, 1.0]),
        },
    ];

    for case in cases {
        match case.expected {
            Expected::Channels(channels) => {
                assert_eq!(
                    computed_color_after_load(case.declaration),
                    channels,
                    "{} should compute the admitted channels",
                    case.name,
                );
            }
            Expected::Dropped => {
                assert_decl_drop(
                    &format!(".{} {{ color: {} }}", case.name, case.declaration),
                    0,
                    1,
                );
            }
        }
    }
}
