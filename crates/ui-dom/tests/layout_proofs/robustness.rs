#![allow(clippy::float_cmp)]
// The style pass resolves px lengths to exact `f32` literals (integer px and
// the computed groups' initial values are bit-exact), so the typed
// computed-group comparisons in this module are deliberate exact checks.

//! Robustness safety net for `ui-dom` (PLAN.md B12).
//!
//! Purpose: pin the no-crash side of the crate's boundary handling over
//! adversarial inputs. Every scenario here asserts BOTH the defined outcome
//! AND that the call returns without panic. This is a safety net, not an
//! admission track: a failure here is a boundary-policy bug — the malformed
//! input must be met with a typed `DomError` or a counted drop (the two
//! sanctioned outcomes), never a panic.
//!
//! Class note: `robustness_*` is orthogonal to the registry classes in
//! `layout_proofs/README.md`. A robustness pin also carries the
//! divergent/unimplemented/on-spec class of the behavior it guards, but its
//! job is the crash-resistance contract, so it sits at the suite root
//! (`robustness.rs`) rather than in a style-domain file.
//!
//! Cross-reference: the zero-font-size render/caret family
//! (`robustness_zero_font_size_renders_nothing`,
//! `robustness_zero_font_size_textarea_caret_does_not_crash`) lives in
//! `style/unsupported_values.rs`, where the B1 fix and its cosmic-text guard
//! are documented. This file covers the rest of the adversarial surface:
//! empty/whitespace input, block-structure abuse, comments, volume, extreme
//! values, typed-writer abuse, and encoding.

use std::fmt::Write as _;

use ui_dom::{DomError, DomPointerUpdate, StylesheetLoadReport};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_decl_drop, assert_root_dom_update,
    assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup, box_px,
    take_staged_layer_paint_plans, theme_stylesheet_origin,
};

/// Loads `css` on a fresh root and returns the load report (the positive
/// outcomes used by the empty/whitespace and volume scenarios).
fn load_report(css: &str) -> StylesheetLoadReport {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load")
}

/// Loads `css` on a fresh root and returns the typed load error (fatal load
/// rejection is the defined outcome for the block-structure and comment rows).
fn load_error(css: &str) -> DomError {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect_err("stylesheet should be rejected at load")
}

#[test]
// ROBUSTNESS (class: on-spec keeps, with one honesty correction vs the plan):
// - Empty and whitespace-only stylesheets load `Ok` with zero rules and zero
//   applied/dropped declarations (no blocks, no declarations).
// - `.a { }` and `.a { ; ; }` load `Ok` with ONE rule each (honesty
//   correction: an empty rule is a valid CSS rule and is committed with an
//   empty declaration block; `rules_inserted` is 1, NOT the 0 the plan
//   guessed). Their applied/dropped counts are both 0 (no declarations).
// - `style=""` and `style=";"` both route into the inline declaration store,
//   parse to an empty declaration list, and replace the inline block with
//   nothing (`canonical_style_snapshot` is empty) — never a crash.
fn robustness_empty_and_whitespace_inputs() {
    struct Case {
        name: &'static str,
        css: &'static str,
        rules: usize,
    }

    let cases = [
        Case {
            name: "empty",
            css: "",
            rules: 0,
        },
        Case {
            name: "whitespace_only",
            css: "   \n\t  ",
            rules: 0,
        },
        Case {
            name: "empty_rule",
            css: ".a { }",
            rules: 1,
        },
        Case {
            name: "semicolons_only_rule",
            css: ".a { ; ; }",
            rules: 1,
        },
    ];

    for case in cases {
        let report = load_report(case.css);
        assert_eq!(
            report.rules_inserted, case.rules,
            "{} rules_inserted",
            case.name,
        );
        assert_eq!(
            report.declarations_applied, 0,
            "{} declarations_applied",
            case.name,
        );
        assert_eq!(
            report.declarations_dropped, 0,
            "{} declarations_dropped",
            case.name,
        );
    }

    // Empty inline style lists replace the block with nothing.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    for value in ["", ";"] {
        root.set_attr_str(target, "style", value)
            .expect("empty style list should apply");
        assert!(
            root.canonical_style_snapshot(target).styles.is_empty(),
            "style={value:?} should leave no inline declarations",
        );
    }
    // A full update after the empty writes still runs clean (no crash anywhere).
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
}

#[test]
// ROBUSTNESS (class: unimplemented — D6 admission-gated load-error gate).
// This file pins the no-crash side; B8 (`style/unsupported_selectors.rs`)
// pins the class. The four rows are the whole-load `Err` outcomes, verified
// per `parse_css_blocks` (`src/domains/style/mod.rs:240-267`):
//   missing close brace `.a { width: 1px;`  → `InvalidAttributeValue`
//     (the no-close-brace arm, `mod.rs:256-258`)
//   stray close brace   `} .a { width: 1px }` → `InvalidSelector`
//     (the leading-`}` arm, `mod.rs:245-247`)
//   extra close brace   `.a { width: 1px } }` → `InvalidSelector`
//     (the first block parses, the leftover `}` hits the leading-`}` arm)
//   bare open brace     `{`                   → `InvalidSelector`
//     (empty selector arm, `mod.rs:252-254`)
fn robustness_block_structure_abuse() {
    struct Case {
        name: &'static str,
        css: &'static str,
        expected: DomError,
    }

    let cases = [
        Case {
            name: "missing_close_brace",
            css: ".a { width: 1px;",
            expected: DomError::InvalidAttributeValue,
        },
        Case {
            name: "stray_close_brace",
            css: "} .a { width: 1px }",
            expected: DomError::InvalidSelector,
        },
        Case {
            name: "extra_close_brace",
            css: ".a { width: 1px } }",
            expected: DomError::InvalidSelector,
        },
        Case {
            name: "bare_open_brace",
            css: "{",
            expected: DomError::InvalidSelector,
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            case.expected,
            "{} should fail the whole load with {:?}",
            case.name,
            case.expected,
        );
    }
}

#[test]
// ROBUSTNESS (comment handling; classes mixed, verified actual per part):
// (a) `/*a*/ .a { width: 1px }` — comment-adjacent declaration. `strip_block_comments`
//     removes the comment, the rule applies, computed width is 1px.
// (b) `/* unterminated` swallows everything to EOF: `strip_block_comments`
//     (`src/domains/style/mod.rs:269-295`) leaves `in_comment` true at EOF and
//     returns `Err(DomError::InvalidSelector)` (`mod.rs:291-293`), so the whole
//     load fails atomically — nothing is committed.
// (c) `width: 10px; // junk` — `//` is NOT a line comment in this parser; the
//     declaration splitter (`parse_segment`, `decl_parse.rs:101-112`) treats
//     `// junk` as a bare segment with no colon and drops + counts it while
//     `width: 10px` applies: applied == 1, dropped == 1.
// (d) `// junk\n.a { width: 1px }` at statement level — `//` reaches the
//     selector parser and fails `is_valid_selector_ident`
//     (`src/domains/style/mod.rs:453-457`): the whole load fails with
//     `InvalidSelector`.
fn robustness_comment_handling() {
    // (a) comment-adjacent declaration applies.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "a");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), "/*a*/ .a { width: 1px }")
        .expect("comment-adjacent rule should load");
    assert_eq!(report.rules_inserted, 1);
    assert_eq!(report.declarations_applied, 1);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(1.0),
    );

    // (b) unterminated block comment → whole load fails with InvalidSelector.
    assert_eq!(
        load_error("/* unterminated\n.a { width: 1px }"),
        DomError::InvalidSelector,
    );

    // (c) `// junk` as a declaration segment: width applies, `// junk` drops.
    let report = load_report(".a { width: 10px; // junk }");
    assert_eq!(report.rules_inserted, 1);
    assert_eq!(report.declarations_applied, 1);
    assert_eq!(report.declarations_dropped, 1);

    // (d) `// junk` at statement level: `//` fails the ident check.
    assert_eq!(
        load_error("// junk\n.a { width: 1px }"),
        DomError::InvalidSelector,
    );
}

#[test]
// ROBUSTNESS (volume; outcome-only — no wall-clock pins):
// (a) a 10,000-rule stylesheet loads `Ok` with `rules_inserted == 10_000` and
//     the last rule (`.r9999`) matches a classed target. Load-time stress over
//     the `SlotMap` arena density, outside the pinned update windows.
// (b) one rule with 200 declaration segments (cycled over 20 distinct
//     longhand names) reports `declarations_applied == 200` while the block
//     holds the UPSERTED count — 20 distinct longhands, last-wins per property
//     (observed via `compute_style_map` = 21 winners: 20 declared + the div's
//     built-in `display: block`, and via the last `width` write, 180px).
// (c) a 64-compound descendant selector over a 64-deep div nest matches the
//     leaf. TRIPWIRE, not a discrimination proof: the load-bearing invariant
//     is that the parent-chain scan in `selector_matches_node`
//     (`src/domains/style/mod.rs:185-223`) is iterative by construction; this
//     row exists to catch a regression to recursion, not to prove a depth
//     limit (64 frames discriminates nothing).
fn robustness_volume() {
    // (a) 10,000 rules.
    let mut css = String::new();
    for i in 0..10_000 {
        write!(css, ".r{i} {{ width: 1px }}").expect("writing to a String should not fail");
    }
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), &css)
        .expect("10k-rule sheet should load");
    assert_eq!(report.rules_inserted, 10_000);
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "r9999");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(1.0),
    );

    // (b) one rule, 200 declaration segments, many repeats with last-wins upserts.
    let names = [
        "width",
        "height",
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
        "top",
        "left",
        "right",
        "bottom",
        "font-size",
        "line-height",
        "row-gap",
        "column-gap",
        "outline-width",
        "outline-offset",
    ];
    let mut decls = Vec::new();
    for i in 0..200 {
        decls.push(format!("{}: {i}px", names[i % names.len()]));
    }
    let css = format!(".a {{ {} }}", decls.join("; "));
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "a");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), &css)
        .expect("200-declaration rule should load");
    assert_eq!(report.declarations_applied, 200);
    assert_eq!(report.declarations_dropped, 0);
    assert_eq!(report.rules_inserted, 1);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    // 20 distinct longhands survive the upsert (not 200); `compute_style_map`
    // also exposes the div's built-in `display: block` base behavior, so the
    // cascade winner count is 20 + 1. The last `width` write (180px) wins.
    assert_eq!(
        root.compute_style_map(target)
            .expect("cascade query should succeed")
            .len(),
        21,
    );
    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        box_px(180.0),
    );

    // (c) 64-compound descendant selector over a 64-deep div nest.
    let selector = vec!["div"; 64].join(" ");
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let mut leaf = document_root;
    for _ in 0..64 {
        leaf = assert_append_div_to(&mut root, leaf);
    }
    root.context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            &format!("{selector} {{ width: 1px }}"),
        )
        .expect("64-compound selector should load");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(leaf).expect("leaf").as_box().width,
        box_px(1.0),
    );
}

#[test]
// ROBUSTNESS (extreme values; classes mixed):
// (a) `font-size: 100000px` on a container with text — a full update completes
//     and the staged paint plan exists (no crash on giant shaping).
// (b) `margin-left: -3.4e38px` — a legal negative margin (finite f32) is
//     admitted and reaches the computed box group unclamped.
// (c) `opacity: 1.0000001` — the f32 round-trip lands at 1.000000119…, just
//     above 1.0; `parse_opacity` (`decl_parse.rs`) clamps any finite f32 into
//     `[0, 1]`, so it is admitted and computes to exactly `1.0` (same clamp
//     behavior as the on-spec opacity pin).
fn robustness_extreme_values() {
    // (a) giant font-size renders without crash.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "x");
    let _text = assert_append_text_to(&mut root, container, "giant glyphs");
    let report = root
        .context()
        .load_stylesheet(theme_stylesheet_origin(), ".x { font-size: 100000px }")
        .expect("font-size: 100000px should admit");
    assert_eq!(report.declarations_dropped, 0);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let plans = take_staged_layer_paint_plans(&root);
    assert!(!plans.is_empty(), "a giant-font update should stage a plan");
    assert!(
        plans.iter().any(|plan| !plan.text_runs.is_empty()),
        "a giant-font container should stage a text run",
    );

    // (b) legal negative margin at the finite-f32 extreme.
    let (mut world, mut root, target, report) = {
        let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let target = assert_append_div_to(&mut root, document_root);
        assert_set_attr_str(&mut root, target, "class", "x");
        let report = root
            .context()
            .load_stylesheet(theme_stylesheet_origin(), ".x { margin-left: -3.4e38px }")
            .expect("margin-left: -3.4e38px should admit");
        (world, root, target, report)
    };
    assert_eq!(report.declarations_applied, 1);
    assert_eq!(report.declarations_dropped, 0);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").as_box().margin[3],
        box_px(-3.4e38_f32),
    );

    // (c) opacity just past 1.0 clamps to exactly 1.0 (f32 1.000000119 → 1.0).
    let (mut world, mut root, target, report) = {
        let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
        let document_root = root.document_node().0;
        let target = assert_append_div_to(&mut root, document_root);
        assert_set_attr_str(&mut root, target, "class", "x");
        let report = root
            .context()
            .load_stylesheet(theme_stylesheet_origin(), ".x { opacity: 1.0000001 }")
            .expect("opacity: 1.0000001 should admit");
        (world, root, target, report)
    };
    assert_eq!(report.declarations_applied, 1);
    assert_eq!(report.declarations_dropped, 0);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target)
            .expect("target")
            .as_visual()
            .opacity,
        1.0,
    );
}

#[test]
// ROBUSTNESS (typed-writer abuse): the `StyleMut` error paths are the typed
// boundary, never a panic.
// - `set("", "")` and `set("width", "")` fail the strict single-declaration
//   contract with `InvalidAttributeValue` (empty name/value).
// - `set_all("width: 10px; height: 5px")` reports applied + dropped == 2.
// - a 200-declaration `set_all` completes and reports applied + dropped == 200
//   exactly (the bounded report), with the inline block holding the upserted
//   20 distinct longhands.
// - `set` on a removed-node key → `InvalidNodeKey`.
// - `set` on a text node / the document root → `InvalidAttributeTarget`.
fn robustness_typed_writer_abuse() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);

    // Strict single-declaration failures.
    assert_eq!(
        root.style_mut(target).set("", ""),
        Err(DomError::InvalidAttributeValue),
    );
    assert_eq!(
        root.style_mut(target).set("width", ""),
        Err(DomError::InvalidAttributeValue),
    );

    // Declaration-list contract: 2 declarations, all applied.
    let report = root
        .style_mut(target)
        .set_all("width: 10px; height: 5px")
        .expect("set_all should apply");
    assert_eq!(report.applied + report.dropped, 2);

    // 200-declaration set_all: the report covers all 200, the call completes.
    let names = [
        "width",
        "height",
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
        "top",
        "left",
        "right",
        "bottom",
        "font-size",
        "line-height",
        "row-gap",
        "column-gap",
        "outline-width",
        "outline-offset",
    ];
    let mut decls = Vec::new();
    for i in 0..200 {
        decls.push(format!("{}: {i}px", names[i % names.len()]));
    }
    let report = root
        .style_mut(target)
        .set_all(&decls.join("; "))
        .expect("200-declaration set_all should complete");
    assert_eq!(report.applied + report.dropped, 200);
    assert_eq!(report.applied, 200);
    assert_eq!(report.dropped, 0);
    assert_eq!(
        root.canonical_style_snapshot(target).styles.len(),
        20,
        "200 segments should upsert to 20 distinct inline longhands",
    );

    // A full update after the writes still runs clean.
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    // Removed-node key → InvalidNodeKey (the removed child key is gone).
    let removed_parent = assert_append_div_to(&mut root, document_root);
    let removed_child = assert_append_div_to(&mut root, removed_parent);
    root.remove_node(removed_parent).expect("remove parent");
    assert_eq!(
        root.style_mut(removed_child).set("width", "10px"),
        Err(DomError::InvalidNodeKey),
    );

    // Non-element targets → InvalidAttributeTarget.
    let text = assert_append_text_to(&mut root, document_root, "label");
    assert_eq!(
        root.style_mut(text).set("width", "10px"),
        Err(DomError::InvalidAttributeTarget),
    );
    assert_eq!(
        root.style_mut(document_root).set("width", "10px"),
        Err(DomError::InvalidAttributeTarget),
    );
}

#[test]
// ROBUSTNESS (encoding; classes mixed, verified actual):
// (a) a UTF-8 BOM (`\u{FEFF}`) at the stylesheet start is NOT trimmed by
//     `str::trim()` (U+FEFF is not `char::is_whitespace`), so it reaches the
//     selector parser attached to `.a` and fails `is_valid_selector_ident`
//     (`src/domains/style/mod.rs:453-457`): the whole load fails with
//     `InvalidSelector`.
// (b) `font-family: ヒラギノ角ゴ` (unquoted CJK) and
//     `font-family: "Source Han Sans"` (quoted with spaces) both intern
//     verbatim through the `Text` grammar; `font_family()` resolves the exact
//     strings back (non-ASCII interning round-trips).
// (c) a NUL byte in a declaration value (`width: 10\0px`) fails
//     `parse_finite_length_scalar` (`dom_attr_parse.rs:342-347`) and drops +
//     counts — no crash.
fn robustness_encoding() {
    // (a) BOM-prefixed stylesheet → selector error (BOM is not trimmed).
    assert_eq!(
        load_error("\u{FEFF}.a { width: 1px }"),
        DomError::InvalidSelector,
    );

    // (b) non-ASCII font-family values intern and round-trip exactly.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    let report = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".x { font-family: ヒラギノ角ゴ } .x { font-family: \"Source Han Sans\" }",
        )
        .expect("non-ASCII font-family should admit");
    assert_eq!(report.declarations_dropped, 0);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    // Two rules share specificity/origin; the later rule's declaration wins.
    assert_eq!(
        root.computed_style(target).expect("target").font_family(),
        Some("\"Source Han Sans\""),
    );

    // The unquoted CJK form round-trips on its own.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    root.context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".x { font-family: ヒラギノ角ゴ }",
        )
        .expect("unquoted CJK font-family should admit");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").font_family(),
        Some("ヒラギノ角ゴ"),
    );

    // (c) NUL byte in a declaration value drops + counts, no crash.
    assert_decl_drop(".x { width: 10\u{0}px }", 0, 1);
}
