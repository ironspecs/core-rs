#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em lengths and the computed groups' initial values
// to exact `f32` literals, and text intrinsics for identical text/font are
// bit-identical, so the typed computed-group and intrinsic comparisons in this
// module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Admitted grammars whose computed values are ignored, approximated, or
//! quirky downstream (PLAN.md B4; and B5's preserved-candidates family).
//!
//! Purpose: the "within-vocabulary honesty" file. These pins cover declarations
//! that ARE admitted by the style grammar but whose downstream consumption
//! diverges from the cited spec — either the computed value is ignored
//! (`font-family`/`font-weight`/`font-style` resolve but text layout renders
//! `UiSans` regardless), the computed value IS consumed on-spec (`text-align:
//! justify`), the closed keyword set drops wider CSS values (`font-style:
//! oblique`), a white-space mode is folded into another mode's transform
//! (`white-space: break-spaces` shares `pre-wrap`'s preserve transform),
//! a value cannot schedule a transition (`transform`), an authored zero radius
//! falls back to a control's pill default (range), a content attribute merges
//! with a property (`dir` vs `direction`), or a content-side channel is the
//! only live path while the stylesheet channel is dropped (`pointer-events`).
//!
//! Class notes: `off_spec_*` rows are divergent (pinned at current values,
//! expected to move on-spec); `unsupported_*` rows are unimplemented (D6
//! admission-gated drop-and-continue); the `text_align_justify_is_consumed` and
//! `dir_attribute_and_direction_property_merge` and `pointer_events_content_channel`
//! rows are on-spec keeps; `white_space_pre_line_is_consumed` is an on-spec
//! keep while `off_spec_break_spaces_approximated_as_pre_wrap` is classed
//! divergent (folded into `pre-wrap`). B5's preserved candidates split
//! across the classes: `initial_color_is_near_white_by_design` and
//! `initial_border_widths_are_zero_by_design` are deliberate (crate-owned
//! defaults pinned permanently), while
//! `off_spec_line_height_normal_resolves_to_one_times_font_size` is divergent
//! (expected to move on-spec).

use bevy::prelude::World;
use ui_dom::{
    DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerState, DomPointerUpdate, DomPseudoState,
    DomTextDirection, Keyword, RootDom, TextAlign, TextIntrinsic,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_input_to, assert_append_text_to, assert_close,
    assert_decl_drop, assert_load_root_stylesheet, assert_node_layout, assert_root_dom_update,
    assert_set_attr_str, assert_style_set, attached_gpu_root_dom_default_viewport_setup,
    take_staged_layer_paint_plans, theme_stylesheet_origin,
};

/// Returns `(world, root, target, report)`: a fresh root with a single
/// class-"x" div target under the document root, `css` loaded (not yet
/// resolved — the caller runs the update), and the stylesheet load report.
fn styled_target_load(css: &str) -> (World, RootDom, DomNodeSlotKey, ui_dom::StylesheetLoadReport) {
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
        _ => panic!("admitted-gaps proof text node should keep text intrinsic state"),
    }
}

/// Builds two sibling class-"a"/"b" divs under the document root, each holding
/// the same text node, with `.b { <css> }` loaded and one update already run.
/// Returns `(root, plain_text, styled_text, styled)`.
fn text_pair_with_css(css: &str) -> (RootDom, DomNodeSlotKey, DomNodeSlotKey, DomNodeSlotKey) {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), &format!(".b {{ {css} }}"));
    let document_root = root.document_node().0;
    let plain = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, plain, "class", "a");
    let plain_text = assert_append_text_to(&mut root, plain, "Hello world");
    let styled = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, styled, "class", "b");
    let styled_text = assert_append_text_to(&mut root, styled, "Hello world");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    (root, plain_text, styled_text, styled)
}

/// Asserts two text nodes' intrinsic min/max content widths are bit-identical.
fn assert_intrinsics_equal(
    root: &RootDom,
    plain_text: DomNodeSlotKey,
    styled_text: DomNodeSlotKey,
    label: &str,
) {
    let plain = text_intrinsic(root, plain_text);
    let styled = text_intrinsic(root, styled_text);
    assert_eq!(
        plain.min_content_width_px, styled.min_content_width_px,
        "{label} should leave min-content width unchanged",
    );
    assert_eq!(
        plain.max_content_width_px, styled.max_content_width_px,
        "{label} should leave max-content width unchanged",
    );
}

/// Whether one element currently carries the hover pseudo-state.
fn is_hovered(root: &RootDom, key: DomNodeSlotKey) -> bool {
    root.node(key)
        .and_then(DomNode::as_element)
        .is_some_and(|element| element.has_pseudo_state(DomPseudoState::Hover))
}

/// A pointer update at a point inside the 60×60 child box (origin-centered).
fn pointer_set(x_px: f32, y_px: f32) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down: false,
        scroll_delta_y_px: 0.0,
    })
}

/// The retained white-space layout facts one text node exposes after one update.
struct WhiteSpaceLayout {
    block_height: f32,
    text_height: f32,
    line_height: f32,
    max_content_width: f32,
}

/// Measures one text node under a width-constrained block with `white_space`.
fn layout_for_white_space(text: &str, white_space: &str, width_px: f32) -> WhiteSpaceLayout {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, text);

    assert_style_set(&mut root, container, "width", &format!("{width_px}px"));
    assert_style_set(&mut root, block, "display", "block");
    assert_style_set(&mut root, block, "white-space", white_space);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let block_layout = assert_node_layout(&root, block);
    let text_layout = assert_node_layout(&root, text_node);
    let intrinsic = text_intrinsic(&root, text_node);

    WhiteSpaceLayout {
        block_height: block_layout.height_px,
        text_height: text_layout.height_px,
        line_height: intrinsic.line_height_px,
        max_content_width: intrinsic.max_content_width_px,
    }
}

// — B4: computed-but-unconsumed font family — //

#[test]
// OFF-SPEC (divergent, step-5 follower): CSS Fonts 4 §3.1 `font-family` selects
// the family used for glyph rendering.
// <https://drafts.csswg.org/css-fonts-4/#font-family-prop>
//
// CURRENT (asserted below): `font-family: Source Han Sans` resolves in the
// computed group (`font_family() == Some("Source Han Sans")`) but the text
// intrinsic widths are IDENTICAL to a sibling with no family — the family is
// never consumed by text layout, which renders UiSans regardless.
// SPEC-CORRECT: text layout resolves the computed family and measures with it,
// so a Source Han Sans container and a default container measure differently.
// DISCRIMINATOR: `Source Han Sans` IS in the installed catalog (the Japanese
// face, `UI_DOM_FONT_SOURCE_HAN_SANS_J_PATH`, `text_render.rs:16-17`), so a
// consuming engine would resolve it to that face and measure `Hello world` with
// its Latin advance widths, which differ from the IBM Plex Sans default. A
// generic `serif` would NOT discriminate: no serif face is installed (the
// catalog is IBM Plex Sans + three Source Han Sans faces, per
// `install_text_font_catalog`), so consumption would fall back to the same
// default and the pin would pass even after moving on-spec.
// RESIDUAL ASSUMPTION: Source Han Sans's Latin advance widths differ from IBM
// Plex Sans for this text. If the two faces' metrics coincided for `Hello
// world`, the width-equality half would be weak (the computed-style assertion
// that the family resolves would still hold).
// ROOT CAUSE: `TextFontFamilyId::UiSans` one-variant enum
// (`text_layout/mod.rs:36-39`), hardcoded in `TextLayoutRequest::for_text`
// (`text_layout_request.rs:33-48`) and resolved by `preferred_repo_font_family`
// (`text_render.rs:23-63`).
// RECLASSIFY: consumption is the named step-5 follower; this pin flips on-spec
// when the resolved family reaches text layout.
fn off_spec_font_family_computed_but_unconsumed() {
    let (root, plain_text, styled_text, styled) =
        text_pair_with_css("font-family: Source Han Sans");

    assert_eq!(
        root.computed_style(styled)
            .expect("styled container")
            .font_family(),
        Some("Source Han Sans"),
        "font-family should resolve in the computed group",
    );

    assert_intrinsics_equal(&root, plain_text, styled_text, "font-family");
}

#[test]
// OFF-SPEC (divergent, same family as `off_spec_font_family_computed_but_unconsumed`):
// CSS Fonts 4 §3.2 `font-weight` and §3.3 `font-style` select the weight and
// style used for glyph rendering.
// <https://drafts.csswg.org/css-fonts-4/#font-weight-prop>
// <https://drafts.csswg.org/css-fonts-4/#font-style-prop>
//
// CURRENT (asserted below): `font-weight: bold` computes `weight == 700` and
// `font-style: italic` computes `style == Keyword::Italic`, but the text
// intrinsic widths stay identical to the default sibling — neither is consumed
// (rendered regular/upright via the hardcoded UiSans family).
// SPEC-CORRECT: text layout consumes the computed weight/style, so the rendered
// run changes.
// PIN STRENGTH BY HALF: the weight half discriminates — the installed IBM Plex
// Sans is a variable font, so a consumed `weight: 700` selects a different
// advance-width axis instance than the default 400. The style/italic half is
// inherently weak on this catalog: synthesized obliqing does not change advance
// widths, so its strength is the computed-style assertion plus render-path
// reachability, NOT a width difference.
// ROOT CAUSE: the hardcoded `TextFontFamilyId::UiSans` request
// (`text_layout_request.rs:33-48`) carries no weight/style into the shaper.
// RECLASSIFY: consumption rides the same step-5 follower as font-family.
fn off_spec_font_weight_and_style_computed_but_unconsumed() {
    enum FontAssertion {
        Weight,
        Style,
    }

    let cases = [
        (
            "font_weight_bold",
            "font-weight: bold",
            FontAssertion::Weight,
        ),
        (
            "font_style_italic",
            "font-style: italic",
            FontAssertion::Style,
        ),
    ];

    for (name, css, assertion) in cases {
        let (root, plain_text, styled_text, styled) = text_pair_with_css(css);

        let view = root.computed_style(styled).expect("styled container");
        match assertion {
            FontAssertion::Weight => assert_eq!(
                view.as_font().weight,
                700,
                "{name} should compute font-weight 700",
            ),
            FontAssertion::Style => assert_eq!(
                view.as_font().style,
                Keyword::Italic,
                "{name} should compute font-style italic",
            ),
        }

        assert_intrinsics_equal(&root, plain_text, styled_text, name);
    }
}

// — B4: on-spec keep (verified consumed) — //

#[test]
// ON-SPEC: CSS Text 3 §7.1 `text-align: justify` justifies line boxes.
// <https://drafts.csswg.org/css-text-3/#text-align-property>
// Verified consumed: `text_render.rs:104-106` maps `TextAlign::Justified →
// cosmic_text::Align::Justified`, and the staged `TextRunPaintOp.alignment`
// carries it through `text_align_for_css_value` (`text_layout/mod.rs:57-72`).
fn text_align_justify_is_consumed() {
    let cases = [
        ("justify", "justify", TextAlign::Justified),
        ("left_control", "left", TextAlign::Left),
    ];

    for (name, value, expected) in cases {
        let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let container = assert_append_div_to(&mut root, document_root);
        assert_style_set(&mut root, container, "width", "80px");
        assert_style_set(&mut root, container, "text-align", value);
        assert_append_text_to(&mut root, container, "alpha beta gamma delta");

        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        let plans = take_staged_layer_paint_plans(&root);
        assert_eq!(plans.len(), 1, "{name} should stage one layer plan");
        assert_eq!(
            plans[0].text_runs.len(),
            1,
            "{name} should stage one text run"
        );
        assert_eq!(
            plans[0].text_runs[0].alignment, expected,
            "{name} text run should carry {expected:?}",
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Fonts 4 §3.3 admits `font-style: oblique` (and the wider
//   `oblique <angle>?` form). <https://drafts.csswg.org/css-fonts-4/#font-style-prop>
// - CURRENT (asserted): `font-style: oblique` drops + counts
//   (`declarations_dropped == 1`) and the computed style stays
//   `Keyword::Normal` — the admitted set is exactly normal/italic.
// - SPEC-CORRECT: admit `oblique` (the computed group's `style` field is a
//   `Keyword`, so admission widens the keyword set).
// - D6: admission-gated; `FONT_STYLE_KEYWORDS` (`presentation_property.rs:213`)
//   is the closed set.
fn unsupported_font_style_oblique_drops() {
    assert_decl_drop(".x { font-style: oblique }", 0, 1);
    let (mut world, mut root, target, _report) = styled_target_load(".x { font-style: oblique }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_font().style,
        Keyword::Normal,
        "font-style: oblique should stay at the normal initial",
    );
}

// — B4: white-space consumption (verify per keyword) — //

#[test]
// ON-SPEC: CSS Text 4 §white-space `pre-line` collapses spaces/tabs/form-feed
// and preserves `\n`/`\r\n` segment breaks (collapse spaces/tabs, preserve
// newlines, wrap). <https://www.w3.org/TR/css-text-4/#white-space-property>
// Verified: `pre-line` maps to `collapse_spaces_preserving_segment_breaks`
// (`types/white_space.rs:147-180`, reached via
// `text_layout_input_for_white_space` at `white_space.rs:67`).
fn white_space_pre_line_is_consumed() {
    let repeated = layout_for_white_space("Alpha    Beta\nGamma", "pre-line", 300.0);
    let collapsed = layout_for_white_space("Alpha Beta\nGamma", "pre-line", 300.0);
    assert_close(
        repeated.max_content_width,
        collapsed.max_content_width,
        "pre-line should collapse repeated-space advance",
    );
    assert_close(
        repeated.text_height,
        repeated.line_height * 2.0,
        "pre-line should preserve the explicit segment break as a second line",
    );
}

#[test]
// OFF-SPEC (divergent): CSS Text 4 §white-space `break-spaces` preserves every
// trailing space (it takes advance and wraps or hangs, never collapses).
// <https://www.w3.org/TR/css-text-4/#white-space-property>
//
// CURRENT (asserted below): `break-spaces` maps to `preserve_space_run_advances`
// (`types/white_space.rs:255-277`, reached via `white_space.rs:64`) — the SAME
// transform as `pre`/`pre-wrap`, not a distinct break-spaces policy. Repeated
// trailing spaces are NBSP-substituted so they take advance and wrap
// (coincidentally matching break-spaces for that input), but a LONE trailing
// space is not preserved: `text_has_repeated_ascii_spaces` short-circuits the
// transform, so the raw space reaches the shaper and is collapsed at the line
// end.
// SPEC-CORRECT: a lone trailing space is preserved; at a width where it
// overflows the line it wraps onto a second line (or hangs), so the block
// height grows to two lines.
// ROOT CAUSE: `text_layout_input_for_white_space` (`types/white_space.rs:58-69`)
// maps `BreakSpaces` to the same `preserve_space_run_advances` as `Pre`/`PreWrap`.
// RECLASSIFY: a distinct break-spaces transform flips this pin on-spec.
fn off_spec_break_spaces_approximated_as_pre_wrap() {
    // (a) repeated trailing spaces: NBSP-substituted, take advance and wrap onto
    // their own line (the actual pre-wrap transform, which happens to match
    // break-spaces for this input).
    let trimmed = layout_for_white_space("Alpha Beta", "break-spaces", 70.0);
    let trailing = layout_for_white_space("Alpha Beta   ", "break-spaces", 70.0);
    assert_close(
        trimmed.block_height,
        trimmed.line_height,
        "break-spaces without trailing spaces fits one line at this width",
    );
    assert_close(
        trailing.block_height,
        trailing.line_height * 2.0,
        "break-spaces repeated trailing spaces wrap onto their own line",
    );

    // (b) discriminating case: ONE lone trailing space at 70px. `Alpha Beta`
    // measures 68.85px max-content and fits one line; `Alpha Beta ` measures
    // 72.16px, so a preserved space would overflow and wrap to a second line.
    // Spec break-spaces preserves it (wrap); the current pre-wrap fold collapses
    // it, so the block stays one line — the jump is discriminated.
    let lone = layout_for_white_space("Alpha Beta ", "break-spaces", 70.0);
    assert_close(
        lone.block_height,
        lone.line_height,
        "break-spaces lone trailing space is collapsed by the pre-wrap fold (spec would wrap it onto a second line)",
    );
}

// — B4: transform transitions never schedule — //

#[test]
// OFF-SPEC (divergent, documented in MIGRATION.md): CSS Transitions 1 §2
// `transition: transform 180ms` should interpolate a `translateX` change over
// the duration. <https://drafts.csswg.org/css-transitions-1/>
//
// CURRENT (asserted below): `transition: transform 180ms` with a
// `translateX(0px) → translateX(10px)` change presents the target immediately
// at the next update — no transition is scheduled, so the target is STILL
// presented at progress 0.5 (t=0.09s, half the 180ms duration).
// SPEC-CORRECT: the transform interpolates; at progress 0.5 the `translate_px`
// is strictly between 0 and 10.
// ROOT CAUSE: `decl_value_for_groups` (`root_dom.rs:4695-4703`) returns `None`
// for `PresentationProperty::Transform` (the transform `DeclValue` is interned
// text, which the scheduler cannot interpolate; the typed `[f32; 2]` pair is
// the admission shape), so `schedule_pass_transitions` never installs one.
// RECLASSIFY: typed-translate interpolation (a small admission-gated follow-up)
// flips this pin on-spec.
fn off_spec_transform_transitions_do_not_schedule() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_style_set(&mut root, container, "transform", "translateX(0px)");
    assert_style_set(&mut root, container, "transition", "transform 180ms");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_transform()
            .translate_px,
        [0.0, 0.0],
        "initial transform should be at the authored origin",
    );

    assert_style_set(&mut root, container, "transform", "translateX(10px)");
    // Pass-driven scheduling (D2 one-timing): the first update detects the
    // change. Because Transform is never transitionable, the target presents
    // immediately instead of starting a transition.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_transform()
            .translate_px,
        [10.0, 0.0],
        "transform should jump to the target on the change-detection update",
    );

    // Mid-time (progress 0.5): the presented value is STILL the target — no
    // mid-flight interpolated value exists. A scheduled transition would
    // present a value strictly between [0.0, 0.0] and [10.0, 0.0] here, so the
    // jump is discriminated (matches `pipeline/transitions.rs`'s
    // MID_TRANSITION_TIME_S = 0.09 convention).
    assert_root_dom_update(&mut world, &mut root, 0.09, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(container)
            .expect("container")
            .as_transform()
            .translate_px,
        [10.0, 0.0],
        "transform should stay at the target at progress 0.5 (a scheduled transition would interpolate strictly between [0,0] and [10,0])",
    );
}

// — B4: range control authored zero radius falls back to pill — //

#[test]
// OFF-SPEC (divergent, documented): CSS Backgrounds 3 §5.1 `border-radius: 0`
// authored on an element should win over any control default.
// <https://drafts.csswg.org/css-backgrounds-3/#the-border-radius>
//
// CURRENT (asserted below): a range control with `border-radius: 0px` stages
// its track and thumb with the PILL default (track `[track_height/2; 4]`,
// thumb `[thumb_size/2; 4]`) instead of `[0.0; 4]` — the group flattens
// authored presence to value, so "authored" maps to "any corner radius > 0"
// and an authored zero falls back to the pill.
// SPEC-CORRECT: the authored `0px` wins; the track/thumb corner radii are
// `[0.0; 4]`.
// ROOT CAUSE: `has_authored_border_radius` (`element_behaviors/paint.rs:386`)
// reads `any(|r| r > 0.0)`; `range_track_corner_radii_px` /
// `range_thumb_corner_radii_px` (`element_behaviors/paint.rs:449-469`) emit the
// pill when that is false.
// RECLASSIFY: the fix belongs to the behavior-sheet tier (F): the pill default
// becomes a base behavior declaration the authored `0px` overrides.
fn off_spec_range_authored_zero_radius_falls_back_to_pill() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let input = assert_append_input_to(&mut root, document_root);
    assert_set_attr_str(&mut root, input, "type", "range");
    assert_set_attr_str(&mut root, input, "value", "0");
    assert_style_set(&mut root, input, "width", "120px");
    assert_style_set(&mut root, input, "height", "24px");
    assert_style_set(&mut root, input, "border-radius", "0px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plans = take_staged_layer_paint_plans(&root);
    assert_eq!(plans.len(), 1, "range should stage one layer plan");
    let rects = &plans[0].rects;

    // Track height is (24 * 0.25).max(4) = 6px; its pill radius is 3px.
    let track = rects
        .iter()
        .find(|rect| (rect.size_px[1] - 6.0).abs() <= 0.001)
        .expect("range should stage a 6px-tall track rect");
    assert_eq!(
        track.corner_radii_px, [3.0; 4],
        "range track should keep the pill radius (track_height/2 = 3px), not the authored 0px",
    );

    // Thumb is a 24px square; its pill radius is 12px.
    let thumb = rects
        .iter()
        .find(|rect| {
            (rect.size_px[0] - 24.0).abs() <= 0.001 && (rect.size_px[1] - 24.0).abs() <= 0.001
        })
        .expect("range should stage a 24px-square thumb rect");
    assert_eq!(
        thumb.corner_radii_px, [12.0; 4],
        "range thumb should keep the pill radius (thumb_size/2 = 12px), not the authored 0px",
    );
}

// — B4: dir content attribute vs direction property — //

#[test]
// ON-SPEC: CSS Writing Modes/HTML `dir` interaction — the `direction` property
// (author cascade) wins over the `dir` content attribute; the attribute only
// feeds `direction` when the property is absent, and the initial is `ltr`.
// <https://drafts.csswg.org/css-writing-modes-4/#direction>
// Verified: `apply_dir_content_attribute` (`winners_fold.rs:339-360`) merges
// the `dir` attribute only when the `Direction` longhand has no winner.
fn dir_attribute_and_direction_property_merge() {
    // (a) dir="rtl" only → the content attribute merges into Rtl.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let rtl_only = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, rtl_only, "dir", "rtl");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(rtl_only)
            .expect("rtl-only")
            .as_text()
            .direction,
        DomTextDirection::Rtl,
        "dir=rtl content attribute should merge into Rtl",
    );

    // (b) dir="rtl" + direction: ltr → the property wins.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let overridden = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, overridden, "dir", "rtl");
    assert_style_set(&mut root, overridden, "direction", "ltr");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(overridden)
            .expect("overridden")
            .as_text()
            .direction,
        DomTextDirection::Ltr,
        "direction: ltr should win over dir=rtl",
    );

    // (c) neither → initial Ltr.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let bare = assert_append_div_to(&mut root, document_root);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(bare).expect("bare").as_text().direction,
        DomTextDirection::Ltr,
        "a bare node should keep the Ltr initial",
    );
}

// — B4: pointer-events content channel — //

#[test]
// ON-SPEC for `auto`/`none` + unimplemented for wider values + the stylesheet
// channel: the `pointer-events` content-side `Custom("pointer-events")` channel
// is consumed by hit-testing at `hit_test.rs:331-337` — `"none"` → disabled,
// `"auto"` → enabled, anything else → inherit the parent's state. It is NOT a
// presentation property by design (behavior input, not cascaded style).
//
// - (a)/(b)/(c): `auto`/`none` subtree disable + re-enable + child re-enable
//   are on-spec.
// - (d): a wider CSS value (`visiblePainted`) is stored but ignored — the
//   enabled state inherits the parent — unimplemented.
// - (e): `pointer-events` in a STYLESHEET drops + counts (the name is not in
//   the presentation table) and hit-testing is unaffected — the consumer
//   surprise row, unimplemented.
fn pointer_events_content_channel() {
    // (a)+(b): subtree disable, then re-enable. Distinct pointer positions so
    // each update re-syncs hover (hover re-sync rides the pointer-move path).
    let (mut world, mut root, parent, child) = pointer_scenario();
    assert_set_attr_str(&mut root, parent, "pointer-events", "none");
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 30.0));
    assert!(
        !is_hovered(&root, child),
        "(a) pointer-events none should suppress child hover",
    );

    assert_set_attr_str(&mut root, parent, "pointer-events", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 31.0));
    assert!(
        is_hovered(&root, child),
        "(b) pointer-events auto should re-enable child hover",
    );

    // (c): a child `pointer-events: auto` re-enables under a `none` parent.
    let (mut world, mut root, parent, child) = pointer_scenario();
    assert_set_attr_str(&mut root, parent, "pointer-events", "none");
    assert_set_attr_str(&mut root, child, "pointer-events", "auto");
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 30.0));
    assert!(
        is_hovered(&root, child),
        "(c) child auto under a none parent should re-enable hover",
    );

    // (d): a wider CSS value is stored but ignored — enabled inherits.
    let (mut world, mut root, parent, child) = pointer_scenario();
    assert_set_attr_str(&mut root, parent, "pointer-events", "visiblePainted");
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 30.0));
    assert!(
        is_hovered(&root, child),
        "(d) visiblePainted should be ignored and inherit the enabled parent state",
    );

    // (e): the stylesheet channel drops + counts and leaves hit-testing alone.
    assert_decl_drop(".x { pointer-events: none }", 0, 1);
    let (mut world, mut root, _parent, child) = pointer_scenario();
    assert_set_attr_str(&mut root, child, "class", "x");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { pointer-events: none }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 30.0));
    assert!(
        is_hovered(&root, child),
        "(e) stylesheet pointer-events should be inert and leave child hover working",
    );
}

/// Returns `(world, root, parent, child)`: a 100×100 parent div holding a
/// 60×60 child div under the document root, ready for one pointer update.
fn pointer_scenario() -> (World, RootDom, DomNodeSlotKey, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let child = assert_append_div_to(&mut root, parent);
    assert_style_set(&mut root, parent, "width", "100px");
    assert_style_set(&mut root, parent, "height", "100px");
    assert_style_set(&mut root, child, "width", "60px");
    assert_style_set(&mut root, child, "height", "60px");
    (world, root, parent, child)
}

// — B5: preserved candidates — //

#[test]
// DELIBERATE (crate-owned design-system default; PLAN.md B5 decision log):
// CSS Color 4 §12 `color` initial is `canvastext` (opaque black in light
// contexts — the canvas's text color).
// <https://drafts.csswg.org/css-color-4/#propdef-color>
//
// CURRENT (asserted below): a plain div with text and no `color` declaration
// anywhere (no stylesheet, no `style_mut`) computes the near-white
// `[240/255;3, 1.0]` (`DEFAULT_TEXT_COLOR_RGBA`, `computed_color_group.rs:23`).
// SPEC-CORRECT: the CSS initial `canvastext` (black in light contexts).
// RATIONALE (deliberate, recorded in PLAN.md's decision log): the crate's
// dark-theme design-system default — the spec's `canvastext` default has no
// meaning for this product surface. Pinned permanently, re-reviewed if the
// design system changes.
fn initial_color_is_near_white_by_design() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let div = assert_append_div_to(&mut root, document_root);
    assert_append_text_to(&mut root, div, "Hello world");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(div)
            .expect("plain div")
            .as_color()
            .color_rgba,
        [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0],
        "an absent color should compute the near-white design-system default",
    );
}

#[test]
// DIVERGENT (PLAN.md B5 decision log): CSS 2.1 §10.8 `line-height: normal` is
// UA-dependent — "roughly 1.2" in desktop browsers.
// <https://www.w3.org/TR/CSS21/visudet.html#propdef-line-height>
//
// CURRENT (asserted below): a `font-size: 20px` container with no `line-height`
// measures its text node's intrinsic line height at exactly `1.0 × font-size`
// = 20.0 (`effective_line_height_px`, `text_layout/mod.rs:714-722`).
// SPEC-CORRECT: font-metrics-derived normal — Blink's
// `FontMetrics::lineSpacing`, Gecko's hhea ascent+descent+lineGap (≈1.2 for
// most text faces).
// RATIONALE: pinned divergent per PLAN.md's decision log (D6 favors the
// browser answer; moves when a consumer need is named — reclassify deliberate
// only if the design system explicitly confirms tight rows).
fn off_spec_line_height_normal_resolves_to_one_times_font_size() {
    let (root, _plain_text, styled_text, _styled) = text_pair_with_css("font-size: 20px");

    let intrinsic = text_intrinsic(&root, styled_text);
    assert_eq!(
        intrinsic.line_height_px, 20.0,
        "line-height: normal should resolve to exactly 1.0 × font-size",
    );
}

#[test]
// DELIBERATE (crate-owned width-gates-border model; PLAN.md B5 decision log):
// CSS Backgrounds 3 §4.2 `border-width` initial is `medium`, but `border-style`
// initial is `none` (§4.1), so undeclared borders are invisible in browsers
// too. <https://drafts.csswg.org/css-backgrounds-3/#the-border-width>
//
// CURRENT (asserted below): a plain div with no border declarations computes
// `border_width == [0.0; 4]` — the width-gates-border model
// (`ComputedBoxGroup::initial()` at `computed_box_group.rs`).
// SPEC-CORRECT: `medium` (a UA-dependent width) × `style: none` — invisible
// for the undeclared case, but non-zero at the computed-value level.
// RATIONALE (deliberate, PLAN.md decision log): observably identical to CSS's
// `medium × style:none` for the undeclared case; the `border-style` admission
// (tracked in B11) would revisit this.
fn initial_border_widths_are_zero_by_design() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let div = assert_append_div_to(&mut root, document_root);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(div)
            .expect("plain div")
            .as_box()
            .border_width,
        [0.0; 4],
        "an undeclared border should compute zero widths",
    );
}
