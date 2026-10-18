#![allow(rustdoc::bare_urls)]
// The style pass resolves px lengths and the computed groups' initial values
// to exact `f32` literals, and a color's parsed rgba channels round-trip
// bit-identically through `parse_css_color_rgba`, so the typed computed-group
// comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! Cascade-tier boundary pins for the style system (PLAN.md B9).
//!
//! Purpose: pin the boundary of the cascade tiers the engine has deliberately
//! set aside — `!important` (F stage 1), custom properties/`var()`,
//! `@layer` (F stage 4), and the animation tier (F stage 3) — plus the
//! documented UA-sheet workaround (the on-spec pin that must not silently
//! break). The precedence record is `{ origin_priority, inline, specificity,
//! order }` (`decl_value.rs:39-41`), widened at redesign time exactly so the
//! deferred tiers are additive; the behavior sheet folds at `i32::MIN` as the
//! UA-ish tier (`winners_fold.rs::behavior_precedence`), so a consumer's
//! low-priority origin models a real UA sheet sitting between it and author
//! sheets.
//!
//! Class notes: every `unsupported_*` row is unimplemented (D6 admission-gated
//! absence per `MIGRATION.md`). The `!important`, custom-property/`var()`, and
//! animation rows pin the drop-and-count gate (a malformed/unadmitted
//! declaration is dropped and counted, no crash); the `@layer` row pins the
//! whole-load typed `Err` (at-rule, the same mechanics as B8's at-rule rows);
//! the UA-sheet workaround row is on-spec (pinned so it cannot silently
//! break). There are no `off_spec_*` (divergent) rows in this file — the
//! cascade tiers are absent, not wrong.

use bevy::prelude::World;
use ui_dom::{
    DomError, DomNode, DomNodeSlotKey, DomPointerState, DomPointerUpdate, DomPseudoState, RootDom,
    RootDomMetricName, StyleRuleOrigin, StylesheetLoadReport, parse_css_color_rgba,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_input_to, assert_decl_drop, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
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
// UNSUPPORTED (class: unimplemented — F stage 1, D6 admission-gated):
// - SPEC: CSS Cascade 5 §6.1 — declarations marked `!important` form the
//   important tier, above every normal author declaration.
//   <https://drafts.csswg.org/css-cascade-5/#cascade-origin>
// - CURRENT (asserted): `.a { width: 10px !important; height: 5px }` reports
//   `declarations_applied == 1` (height) and `declarations_dropped == 1`
//   (width) — the `!important` token is left in the value text, so the width
//   value fails the grammar and the DECLARATION drops while the rest of the
//   rule survives. Computed: `height == Some(5.0)`, `width == None`.
// - SPEC-CORRECT: `!important` is a declaration-level flag (CSS Syntax 3 §5.5),
//   stripped before the value grammar; the declaration applies in the
//   important tier. F stage 1's design is already shaped: importance bits
//   arrive as a parallel bit-vector (the Servo `declarations_importance`
//   shape named in the redesign's F verdict; `decl_block.rs`'s header notes
//   the future arrival).
// - ROOT CAUSE: `parse_non_negative_length_or_unset` (`decl_parse.rs`) rejects
//   the trailing `!important` in `10px !important`, so `expand_shape` returns
//   `None` and the declaration drops.
fn unsupported_important_declarations_drop_but_the_rule_survives() {
    let (mut world, mut root, target, report) =
        styled_target_load("a", ".a { width: 10px !important; height: 5px }");

    assert_eq!(
        report.declarations_applied, 1,
        "height should apply while width !important drops",
    );
    assert_eq!(
        report.declarations_dropped, 1,
        "the width !important declaration should be dropped and counted",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let box_ = root.computed_style(target).expect("target").as_box();
    assert_eq!(box_.height, Some(5.0), "height should compute 5px");
    assert_eq!(box_.width, None, "width should stay absent after the drop");
}

#[test]
// UNSUPPORTED (class: unimplemented — the same F stage 1 tier):
// - SPEC: css-style-attr §3 — the style attribute is a declaration list parsed
//   with forward-compatible error handling (drop-and-continue, counted).
//   <https://drafts.csswg.org/css-style-attr/#syntax>
// - CURRENT (asserted): `set_attr_str(target, "style", "width: 10px !important")`
//   returns `Ok(())` (the style route is drop-and-continue, never a rejection),
//   counts exactly one `RootDomMetricName::StyleDeclarationDropped`, and
//   computes `width == None`.
// - SPEC-CORRECT: `!important` admits the declaration into the important tier
//   (F stage 1); the value then applies at `10px`.
// - ROOT CAUSE: the style route parses through `parse_declaration_block`
//   (`root_dom.rs::route_style_attribute`), which drops the `!important`
//   value at grammar time — the same mechanics as the stylesheet route.
fn unsupported_important_in_style_attribute_drops() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);

    root.set_attr_str(target, "style", "width: 10px !important")
        .expect("the style route should drop-and-continue, not reject");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.count(RootDomMetricName::StyleDeclarationDropped),
        1,
        "the !important style-attribute declaration should be dropped and counted",
    );
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        None,
        "width should stay absent after the style-attribute drop",
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
        None,
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
        Some(20.0),
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
