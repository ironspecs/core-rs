#![allow(rustdoc::bare_urls)]

//! Selector-subset and at-rule boundary pins for the style system (PLAN.md B8).
//!
//! Purpose: pin the boundary of the admitted selector grammar — which selector
//! forms compile into `CompiledRule`s and which fail the WHOLE load with a
//! typed error — plus the at-rule boundary (`@media`/`@supports`/`@keyframes`/
//! `@font-face`/`@import`/`@charset` are not statements the block parser
//! understands, so they fail the whole load too). Unlike declaration drops
//! (which are drop-and-continue, CSS 2.1 §4.1.7), an unsupported selector is
//! FATAL at load: `parse_selector` returns `Err` → `DomError::InvalidSelector`
//! → `load_stylesheet` returns `Err` to the caller. That is the whole-load
//! rejection this file pins, row by row, against the closed grammar in
//! `src/domains/style/mod.rs` (`parse_css_blocks`, `parse_selector`,
//! `parse_simple_selector`).
//!
//! Class notes: every `unsupported_*` row is unimplemented (D6 admission-gated
//! absence per `MIGRATION.md`). The unimplemented rows pin the load-error gate
//! (no crash, typed `Err`) so a later admission flips exactly these rows
//! on-spec; the F1 atomicity row is on-spec (see the F1 note). The
//! combinator/attribute/pseudo-element/pseudo-class/selector-list syntax is
//! rejected by `contains_unsupported_selector_syntax` (`[`, `]`, `>`, `+`, `~`,
//! `,`, `(`, `)`, `{`, `}`, `=`, whitespace controls) and by
//! `split_selector_pseudo_state` (only `:hover`/`:active`/`:focus`/
//! `:focus-visible` are admitted); at-rules and `@import`/`@charset` are
//! rejected because the block parser treats everything before `{` as a
//! selector, so `@media (…) { … }`'s trailing `}` or `@import …;`'s missing
//! `{`/selector grammar each fail with `InvalidSelector` (verified actual; see
//! the per-test notes).
//!
//! F1 note (on-spec, relabeled from `off_spec_stylesheet_load_failure_partially_applies`
//! in B8 landing (b)): `stylesheet_load_failure_is_atomic` pins the atomic
//! load — `load_stylesheet` is buffer-then-commit, so rules, indexes, order,
//! and revision mutate only after every block parses, and a typed `Err` means
//! nothing applied. See PLAN.md B8 "The atomicity truth (F1, adjudicated)".

use ui_dom::{DomError, DomPointerUpdate, StylesheetLoadReport};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
};

/// Loads `css` on a fresh root and returns the typed load error. Selector and
/// at-rule rejection is fatal at load (unlike declaration drops), so the
/// `Err` variant is the whole pin for the `unsupported_*` rows.
fn load_error(css: &str) -> DomError {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect_err("stylesheet should be rejected at load")
}

/// Loads `css` on a fresh root and returns the load report (the positive
/// control used by the block-structure honesty correction).
fn load_report(css: &str) -> StylesheetLoadReport {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    root.context()
        .load_stylesheet(theme_stylesheet_origin(), css)
        .expect("stylesheet should load")
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: Selectors 4 §13 defines the combinators `>` (child), `+`
//   (next-sibling), and `~` (subsequent-sibling).
//   <https://drafts.csswg.org/selectors-4/#combinators>
// - CURRENT (asserted): `div > span`, `div + span`, and `div ~ span` each fail
//   the whole load with `Err(DomError::InvalidSelector)` — the combinator
//   characters are rejected by `contains_unsupported_selector_syntax`
//   (`src/domains/style/mod.rs:346-353`), so `parse_simple_selector` returns
//   `Err` and `load_stylesheet`'s fatal `?` aborts the load.
// - SPEC-CORRECT: a `Combinator` enum between compounds; the matcher replaces
//   the parent-chain scan with the exact relation (PLAN.md B8 Phase 2 preview).
fn unsupported_combinator_selectors_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "child_combinator",
            css: "div > span { }",
        },
        Case {
            name: "next_sibling_combinator",
            css: "div + span { }",
        },
        Case {
            name: "subsequent_sibling_combinator",
            css: "div ~ span { }",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: Selectors 4 §6 defines attribute selectors (`[attr]`,
//   `[attr=value]`, `[attr~=value]`, …).
//   <https://drafts.csswg.org/selectors-4/#attribute-selectors>
// - CURRENT (asserted): `[type]`, `[type=button]`, and `[class~=x]` each fail
//   the whole load with `Err(DomError::InvalidSelector)` — `[`, `]`, `=`, and
//   `~` are all in the `contains_unsupported_selector_syntax` rejection set.
// - SPEC-CORRECT: a `SimpleSelector::Attr { name, value_matcher }` variant;
//   matching reads stored content attrs (PLAN.md B8 Phase 2 preview).
fn unsupported_attribute_selectors_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "presence",
            css: "[type] { }",
        },
        Case {
            name: "exact_value",
            css: "[type=button] { }",
        },
        Case {
            name: "word_match",
            css: "[class~=x] { }",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: Selectors 4 §13.1 defines the pseudo-elements `::before`,
//   `::first-line`, and `::first-letter`.
//   <https://drafts.csswg.org/selectors-4/#pseudo-elements>
// - CURRENT (asserted): each fails the whole load with
//   `Err(DomError::InvalidSelector)` — `split_selector_pseudo_state`
//   (`src/domains/style/mod.rs:355-372`) splits on the FIRST `:` and rejects
//   any pseudo-state text that itself contains another `:` (the double-colon
//   pseudo-element form).
// - SPEC-CORRECT: a pseudo-element arm in the selector model (PLAN.md B8 Phase
//   2 preview; `::before` content generation is deferred).
fn unsupported_pseudo_elements_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "before",
            css: "div::before { }",
        },
        Case {
            name: "first_line",
            css: "p::first-line { }",
        },
        Case {
            name: "first_letter",
            css: "p::first-letter { }",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: Selectors 4 §7 defines the structural (`:first-child`,
//   `:last-child`, `:nth-child()`, `:root`) and state (`:focus-within`,
//   `:checked`, `:disabled`) pseudo-classes, and §4 defines the negation
//   pseudo-class `:not()`.
//   <https://drafts.csswg.org/selectors-4/#structural-pseudos>
// - CURRENT (asserted): each (`div:first-child { }`, `div:last-child { }`,
//   `div:nth-child(2) { }`, `div:not(.x) { }`, `div:focus-within { }`,
//   `div:checked { }`, `div:disabled { }`, `div:root { }`) fails the whole
//   load with `Err(DomError::InvalidSelector)` — `split_selector_pseudo_state`
//   admits exactly `hover`/`active`/`focus`/`focus-visible` and rejects every
//   other pseudo-state token.
// - SPEC-CORRECT: structural pseudo-classes computed on the visit from sibling
//   links; state pseudo-classes read widget state bits (PLAN.md B8 Phase 2
//   preview).
fn unsupported_pseudo_classes_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "first_child",
            css: "div:first-child { }",
        },
        Case {
            name: "last_child",
            css: "div:last-child { }",
        },
        Case {
            name: "nth_child",
            css: "div:nth-child(2) { }",
        },
        Case {
            name: "not",
            css: "div:not(.x) { }",
        },
        Case {
            name: "focus_within",
            css: "div:focus-within { }",
        },
        Case {
            name: "checked",
            css: "div:checked { }",
        },
        Case {
            name: "disabled",
            css: "div:disabled { }",
        },
        Case {
            name: "root",
            css: "div:root { }",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: Selectors 4 §5 admits a selector list (`,`-separated) as a single
//   rule's selector, matching any listed member.
//   <https://drafts.csswg.org/selectors-4/#grouping>
// - CURRENT (asserted): `.a, .b { }` fails the whole load with
//   `Err(DomError::InvalidSelector)` — `,` is in the
//   `contains_unsupported_selector_syntax` rejection set.
// - SPEC-CORRECT: `parse_css_blocks` splits the selector text at top-level
//   commas; each member compiles to its own `CompiledRule` sharing one
//   `DeclBlockKey` (PLAN.md B8 Phase 2 preview).
fn unsupported_selector_lists_fail_the_load() {
    assert_eq!(
        load_error(".a, .b { }"),
        DomError::InvalidSelector,
        "a selector list should fail the whole load with InvalidSelector",
    );
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Conditional 3 §`@media`/§`@supports`, CSS Animations 1
//   §`@keyframes`, CSS Fonts 4 §`@font-face`.
// - CURRENT (asserted, VERIFIED ACTUAL): all four fail the whole load with
//   `Err(DomError::InvalidSelector)` — the block parser treats the at-rule
//   prelude as a selector. `@media (…) { .a { } }`/`@supports (…) { .a { } }`/
//   `@keyframes … { from { } to { } }` parse their prelude as a selector, then
//   hit a trailing `}` (the nested block's closer) which `parse_css_blocks`
//   rejects as a stray `}` → `InvalidSelector`; `@font-face { font-family: x; }`
//   parses one block whose selector `@font-face` fails
//   `is_valid_selector_ident` (the `@` is not an ident character) →
//   `InvalidSelector`.
// - SPEC-CORRECT: the block parser learns at-rule parsing (conditional rules,
//   feature queries); `@keyframes` is deferred to F stage 3 (PLAN.md B8 Phase
//   2 preview).
fn unsupported_at_rules_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "media",
            css: "@media (min-width: 100px) { .a { } }",
        },
        Case {
            name: "supports",
            css: "@supports (display: flex) { .a { } }",
        },
        Case {
            name: "keyframes",
            css: "@keyframes spin { from { } to { } }",
        },
        Case {
            name: "font_face",
            css: "@font-face { font-family: x; }",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated):
// - SPEC: CSS Cascade 5 §`@import`; CSS Syntax 3 §`@charset`.
// - CURRENT (asserted, VERIFIED ACTUAL): `@import url("theme.css");` and
//   `@charset "utf-8";` each fail the whole load with
//   `Err(DomError::InvalidSelector)` — neither has a `{`, so `parse_css_blocks`
//   hits its no-open-brace arm (`src/domains/style/mod.rs:248-250`) and returns
//   `InvalidSelector` (NOT `InvalidAttributeValue`; that variant belongs to the
//   missing-close-brace block case).
// - SPEC-CORRECT: `@import` fetches/loads a nested stylesheet; `@charset` is a
//   byte-encoding directive handled before parsing.
fn unsupported_import_and_charset_fail_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
    }

    let cases = [
        Case {
            name: "import",
            css: "@import url(\"theme.css\");",
        },
        Case {
            name: "charset",
            css: "@charset \"utf-8\";",
        },
    ];

    for case in cases {
        assert_eq!(
            load_error(case.css),
            DomError::InvalidSelector,
            "{} should fail the whole load with InvalidSelector",
            case.name,
        );
    }
}

#[test]
// UNSUPPORTED (class: unimplemented — D6 admission-gated; two honesty
// corrections vs PLAN.md B8's original "InvalidAttributeValue, block
// structure" claim):
// - SPEC: CSS Syntax 3 §malformed statements — a malformed statement is
//   skipped to the next `}`/`;` and counted, not fatal.
// - CURRENT (asserted, VERIFIED ACTUAL per row):
//     stray closing brace  `} .a { width: 1px; }`  → `Err(InvalidSelector)`
//       (`parse_css_blocks` leading-`}` arm, `src/domains/style/mod.rs:245-247`)
//     missing closing brace `.a { width: 1px;`     → `Err(InvalidAttributeValue)`
//       (`parse_css_blocks` no-close-brace arm, `src/domains/style/mod.rs:256-258`)
//     "stray text" premise  `garbage .a { }`        → loads OK (NOT an error):
//       `garbage` is a valid tag identifier in the closed grammar
//       (`is_valid_selector_ident` admits any ASCII alphanumeric/hyphen/
//       underscore run), so the whole thing parses as a valid `garbage .a`
//       DESCENDANT selector and inserts one rule. The "stray text before a
//       rule" premise does not exist for this input.
// - SPEC-CORRECT: statement-level error recovery (CSS 2.1 §4.1.7's
//   malformed-statement rule — skip to the next `}` and count) is an
//   admission-gated parser feature; the atomicity fix (landing (b)) makes the
//   remaining load-error paths honest in the meantime (PLAN.md B8).
fn malformed_block_structure_fails_the_load() {
    struct Case {
        name: &'static str,
        css: &'static str,
        expected: DomError,
    }

    let cases = [
        Case {
            name: "stray_closing_brace",
            css: "} .a { width: 1px; }",
            expected: DomError::InvalidSelector,
        },
        Case {
            name: "missing_closing_brace",
            css: ".a { width: 1px;",
            expected: DomError::InvalidAttributeValue,
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

    // Honesty correction (VERIFIED ACTUAL): the "stray text before a rule"
    // premise, concretized as `garbage .a { }`, does NOT fail the load —
    // `garbage` is a valid tag identifier, so this is a valid descendant
    // selector, not stray text.
    let report = load_report("garbage .a { }");
    assert_eq!(
        report.rules_inserted, 1,
        "garbage .a {{ }} is a valid descendant selector and should load one rule",
    );
}

#[test]
// ON-SPEC (relabeled from the F1 divergent `off_spec_stylesheet_load_failure_partially_applies`
// row in B8 landing (b)):
// - SPEC: a failed stylesheet load must be atomic — a typed `Err` means
//   "nothing happened". CSS Syntax 3 parse errors reject the whole sheet
//   without side effects.
// - CURRENT (asserted): loading `.good { width: 1px } div > span { }` returns
//   `Err(DomError::InvalidSelector)` AND nothing applies — the valid `.good`
//   prefix is not committed, so after classing the target "good" and running
//   an update, `computed width == None`.
// - IMPLEMENTATION: `style_runtime.rs::load_stylesheet` is buffer-then-commit
//   (rules, indexes, order, and revision mutate only after every block
//   parses), so the early return discards the staged prefix with no side
//   effects.
fn stylesheet_load_failure_is_atomic() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);

    let error = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".good { width: 1px } div > span { }",
        )
        .expect_err("the bad `div > span` selector should fail the load");
    assert_eq!(
        error,
        DomError::InvalidSelector,
        "the load should fail on the `div > span` combinator selector",
    );

    // The class write dirties the node; the update runs the style pass, which
    // must find NO `.good` rule (the failed load committed nothing).
    assert_set_attr_str(&mut root, target, "class", "good");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.computed_style(target).expect("target").as_box().width,
        None,
        "NOTHING applied: the valid `.good` prefix must not be live after the load Err",
    );
}

#[test]
// ON-SPEC (the G drop-report contract surviving the atomicity fix): a malformed
// declaration in the valid prefix still counts its drop even when a later
// block's selector fails the whole load. Drops are observations of malformed
// input, not rule-store mutations, so the counter moves while nothing applies.
fn failed_stylesheet_load_still_reports_declaration_drops() {
    let (_world, root) = attached_gpu_root_dom_default_viewport_setup();
    let before = root.context().declaration_drop_count();

    let error = root
        .context()
        .load_stylesheet(
            theme_stylesheet_origin(),
            ".good { width: 1px; bogus } div > span { }",
        )
        .expect_err("the bad `div > span` selector should fail the load");
    assert_eq!(
        error,
        DomError::InvalidSelector,
        "the load should fail on the `div > span` combinator selector",
    );

    assert_eq!(
        root.context().declaration_drop_count(),
        before + 1,
        "the malformed `bogus` declaration in the valid prefix still counts its drop",
    );
}
