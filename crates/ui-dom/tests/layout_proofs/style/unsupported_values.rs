#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals (integer px
// and the computed groups' initial values are bit-exact), so the typed
// computed-group comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Sign-rule and numeric-edge pins for the style system (PLAN.md B1, landing a).
//!
//! Purpose: pin the CURRENT observable behavior of the admitted length grammar
//! at its sign-rule and numeric edges — the divergent rows (negative lengths
//! admitted through the longhand and shorthand paths), the on-spec keeps
//! (legal negatives, the already-rejected illegal ones, and the unitless-zero
//! normalization), and the crash-resistance robustness pins over adversarial
//! numeric inputs.
//!
//! Class notes: divergent rows are named `off_spec_*` and pinned at their
//! current accepted behavior; the `negative_*`, `width_zero_*`, and
//! `fractional_*` rows are on-spec keeps; `robustness_*` rows assert no-panic
//! plus the defined outcome. Two `off_spec_*` rows — `opacity` out-of-range and
//! the `line-height: 0` floor — are reject-side/render-floor divergences that
//! do NOT flip in landing (b) (see their per-test headers).
//!
//! Pointer: the admit-side divergent rows (negative padding/box sizes/border
//! widths/shorthand lengths/font-size, and `font-size: unset`) flip on-spec in
//! the immediately following landing (b), which lands the grammar fix; those
//! `off_spec_*` proof names then relabel on-spec and the registry updates.

use bevy::prelude::World;
use ui_dom::{
    DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, RootDom, StylesheetLoadReport,
    TextIntrinsic,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_decl_drop, assert_root_dom_update,
    assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    take_staged_layer_paint_plans, theme_stylesheet_origin,
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

/// Clones one text node's retained intrinsic metrics.
fn text_intrinsic(root: &RootDom, key: DomNodeSlotKey) -> TextIntrinsic {
    match root.node(key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("proof text node should keep text intrinsic state"),
    }
}

// — Divergent: negative lengths admitted (flip in landing b) — //

#[test]
// OFF-SPEC (divergent):
// - SPEC: CSS Box 3 §4.4 `padding-*` takes `<length [0,∞]>`; a negative
//   length is invalid. <https://drafts.csswg.org/css-box-3/#padding-physical>
// - CURRENT (asserted): `.x { padding-left: -10px }` loads with
//   `declarations_dropped == 0` and computed `padding[3] == -10.0`.
// - SPEC-CORRECT: the declaration drops and is counted; `padding-left`
//   stays at its `0.0` initial.
// - ROOT CAUSE: `parse_length` (`decl_parse.rs:319-321`) is sign-agnostic;
//   the `Length` grammar admits negative scalars for all four padding sides.
// - RECLASSIFY: flips on-spec in landing (b) when the `NonNegativeLength`
//   grammar arm lands for padding.
fn off_spec_negative_padding_accepted() {
    let (mut world, mut root, target, report) = styled_target_load(".x { padding-left: -10px }");
    assert_eq!(report.declarations_applied, 1, "padding-left should apply");
    assert_eq!(
        report.declarations_dropped, 0,
        "padding-left should not drop"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .padding[3],
        -10.0,
        "negative padding-left should reach the box group",
    );
}

#[test]
// OFF-SPEC (divergent):
// - SPEC: CSS Sizing 3 §3.1 `width`/`height` and §3.3/§3.4 `min-*`/`max-*`
//   take `<length-percentage [0,∞] | auto | …>`; negative lengths are
//   invalid. <https://drafts.csswg.org/css-sizing-3/#preferred-size-properties>
// - CURRENT (asserted): each of `width`/`height`/`min-width`/`max-width`/
//   `min-height`/`max-height` at `-1px` loads with `declarations_dropped == 0`
//   and a computed `Some(-1.0)`.
// - SPEC-CORRECT: each declaration drops and is counted; the field stays at
//   its initial (`None`).
// - ROOT CAUSE: the six box-size longhands all share the sign-agnostic
//   `Length` grammar (`presentation_property.rs` METADATA), resolved verbatim
//   by `write_optional_length`.
// - RECLASSIFY: flips on-spec in landing (b) when the `NonNegativeLength`
//   grammar arm lands for box sizes.
fn off_spec_negative_box_sizes_accepted() {
    enum BoxField {
        Width,
        Height,
        MinWidth,
        MaxWidth,
        MinHeight,
        MaxHeight,
    }

    struct Case {
        name: &'static str,
        declaration: &'static str,
        field: BoxField,
    }

    let cases = [
        Case {
            name: "width",
            declaration: "width: -1px",
            field: BoxField::Width,
        },
        Case {
            name: "height",
            declaration: "height: -1px",
            field: BoxField::Height,
        },
        Case {
            name: "min_width",
            declaration: "min-width: -1px",
            field: BoxField::MinWidth,
        },
        Case {
            name: "max_width",
            declaration: "max-width: -1px",
            field: BoxField::MaxWidth,
        },
        Case {
            name: "min_height",
            declaration: "min-height: -1px",
            field: BoxField::MinHeight,
        },
        Case {
            name: "max_height",
            declaration: "max-height: -1px",
            field: BoxField::MaxHeight,
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) =
            styled_target_load(&format!(".x {{ {} }}", case.declaration));
        assert_eq!(report.declarations_applied, 1, "{} should apply", case.name);
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should not drop",
            case.name
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let box_ = root.computed_style(target).expect("target").as_box();
        let computed = match case.field {
            BoxField::Width => box_.width,
            BoxField::Height => box_.height,
            BoxField::MinWidth => box_.min_width,
            BoxField::MaxWidth => box_.max_width,
            BoxField::MinHeight => box_.min_height,
            BoxField::MaxHeight => box_.max_height,
        };
        assert_eq!(computed, Some(-1.0), "{} computed value", case.name);
    }
}

#[test]
// OFF-SPEC (divergent):
// - SPEC: CSS Backgrounds 3 §4.3 `border-width` and CSS UI 4 §4.3
//   `outline-width` take `<line-width>` (non-negative); a negative width is
//   invalid. <https://drafts.csswg.org/css-backgrounds-3/#the-border-width>
// - CURRENT (asserted): `border-top-width: -2px` and `outline-width: -2px`
//   both load with `declarations_dropped == 0`; computed `border_width[0]` and
//   `outline_width` are `-2.0`.
// - SPEC-CORRECT: both drop and are counted; the fields stay at their `0.0`
//   initial.
// - ROOT CAUSE: both longhands use the sign-agnostic `Length` grammar.
// - RECLASSIFY: flips on-spec in landing (b) when the `NonNegativeLength`
//   grammar arm lands for border/outline widths.
fn off_spec_negative_border_and_outline_widths_accepted() {
    let (mut world, mut root, target, report) =
        styled_target_load(".x { border-top-width: -2px; outline-width: -2px }");
    assert_eq!(report.declarations_applied, 2, "both widths should apply");
    assert_eq!(report.declarations_dropped, 0, "neither width should drop");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target");
    assert_eq!(
        view.as_box().border_width[0],
        -2.0,
        "negative border-top-width should reach the box group",
    );
    assert_eq!(
        view.as_visual().outline_width,
        -2.0,
        "negative outline-width should reach the visual group",
    );
}

#[test]
// OFF-SPEC (divergent):
// - SPEC: CSS Backgrounds 3 §5.1 `border-radius` and CSS Box Alignment 3 §4.4
//   `gap` take non-negative `<length-percentage>`; `border-top` (border-width
//   via the side shorthand) is `<line-width>` — all non-negative.
// - CURRENT (asserted): `border-radius: -4px`, `gap: -4px`, and
//   `border-top: -2px` all load with `declarations_dropped == 0`; the computed
//   groups carry the negative values.
// - SPEC-CORRECT: each shorthand drops and is counted; the longhands stay at
//   their initial values.
// - ROOT CAUSE: the expansion helpers bypass the grammar layer — F2 in
//   PLAN.md: `expand_border_radius` (`decl_parse.rs:660`), `expand_gap` (:703),
//   and `expand_border_side` (:930) call the sign-agnostic `parse_length`
//   directly and construct declarations by hand.
// - RECLASSIFY: flips on-spec in landing (b) when expansion applies the
//   per-longhand grammar.
fn off_spec_negative_shorthand_lengths_accepted() {
    enum Shorthand {
        BorderRadius,
        Gap,
        BorderTop,
    }

    struct Case {
        name: &'static str,
        css: &'static str,
        shorthand: Shorthand,
    }

    let cases = [
        Case {
            name: "border_radius_shorthand",
            css: ".x { border-radius: -4px }",
            shorthand: Shorthand::BorderRadius,
        },
        Case {
            name: "gap_shorthand",
            css: ".x { gap: -4px }",
            shorthand: Shorthand::Gap,
        },
        Case {
            name: "border_top_side_alias",
            css: ".x { border-top: -2px }",
            shorthand: Shorthand::BorderTop,
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) = styled_target_load(case.css);
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should not drop",
            case.name
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let view = root.computed_style(target).expect("target");
        match case.shorthand {
            Shorthand::BorderRadius => assert_eq!(
                view.as_visual().radius,
                [-4.0; 4],
                "{} radius expansion",
                case.name
            ),
            Shorthand::Gap => assert_eq!(
                view.as_flex().gap,
                [-4.0, -4.0],
                "{} gap expansion",
                case.name
            ),
            Shorthand::BorderTop => assert_eq!(
                view.as_box().border_width[0],
                -2.0,
                "{} border-top expansion",
                case.name
            ),
        }
    }
}

#[test]
// OFF-SPEC (divergent, crash-adjacent):
// - SPEC: CSS Fonts 4 §3.5 `font-size` takes `<absolute-size> |
//   <relative-size> | <length-percentage [0,∞]> | math`; negative is invalid.
//   <https://drafts.csswg.org/css-fonts-4/#font-size-prop>
// - CURRENT (asserted): `font-size: -10px` loads with `declarations_dropped
//   == 0` and computed `size_px == -10.0` — a negative font size flowing into
//   text layout (crash-adjacent, same family as `font-size: 0`).
// - SPEC-CORRECT: the declaration drops and is counted; `size_px` stays at the
//   inherited seed.
// - ROOT CAUSE: `font-size`'s grammar is `LengthNoUnset`
//   (`presentation_property.rs`), sign-agnostic; `resolve_own_font_size_px`
//   (`winners_fold.rs:324`) stores the px verbatim with no clamp.
// - RECLASSIFY: flips on-spec in landing (b) when `font-size` gains a
//   non-negative grammar and admits `unset` to the seed.
fn off_spec_negative_font_size_accepted() {
    let (mut world, mut root, target, report) = styled_target_load(".x { font-size: -10px }");
    assert_eq!(report.declarations_applied, 1, "font-size should apply");
    assert_eq!(report.declarations_dropped, 0, "font-size should not drop");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_font()
            .size_px,
        -10.0,
        "negative font-size should reach the font group",
    );
}

#[test]
// OFF-SPEC (divergent):
// - SPEC: CSS Cascade 5 §4.3 — `unset` is a CSS-wide keyword; on an inherited
//   property (font-size is inherited, CSS Fonts 4 §3.5 "Inherited: yes") it
//   resolves to inherit. <https://drafts.csswg.org/css-cascade-5/#defaulting-keywords>
// - CURRENT (asserted): `font-size: unset` is dropped + counted
//   (`declarations_dropped == 1`) — the grammar is `LengthNoUnset`, which
//   rejects `unset`.
// - SPEC-CORRECT: `unset` is admitted and resolves to the inherited seed (≈
//   inherit, the winners fold's seed path).
// - ROOT CAUSE: `parse_length_no_unset` (`decl_parse.rs:323-328`) returns
//   `None` for `AuthoredLength::Unset`.
// - RECLASSIFY: flips on-spec in landing (b) when `font-size` admits `unset`.
fn off_spec_font_size_unset_rejected() {
    assert_decl_drop(".x { font-size: unset }", 0, 1);
}

#[test]
// OFF-SPEC (divergent, reject-side — does NOT flip in landing b):
// - SPEC: CSS Color 4 §12.2 `opacity` clamps out-of-range `<opacity-value>`
//   to `[0, 1]` at computed-value time; `1.5` → `1.0`, `-0.5` → `0.0`.
//   <https://drafts.csswg.org/css-color-4/#transparency>
// - CURRENT (asserted): `opacity: 1.5` and `opacity: -0.5` each drop + count
//   (`declarations_dropped == 1`) — range enforced at parse, not computed time.
// - SPEC-CORRECT: admit and clamp at computed time (reject-side divergence of
//   an admitted property's semantics — moves through the D6 admission policy).
// - ROOT CAUSE: `parse_opacity` (`decl_parse.rs:360-367`) rejects the 0.0..=1.0
//   range rather than clamping.
// - RECLASSIFY: stays divergent (reject-side); moves on-spec through admission,
//   NOT through the B1 sign-rule fix.
fn off_spec_opacity_out_of_range_rejected() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "opacity_above_one",
            body: "opacity: 1.5",
        },
        Case {
            name: "opacity_below_zero",
            body: "opacity: -0.5",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// OFF-SPEC (divergent, render-floor — does NOT flip in landing b):
// - SPEC: CSS Inline Layout 3 / CSS 2.1 §10.8 `line-height` takes
//   `<number> | <length> | <percentage> | normal`; `0` is legal and produces
//   zero-height lines. <https://www.w3.org/TR/CSS21/visudet.html#propdef-line-height>
// - CURRENT (asserted): `line-height: 0` is admitted (dropped == 0), but the
//   rendered line height is floored to 1px — the resolver floors at
//   `project.rs:137` and the render side floors at `text_layout/mod.rs:719-722`;
//   the text node's intrinsic `line_height_px == 1.0`.
// - SPEC-CORRECT: zero-height lines render (nothing), no floor.
// - ROOT CAUSE: `line_height.max(1.0)` in `resolve_font_field`
//   (`project.rs:137`) and `effective_line_height_px` (`text_layout/mod.rs:720`);
//   the floor is load-bearing against cosmic-text's `line_height != 0` assert
//   (see the B1 landing note) — its adjudication path is pending.
// - RECLASSIFY: stays divergent; adjudication (remove the floor if cosmic
//   tolerates zero-height lines, else deliberate-with-crash-reason) is a
//   separate call, NOT landing (b).
fn off_spec_line_height_zero_floored_at_one_px() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "x");
    let text = assert_append_text_to(&mut root, container, "zero-height line");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), ".x { line-height: 0 }")
        .expect("line-height: 0 should admit");
    assert_eq!(
        report.declarations_dropped, 0,
        "line-height: 0 should be admitted"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        text_intrinsic(&root, text).line_height_px,
        1.0,
        "line-height: 0 should floor to 1px for the text intrinsic",
    );
}

// — On-spec keeps (pin current-correct) — //

#[test]
// ON-SPEC: CSS 2.1 §8.3/§9.3 and CSS UI 4 §4.3 allow negative margins,
// insets, and outline-offset — these are legal and must stay admitted.
fn negative_margins_and_offsets_apply() {
    let (mut world, mut root, target, report) =
        styled_target_load(".x { margin-left: -10px; top: -5px; outline-offset: -2px }");
    assert_eq!(report.declarations_applied, 3, "all three should apply");
    assert_eq!(report.declarations_dropped, 0, "none should drop");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target");
    assert_eq!(view.as_box().margin[3], -10.0, "negative margin-left");
    assert_eq!(view.as_box().top, Some(-5.0), "negative top inset");
    assert_eq!(
        view.as_visual().outline_offset,
        -2.0,
        "negative outline-offset"
    );
}

#[test]
// ON-SPEC: CSS 2.1 §10.8 (`line-height` non-negative), CSS Flexbox 1 §7.3.1
// `flex-grow` and §7.3.2 `flex-shrink` (`<number [0,∞]>`) — negatives are
// illegal and already rejected by the grammar.
fn negative_line_height_and_flex_factors_rejected() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "negative_line_height",
            body: "line-height: -5px",
        },
        Case {
            name: "negative_flex_grow",
            body: "flex-grow: -1",
        },
        Case {
            name: "negative_flex_shrink",
            body: "flex-shrink: -1",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// ON-SPEC: CSS Values 4 §4.1.1 — a zero `<length>` may be written unitless;
// the grammar's `0` arm admits it and normalizes to `0px` (`parse_authored_length`,
// `dom_attr_parse.rs:120-122`). Guard pin against the "unitless zero is illegal"
// direction.
fn width_zero_unitless_admitted_and_normalized() {
    let (mut world, mut root, target, report) = styled_target_load(".x { width: 0 }");
    assert_eq!(report.declarations_applied, 1, "width: 0 should apply");
    assert_eq!(report.declarations_dropped, 0, "width: 0 should not drop");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        Some(0.0),
        "unitless zero should normalize to 0px",
    );
}

#[test]
// ON-SPEC: CSS Flexbox 1 §5.4 `order` takes an `<integer>`; `1.5` is not an
// integer and is rejected by the grammar.
fn fractional_order_rejected() {
    assert_decl_drop(".x { order: 1.5 }", 0, 1);
}

// — Robustness (no panic + defined outcome) — //

#[test]
// ROBUSTNESS: non-finite length scalars are rejected by
// `parse_finite_length_scalar` (`dom_attr_parse.rs:339-344`), so each of
// `1e400px` (overflow → inf), `NaNpx`, `infpx`, `-infpx` drops + counts with
// no crash. Finite-f32 is a computed-group field invariant (PLAN.md B1 data
// structures).
fn robustness_non_finite_lengths_rejected() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "overflow_to_infinity",
            body: "width: 1e400px",
        },
        Case {
            name: "nan_length",
            body: "width: NaNpx",
        },
        Case {
            name: "infinity_length",
            body: "width: infpx",
        },
        Case {
            name: "negative_infinity_length",
            body: "width: -infpx",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// ROBUSTNESS: the finite extremes of the f32 range — `3.4028235e38px`
// (rounds to `f32::MAX`) and `1.4e-45px` (min subnormal) — parse as finite and
// are admitted without crash. The assertion pins "accepted + finite", not the
// exact bit pattern (the parser's rounding is its own contract).
fn robustness_extreme_finite_lengths() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "max_finite_f32",
            body: "width: 3.4028235e38px",
        },
        Case {
            name: "min_subnormal_f32",
            body: "width: 1.4e-45px",
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_eq!(report.declarations_applied, 1, "{} should apply", case.name);
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should not drop",
            case.name
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let width = root.computed_style(target).expect("target").as_box().width;
        match width {
            Some(w) => assert!(
                w.is_finite(),
                "{} computed width should be finite",
                case.name
            ),
            None => panic!("{} should resolve a finite computed width", case.name),
        }
    }
}

#[test]
// ROBUSTNESS: `border-radius: 10000px` on a 10px box renders without crash.
// The staged paint plan carries the radius verbatim (`corner_radii_px ==
// [10000.0; 4]`) — there is NO clamp at the staged-op level (the paint
// question from PLAN.md's open verifications); any clamp lives in the GPU
// render world, outside this crate's observable surface.
fn robustness_oversized_border_radius() {
    let (mut world, mut root, _target, report) =
        styled_target_load(".x { width: 10px; height: 10px; border-radius: 10000px }");
    assert_eq!(
        report.declarations_dropped, 0,
        "oversized radius should be admitted"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "one layer plan");
    assert_eq!(plans[0].rects.len(), 1, "one rect op for the 10px box");
    assert_eq!(
        plans[0].rects[0].corner_radii_px, [10000.0; 4],
        "oversized radius carried verbatim into the staged rect op",
    );
}

#[test]
// ROBUSTNESS: `translate(999999px, -999999px)` is a finite, legal transform
// and is admitted without clamp; the computed transform carries the raw pair.
// CSS Transforms 1 §5. <https://drafts.csswg.org/css-transforms-1/#funcdef-transform-translate>
fn robustness_extreme_translate() {
    let (mut world, mut root, target, report) =
        styled_target_load(".x { transform: translate(999999px, -999999px) }");
    assert_eq!(
        report.declarations_dropped, 0,
        "extreme translate should be admitted"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_transform()
            .translate_px,
        [999_999.0, -999_999.0],
        "extreme translate should reach the transform group unclamped",
    );
}

#[test]
// ROBUSTNESS (class: unimplemented — implementation range limit):
// CSS Flexbox 1 §5.4 admits any `<integer>`, but the resolver clamps i64 → i32
// saturatingly (`project.rs:444-445`), so `9223372036854775807` (i64::MAX)
// is admitted and clamps to `i32::MAX`. The pin names the clamp; a full-range
// admission is a separate D6 decision (see PLAN.md B1 row).
fn robustness_order_i64_max_clamps_safely() {
    let (mut world, mut root, target, report) =
        styled_target_load(".x { order: 9223372036854775807 }");
    assert_eq!(
        report.declarations_dropped, 0,
        "i64::MAX order should be admitted"
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_flex().order,
        i32::MAX,
        "i64::MAX order should clamp to i32::MAX",
    );
}

#[test]
// ROBUSTNESS (class: unimplemented — same range limit): `99999999999999999999`
// exceeds i64 and fails the integer grammar, dropping + counting with no crash.
fn robustness_order_beyond_i64_dropped() {
    assert_decl_drop(".x { order: 99999999999999999999 }", 0, 1);
}

#[test]
#[should_panic(expected = "font size cannot be 0")]
// OFF-SPEC (divergent): `font-size: 0` is CSS-legal (CSS Fonts 4 §3.5 admits
// zero font sizes; zero-size text simply has zero-size glyphs) and the
// declaration is admitted by our grammar — but measuring or shaping the text
// then PANICS inside cosmic-text (`assert_ne!(font_size, 0.0)`,
// cosmic-text-0.18.2/src/buffer.rs:688, reached via `CosmicMetrics::new` from
// `text_layout/mod.rs`). Boundary data must never panic (AGENTS.md "Panic and
// boundary policy"), so this is a boundary-policy violation, pinned here as
// the current behavior via `should_panic`.
// SPEC-CORRECT: `font-size: 0` measures zero-width text and renders nothing —
// no panic.
// ROOT CAUSE: the zero value flows verbatim from the font group into
// `CosmicMetrics::new` with no zero guard at the text-layout boundary.
// RECLASSIFY: the crash-hardening fix in landing (b) relabels this as
// `robustness_zero_font_size_renders_nothing` — see the off-spec registry.
fn off_spec_font_size_zero_panics() {
    for body in ["font-size: 0", "font-size: 0px"] {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let container = assert_append_div_to(&mut root, document_root);
        assert_set_attr_str(&mut root, container, "class", "x");
        let _text = assert_append_text_to(&mut root, container, "zero-size text");
        let report = root
            .context()
            .load_stylesheet(theme_stylesheet_origin(), &format!(".x {{ {body} }}"))
            .expect("font-size: 0 should admit");
        assert_eq!(report.declarations_dropped, 0, "{body} should be admitted");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    }
}
