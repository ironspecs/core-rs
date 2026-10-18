#![allow(rustdoc::bare_urls)]
// The style pass resolves px lengths and the computed groups' initial values
// to exact `f32` literals, and a color's parsed rgba channels round-trip
// bit-identically through `parse_css_color_rgba`, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Cascade-tier boundary pins for the style system (PLAN.md B9).
//!
//! Purpose: pin the boundary of the cascade tiers the engine has deliberately
//! set aside — custom properties/`var()`, `@layer` (F stage 4), and the
//! animation tier (F stage 3) — plus the ADMITTED `!important` tier (F stage 1,
//! landed in PR 1 item 4) and the documented UA-sheet workaround (the on-spec
//! pin that must not silently break). The precedence record is `{ tier,
//! origin_priority, inline, specificity, order }` (`decl_value.rs`), widened at
//! redesign time exactly so the deferred tiers are additive: `tier` descends
//! `ImportantAuthor > NormalAuthor > NormalUa` (`cascade_tier.rs`), and the
//! behavior sheet folds at the `NormalUa` tier with `origin_priority: i32::MIN`
//! (`winners_fold.rs::behavior_precedence`), so a consumer's low-priority
//! origin models a real UA sheet sitting between it and author sheets.
//!
//! Class notes: every `unsupported_*` row is unimplemented (D6 admission-gated
//! absence per `MIGRATION.md`). The custom-property/`var()` and animation rows
//! pin the drop-and-count gate (a malformed/unadmitted declaration is dropped
//! and counted, no crash); the `@layer` row pins the whole-load typed `Err`
//! (at-rule, the same mechanics as B8's at-rule rows); the `!important` rows
//! relabeled on-spec in PR 1 item 4 and the UA-sheet workaround row are on-spec
//! (pinned so they cannot silently break). There are no `off_spec_*`
//! (divergent) rows in this file — the remaining cascade tiers are absent, not
//! wrong.

use bevy::prelude::World;
use ui_dom::{
    BoxLength, DomError, DomNode, DomNodeSlotKey, DomPointerState, DomPointerUpdate,
    DomPseudoState, RootDom, RootDomMetricName, StyleRuleOrigin, StylesheetLoadReport,
    parse_css_color_rgba,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_input_to, assert_decl_drop, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, box_px, theme_stylesheet_origin,
};

/// Returns `(world, root, target, report)`: a fresh root with a single
/// classed div target under the document root, `css` loaded (not yet resolved
/// — the caller runs the update), and the stylesheet load report.
fn styled_target_load(
    class: &str,
    css: &str,
) -> (World, RootDom, DomNodeSlotKey, StylesheetLoadReport) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", class);
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load");
    (world, root, target, report)
}

/// Loads `css` on a fresh root and returns the typed load error. At-rule
/// rejection is fatal at load (unlike declaration drops), so the `Err` variant
/// is the whole pin for the `unsupported_*` at-rule row.
fn load_error(css: &str) -> DomError {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect_err("stylesheet should be rejected at load")
}

/// A pointer update at the given point (drives the hover pseudo-state on the
/// element under it, mirroring the pointer-update pattern in `style/cascade.rs`
/// and `style/admitted_gaps.rs`).
fn pointer_set(x_px: f32, y_px: f32) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down: false,
        scroll_delta_y_px: 0.0,
    })
}

/// Whether one element currently carries the hover pseudo-state.
fn is_hovered(root: &RootDom, key: DomNodeSlotKey) -> bool {
    root.node(key)
        .and_then(DomNode::as_element)
        .is_some_and(|element| element.has_pseudo_state(DomPseudoState::Hover))
}

#[test]
// ON-SPEC (relabeled from `unsupported_important_declarations_drop_but_the_rule_survives` in PR 1
// item 4 after the importance tier landed):
// - SPEC: CSS Cascade 5 §6.1 — declarations marked `!important` form the
//   important tier, above every normal author declaration; §6.4.1 — within the
//   important tier, origin order is reversed and important inline style sits at
//   top specificity.
//   <https://drafts.csswg.org/css-cascade-5/#cascade-origin>
// - ASSERTED: the `!important` suffix is stripped at parse (CSS Syntax 3 §5.5),
//   the declaration applies in the `ImportantAuthor` tier, and the four
//   cross-tier orderings hold (see each sub-scenario).
fn important_declarations_win_over_normal_author_rules() {
    // (a) a mixed rule: the important width applies alongside the normal height.
    let (mut world, mut root, target, report) =
        styled_target_load("a", ".a { width: 10px !important; height: 5px }");
    assert_eq!(
        report.declarations_applied, 2,
        "both the important width and the normal height should apply",
    );
    assert_eq!(
        report.declarations_dropped, 0,
        "no declaration should drop once !important is stripped at parse",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let box_ = root.computed_style(target).expect("target").as_box();
    assert_eq!(
        box_.width,
        box_px(10.0),
        "the important width should compute 10px"
    );
    assert_eq!(
        box_.height,
        box_px(5.0),
        "the normal height should compute 5px"
    );

    // (b) important beats a normal rule of HIGHER specificity on the same
    // element (tier over specificity).
    let (mut world, mut root, target, _) =
        styled_target_load("a b", ".a { width: 10px !important } div.b { width: 20px }");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(10.0),
        "(b) the important declaration should beat a higher-specificity normal rule",
    );

    // (c) important at origin_priority -100 beats normal at origin_priority 100
    // (tier over origin).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("ua", -100),
        ".x { width: 10px !important }",
    );
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("author", 100),
        ".x { width: 20px }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(10.0),
        "(c) the -100 important declaration should beat the 100 normal declaration",
    );

    // (d) important author declaration beats normal inline style.
    let (mut world, mut root, target, _) = styled_target_load("a", ".a { width: 10px !important }");
    root.set_attr_str(target, "style", "width: 30px")
        .expect("normal inline style should route");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(10.0),
        "(d) the important author declaration should beat normal inline style",
    );

    // (e) important inline style beats an important author rule (top specificity
    // within the important tier).
    let (mut world, mut root, target, _) = styled_target_load("a", ".a { width: 10px !important }");
    root.set_attr_str(target, "style", "width: 30px !important")
        .expect("important inline style should route");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(30.0),
        "(e) important inline style should beat an important author rule",
    );
}

#[test]
// ON-SPEC (relabeled from `unsupported_important_in_style_attribute_drops` in PR 1
// item 4 after the importance tier landed):
// - SPEC: css-style-attr §3 — the style attribute is a declaration list parsed
//   with forward-compatible error handling; a `!important` declaration in it
//   applies in the important tier. CSS Cascade 5 §6.1 — important inline style
//   sits at top specificity within the important tier.
//   <https://drafts.csswg.org/css-style-attr/#syntax>
// - ASSERTED: `set_attr_str(target, "style", "width: 30px !important")` applies
//   the declaration and it wins over `.a { width: 10px !important }`.
fn important_style_attribute_declarations_apply_and_win() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "a");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".a { width: 10px !important }",
    );

    root.set_attr_str(target, "style", "width: 30px !important")
        .expect("the important style-attribute declaration should apply, not drop");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.count(RootDomMetricName::StyleDeclarationDropped),
        0,
        "the important style-attribute declaration should not be dropped",
    );
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(30.0),
        "the important inline width should win over the important author rule",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — F stage 4, D6 admission-gated):
// - SPEC: CSS Cascade 5 §6.4.2 cascade layers — `@layer a { … }` establishes a
//   named layer whose declarations cascade by layer order.
//   <https://drafts.csswg.org/css-cascade-5/#layering>
// - CURRENT (asserted, VERIFIED ACTUAL): `@layer a { .x { width: 1px } }`
//   fails the whole load with `Err(DomError::InvalidSelector)` — the block
//   parser treats the at-rule prelude `@layer a` as a selector, then the
//   nested block's closer becomes a stray `}` which `parse_css_blocks` rejects
//   (the same mechanics as B8's at-rule rows). The F1 atomicity fix means
//   nothing is inserted on that `Err`.
// - SPEC-CORRECT: `@layer` opens a layer block and the inner rule parses as a
//   rule in that layer (F stage 4: a layer table in `StyleRuntime` and a
//   `layer: u16` field inserted into `Precedence` per §6.1).
fn unsupported_layer_at_rule_fails_the_load() {
    assert_eq!(
        load_error("@layer a { .x { width: 1px } }"),
        DomError::InvalidSelector,
        "@layer should fail the whole load with InvalidSelector",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Custom Properties 1 §2 defines custom properties (`--*`), and
//   §3 defines `var()` substitution.
//   <https://drafts.csswg.org/css-variables-1/>
// - CURRENT (asserted): `.a { --brand: #112233; width: var(--brand) }`
//   reports `declarations_applied == 0` and `declarations_dropped == 2` —
//   `--brand` drops at property-name lookup (the name is not in the
//   `property_shape` table) and `width: var(--brand)` drops at the value
//   grammar — and computes `width == None`.
// - SPEC-CORRECT: `--brand` registers a custom property; `var(--brand)`
//   substitutes at computed-value time (F: a string-keyed custom-property map
//   with cycle detection and the guaranteed-invalid concept).
fn unsupported_custom_properties_and_var_drop() {
    let (mut world, mut root, target, report) =
        styled_target_load("a", ".a { --brand: #112233; width: var(--brand) }");

    assert_eq!(
        report.declarations_applied, 0,
        "neither the custom property nor the var() declaration should apply",
    );
    assert_eq!(
        report.declarations_dropped, 2,
        "the custom property and the var() declaration should each drop and count",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        BoxLength::Auto,
        "width should stay absent after the var() drop",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — F stage 3, D6 admission-gated):
// - SPEC: CSS Animations 1 §3 `animation` — the animation shorthand and its
//   longhands animate declared property values over time.
//   <https://drafts.csswg.org/css-animations-1/>
// - CURRENT (asserted): `.a { animation: spin 1s }` drops + counts the
//   declaration (`animation` is not in the `property_shape` name table).
// - SPEC-CORRECT: `animation` admits into the animation tier (F stage 3:
//   keyframes storage plus a per-property keyframed-interpolation sampler
//   sharing the transition machinery's `DeclValue` lerp table).
fn unsupported_animation_declarations_drop() {
    assert_decl_drop(".a { animation: spin 1s }", 0, 1);
}

#[test]
// ON-SPEC (the documented UA-sheet workaround — pinned so it cannot silently
// break): CSS Cascade 5 §6.4.1 origin order — a consumer's low-priority origin
// models a real UA sheet sitting between the behavior sheet and author sheets.
// <https://drafts.csswg.org/css-cascade-5/#cascading-origins>
//
// The crate models a UA sheet as a low-priority origin
// (`StyleRuleOrigin::new("ua", -100)`), and the behavior sheet folds at
// `i32::MIN` as the UA-ish tier (`winners_fold.rs::behavior_precedence`). Two
// scenarios pin the ordering:
//   (a) a classed div with `ua@-100` `.x { width: 10px }` and `author@0`
//       `.x { width: 20px }` computes `width == Some(20.0)` — higher origin
//       priority wins at equal specificity;
//   (b) a hovered input (its tag carries `with_default_field_state_styles`
//       hover `background-color: #191d26`, `dom_tag.rs:113-153`) with ONLY the
//       `-100` sheet `.x { background-color: #445566 }` computes `#445566` on
//       hover — the behavior sheet folds below every origin, so even a `-100`
//       author sheet beats it.
fn ua_sheet_workaround_low_priority_origin_loses_to_author_and_beats_behavior() {
    // (a) a real-UA analog at -100 loses to an author origin at 0.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("ua", -100),
        ".x { width: 10px }",
    );
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("author", 0),
        ".x { width: 20px }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(20.0),
        "(a) the author origin should beat the -100 UA analog at equal specificity",
    );

    // (b) the -100 UA analog beats the behavior sheet (i32::MIN) on hover.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let input = assert_append_input_to(&mut root, document_root);
    assert_set_attr_str(&mut root, input, "class", "x");
    assert_style_set(&mut root, input, "width", "60px");
    assert_style_set(&mut root, input, "height", "60px");
    assert_load_root_stylesheet(
        &root,
        StyleRuleOrigin::new("ua", -100),
        ".x { background-color: #445566 }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_set(30.0, 30.0));

    assert!(
        is_hovered(&root, input),
        "(b) the input should be hovered before asserting the background",
    );
    assert_eq!(
        root.computed_style(input)
            .expect("input")
            .as_visual()
            .background_rgba,
        parse_css_color_rgba("#445566").expect("proof color should parse"),
        "(b) the -100 UA-analog background should beat the behavior sheet's #191d26 hover",
    );
}
