#![allow(rustdoc::bare_urls)]
// The style pass resolves px/em and rgba channels to exact `f32` literals
// (integer px and the computed groups' initial values are bit-exact), so the
// typed computed-group comparisons in this module are deliberate exact checks.
#![allow(clippy::float_cmp)]

//! CSS cascade and inheritance layout proofs.
//!
//! This module owns the integration proofs for the `ui-dom` cascade model:
//! selector specificity, source-order and origin precedence, inheritance
//! transmission, pseudo-state rule application, and declaration-block error
//! handling (stylesheet and `style`-attribute halves, both on-spec). Every
//! family is table-driven and asserts through the public `RootDom` surface:
//! staged paint plans are the primary measure for geometry families, and
//! typed computed-group reads (`root.computed_style(...)`) plus retained text
//! intrinsics pin the non-paintable inheritance outcomes.
//!
//! Spec references:
//! - CSS Cascade 5 §3 (inheritance):
//!   <https://drafts.csswg.org/css-cascade-5/#inheriting>
//! - CSS Cascade 5 §6 (the cascade):
//!   <https://drafts.csswg.org/css-cascade-5/#cascading>
//! - Selectors 3 §17 (specificity):
//!   <https://www.w3.org/TR/selectors-3/#specificity>
//! - CSS 2.1 §6.4 (the cascade):
//!   <https://www.w3.org/TR/CSS21/cascade.html#cascading-order>
//! - CSS 2.1 §4.1.7 (rules for handling parsing errors):
//!   <https://www.w3.org/TR/CSS21/syndata.html#parsing-errors>
//! - css-style-attr §3 (syntax and parsing of the style attribute):
//!   <https://drafts.csswg.org/css-style-attr/#syntax>
//! - CSS Fonts 4 §3.1–3.3 (font-family, font-weight, font-style; each
//!   "Inherited: yes"):
//!   <https://drafts.csswg.org/css-fonts-4/#font-family-prop>
//!
//! Browser implementation references:
//! - Blink match result / selector / style engine:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/css/resolver/match_result.cc>
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/css/css_selector.cc>
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/css/style_engine.cc>
//! - Gecko stylist / rule tree:
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/servo/components/style/stylist.rs>
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/servo/components/style/rule_tree/mod.rs>
//!
//! Invariants and rules:
//!
//! - cascade precedence is exactly `(origin.priority, specificity, order)`;
//!   `!important`, a built-in UA stylesheet, and cascade layers are absent by
//!   design (see the off-spec registry in `layout_proofs/README.md`)
//! - the transmitting inherited set is exactly `color`, `font-size`,
//!   `line-height`, `font-family`, `font-weight`, `font-style`, `text-align`,
//!   `direction`, `white-space`, `overflow-wrap`, and `word-break`; each is
//!   carried in the descendant's computed font/text/color groups (spine
//!   3.3g-2a), while non-inherited box/visual/flex longhands stay at their
//!   initial values on the descendant
//! - stylesheet and `style`-attribute declaration blocks both parse
//!   drop-and-continue with a counted, observable report (CSS 2.1 §4.1.7,
//!   css-style-attr §3); this module has no off-spec proofs (see the registry
//!   in `layout_proofs/README.md` for the one remaining divergence, in
//!   `layout/normal_flow.rs`)

use bevy::prelude::World;
use ui_dom::{
    AlignContentKw, DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerState, DomPointerUpdate,
    DomTextAlign, DomTextDirection, FlexBasisKw, JustifyContentKw, Keyword, RootDom,
    RootDomMetricName, StyleRuleOrigin, TextIntrinsic, parse_css_color_rgba,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_span_to, assert_append_text_to, assert_close,
    assert_load_root_stylesheet, assert_node_layout, assert_rect_op_fill, assert_rect_op_geometry,
    assert_root_dom_update, assert_set_attr_str, assert_style_set,
    attached_gpu_root_dom_default_viewport_setup, take_staged_layer_paint_plans,
    theme_stylesheet_origin,
};

/// The border-box height every geometry family pins on the target div so a
/// rect op exists; width comes from the competing rules under test.
const TARGET_HEIGHT_PX: f32 = 40.0;

/// Asserts one geometry-family update produced a single layer plan with a
/// single transparent-filled rect at the origin with the given border-box
/// width and the shared target height.
fn assert_single_rect_target_width(root: &RootDom, expected_width_px: f32, label: &str) {
    let plans = take_staged_layer_paint_plans(root);
    assert_eq!(plans.len(), 1, "{label} should have one layer plan");
    assert_eq!(plans[0].rects.len(), 1, "{label} should have one rect op");
    assert_rect_op_geometry(
        &plans[0].rects[0],
        0.0,
        0.0,
        expected_width_px,
        TARGET_HEIGHT_PX,
        label,
    );
    assert_rect_op_fill(&plans[0].rects[0], [0.0; 4], label);
}

/// Clones one text node's retained intrinsic metrics.
fn text_intrinsic(root: &RootDom, key: DomNodeSlotKey) -> TextIntrinsic {
    match root.node(key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("cascade proof text node should keep text intrinsic state"),
    }
}

/// Returns `(world, root, target)`: a target div appended to the document root
/// with an optional id, an optional class list, and the shared height.
fn target_div_setup(id: Option<&str>, class: &str) -> (World, RootDom, DomNodeSlotKey) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    if let Some(id) = id {
        assert_set_attr_str(&mut root, target, "id", id);
    }
    if !class.is_empty() {
        assert_set_attr_str(&mut root, target, "class", class);
    }
    assert_style_set(&mut root, target, "height", "40px");
    (world, root, target)
}

#[test]
// Selectors 3 §17: specificity is the (a, b, c) triple compared
// lexicographically; the universal selector contributes zero.
fn specificity_orders_by_id_class_tag_with_universal_zero() {
    struct SpecificityCase {
        name: &'static str,
        css: &'static str,
        id: Option<&'static str>,
        class: &'static str,
        expected_width_px: f32,
    }

    let cases = [
        SpecificityCase {
            name: "id_beats_class",
            css: "#i{width:10px} .a{width:20px}",
            id: Some("i"),
            class: "a",
            expected_width_px: 10.0,
        },
        SpecificityCase {
            name: "class_beats_tag",
            css: ".a{width:20px} div{width:30px}",
            id: None,
            class: "a",
            expected_width_px: 20.0,
        },
        SpecificityCase {
            name: "two_classes_beat_one",
            css: ".a.b{width:10px} .a{width:20px}",
            id: None,
            class: "a b",
            expected_width_px: 10.0,
        },
        SpecificityCase {
            name: "tag_plus_class_beats_class",
            css: "div.a{width:10px} .a{width:20px}",
            id: None,
            class: "a",
            expected_width_px: 10.0,
        },
        SpecificityCase {
            name: "id_beats_tag_and_two_classes",
            css: "#i{width:10px} div.a.b{width:20px}",
            id: Some("i"),
            class: "a b",
            expected_width_px: 10.0,
        },
        SpecificityCase {
            name: "tag_beats_universal",
            css: "*{width:30px} div{width:20px}",
            id: None,
            class: "",
            expected_width_px: 20.0,
        },
    ];

    for case in cases {
        let (mut world, mut root, _target) = target_div_setup(case.id, case.class);
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), case.css);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_single_rect_target_width(&root, case.expected_width_px, case.name);
    }
}

#[test]
// CSS Cascade 5 §6.4.4: at equal origin and specificity, the later declaration
// in source order wins.
fn equal_precedence_source_order_later_rule_wins() {
    enum SourceOrderLoad {
        SingleSheet(&'static str),
        TwoLoads(&'static str, &'static str),
    }

    struct SourceOrderCase {
        name: &'static str,
        load: SourceOrderLoad,
        class: &'static str,
        expected_width_px: f32,
    }

    let cases = [
        SourceOrderCase {
            name: "single_sheet_later_rule_wins",
            load: SourceOrderLoad::SingleSheet(".x{width:10px} .x{width:20px}"),
            class: "x",
            expected_width_px: 20.0,
        },
        SourceOrderCase {
            name: "single_sheet_reversed_later_rule_wins",
            load: SourceOrderLoad::SingleSheet(".x{width:20px} .x{width:10px}"),
            class: "x",
            expected_width_px: 10.0,
        },
        SourceOrderCase {
            name: "two_loads_later_sheet_wins",
            load: SourceOrderLoad::TwoLoads(".x{width:10px}", ".x{width:20px}"),
            class: "x",
            expected_width_px: 20.0,
        },
        SourceOrderCase {
            name: "two_loads_reversed_later_sheet_wins",
            load: SourceOrderLoad::TwoLoads(".x{width:20px}", ".x{width:10px}"),
            class: "x",
            expected_width_px: 10.0,
        },
        SourceOrderCase {
            name: "equal_specificity_two_classes_later_rule_wins",
            load: SourceOrderLoad::SingleSheet(".x{width:10px} .y{width:20px}"),
            class: "x y",
            expected_width_px: 20.0,
        },
    ];

    for case in cases {
        let (mut world, mut root, _target) = target_div_setup(None, case.class);
        match case.load {
            SourceOrderLoad::SingleSheet(css) => {
                assert_load_root_stylesheet(&root, theme_stylesheet_origin(), css);
            }
            SourceOrderLoad::TwoLoads(first, second) => {
                assert_load_root_stylesheet(&root, theme_stylesheet_origin(), first);
                assert_load_root_stylesheet(&root, theme_stylesheet_origin(), second);
            }
        }
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_single_rect_target_width(&root, case.expected_width_px, case.name);
    }
}

#[test]
// The crate origin model maps to CSS Cascade 5 §6.4.1: origins beat
// specificity. `!important` and cascade layers are absent by design (registry).
fn origin_priority_dominates_specificity() {
    struct OriginCase {
        name: &'static str,
        base_css: &'static str,
        base_priority: i32,
        theme_css: &'static str,
        theme_priority: i32,
        id: Option<&'static str>,
        class: &'static str,
        expected_width_px: f32,
    }

    let cases = [
        OriginCase {
            name: "theme_priority_beats_base_id_specificity",
            base_css: "#i{width:10px}",
            base_priority: 0,
            theme_css: "div{width:30px}",
            theme_priority: 1,
            id: Some("i"),
            class: "",
            expected_width_px: 30.0,
        },
        OriginCase {
            name: "theme_id_beats_base_tag_within_origin_priority",
            base_css: "div{width:30px}",
            base_priority: 0,
            theme_css: "#i{width:10px}",
            theme_priority: 1,
            id: Some("i"),
            class: "",
            expected_width_px: 10.0,
        },
        OriginCase {
            name: "equal_priority_specificity_orders",
            base_css: "#i{width:10px}",
            base_priority: 1,
            theme_css: ".a{width:20px}",
            theme_priority: 1,
            id: Some("i"),
            class: "a",
            expected_width_px: 10.0,
        },
    ];

    for case in cases {
        let (mut world, mut root, _target) = target_div_setup(case.id, case.class);
        assert_load_root_stylesheet(
            &root,
            StyleRuleOrigin::new("base", case.base_priority),
            case.base_css,
        );
        assert_load_root_stylesheet(
            &root,
            StyleRuleOrigin::new("theme", case.theme_priority),
            case.theme_css,
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_single_rect_target_width(&root, case.expected_width_px, case.name);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
// ON-SPEC (relabeled from
// `off_spec_direct_attr_write_loses_to_stylesheet_on_next_recompute`):
// css-style-attr §4 — declarations in a style attribute "are considered to
// have author origin and a specificity higher than any selector", and "must be
// interpreted exactly as if they were given in a CSS style rule that applies to
// the element". PLAN.md proposal A acceptance test: the inline declaration is
// element-owned and beats the stylesheet at all times, surviving class
// rewrites, any stylesheet load order, and stylesheet unload (unload re-folds,
// and the element-owned inline block is the only remaining width source).
// <https://drafts.csswg.org/css-style-attr/>
// Gecko: servo/components/style/stylist.rs; Blink: core/css/resolver/match_result.cc.
// Closest WPT: css/css-cascade/important-and-inline-*.html family.
fn inline_style_beats_stylesheet_and_survives_unload() {
    const THEME_CSS: &str = ".s { width: 20px; height: 40px }";

    enum InlineWriteScenario {
        AfterStyle,
        ThenClassRewrite,
        BeforeLoad,
        ThenUnload,
    }

    let scenarios = [
        ("after_style", InlineWriteScenario::AfterStyle),
        ("class_rewrite", InlineWriteScenario::ThenClassRewrite),
        ("before_load", InlineWriteScenario::BeforeLoad),
        ("then_unload", InlineWriteScenario::ThenUnload),
    ];

    for (name, scenario) in scenarios {
        let (mut world, mut root, target) = target_div_setup(None, "s");
        match scenario {
            InlineWriteScenario::BeforeLoad => {
                assert_style_set(&mut root, target, "width", "10px");
                assert_load_root_stylesheet(&root, StyleRuleOrigin::new("theme", 1), THEME_CSS);
                assert_set_attr_str(&mut root, target, "class", "s");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
            }
            InlineWriteScenario::AfterStyle
            | InlineWriteScenario::ThenClassRewrite
            | InlineWriteScenario::ThenUnload => {
                assert_load_root_stylesheet(&root, StyleRuleOrigin::new("theme", 1), THEME_CSS);
                assert_set_attr_str(&mut root, target, "class", "s");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
                assert_style_set(&mut root, target, "width", "10px");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
                match scenario {
                    InlineWriteScenario::ThenClassRewrite => {
                        assert_set_attr_str(&mut root, target, "class", "s b");
                        assert_root_dom_update(
                            &mut world,
                            &mut root,
                            0.0,
                            DomPointerUpdate::Unchanged,
                        );
                    }
                    InlineWriteScenario::ThenUnload => {
                        let _unloaded = root.context().unload_stylesheet("theme");
                        assert_root_dom_update(
                            &mut world,
                            &mut root,
                            0.0,
                            DomPointerUpdate::Unchanged,
                        );
                    }
                    InlineWriteScenario::AfterStyle | InlineWriteScenario::BeforeLoad => {}
                }
            }
        }

        // The inline width wins in every scenario — after the class rewrite,
        // regardless of stylesheet load order, and after stylesheet unload
        // (the element-owned inline declaration is restored by re-folding).
        assert_eq!(
            root.computed_style(target)
                .expect("target should remain readable")
                .as_box()
                .width,
            Some(10.0),
            "{name} inline width should win",
        );
        assert_single_rect_target_width(&root, 10.0, name);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
// CSS Cascade 5 §3 (inheritance) and §6 (cascade). The transmitting inherited
// set is exactly color/font-size/line-height/text-align/direction/white-space/
// overflow-wrap/word-break; each is carried in the descendant's computed groups
// (spine 3.3g-2a: the font/text/color groups hold the inherited value), while
// non-inherited box/visual/flex longhands stay at their initial values on the
// descendant.
fn inherited_properties_transmit_and_non_inherited_do_not() {
    enum InheritedAssertion {
        SpanColorEquals { hex: &'static str },
        SpanFontSizeEquals { px: f32, text_font_size_px: f32 },
        SpanTextAlignEquals { align: DomTextAlign },
        SpanDirectionEquals { direction: DomTextDirection },
        LineHeightInherited { text_line_height_px: f32 },
        TextSingleLine,
        TextWraps,
        NonInheritedLonghandsAbsent,
        TextFontSizeEquals { px: f32 },
    }

    struct InheritedCase {
        name: &'static str,
        css: &'static str,
        scenario: InheritedScenario,
        assertion: InheritedAssertion,
        text: &'static str,
        width_constraint: WidthConstraint,
    }

    let cases = [
        InheritedCase {
            name: "color_transmits_to_span",
            css: ".p { color: #abcdef; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanColorEquals { hex: "#abcdef" },
            text: "color carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "font_size_transmits_to_span_and_text",
            css: ".p { font-size: 20px; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanFontSizeEquals {
                px: 20.0,
                text_font_size_px: 20.0,
            },
            text: "font size carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "text_align_transmits_to_span",
            css: ".p { text-align: center; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanTextAlignEquals {
                align: DomTextAlign::Center,
            },
            text: "text align carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "direction_transmits_to_span",
            css: ".p { direction: rtl; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanDirectionEquals {
                direction: DomTextDirection::Rtl,
            },
            text: "direction carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "line_height_context_carried_to_text",
            css: ".p { line-height: 28px; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::LineHeightInherited {
                text_line_height_px: 28.0,
            },
            text: "line height carry",
            width_constraint: WidthConstraint::Authored,
        },
        // PREDICT: white-space is context-carried; the descendant stays on one
        // overflowing line under the constrained container width.
        InheritedCase {
            name: "white_space_nowrap_single_line",
            css: ".p { white-space: nowrap; width: 60px; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::TextSingleLine,
            text: "alpha beta gamma",
            width_constraint: WidthConstraint::Authored,
        },
        // PREDICT: overflow-wrap:anywhere rewraps a long unbreakable word.
        InheritedCase {
            name: "overflow_wrap_anywhere_wraps",
            css: ".p { overflow-wrap: anywhere; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::TextWraps,
            text: "AlphaBeta",
            width_constraint: WidthConstraint::MinContentMinusOne,
        },
        // PREDICT: word-break:keep-all suppresses Japanese wrapping.
        InheritedCase {
            name: "word_break_keep_all_single_line",
            css: ".p { word-break: keep-all; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::TextSingleLine,
            text: "窓ぎわのトットちゃん",
            width_constraint: WidthConstraint::MaxContentMinusOne,
        },
        InheritedCase {
            name: "non_inherited_box_attrs_absent_on_span",
            css: ".p { width: 100px; margin-left: 7px; padding-left: 6px; background-color: #112233; border-top: 2px; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::NonInheritedLonghandsAbsent,
            text: "non inherited",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "em_chain_resolves_against_inherited_base",
            css: ".p { font-size: 10px; } .mid { font-size: 1.5em; }",
            scenario: InheritedScenario::MiddleDiv,
            assertion: InheritedAssertion::TextFontSizeEquals { px: 15.0 },
            text: "em chain",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "layer_boundary_seeds_font_size_from_ancestor",
            css: ".p { font-size: 22px; }",
            scenario: InheritedScenario::FixedLayer,
            assertion: InheritedAssertion::TextFontSizeEquals { px: 22.0 },
            text: "layer seed",
            width_constraint: WidthConstraint::Authored,
        },
    ];

    for case in cases {
        let (mut world, mut root, container, mid, text) =
            build_inherited_scenario(case.scenario, case.css, case.text);
        match case.width_constraint {
            WidthConstraint::Authored => {}
            WidthConstraint::MinContentMinusOne => {
                let baseline = text_intrinsic(&root, text);
                let constrained = (baseline.min_content_width_px - 1.0).max(1.0);
                assert_style_set(&mut root, container, "width", &format!("{constrained}px"));
            }
            WidthConstraint::MaxContentMinusOne => {
                let baseline = text_intrinsic(&root, text);
                let constrained = (baseline.max_content_width_px - 1.0).max(1.0);
                assert_style_set(&mut root, container, "width", &format!("{constrained}px"));
            }
        }
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        match case.assertion {
            InheritedAssertion::SpanColorEquals { hex } => {
                assert_eq!(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_color()
                        .color_rgba,
                    parse_css_color_rgba(hex).expect("proof color should parse"),
                    "{} span computed color",
                    case.name
                );
            }
            InheritedAssertion::SpanFontSizeEquals {
                px,
                text_font_size_px,
            } => {
                assert_close(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_font()
                        .size_px,
                    px,
                    &format!("{} span computed font-size", case.name),
                );
                assert_close(
                    text_intrinsic(&root, text).font_size_px,
                    text_font_size_px,
                    &format!("{} text intrinsic font size", case.name),
                );
            }
            InheritedAssertion::SpanTextAlignEquals { align } => {
                assert_eq!(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_text()
                        .align,
                    align,
                    "{} span computed text-align",
                    case.name
                );
            }
            InheritedAssertion::SpanDirectionEquals { direction } => {
                assert_eq!(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_text()
                        .direction,
                    direction,
                    "{} span computed direction",
                    case.name
                );
            }
            InheritedAssertion::LineHeightInherited {
                text_line_height_px,
            } => {
                assert_close(
                    text_intrinsic(&root, text).line_height_px,
                    text_line_height_px,
                    &format!("{} text intrinsic line-height", case.name),
                );
                assert_eq!(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_font()
                        .line_height_px,
                    Some(text_line_height_px),
                    "{} span computed line-height",
                    case.name
                );
            }
            InheritedAssertion::TextSingleLine => {
                let text_layout = assert_node_layout(&root, text);
                let intrinsic = text_intrinsic(&root, text);
                assert_close(
                    text_layout.height_px,
                    intrinsic.line_height_px,
                    &format!("{} text should stay single-line", case.name),
                );
            }
            InheritedAssertion::TextWraps => {
                let text_layout = assert_node_layout(&root, text);
                let intrinsic = text_intrinsic(&root, text);
                assert!(
                    text_layout.height_px > intrinsic.line_height_px,
                    "{} text should wrap to multiple lines",
                    case.name
                );
            }
            InheritedAssertion::NonInheritedLonghandsAbsent => {
                let box_ = root
                    .computed_style(mid)
                    .expect("span should remain readable")
                    .as_box();
                assert_eq!(
                    box_.width, None,
                    "{} span should not inherit width",
                    case.name
                );
                assert_eq!(
                    box_.margin[3], 0.0,
                    "{} span should not inherit margin-left",
                    case.name
                );
                assert_eq!(
                    box_.padding[3], 0.0,
                    "{} span should not inherit padding-left",
                    case.name
                );
                assert_eq!(
                    box_.border_width[0], 0.0,
                    "{} span should not inherit border-top width",
                    case.name
                );
                assert_eq!(
                    root.computed_style(mid)
                        .expect("span should remain readable")
                        .as_visual()
                        .background_rgba,
                    [0.0; 4],
                    "{} span should not inherit background-color",
                    case.name
                );
            }
            InheritedAssertion::TextFontSizeEquals { px } => {
                assert_close(
                    text_intrinsic(&root, text).font_size_px,
                    px,
                    &format!("{} text intrinsic font size", case.name),
                );
            }
        }
    }
}

/// The DOM shape one inheritance row authors under the class-"p" container.
#[derive(Clone, Copy)]
enum InheritedScenario {
    SpanText,
    MiddleDiv,
    FixedLayer,
}

/// How a wrap row constrains the container width to exercise its wrap policy.
#[derive(Clone, Copy)]
enum WidthConstraint {
    /// The width is authored in the stylesheet text (or left auto).
    Authored,
    /// Narrow below the text's min-content width so a break is forced.
    MinContentMinusOne,
    /// Narrow below the text's max-content width so keep-all can suppress it.
    MaxContentMinusOne,
}

/// Builds one inheritance scenario: a `container` div (class "p") styled by
/// `css`, holding either a span, a middle div (class "mid"), or a fixed layer,
/// and a text node under it. Returns `(world, root, container, mid, text)`.
fn build_inherited_scenario(
    scenario: InheritedScenario,
    css: &str,
    text: &str,
) -> (
    World,
    RootDom,
    DomNodeSlotKey,
    DomNodeSlotKey,
    DomNodeSlotKey,
) {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), css);
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, container, "class", "p");
    let (mid, text_key) = match scenario {
        InheritedScenario::SpanText => {
            let span = assert_append_span_to(&mut root, container);
            assert_style_set(&mut root, span, "display", "block");
            let text_key = assert_append_text_to(&mut root, span, text);
            (span, text_key)
        }
        InheritedScenario::MiddleDiv => {
            let middle = assert_append_div_to(&mut root, container);
            assert_set_attr_str(&mut root, middle, "class", "mid");
            let text_key = assert_append_text_to(&mut root, middle, text);
            (middle, text_key)
        }
        InheritedScenario::FixedLayer => {
            let layer = assert_append_div_to(&mut root, container);
            assert_style_set(&mut root, layer, "position", "fixed");
            let text_key = assert_append_text_to(&mut root, layer, text);
            (layer, text_key)
        }
    };
    (world, root, container, mid, text_key)
}

#[test]
// Selectors 3 §17 (pseudo-classes count as class) + CSS Cascade 5 §6: a
// `:hover` rule applies only in the hovered state and follows the same
// specificity/source-order precedence as any other class-qualified rule.
fn pseudo_state_rules_apply_on_state_and_follow_specificity() {
    struct HoverCase {
        name: &'static str,
        css: &'static str,
        id: Option<&'static str>,
        class: &'static str,
        expected_before_width_px: f32,
        expected_hovered_width_px: f32,
        expected_after_width_px: f32,
    }

    let cases = [
        HoverCase {
            name: "hover_rule_applies_then_clears",
            css: ".b{width:10px;height:40px} .b:hover{width:20px}",
            id: None,
            class: "b",
            expected_before_width_px: 10.0,
            expected_hovered_width_px: 20.0,
            expected_after_width_px: 10.0,
        },
        HoverCase {
            name: "id_beats_hover_pseudo_class",
            css: "#i{width:10px;height:40px} .b:hover{width:20px}",
            id: Some("i"),
            class: "b",
            expected_before_width_px: 10.0,
            expected_hovered_width_px: 10.0,
            expected_after_width_px: 10.0,
        },
        HoverCase {
            name: "equal_hover_specificity_later_rule_wins",
            css: "div{width:10px;height:40px} .b:hover{width:20px} .c:hover{width:30px}",
            id: None,
            class: "b c",
            expected_before_width_px: 10.0,
            expected_hovered_width_px: 30.0,
            expected_after_width_px: 10.0,
        },
    ];

    for case in cases {
        let (mut world, mut root, target) = target_div_setup(case.id, case.class);
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), case.css);

        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_hover_phase_width(
            &root,
            target,
            case.expected_before_width_px,
            None,
            &format!("{} before hover", case.name),
        );

        assert_root_dom_update(
            &mut world,
            &mut root,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 5.0,
                y_px: 20.0,
                primary_button_down: false,
                scroll_delta_y_px: 0.0,
            }),
        );
        assert_hover_phase_width(
            &root,
            target,
            case.expected_hovered_width_px,
            Some(case.expected_before_width_px),
            &format!("{} hovered", case.name),
        );

        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Clear);
        assert_hover_phase_width(
            &root,
            target,
            case.expected_after_width_px,
            Some(case.expected_hovered_width_px),
            &format!("{} after clear", case.name),
        );
    }
}

/// Asserts one hover phase: the target's computed width equals `expected_px`,
/// and — when that width changed from the previous phase — one repaint rect
/// exists at that width; when it is unchanged, no repaint is staged.
fn assert_hover_phase_width(
    root: &RootDom,
    target: DomNodeSlotKey,
    expected_width_px: f32,
    previous_width_px: Option<f32>,
    label: &str,
) {
    assert_eq!(
        root.computed_style(target)
            .expect("hover target should remain readable")
            .as_box()
            .width,
        Some(expected_width_px),
        "{label} computed width",
    );
    match previous_width_px {
        Some(previous) if (expected_width_px - previous).abs() <= f32::EPSILON => {
            let plans = take_staged_layer_paint_plans(root);
            assert_eq!(
                plans.len(),
                0,
                "{label} should not repaint when the cascade winner is unchanged"
            );
        }
        _ => assert_single_rect_target_width(root, expected_width_px, label),
    }
}

#[test]
// CSS Cascade 5 §3 (shorthand expansion) + §6 (cascade order): a shorthand
// resets its longhands at declaration time, and shorthand-vs-longhand
// precedence resolves by cascade order.
// <https://drafts.csswg.org/css-cascade-5/#shorthand>
// Gecko: servo/components/style/properties/declaration_block.rs (shorthands
// expand to longhand drains at parse); Blink:
// core/css/css_property_value_set.h (shorthand expansion at set time).
// Closest WPT family: css/css-flexbox/ (flex shorthand expansion and
// longhand override).
//
// Relabeled from `off_spec_shorthand_and_longhand_coexist_without_cascade_reset`
// in spine step 2: the typed declaration store expands shorthands at parse
// time (A1), so the shorthand attribute no longer coexists verbatim — the
// longhand is the only winning value at the cascade level.
fn shorthand_and_longhand_cascade_resets_longhands() {
    enum ShorthandCase {
        Flex {
            css: &'static str,
            class: &'static str,
        },
        PlaceContent {
            css: &'static str,
            class: &'static str,
        },
    }

    let cases = [
        ShorthandCase::Flex {
            css: ".f { flex: 1 0 auto; flex-grow: 3 }",
            class: "f",
        },
        ShorthandCase::PlaceContent {
            css: ".p { place-content: center start; align-content: space-around }",
            class: "p",
        },
    ];

    for case in cases {
        let (name, css, class) = match case {
            ShorthandCase::Flex { css, class } => {
                ("flex_shorthand_expands_and_grow_longhand_wins", css, class)
            }
            ShorthandCase::PlaceContent { css, class } => (
                "place_content_expands_and_align_content_longhand_wins",
                css,
                class,
            ),
        };
        let (mut world, mut root, target) = target_div_setup(None, class);
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), css);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        let flex = root.computed_style(target).expect("target").as_flex();
        match case {
            ShorthandCase::Flex { .. } => {
                // The flex shorthand expands to grow/shrink/basis, then the
                // later flex-grow longhand wins; the shorthand itself has no
                // computed-group field (it expanded at parse time).
                assert_eq!(flex.grow, 3.0, "{name} winning flex-grow longhand");
                assert_eq!(flex.shrink, 0.0, "{name} shorthand shrink expansion");
                assert_eq!(
                    flex.basis,
                    FlexBasisKw::Auto,
                    "{name} shorthand basis expansion"
                );
            }
            ShorthandCase::PlaceContent { .. } => {
                // place-content expands to align-content + justify-content;
                // the later align-content longhand wins.
                assert_eq!(
                    flex.align_content,
                    AlignContentKw::SpaceAround,
                    "{name} winning align-content longhand"
                );
                assert_eq!(
                    flex.justify_content,
                    JustifyContentKw::Start,
                    "{name} place-content justify expansion"
                );
            }
        }
    }
}

#[test]
// CSS 2.1 §4.1.7 + css-style-attr §3 (drop-and-continue, reported, never
// silent): a malformed declaration in a stylesheet block is dropped and
// parsing continues with the next declaration; the drop is counted in the
// load report and the context's monotonic drop counter.
// <https://www.w3.org/TR/CSS21/syndata.html#parsing-errors>
// <https://drafts.csswg.org/css-style-attr/#syntax>
// Gecko: servo/components/style/stylesheets/rule_parser.rs (error-recovering
// declaration parse); Blink: core/css/parser/css_parser_impl.cc (consume
// declaration, drop on error).
// Closest WPT families: css/css2/syntax/ (CSS 2.1 §4.1.7 error recovery).
//
// This relabels the stylesheet half of the former off-spec proof
// `off_spec_malformed_declaration_rejects_entire_declaration_block`: the
// typed declaration store (spine step 2) parses declaration blocks
// drop-and-continue with a counted report, never silent.
fn malformed_stylesheet_declarations_drop_and_continue_with_report() {
    struct MalformedCase {
        name: &'static str,
        declarations: &'static str,
    }

    let cases = [
        MalformedCase {
            name: "declaration_missing_separator",
            declarations: "width: 20px; height 40px",
        },
        MalformedCase {
            name: "declaration_missing_name",
            declarations: "width: 20px; : 40px",
        },
        MalformedCase {
            name: "declaration_missing_value",
            declarations: "width: 20px; height:",
        },
        MalformedCase {
            name: "value_failing_grammar",
            declarations: "width: 20px; height: lots",
        },
        MalformedCase {
            name: "unadmitted_property_name",
            declarations: "width: 20px; frobnicate: 1",
        },
    ];

    for case in cases {
        let (mut world, mut root, target) = target_div_setup(None, "s");
        let report = root
            .context()
            .load_stylesheet(
                theme_stylesheet_origin(),
                &format!(".s {{ {} }}", case.declarations),
            )
            .expect("malformed declaration should drop, not reject the load");
        assert_eq!(
            report.declarations_dropped, 1,
            "{}: exactly one malformed declaration is dropped",
            case.name
        );
        assert_eq!(
            report.declarations_applied, 1,
            "{}: the valid width declaration applies",
            case.name
        );
        assert_eq!(
            root.context().declaration_drop_count(),
            1,
            "{}: the drop counter records the dropped declaration",
            case.name
        );
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
        assert_eq!(
            root.computed_style(target)
                .expect("target should remain readable")
                .as_box()
                .width,
            Some(20.0),
            "{}: valid width declaration must apply after drop-and-continue",
            case.name
        );
    }
}

#[test]
// ON-SPEC (relabeled from
// `off_spec_malformed_declaration_rejects_entire_style_attribute_write`):
// css-style-attr §3 — the style attribute value is a declaration list parsed
// with forward-compatible error handling: a malformed declaration is dropped
// and parsing continues, and the drop is observable (counted), never silent.
// The `style` attribute routes into the inline declaration store
// (spine 3.3g-2a), which counts each drop on
// `RootDomMetricName::StyleDeclarationDropped`.
// <https://drafts.csswg.org/css-style-attr/#syntax>
// Gecko: servo/components/style/stylesheets/rule_parser.rs; Blink:
// core/css/parser/css_parser_impl.cc.
// Closest WPT family: css/css-style-attr/ (style attribute parsing).
fn style_attribute_drops_malformed_declarations_and_counts_them() {
    struct DropCase {
        name: &'static str,
        declarations: &'static str,
    }

    let cases = [
        DropCase {
            name: "declaration_missing_separator",
            declarations: "width: 20px; height 40px",
        },
        DropCase {
            name: "declaration_missing_name",
            declarations: "width: 20px; : 40px",
        },
        DropCase {
            name: "declaration_missing_value",
            declarations: "width: 20px; height:",
        },
        DropCase {
            name: "value_failing_grammar",
            declarations: "width: 20px; height: lots",
        },
        DropCase {
            name: "unadmitted_property_name",
            declarations: "width: 20px; frobnicate: 1",
        },
    ];

    for case in cases {
        let (mut world, mut root, target) = target_div_setup(None, "");
        root.set_attr_str(target, "style", case.declarations)
            .expect("style attribute should drop-and-continue, not reject");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        assert_eq!(
            root.computed_style(target)
                .expect("target should remain readable")
                .as_box()
                .width,
            Some(20.0),
            "{}: valid width declaration must apply after drop-and-continue",
            case.name
        );
        assert_eq!(
            root.count(RootDomMetricName::StyleDeclarationDropped),
            1,
            "{}: exactly one malformed declaration is dropped and counted",
            case.name
        );
    }
}

#[test]
// ON-SPEC (relabeled from `off_spec_font_family_weight_and_style_do_not_inherit`):
// font-family, font-weight, and font-style are inherited properties — CSS Fonts
// 4 §3.1 (font-family), §3.2 (font-weight), and §3.3 (font-style) each state
// "Inherited: yes". PLAN.md A5 group-copy inheritance: the descendant's
// computed font group copies the parent's resolved family/weight/style.
// <https://drafts.csswg.org/css-fonts-4/#font-family-prop>
// Gecko: servo/components/style/values/computed/font.rs (the computed font
// struct propagates through inheritance); Blink:
// core/css/resolver/style_resolver.cc (parent-style propagation).
// Closest WPT: css/css-fonts/inheritance.html.
fn font_family_weight_and_style_inherit() {
    enum FontAssertion {
        Family,
        Weight,
        Style,
    }

    let cases = [
        (
            "font_family",
            ".p { font-family: ui-sans, sans-serif; }",
            FontAssertion::Family,
        ),
        (
            "font_weight",
            ".p { font-weight: 600; }",
            FontAssertion::Weight,
        ),
        (
            "font_style",
            ".p { font-style: italic; }",
            FontAssertion::Style,
        ),
    ];

    for (name, css, assertion) in cases {
        let (mut world, mut root, _container, span, _text) =
            build_inherited_scenario(InheritedScenario::SpanText, css, "font carry");
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        let view = root
            .computed_style(span)
            .expect("span should remain readable");
        match assertion {
            FontAssertion::Family => {
                assert_eq!(
                    view.font_family(),
                    Some("ui-sans, sans-serif"),
                    "{name} span should inherit font-family"
                );
            }
            FontAssertion::Weight => {
                assert_eq!(
                    view.as_font().weight,
                    600,
                    "{name} span should inherit font-weight"
                );
            }
            FontAssertion::Style => {
                assert_eq!(
                    view.as_font().style,
                    Keyword::Italic,
                    "{name} span should inherit font-style"
                );
            }
        }
    }
}
