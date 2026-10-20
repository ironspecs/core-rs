#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths to exact `f32` literals (integer px
// and the computed groups' initial values are bit-exact), so the typed
// computed-group comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Sign-rule, numeric-edge, and units/sizing-keyword pins for the style system
//! (PLAN.md B1 + B2).
//!
//! Purpose: pin the observable behavior of the admitted length grammar at its
//! sign-rule and numeric edges (B1) and the deliberately-unadmitted unit and
//! sizing-keyword grammar (B2). B1 covers the on-spec rejects (negative lengths
//! dropped through both the longhand and shorthand paths), the on-spec keeps
//! (legal negatives, the already-rejected illegal ones, `font-size: unset`
//! inheritance, and the unitless-zero normalization), the deliberate
//! `line-height: 0` render floor, and the crash-resistance robustness pins over
//! adversarial numeric inputs. B2 pins every unadmitted unit (percentages,
//! `calc()`/`min()`/`max()`/`clamp()`, viewport and font-relative units) and
//! sizing keyword (`auto`, intrinsic-size keywords, CSS-wide keywords) as a
//! drop-and-continue gate: the declaration is dropped + counted and the computed
//! value stays at the group initial.
//!
//! Class notes: the `negative_*`, `width_zero_*`, `fractional_*`,
//! `unitless_nonzero_*`, `font_size_unset_*`, and `gap_unset_*` rows are
//! on-spec keeps;
//! `opacity_out_of_range_clamps_at_computed_time` is on-spec (relabeled from
//! `off_spec_opacity_out_of_range_rejected` in PR 1 item 5 after the opacity
//! grammar moved from reject to clamp);
//! `unsupported_*` rows are unimplemented (D6 admission-gated drop-and-continue
//! — the value or grammar form is not admitted); `robustness_*` rows assert
//! no-panic plus the defined outcome; `line_height_zero_floors_at_one_px` is a
//! deliberate crate-owned render floor (see its per-test header). There are no
//! `off_spec_*` rows in this file.

use bevy::prelude::World;
use ui_dom::{
    AlignContentKw, AlignItemsKw, BoxLength, ComputedFlexGroup, DomIntrinsic, DomNode,
    DomNodeSlotKey, DomPointerUpdate, JustifyContentKw, RootDom, StylesheetLoadReport,
    TextIntrinsic,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_span_to, assert_append_text_to, assert_append_textarea_to,
    assert_close, assert_decl_drop, assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, box_px, take_staged_layer_paint_plans,
    theme_stylesheet_origin,
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

// — On-spec sign-rule rejects: negative lengths dropped (flipped in B1 landing b) — //

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Box 3 §4.4 `padding-*` takes `<length [0,∞]>`; a negative
//   length is invalid. <https://drafts.csswg.org/css-box-3/#padding-physical>
// - Relabeled from `off_spec_negative_padding_accepted` in B1 landing (b) after
//   the sign-rule grammar fix.
fn negative_padding_rejected() {
    assert_decl_drop(".x { padding-left: -10px }", 0, 1);
    let (mut world, mut root, target, _report) = styled_target_load(".x { padding-left: -10px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .padding[3],
        box_px(0.0),
        "negative padding-left should stay at its 0.0 initial",
    );
}

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Sizing 3 §3.1 `width`/`height` and §3.3/§3.4 `min-*`/`max-*`
//   take `<length-percentage [0,∞] | auto | …>`; negative lengths are
//   invalid. <https://drafts.csswg.org/css-sizing-3/#preferred-size-properties>
// - Relabeled from `off_spec_negative_box_sizes_accepted` in B1 landing (b)
//   after the sign-rule grammar fix.
fn negative_box_sizes_rejected() {
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
        property: &'static str,
        field: BoxField,
    }

    let cases = [
        Case {
            name: "width",
            property: "width",
            field: BoxField::Width,
        },
        Case {
            name: "height",
            property: "height",
            field: BoxField::Height,
        },
        Case {
            name: "min_width",
            property: "min-width",
            field: BoxField::MinWidth,
        },
        Case {
            name: "max_width",
            property: "max-width",
            field: BoxField::MaxWidth,
        },
        Case {
            name: "min_height",
            property: "min-height",
            field: BoxField::MinHeight,
        },
        Case {
            name: "max_height",
            property: "max-height",
            field: BoxField::MaxHeight,
        },
    ];

    for case in cases {
        let css = format!(".x {{ {}: -1px }}", case.property);
        assert_decl_drop(&css, 0, 1);
        let (mut world, mut root, target, _report) = styled_target_load(&css);
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
        assert_eq!(
            computed,
            BoxLength::Auto,
            "{} should stay at its initial Auto",
            case.name
        );
    }
}

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Backgrounds 3 §4.3 `border-width` and CSS UI 4 §4.3
//   `outline-width` take `<line-width>` (non-negative); a negative width is
//   invalid. <https://drafts.csswg.org/css-backgrounds-3/#the-border-width>
// - Relabeled from `off_spec_negative_border_and_outline_widths_accepted` in
//   B1 landing (b) after the sign-rule grammar fix.
fn negative_border_and_outline_widths_rejected() {
    assert_decl_drop(".x { border-top-width: -2px; outline-width: -2px }", 0, 2);
    let (mut world, mut root, target, _report) =
        styled_target_load(".x { border-top-width: -2px; outline-width: -2px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target");
    assert_eq!(
        view.as_box().border_width[0],
        0.0,
        "negative border-top-width should stay at its 0.0 initial",
    );
    assert_eq!(
        view.as_visual().outline_width,
        0.0,
        "negative outline-width should stay at its 0.0 initial",
    );
}

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Backgrounds 3 §5.1 `border-radius` and CSS Box Alignment 3 §4.4
//   `gap` take non-negative `<length-percentage>`; `border-top` (border-width
//   via the side shorthand) is `<line-width>` — all non-negative.
// - Relabeled from `off_spec_negative_shorthand_lengths_accepted` in B1
//   landing (b) after the sign-rule grammar fix (expansion now routes through
//   the per-longhand grammar).
fn negative_shorthand_lengths_rejected() {
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
        assert_decl_drop(case.css, 0, 1);
        let (mut world, mut root, target, _report) = styled_target_load(case.css);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let view = root.computed_style(target).expect("target");
        match case.shorthand {
            Shorthand::BorderRadius => assert_eq!(
                view.as_visual().radius,
                [0.0; 4],
                "{} radius should stay at its 0.0 initial",
                case.name
            ),
            Shorthand::Gap => assert_eq!(
                view.as_flex().gap,
                [0.0, 0.0],
                "{} gap should stay at its 0.0 initial",
                case.name
            ),
            Shorthand::BorderTop => assert_eq!(
                view.as_box().border_width[0],
                0.0,
                "{} border-top should stay at its 0.0 initial",
                case.name
            ),
        }
    }
}

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Fonts 4 §3.5 `font-size` takes `<absolute-size> |
//   <relative-size> | <length-percentage [0,∞]> | math`; negative is invalid.
//   <https://drafts.csswg.org/css-fonts-4/#font-size-prop>
// - Relabeled from `off_spec_negative_font_size_accepted` in B1 landing (b)
//   after the sign-rule grammar fix.
fn negative_font_size_rejected() {
    assert_decl_drop(".x { font-size: -10px }", 0, 1);
    let (mut world, mut root, target, _report) = styled_target_load(".x { font-size: -10px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_font()
            .size_px,
        14.0,
        "negative font-size should stay at the inherited 14px seed",
    );
}

#[test]
// ON-SPEC (flipped in B1 landing b):
// - SPEC: CSS Cascade 5 §4.3 — `unset` is a CSS-wide keyword; on an inherited
//   property (font-size is inherited, CSS Fonts 4 §3.5 "Inherited: yes") it
//   resolves to inherit. <https://drafts.csswg.org/css-cascade-5/#defaulting-keywords>
// - Relabeled from `off_spec_font_size_unset_rejected` in B1 landing (b) after
//   the sign-rule grammar fix (font-size now admits `unset`).
// - Competing-rule row (b): the admitted `unset` participates in the cascade —
//   a later `.a { font-size: unset }` beats an earlier `.b { font-size: 30px }`
//   on the same element (equal specificity, source order wins), takes the
//   winners slot, then defaults to inherit (20px), NOT 30px.
fn font_size_unset_resolves_to_inherited_seed() {
    // (a) baseline: unset on an inherited property resolves to inherit.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "p");
    let span = assert_append_span_to(&mut root, container);
    assert_set_attr_str(&mut root, span, "class", "x");
    let report = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".p { font-size: 20px } .x { font-size: unset }",
        )
        .expect("font-size: unset should admit");
    assert_eq!(
        report.declarations_dropped, 0,
        "font-size: unset should be admitted",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(span).expect("span").as_font().size_px,
        20.0,
        "font-size: unset should resolve to the inherited 20px seed",
    );

    // (b) competing-rule cascade: the admitted `unset` takes the winners slot
    // (the later `.a` beats the earlier `.b` by source order at equal
    // specificity), then defaults to inherit — the parent's 20px, NOT the 30px
    // `.b` wrote.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "p");
    let span = assert_append_span_to(&mut root, container);
    assert_set_attr_str(&mut root, span, "class", "b a");
    let report = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".p { font-size: 20px } .b { font-size: 30px } .a { font-size: unset }",
        )
        .expect("font-size: unset should admit");
    assert_eq!(
        report.declarations_dropped, 0,
        "font-size: unset should be admitted",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(span).expect("span").as_font().size_px,
        20.0,
        "the admitted unset should win its slot over the 30px rule and resolve to the inherited 20px",
    );
}

#[test]
// ON-SPEC (admitted since the B1 grammar change):
// - SPEC: CSS Cascade 5 §4.3 — `unset` on a NON-inherited property (`gap` is
//   not inherited, CSS Box Alignment 3 §4.4 "Inherited: no") resolves to the
//   property's initial value. <https://drafts.csswg.org/css-cascade-5/#defaulting-keywords>
// - B1b admission: `gap: unset` expands to `row-gap: unset; column-gap: unset`
//   (`expand_gap`, `decl_parse.rs:725-735`) and the resolver's
//   `write_inset_length` (`project.rs:605-611`) writes the initial 0.0 into each
//   flex-gap slot. Mirrors `font_size_unset_resolves_to_inherited_seed`'s
//   shape, but on a non-inherited property the default is the initial, not the
//   inherited value.
fn gap_unset_resolves_to_zero() {
    let (mut world, mut root, target, report) = styled_target_load(".x { gap: unset }");
    assert_eq!(
        report.declarations_dropped, 0,
        "gap: unset should be admitted",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_flex().gap,
        [0.0, 0.0],
        "gap: unset should resolve to the [0.0, 0.0] initial",
    );
}

#[test]
// ON-SPEC (relabeled from `off_spec_opacity_out_of_range_rejected` in PR 1
// item 5 after the grammar moved from reject to clamp):
// - SPEC: CSS Color 4 §12.2 `opacity` clamps out-of-range `<opacity-value>`
//   to `[0, 1]` at computed-value time; `1.5` → `1.0`, `-0.5` → `0.0`.
//   <https://drafts.csswg.org/css-color-4/#transparency>
// - `parse_opacity` (`decl_parse.rs`) now admits any finite f32 and clamps to
//   `[0, 1]`, so `opacity: 1.5` computes to `1.0` and `opacity: -0.5` to
//   `0.0` (the declaration is valid at any finite value — it clamps, never
//   drops).
fn opacity_out_of_range_clamps_at_computed_time() {
    struct Case {
        name: &'static str,
        body: &'static str,
        expected: f32,
    }

    let cases = [
        Case {
            name: "opacity_above_one",
            body: "opacity: 1.5",
            expected: 1.0,
        },
        Case {
            name: "opacity_below_zero",
            body: "opacity: -0.5",
            expected: 0.0,
        },
    ];

    for case in cases {
        let (mut world, mut root, target, report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_eq!(
            report.declarations_dropped, 0,
            "{} should be admitted, not dropped",
            case.name,
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target)
                .expect("target")
                .as_visual()
                .opacity,
            case.expected,
            "{} should clamp to the [0, 1] range",
            case.name,
        );
    }
}

#[test]
// ON-SPEC (flipped in review F4):
// - SPEC: CSS Box Alignment 3 §8.2 admits `normal | <length-percentage>` on
//   the gap longhands; `normal` computes to zero, exactly as the `gap`
//   shorthand maps it. <https://drafts.csswg.org/css-align-3/#column-row-gap>
// - Relabeled from `off_spec_row_gap_normal_rejected` in review F4 (b) after
//   the gap longhands gained the `Gap` grammar (whose `normal` arm resolves to
//   `0px`, matching the shorthand's `expand_gap`).
// - Coherence guard: the `.x { gap: normal }` shorthand row (already admitted)
//   keeps resolving to `[0.0, 0.0]`, so the longhands and shorthand now agree.
fn row_gap_normal_resolves_to_zero() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "row_gap_normal",
            body: "row-gap: normal",
        },
        Case {
            name: "column_gap_normal",
            body: "column-gap: normal",
        },
        Case {
            name: "gap_shorthand_normal",
            body: "gap: normal",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".x {{ {} }}", case.body), 1, 0);
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target).expect("target").as_flex().gap,
            [0.0, 0.0],
            "{} should resolve to [0.0, 0.0]",
            case.name,
        );
    }
}

#[test]
// DELIBERATE (crate-owned render floor — does NOT flip):
// - CURRENT: `line-height: 0` is admitted and floored to 1px at RESOLVE time
//   (`project.rs:137`) with a render backstop (`text_layout/mod.rs:719-722`);
//   the text node's intrinsic `line_height_px == 1.0`.
// - SPEC-CORRECT: CSS 2.1 §10.8 admits `line-height: 0` (zero-height lines).
//   <https://www.w3.org/TR/CSS21/visudet.html#propdef-line-height>
// - RATIONALE (deliberate): the floor is load-bearing against cosmic-text's
//   `line_height != 0` assertion (`cosmic-text-0.18.2/src/buffer.rs:254`) — an
//   engine-dependency constraint, pinned permanently and re-reviewed if the
//   dependency changes.
fn line_height_zero_floors_at_one_px() {
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
    assert_eq!(
        view.as_box().margin[3],
        box_px(-10.0),
        "negative margin-left"
    );
    assert_eq!(view.as_box().top, box_px(-5.0), "negative top inset");
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
        box_px(0.0),
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
// `parse_finite_length_scalar` (`dom_attr_parse.rs:342-347`), so each of
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
            BoxLength::Px(w) => assert!(
                w.is_finite(),
                "{} computed width should be finite",
                case.name
            ),
            other => panic!(
                "{} should resolve a finite px computed width, got {other:?}",
                case.name
            ),
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
// ROBUSTNESS (flipped in B1 landing b):
// - SPEC: CSS Fonts 4 §3.5 — `font-size: 0` is legal; zero-size text has
//   zero-size glyphs and renders nothing.
//   <https://drafts.csswg.org/css-fonts-4/#font-size-prop>
// - Relabeled from `off_spec_font_size_zero_panics` in B1 landing (b) after
//   the zero-size guard.
fn robustness_zero_font_size_renders_nothing() {
    for body in ["font-size: 0", "font-size: 0px"] {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let container = assert_append_div_to(&mut root, document_root);
        assert_set_attr_str(&mut root, container, "class", "x");
        let text = assert_append_text_to(&mut root, container, "zero-size text");
        let report = root
            .context()
            .load_stylesheet(theme_stylesheet_origin(), &format!(".x {{ {body} }}"))
            .expect("font-size: 0 should admit");
        assert_eq!(report.declarations_dropped, 0, "{body} should be admitted");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let intrinsic = text_intrinsic(&root, text);
        assert_eq!(
            intrinsic.min_content_width_px, 0.0,
            "{body} min content width should be 0",
        );
        assert_eq!(
            intrinsic.max_content_width_px, 0.0,
            "{body} max content width should be 0",
        );
        assert_eq!(intrinsic.font_size_px, 0.0, "{body} font size should be 0");
        let plans = take_staged_layer_paint_plans(&root);
        assert!(
            plans.iter().all(|plan| plan.text_runs.is_empty()),
            "{body} should stage no text runs",
        );
    }

    // Companion: a zero font size with an explicit line height still resolves
    // the line height, with zero-width content.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "x");
    let text = assert_append_text_to(&mut root, container, "zero-size text");
    let report = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".x { font-size: 0; line-height: 20px }",
        )
        .expect("font-size: 0 with line-height should admit");
    assert_eq!(
        report.declarations_dropped, 0,
        "font-size: 0 with line-height should be admitted",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let intrinsic = text_intrinsic(&root, text);
    assert_eq!(
        intrinsic.line_height_px, 20.0,
        "line-height: 20px should resolve with a zero font size",
    );
    assert_eq!(
        intrinsic.min_content_width_px, 0.0,
        "zero font size with line-height should have zero min content width",
    );
    assert_eq!(
        intrinsic.max_content_width_px, 0.0,
        "zero font size with line-height should have zero max content width",
    );
    assert_eq!(
        intrinsic.font_size_px, 0.0,
        "zero font size with line-height should keep font size 0",
    );
}

#[test]
// ROBUSTNESS: a focused multi-line `textarea` with `font-size: 0` must not
// panic (the caret path builds a wrapped buffer with width_px set, which
// reaches cosmic-text's set_metrics_and_size assert without the guard).
// Relates to the zero-size guard family; the multi-line caret path was the
// last uncovered cosmic call site (single-line inputs never reach the assert).
fn robustness_zero_font_size_textarea_caret_does_not_crash() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let textarea = assert_append_textarea_to(&mut root, document_root);
    assert_set_attr_str(&mut root, textarea, "value", "line one\nline two");
    assert_style_set(&mut root, textarea, "width", "120px");
    assert_style_set(&mut root, textarea, "height", "40px");
    assert_style_set(&mut root, textarea, "font-size", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Focus installs a collapsed text selection for the textarea, which is what
    // the caret paint op keys on. The following update rebuilds the paint plan
    // and drives `text_caret_layout` for the focused multi-line control.
    root.focus(textarea).expect("textarea should focus");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    assert!(
        !plans.is_empty(),
        "focused textarea should stage a paint plan",
    );
}

// — B2: units & sizing keywords — //

#[test]
// ON-SPEC (relabeled from `unsupported_percentage_lengths_drop` in PR 1 item 7):
// - SPEC: CSS Values 4 §percentages — `width`, `margin-left`, `font-size`, and
//   `line-height` each admit a `<percentage>`, resolved per property.
//   <https://drafts.csswg.org/css-values-4/#percentages>
// - CURRENT (asserted): box-group percentages stay symbolic in the computed
//   group (`BoxLength::Percent`) and resolve against the containing-block WIDTH
//   at layout; font-relative percentages resolve at style time (font-size
//   against the parent seed, line-height against the own size).
fn percentage_lengths_resolve_against_their_bases() {
    // width: 50% of a 200px container -> computed `BoxLength::Percent(50.0)`
    // AND used width 100.0.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "width", "50%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child).expect("child").as_box().width,
        BoxLength::Percent(50.0),
        "width: 50% should stay symbolic in the computed group",
    );
    assert_eq!(
        ui_dom::node_box(&root, child)
            .expect("box")
            .border_box
            .width_px,
        100.0,
        "width: 50% should resolve to a 100px used width",
    );

    // margin-left: 10% of a 200px containing block -> 20.0 used margin.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "margin-left", "10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child).expect("child").as_box().margin[3],
        BoxLength::Percent(10.0),
        "margin-left: 10% should stay symbolic in the computed group",
    );
    assert_eq!(
        ui_dom::node_box(&root, child).expect("box").margin.left_px,
        20.0,
        "margin-left: 10% should resolve to a 20px used margin",
    );

    // font-size: 120% under a 20px parent -> 24.0 (style-time resolution).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "font-size", "20px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "font-size", "120%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child).expect("child").as_font().size_px,
        24.0,
        "font-size: 120% should resolve against the parent seed",
    );

    // line-height: 150% under a 20px own size -> 30.0.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "font-size", "20px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "font-size", "20px");
    assert_style_set(&mut root, child, "line-height", "150%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child)
            .expect("child")
            .as_font()
            .line_height_px,
        Some(30.0),
        "line-height: 150% should resolve against the own font size",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Values 4 §10 — `calc()`, `min()`, `max()`, `clamp()` are math
//   functions admissible wherever a length is.
//   <https://drafts.csswg.org/css-values-4/#math>
// - CURRENT (asserted): each drops + counts. The paren-bearing values reach the
//   grammar (the declaration splitter is semicolon-based, so nothing splits on
//   the commas/parens) and fail there — no crash, no fatal load rejection.
// - SPEC-CORRECT: admit a bounded `CalcExpr` (see PLAN.md B2 data structures).
// - D6: admission-gated.
fn unsupported_math_functions_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "calc_mixed_percent",
            body: "width: calc(100% - 10px)",
        },
        Case {
            name: "calc_px_sum",
            body: "width: calc(1px + 1px)",
        },
        Case {
            name: "min_fn",
            body: "width: min(1px, 2px)",
        },
        Case {
            name: "max_fn",
            body: "width: max(1px, 2px)",
        },
        Case {
            name: "clamp_fn",
            body: "width: clamp(1px, 2px, 3px)",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Values 4 §7 — `vw`, `vh`, `vmin`, `vmax` are viewport-percentage
//   lengths resolved against the viewport dimensions.
//   <https://drafts.csswg.org/css-values-4/#viewport-relative-lengths>
// - CURRENT (asserted): each viewport unit drops + counts.
// - SPEC-CORRECT: admit viewport units (style-time resolution against the
//   viewport the composite stage knows — see PLAN.md B2).
// - D6: admission-gated.
fn unsupported_viewport_units_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "vw",
            body: "width: 100vw",
        },
        Case {
            name: "vh",
            body: "height: 50vh",
        },
        Case {
            name: "vmin",
            body: "width: 10vmin",
        },
        Case {
            name: "vmax",
            body: "width: 10vmax",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Values 4 §7.1 — `rem` (root em), `ex`, `ch`, `lh`, `rlh` are
//   font-relative lengths.
//   <https://drafts.csswg.org/css-values-4/#font-relative-lengths>
// - CURRENT (asserted): each font-relative unit drops + counts (the grammar
//   admits only `px` and `em` today).
// - SPEC-CORRECT: admit font-relative units (`rem` against the root font size,
//   etc. — see PLAN.md B2).
// - D6: admission-gated.
fn unsupported_font_relative_units_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "rem",
            body: "font-size: 2rem",
        },
        Case {
            name: "ex",
            body: "width: 1ex",
        },
        Case {
            name: "ch",
            body: "width: 2ch",
        },
        Case {
            name: "lh",
            body: "line-height: 1lh",
        },
        Case {
            name: "rlh",
            body: "line-height: 1rlh",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// ON-SPEC (relabeled from `unsupported_auto_lengths_drop` in PR 1 item 7):
// - SPEC: CSS Sizing 3 §3.1 `width` admits `auto` (available/shrink-to-fit);
//   CSS Box 3 §4.2 `margin-top` admits `auto` (free-space absorption at
//   layout, CSS 2.1 §10.3.3).
//   <https://drafts.csswg.org/css-sizing-3/#preferred-size-properties>
// - CURRENT (asserted): `auto` is admitted, kept symbolic in the computed group
//   (`BoxLength::Auto`), and resolved at layout — width to the old `None`
//   auto-fill behavior, vertical auto margins to 0 (§10.3.3), and the
//   block-centering case to equal horizontal margin shares.
fn auto_lengths_are_admitted_and_resolve() {
    // (a) width: auto — admitted, computed width `BoxLength::Auto`, used width
    // fills the 200px container (the old `None` auto-fill behavior).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "width", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child).expect("child").as_box().width,
        BoxLength::Auto,
        "width: auto should stay symbolic in the computed group",
    );
    assert_eq!(
        ui_dom::node_box(&root, child)
            .expect("box")
            .border_box
            .width_px,
        200.0,
        "width: auto should fill the 200px containing block",
    );

    // (b) margin-top: auto — admitted, computed `BoxLength::Auto`, used margin
    // resolves to 0 (vertical auto margins are 0 per §10.3.3).
    let (mut world, mut root, target, _report) = styled_target_load(".x { margin-top: auto }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().margin[0],
        BoxLength::Auto,
        "margin-top: auto should stay symbolic in the computed group",
    );
    assert_eq!(
        ui_dom::node_box(&root, target).expect("box").margin.top_px,
        0.0,
        "margin-top: auto should resolve to a 0 used margin",
    );

    // (c) block centering (the consumer ask): a 100px-wide block with
    // `margin-left: auto; margin-right: auto` in a 200px container absorbs
    // (50.0, 50.0) and lands its left edge at 50.0.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "width", "100px");
    assert_style_set(&mut root, child, "margin-left", "auto");
    assert_style_set(&mut root, child, "margin-right", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let box_ = ui_dom::node_box(&root, child).expect("box");
    assert_eq!(
        box_.margin.left_px, 50.0,
        "centering left margin should absorb 50px"
    );
    assert_eq!(
        box_.margin.right_px, 50.0,
        "centering right margin should absorb 50px",
    );
    assert_eq!(
        box_.border_box.left_px, 50.0,
        "the centered block's left edge should land at 50.0",
    );
}

#[test]
// ON-SPEC (PR 1 follow-up: `margin: 0 auto` shorthand centering):
// - SPEC: CSS Box 3 §4.2 `margin` is a 1–4 value shorthand; the 2-value form
//   `margin: 0 auto` sets the vertical margins to 0 and the horizontal margins
//   to `auto`, and a horizontal `auto` pair absorbs equal free space
//   (CSS 2.1 §10.3.3). <https://drafts.csswg.org/css-box-3/#margin-shorthand>
// - CURRENT (asserted): `margin: 0 auto` expands via the corner map to
//   `margin-top: 0; margin-right: auto; margin-bottom: 0; margin-left: auto`;
//   the horizontal `auto` pair absorbs (50.0, 50.0) and the block's left edge
//   lands at 50.0 — the same outcome as the longhand `margin-left/right: auto`
//   centering pin above.
fn margin_zero_auto_centers_a_fixed_width_block() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "width", "100px");
    assert_style_set(&mut root, child, "margin", "0 auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let box_ = ui_dom::node_box(&root, child).expect("box");
    assert_eq!(
        box_.margin.left_px, 50.0,
        "centering left margin should absorb 50px"
    );
    assert_eq!(
        box_.margin.right_px, 50.0,
        "centering right margin should absorb 50px"
    );
    assert_eq!(
        box_.border_box.left_px, 50.0,
        "the centered block's left edge should land at 50.0"
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated): the `auto` keyword
// is invalid for padding, border widths, and font sizes (they are NOT in the
// closed `AUTO_ADMISSIBLE` table), so each drops + counts and the computed
// value stays at its group initial.
fn padding_auto_is_rejected() {
    assert_decl_drop(".x { padding-left: auto }", 0, 1);
    assert_decl_drop(".x { padding: auto }", 0, 1);
    assert_decl_drop(".x { border-top-width: auto }", 0, 1);
    assert_decl_drop(".x { font-size: auto }", 0, 1);

    let (mut world, mut root, target, _report) = styled_target_load(".x { padding-left: auto }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .padding[3],
        box_px(0.0),
        "padding-left: auto should stay at its 0.0 initial",
    );
}

#[test]
// ON-SPEC sign-rule split (PR 1 item 7): `padding-left: -10%` is invalid
// (padding takes `<length-percentage [0,∞]>`) and drops; `margin-left: -10%`
// is valid (signed `Length`) and resolves negative against the containing
// block.
fn negative_percentage_on_padding_is_rejected() {
    assert_decl_drop(".x { padding-left: -10% }", 0, 1);
    let (mut world, mut root, target, _report) = styled_target_load(".x { padding-left: -10% }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_box()
            .padding[3],
        box_px(0.0),
        "padding-left: -10% should stay at its 0.0 initial",
    );

    // margin-left: -10% is admitted and resolves negative (-20px against a
    // 200px containing block).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "width", "200px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "margin-left", "-10%");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(child).expect("child").as_box().margin[3],
        BoxLength::Percent(-10.0),
        "margin-left: -10% should stay symbolic in the computed group",
    );
    assert_eq!(
        ui_dom::node_box(&root, child).expect("box").margin.left_px,
        -20.0,
        "margin-left: -10% should resolve to a -20px used margin",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Sizing 3 §5 — `min-content`, `max-content`, `fit-content` are
//   intrinsic-size keywords for `width`/`height`.
//   <https://drafts.csswg.org/css-sizing-3/#intrinsic>
// - CURRENT (asserted): each drops + counts.
// - SPEC-CORRECT: admit intrinsic keywords (see PLAN.md B2).
// - D6: admission-gated.
fn unsupported_intrinsic_size_keywords_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "min_content",
            body: "width: min-content",
        },
        Case {
            name: "max_content",
            body: "width: max-content",
        },
        Case {
            name: "fit_content",
            body: "width: fit-content",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Box Alignment 3 §8.2 admits `normal | <length-percentage>` on the
//   gap longhands (and the `gap` shorthand), so `gap: 10%` / `row-gap: 10%` are
//   valid and resolve against the flex container's content box.
//   <https://drafts.csswg.org/css-align-3/#column-row-gap>
// - CURRENT (asserted): the percentage form drops + counts — `parse_gap_longhand`
//   (`decl_parse.rs:478-486`) rejects `DeclLength::Percent`, and `expand_gap`
//   (`decl_parse.rs:914-927`) routes the shorthand through the same gate. The
//   flex gap field is `[f32; 2]` and percentage resolution against the flex
//   container's content box is out of scope (PR 1 item 7); admitting the value
//   with no resolution target would be a silent no-op, so it keeps dropping.
// - SPEC-CORRECT: admit `<length-percentage>` on gap and resolve against the
//   flex container's content box.
// - D6: admission-gated.
fn unsupported_gap_percentages_drop() {
    for body in ["gap: 10%", "row-gap: 10%"] {
        assert_decl_drop(&format!(".x {{ {body} }}"), 0, 1);
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {body} }}"));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target).expect("target").as_flex().gap,
            [0.0, 0.0],
            "{body} should drop and keep the [0.0, 0.0] initial",
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — the grammar form is closed; the drop is
// spec-correct):
// - SPEC: CSS Backgrounds 3 §4.3 `border-*-width` and CSS UI 4 §4.1
//   `outline-width` take `<line-width>` (a `<length>` only, NO percentages);
//   CSS UI 4 §4.3 `outline-offset` takes `<length>` (signed, NO percentages).
//   <https://drafts.csswg.org/css-backgrounds-3/#the-border-width>
//   <https://drafts.csswg.org/css-ui-4/#outline-width>
// - CURRENT (asserted): each percentage drops + counts and the computed field
//   stays at its group initial — the width longhands use the `LineWidth`
//   grammar and outline-offset the `SignedLineWidth` grammar, neither of which
//   admits a percentage arm. The drop IS the spec-correct outcome for these
//   families (no `<length-percentage>` in the grammar); the row is classed
//   unimplemented only in that the percentage form stays unadmitted, not
//   because the drop diverges from spec.
// - SPEC-CORRECT: already spec-correct — `<line-width>`/`<length>` never admit
//   a percentage.
fn unsupported_line_width_percentages_drop() {
    enum TargetField {
        BorderTopWidth,
        OutlineWidth,
        OutlineOffset,
    }

    struct Case {
        name: &'static str,
        body: &'static str,
        field: TargetField,
    }

    let cases = [
        Case {
            name: "border_top_width_percent",
            body: "border-top-width: 10%",
            field: TargetField::BorderTopWidth,
        },
        Case {
            name: "outline_width_percent",
            body: "outline-width: 10%",
            field: TargetField::OutlineWidth,
        },
        Case {
            name: "outline_offset_percent",
            body: "outline-offset: 10%",
            field: TargetField::OutlineOffset,
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".x {{ {} }}", case.body), 0, 1);
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {} }}", case.body));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        let view = root.computed_style(target).expect("target");
        match case.field {
            TargetField::BorderTopWidth => assert_eq!(
                view.as_box().border_width[0],
                0.0,
                "{} should keep border_width[0] at its 0.0 initial",
                case.name
            ),
            TargetField::OutlineWidth => assert_eq!(
                view.as_visual().outline_width,
                0.0,
                "{} should keep outline_width at its 0.0 initial",
                case.name
            ),
            TargetField::OutlineOffset => assert_eq!(
                view.as_visual().outline_offset,
                0.0,
                "{} should keep outline_offset at its 0.0 initial",
                case.name
            ),
        }
    }

    // Positive controls: the same properties with px lengths apply.
    let (mut world, mut root, target, report) =
        styled_target_load(".x { border-top-width: 2px; outline-width: 2px; outline-offset: 1px }");
    assert_eq!(
        report.declarations_dropped, 0,
        "px line widths and offset should not drop",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let view = root.computed_style(target).expect("target");
    assert_eq!(
        view.as_box().border_width[0],
        2.0,
        "border-top-width: 2px should apply",
    );
    assert_eq!(
        view.as_visual().outline_width,
        2.0,
        "outline-width: 2px should apply",
    );
    assert_eq!(
        view.as_visual().outline_offset,
        1.0,
        "outline-offset: 1px should apply",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Backgrounds 3 §5.1 `border-*-radius` takes `<length-percentage>`,
//   so `border-radius: 50%` is valid and resolves per-corner to the elliptical
//   horizontal/vertical radii.
//   <https://drafts.csswg.org/css-backgrounds-3/#the-border-radius>
// - CURRENT (asserted): the percentage drops + counts and the radius stays
//   `[0.0; 4]` — the radius longhands use the `LineWidth` grammar (no
//   percentages), rejected at admission exactly like `gap` percentages
//   (`parse_gap_longhand`). The circular-corner model (`radius: [f32; 4]`, one
//   radius per corner) cannot represent the elliptical resolution CSS requires
//   (50% on a non-square border box computes different horizontal and vertical
//   radii per corner), so admitting the value would be a silent no-op.
// - SPEC-CORRECT: admit `<length-percentage>` and resolve against the border
//   box per corner pair.
// - D6: admission-gated on a corner-pair (elliptical) model.
fn unsupported_border_radius_percentage_drops() {
    for body in ["border-radius: 50%", "border-top-left-radius: 50%"] {
        assert_decl_drop(&format!(".x {{ {body} }}"), 0, 1);
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {body} }}"));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target)
                .expect("target")
                .as_visual()
                .radius,
            [0.0; 4],
            "{body} should drop and keep the [0.0; 4] initial",
        );
    }

    // Positive control: a px radius applies to all four corners.
    let (mut world, mut root, target, report) = styled_target_load(".x { border-radius: 8px }");
    assert_eq!(
        report.declarations_dropped, 0,
        "border-radius: 8px should not drop",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_visual()
            .radius,
        [8.0; 4],
        "border-radius: 8px should compute to [8.0; 4]",
    );
}

#[test]
// ON-SPEC: CSS Values 4 §4.1.1 — a non-zero `<length>` requires a unit;
// `width: 10` is invalid. The grammar's `0` arm admits only the literal zero,
// so `10` drops + counts. Pin to keep (a unitless non-zero length must never
// silently become admitted).
fn unitless_nonzero_length_rejected() {
    assert_decl_drop(".x { width: 10 }", 0, 1);
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Cascade 5 §4 — `inherit`, `initial`, `revert`, `revert-layer`
//   are CSS-wide keywords valid on every property.
//   <https://drafts.csswg.org/css-cascade-5/#defaulting-keywords>
// - CURRENT (asserted): each drops + counts; only `unset` is admitted, and only
//   where the grammar says so (see `font_size_unset_resolves_to_inherited_seed`).
// - SPEC-CORRECT: admit the CSS-wide keyword family.
// - D6: admission-gated.
fn unsupported_css_wide_keywords_drop() {
    struct Case {
        name: &'static str,
        body: &'static str,
    }

    let cases = [
        Case {
            name: "inherit",
            body: "width: inherit",
        },
        Case {
            name: "initial",
            body: "width: initial",
        },
        Case {
            name: "revert",
            body: "width: revert",
        },
        Case {
            name: "revert_layer",
            body: "width: revert-layer",
        },
    ];

    for case in cases {
        assert_decl_drop(&format!(".{} {{ {} }}", case.name, case.body), 0, 1);
    }
}

// — Percentage-height indefinite-basis resolution (PR 17 review fix) — //
//
// The height-axis percentage fields (`height`/`min-height`/`max-height`/
// `top`/`bottom`) resolve against an `Option<f32>` containing-block height
// basis. When the containing block is `height: auto`, that basis is
// indefinite (`None`), and each field follows its own spec rule:
// `height` → auto, `max-height` → none, `min-height` → 0, and the relative
// offsets defer. The four height-axis pins below are on-spec; the flex-basis
// and relative-offset rows are unimplemented and registered in the README.

#[test]
// ON-SPEC:
// - SPEC: CSS 2.1 §10.5 — a percentage `height` on a non-positioned element
//   resolves against the containing block's height; when that height is not
//   explicitly specified (auto), the percentage resolves to `auto` and the
//   box sizes to its content.
//   <https://www.w3.org/TR/CSS21/visudet.html#the-height-property>
// - CURRENT (asserted): the indefinite basis propagates `None` through
//   `BoxLength::resolve_height`, so `height: 50%` under an auto-height parent
//   computes to `auto` and the used height equals the natural content height —
//   NOT 0.0 and NOT half of anything.
fn percentage_height_against_indefinite_basis_computes_auto() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, child, "child content");
    assert_style_set(&mut root, child, "height", "50%");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let content_height = text_intrinsic(&root, text).line_height_px;
    let used_height = ui_dom::node_box(&root, child)
        .expect("box")
        .border_box
        .height_px;
    assert!(
        used_height > 0.0,
        "height: 50% against an indefinite basis should not collapse to 0"
    );
    assert_close(
        used_height,
        content_height,
        "height: 50% should resolve to auto and size to its content height",
    );
}

#[test]
// ON-SPEC:
// - SPEC: CSS 2.1 §10.7 — a percentage `max-height` under an auto-height
//   containing block resolves to `none` (no maximum); the box sizes to its
//   content. <https://www.w3.org/TR/CSS21/visudet.html#min-max-heights>
// - CURRENT (asserted): the indefinite basis propagates `None` through
//   `BoxLength::resolve_max_height`, so no maximum is applied.
fn percentage_max_height_against_indefinite_basis_computes_none() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, child, "child content");
    assert_style_set(&mut root, child, "max-height", "50%");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let content_height = text_intrinsic(&root, text).line_height_px;
    let used_height = ui_dom::node_box(&root, child)
        .expect("box")
        .border_box
        .height_px;
    assert_close(
        used_height,
        content_height,
        "max-height: 50% should resolve to none and not constrain the content height",
    );
}

#[test]
// ON-SPEC:
// - SPEC: CSS 2.1 §10.7 — a percentage `min-height` under an auto-height
//   containing block computes to 0 (the explicit §10.7 rule, DISTINCT from
//   `height`'s auto rule), so it imposes no floor and the box sizes to its
//   content. <https://www.w3.org/TR/CSS21/visudet.html#min-max-heights>
// - CURRENT (asserted): `BoxLength::resolve_min_height` returns `Some(0.0)`
//   for a percentage against an indefinite basis, so no minimum is applied.
fn percentage_min_height_against_indefinite_basis_computes_zero() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, container);
    let text = assert_append_text_to(&mut root, child, "child content");
    assert_style_set(&mut root, child, "min-height", "50%");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let content_height = text_intrinsic(&root, text).line_height_px;
    let used_height = ui_dom::node_box(&root, child)
        .expect("box")
        .border_box
        .height_px;
    assert_close(
        used_height,
        content_height,
        "min-height: 50% should compute to 0 and not impose a floor",
    );
}

#[test]
// ON-SPEC (the discriminating controls):
// - SPEC: CSS 2.1 §10.5/§10.7 — a percentage `height`/`min-height`/`max-height`
//   against a DEFINITE containing-block height resolves to the exact fraction.
//   <https://www.w3.org/TR/CSS21/visudet.html#the-height-property>
// - CURRENT (asserted): with an explicit `height: 200px` parent the height
//   basis is `Some(200.0)`, so each percentage resolves to 100.0.
fn percentage_height_bases_resolve_when_definite() {
    // (a) height: 50% of an explicit 200px parent -> used height exactly 100.0.
    {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let parent = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, parent, "height", "200px");
        let child = assert_append_div_to(&mut root, parent);
        assert_style_set(&mut root, child, "height", "50%");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            ui_dom::node_box(&root, child)
                .expect("box")
                .border_box
                .height_px,
            100.0,
            "height: 50% against a 200px parent should resolve to exactly 100px",
        );
    }

    // (b) tall content + max-height: 50% -> used height capped at exactly 100.0.
    {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let parent = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, parent, "height", "200px");
        let child = assert_append_div_to(&mut root, parent);
        assert_style_set(&mut root, child, "max-height", "50%");
        let tall = assert_append_div_to(&mut root, child);
        assert_style_set(&mut root, tall, "height", "150px");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            ui_dom::node_box(&root, child)
                .expect("box")
                .border_box
                .height_px,
            100.0,
            "max-height: 50% against a 200px parent should cap tall content at exactly 100px",
        );
    }

    // (c) one line of text + min-height: 50% -> used height exactly 100.0 (the
    // floor applies).
    {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let parent = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, parent, "height", "200px");
        let child = assert_append_div_to(&mut root, parent);
        let _text = assert_append_text_to(&mut root, child, "one line");
        assert_style_set(&mut root, child, "min-height", "50%");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            ui_dom::node_box(&root, child)
                .expect("box")
                .border_box
                .height_px,
            100.0,
            "min-height: 50% against a 200px parent should floor one line at exactly 100px",
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Flexbox 1 §7.3.3 — `flex-basis` takes `<content> | <'width'>`,
//   where `<'width'>` is `<length-percentage>`, so `flex-basis: 50%` is valid
//   and resolves against the flex container's main size.
//   <https://drafts.csswg.org/css-flexbox-1/#flex-basis-property>
// - CURRENT (asserted): the closed `FlexBasis` grammar admits only
//   `auto | content | 0 | <px> | <em>` (`parse_flex_basis_length`,
//   `decl_parse.rs:606-612`), so `flex-basis: 50%` drops + counts.
// - SPEC-CORRECT: admit `<length-percentage>` on flex-basis.
// - D6: admission-gated.
fn flex_basis_percentage_drops() {
    assert_decl_drop(".x { flex-basis: 50% }", 0, 1);
}

#[test]
// UNIMPLEMENTED (class: unimplemented — offset-time resolution follow-up):
// - SPEC: CSS 2.1 §9.4.2/§9.4.3 — a relatively positioned box's `top`/`bottom`
//   percentage resolves against the containing block's height. The containing
//   block is laid out before the relative offset applies, so a correct engine
//   resolves `top: 50%` against the laid-out containing-block height.
//   <https://www.w3.org/TR/CSS21/visuren.html#relative-positioning>
// - CURRENT (asserted): `node_style` resolves `top`/`bottom` percentages
//   against the node_style-time height basis (`None` under an auto-height
//   containing block), so `top: 50%` defers to no offset and the box stays at
//   its normal-flow position. Offset-time resolution against the laid-out
//   containing block is a follow-up.
fn relative_top_percentage_defers_to_offset_time() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    // A preceding 100px sibling gives the auto-height container a laid-out
    // height of 100 + 20 = 120px, so a correct offset-time `top: 50%` would
    // offset the child 60px. Today the percentage defers (no offset).
    let spacer = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, spacer, "height", "100px");
    let child = assert_append_div_to(&mut root, container);
    assert_style_set(&mut root, child, "position", "relative");
    assert_style_set(&mut root, child, "top", "50%");
    assert_style_set(&mut root, child, "height", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        ui_dom::node_box(&root, child)
            .expect("box")
            .border_box
            .top_px,
        100.0,
        "relative top: 50% should defer to no offset under an auto-height containing block",
    );
}

/// A named admitted-value control: the `center` declaration body plus a
/// predicate over the flex computed group it should produce.
type AlignmentControl = (&'static str, fn(ComputedFlexGroup) -> bool);

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Box Alignment 3 §8.2/§8.3/§8.4 + CSS Flexbox 1 §8.2–8.4 —
//   `justify-content` admits `normal | <content-distribution> | <overflow-position>? [ <content-position> | left | right ]`;
//   `align-items`/`align-self` admit `normal | stretch | <baseline-position> | <overflow-position>? <self-position>`;
//   `align-content` admits `normal | <baseline-position> | <content-distribution> | <overflow-position>? <content-position>`.
//   <https://drafts.csswg.org/css-align-3/#propdef-justify-content>
//   <https://drafts.csswg.org/css-align-3/#propdef-align-items>
//   <https://drafts.csswg.org/css-align-3/#propdef-align-content>
// - CURRENT (asserted): each `normal`/`left`/`right`/`baseline` form drops +
//   counts at the closed keyword grammar and the computed field stays at its
//   group initial (fail-closed `JUSTIFY_CONTENT_KEYWORDS`,
//   `ALIGN_ITEMS_KEYWORDS`, `ALIGN_SELF_KEYWORDS`, `ALIGN_CONTENT_KEYWORDS` in
//   `presentation_property.rs`); the admitted `center` CONTROL applies to every
//   one of the four properties. No crash.
// - SPEC-CORRECT: admit the positional `left`/`right`, the `normal` initial
//   keyword, and first/last baseline alignment; admission per D6.
fn unsupported_rejected_alignment_keywords_drop() {
    let rejected = [
        ("justify_content_normal", "justify-content: normal"),
        ("justify_content_left", "justify-content: left"),
        ("justify_content_right", "justify-content: right"),
        ("align_items_baseline", "align-items: baseline"),
        ("align_items_normal", "align-items: normal"),
        ("align_self_baseline", "align-self: baseline"),
        ("align_content_normal", "align-content: normal"),
        ("align_content_baseline", "align-content: baseline"),
    ];
    for (name, body) in rejected {
        assert_decl_drop(&format!(".x {{ {body} }}"), 0, 1);
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {body} }}"));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target).expect("target").as_flex(),
            ComputedFlexGroup::initial(),
            "{name} should drop and keep the flex group at its initials",
        );
    }

    // CONTROL: the admitted `center` keyword applies to all four properties,
    // proving the gate rejects only the closed-out vocabulary.
    let controls: [AlignmentControl; 4] = [
        ("justify-content: center", |f| {
            f.justify_content == JustifyContentKw::Center
        }),
        ("align-items: center", |f| {
            f.align_items == AlignItemsKw::Center
        }),
        ("align-self: center", |f| {
            f.align_self == Some(AlignItemsKw::Center)
        }),
        ("align-content: center", |f| {
            f.align_content == AlignContentKw::Center
        }),
    ];
    for (body, check) in controls {
        let (mut world, mut root, target, _report) =
            styled_target_load(&format!(".x {{ {body} }}"));
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert!(
            check(root.computed_style(target).expect("target").as_flex()),
            "{body} control should apply",
        );
    }
}
