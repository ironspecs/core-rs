#![allow(rustdoc::bare_urls)]

//! CSS cascade and inheritance layout proofs.
//!
//! This module owns the integration proofs for the `ui-dom` cascade model:
//! selector specificity, source-order and origin precedence, inheritance
//! transmission, pseudo-state rule application, and the two registered
//! off-spec divergences (direct-attr writes and shorthand/longhand
//! coexistence). Every family is table-driven and asserts through the public
//! `RootDom` surface: staged paint plans are the primary measure for geometry
//! families, and `presented_attr`/retained intrinsics pin the non-paintable
//! inheritance and off-spec outcomes.
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
//!   `line-height`, `text-align`, `direction`, `white-space`, `overflow-wrap`,
//!   and `word-break`; only `color`, `font-size`, `text-align`, and
//!   `direction` are materialized into presented attributes, while the other
//!   four stay context-carried into text intrinsics (architecture pin)
//! - off-spec rows are labeled `off_spec_*` and pin current values; a failure
//!   means the divergence moved (see the registry)

use bevy::prelude::World;
use ui_dom::{
    DomAttribute, DomAttributeValue, DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerState,
    DomPointerUpdate, RootDom, StyleRuleOrigin, TextIntrinsic, parse_css_color_rgba,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_span_to, assert_append_text_to, assert_close,
    assert_load_root_stylesheet, assert_node_layout, assert_rect_op_fill, assert_rect_op_geometry,
    assert_root_dom_update, assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    take_staged_layer_paint_plans, theme_stylesheet_origin,
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

/// Clones one node's presented attribute value, or `None` when absent.
fn presented(
    root: &RootDom,
    key: DomNodeSlotKey,
    attr: &DomAttribute,
) -> Option<DomAttributeValue> {
    root.node(key)
        .and_then(|node| node.presented_attr(attr))
        .cloned()
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
    assert_set_attr_str(&mut root, target, "height", "40px");
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
// OFF-SPEC (divergent): CSS Cascade §6.4.3 — declarations from the style
// attribute win over normal author declarations, deterministically.
// <https://drafts.csswg.org/css-cascade-5/#cascade-sort>
// Gecko: servo/components/style/stylist.rs; Blink: core/css/resolver/match_result.cc.
// Closest WPT: css/css-cascade/important-and-inline-*.html family.
//
// CURRENT (asserted below): the winner between a direct attr write and a
// stylesheet rule is timing-dependent. A direct `width` write stands only
// until the next style recompute dirtied by anything else; the stylesheet
// then re-applies its value and claims the attribute in the style-pass
// ownership set, so unloading the stylesheet removes the attribute entirely
// instead of restoring the direct value.
// SPEC-CORRECT: a direct (inline-style) write beats the stylesheet rule at
// all times, and stylesheet unload restores it.
// ROOT CAUSE: `RootDom::apply_styles` writes the matched map onto retained
// attrs without distinguishing direct-authored values, and
// `style_pass_attrs_by_node` takes ownership of anything the pass has
// applied.
// RECLASSIFY: if this proof fails, the divergence moved — see the off-spec
// registry in tests/layout_proofs/README.md.
fn off_spec_direct_attr_write_loses_to_stylesheet_on_next_recompute() {
    const THEME_CSS: &str = ".s { width: 20px; height: 40px }";

    enum DirectWriteScenario {
        AfterStyle,
        ThenClassRewrite,
        BeforeLoad,
        ThenUnload,
    }

    struct DirectWriteCase {
        name: &'static str,
        scenario: DirectWriteScenario,
        expected_presented_width: Option<&'static str>,
        expected_rect_width_px: Option<f32>,
    }

    let cases = [
        DirectWriteCase {
            name: "direct_write_stands_until_style_recompute",
            scenario: DirectWriteScenario::AfterStyle,
            expected_presented_width: Some("10px"),
            expected_rect_width_px: Some(10.0),
        },
        DirectWriteCase {
            name: "class_rewrite_reapplies_stylesheet_width",
            scenario: DirectWriteScenario::ThenClassRewrite,
            expected_presented_width: Some("20px"),
            expected_rect_width_px: Some(20.0),
        },
        DirectWriteCase {
            name: "stylesheet_wins_when_loaded_after_direct_write",
            scenario: DirectWriteScenario::BeforeLoad,
            expected_presented_width: Some("20px"),
            expected_rect_width_px: Some(20.0),
        },
        DirectWriteCase {
            name: "stylesheet_unload_removes_attribute_instead_of_restoring",
            scenario: DirectWriteScenario::ThenUnload,
            expected_presented_width: None,
            expected_rect_width_px: None,
        },
    ];

    for case in cases {
        let (mut world, mut root, target) = target_div_setup(None, "s");
        match case.scenario {
            DirectWriteScenario::BeforeLoad => {
                assert_set_attr_str(&mut root, target, "width", "10px");
                assert_load_root_stylesheet(&root, StyleRuleOrigin::new("theme", 1), THEME_CSS);
                assert_set_attr_str(&mut root, target, "class", "s");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
            }
            DirectWriteScenario::AfterStyle
            | DirectWriteScenario::ThenClassRewrite
            | DirectWriteScenario::ThenUnload => {
                assert_load_root_stylesheet(&root, StyleRuleOrigin::new("theme", 1), THEME_CSS);
                assert_set_attr_str(&mut root, target, "class", "s");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
                assert_set_attr_str(&mut root, target, "width", "10px");
                assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
                match case.scenario {
                    DirectWriteScenario::ThenClassRewrite => {
                        assert_set_attr_str(&mut root, target, "class", "s b");
                        assert_root_dom_update(
                            &mut world,
                            &mut root,
                            0.0,
                            DomPointerUpdate::Unchanged,
                        );
                    }
                    DirectWriteScenario::ThenUnload => {
                        let _unloaded = root.context().unload_stylesheet("theme");
                        assert_root_dom_update(
                            &mut world,
                            &mut root,
                            0.0,
                            DomPointerUpdate::Unchanged,
                        );
                    }
                    DirectWriteScenario::AfterStyle | DirectWriteScenario::BeforeLoad => {}
                }
            }
        }

        assert_eq!(
            presented(&root, target, &DomAttribute::Width),
            case.expected_presented_width
                .map(|value| DomAttributeValue::String(value.to_string())),
            "{} presented width",
            case.name
        );
        if let Some(width_px) = case.expected_rect_width_px {
            assert_single_rect_target_width(&root, width_px, case.name);
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
// CSS Cascade 5 §3 (inheritance) and §6 (cascade). The transmitting set is
// exactly color/font-size/line-height/text-align/direction/white-space/
// overflow-wrap/word-break. Only color, font-size, text-align, and direction
// materialize into presented attributes (the materialized channel); the other
// four are context-carried into text intrinsics and never appear as presented
// attributes on the descendant (architecture pin).
fn inherited_properties_transmit_and_non_inherited_do_not() {
    enum InheritedAssertion {
        SpanColorEquals {
            hex: &'static str,
        },
        SpanFontSizeEquals {
            value: &'static str,
            text_font_size_px: f32,
        },
        SpanTextAlignEquals {
            value: &'static str,
        },
        SpanDirectionEquals {
            value: &'static str,
        },
        LineHeightContextCarried {
            text_line_height_px: f32,
        },
        TextSingleLine,
        TextWraps,
        NonInheritedAttrsAbsent,
        TextFontSizeEquals {
            px: f32,
        },
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
                value: "20px",
                text_font_size_px: 20.0,
            },
            text: "font size carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "text_align_transmits_to_span",
            css: ".p { text-align: center; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanTextAlignEquals { value: "center" },
            text: "text align carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "direction_transmits_to_span",
            css: ".p { direction: rtl; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::SpanDirectionEquals { value: "rtl" },
            text: "direction carry",
            width_constraint: WidthConstraint::Authored,
        },
        InheritedCase {
            name: "line_height_context_carried_to_text",
            css: ".p { line-height: 28px; }",
            scenario: InheritedScenario::SpanText,
            assertion: InheritedAssertion::LineHeightContextCarried {
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
            assertion: InheritedAssertion::NonInheritedAttrsAbsent,
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
                assert_set_attr_str(&mut root, container, "width", &format!("{constrained}px"));
            }
            WidthConstraint::MaxContentMinusOne => {
                let baseline = text_intrinsic(&root, text);
                let constrained = (baseline.max_content_width_px - 1.0).max(1.0);
                assert_set_attr_str(&mut root, container, "width", &format!("{constrained}px"));
            }
        }
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        match case.assertion {
            InheritedAssertion::SpanColorEquals { hex } => {
                assert_eq!(
                    presented(&root, mid, &DomAttribute::Custom("color".to_string())),
                    Some(DomAttributeValue::Color(
                        parse_css_color_rgba(hex).expect("proof color should parse")
                    )),
                    "{} span presented color",
                    case.name
                );
            }
            InheritedAssertion::SpanFontSizeEquals {
                value,
                text_font_size_px,
            } => {
                assert_eq!(
                    presented(&root, mid, &DomAttribute::FontSize),
                    Some(DomAttributeValue::String(value.to_string())),
                    "{} span presented font-size",
                    case.name
                );
                assert_close(
                    text_intrinsic(&root, text).font_size_px,
                    text_font_size_px,
                    &format!("{} text intrinsic font size", case.name),
                );
            }
            InheritedAssertion::SpanTextAlignEquals { value } => {
                assert_eq!(
                    presented(&root, mid, &DomAttribute::TextAlign),
                    Some(DomAttributeValue::String(value.to_string())),
                    "{} span presented text-align",
                    case.name
                );
            }
            InheritedAssertion::SpanDirectionEquals { value } => {
                assert_eq!(
                    presented(&root, mid, &DomAttribute::Direction),
                    Some(DomAttributeValue::String(value.to_string())),
                    "{} span presented direction",
                    case.name
                );
            }
            InheritedAssertion::LineHeightContextCarried {
                text_line_height_px,
            } => {
                assert_close(
                    text_intrinsic(&root, text).line_height_px,
                    text_line_height_px,
                    &format!("{} text intrinsic line-height", case.name),
                );
                assert_eq!(
                    presented(&root, mid, &DomAttribute::LineHeight),
                    None,
                    "{} span should not materialize line-height",
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
            InheritedAssertion::NonInheritedAttrsAbsent => {
                for attr in [
                    DomAttribute::Width,
                    DomAttribute::MarginLeft,
                    DomAttribute::PaddingLeft,
                    DomAttribute::Custom("background-color".to_string()),
                    DomAttribute::BorderTop,
                ] {
                    assert_eq!(
                        presented(&root, mid, &attr),
                        None,
                        "{} span should not present {attr:?}",
                        case.name
                    );
                }
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
            assert_set_attr_str(&mut root, span, "display", "block");
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
            assert_set_attr_str(&mut root, layer, "position", "fixed");
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

/// Asserts one hover phase: the target's presented width equals `expected_px`,
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
        presented(root, target, &DomAttribute::Width),
        Some(DomAttributeValue::String(format!("{expected_width_px}px"))),
        "{label} presented width",
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
// OFF-SPEC (divergent): CSS shorthand declarations reset their longhands at
// declaration time, and shorthand-vs-longhand precedence resolves by cascade
// order (CSS Cascade §6 + the shorthand reset rules of each property spec).
//
// CURRENT (asserted below): shorthand and longhand are independent attributes
// that never interact in the cascade — both land in presented attrs and
// precedence between them is defined per-consumer, not by the cascade.
// SPEC-CORRECT: `flex: 1 0 auto; flex-grow: 3` yields exactly one winning
// grow factor (3) at the cascade level.
// ROOT CAUSE: the attribute model has no shorthand expansion;
// `parse_style_attr_map` in src/types/dom_attr_parse.rs stores declarations
// verbatim.
// RECLASSIFY: if this proof fails, the divergence moved — see the off-spec
// registry in tests/layout_proofs/README.md.
fn off_spec_shorthand_and_longhand_coexist_without_cascade_reset() {
    struct ShorthandCase {
        name: &'static str,
        css: &'static str,
        class: &'static str,
        shorthand_attr: DomAttribute,
        shorthand_value: &'static str,
        longhand_attr: DomAttribute,
        longhand_value: &'static str,
    }

    let cases = [
        ShorthandCase {
            name: "flex_shorthand_and_grow_longhand_coexist",
            css: ".f { flex: 1 0 auto; flex-grow: 3 }",
            class: "f",
            shorthand_attr: DomAttribute::Flex,
            shorthand_value: "1 0 auto",
            longhand_attr: DomAttribute::FlexGrow,
            longhand_value: "3",
        },
        ShorthandCase {
            name: "place_content_shorthand_and_align_content_longhand_coexist",
            css: ".p { place-content: center start; align-content: space-around }",
            class: "p",
            shorthand_attr: DomAttribute::PlaceContent,
            shorthand_value: "center start",
            longhand_attr: DomAttribute::AlignContent,
            longhand_value: "space-around",
        },
    ];

    for case in cases {
        let (mut world, mut root, target) = target_div_setup(None, case.class);
        assert_load_root_stylesheet(&root, theme_stylesheet_origin(), case.css);
        assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

        assert_eq!(
            presented(&root, target, &case.shorthand_attr),
            Some(DomAttributeValue::String(case.shorthand_value.to_string())),
            "{} presented shorthand",
            case.name
        );
        assert_eq!(
            presented(&root, target, &case.longhand_attr),
            Some(DomAttributeValue::String(case.longhand_value.to_string())),
            "{} presented longhand",
            case.name
        );
    }
}
