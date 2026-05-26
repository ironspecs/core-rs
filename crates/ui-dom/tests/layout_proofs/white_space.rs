#![allow(rustdoc::bare_urls)]

//! CSS `white-space` layout proofs.
//!
//! Spec references:
//! - CSS Text Module Level 4 `white-space` shorthand:
//!   <https://www.w3.org/TR/css-text-4/#white-space-property>
//! - CSS Text Module Level 4 white-space collapsing:
//!   <https://www.w3.org/TR/css-text-4/#white-space-collapsing>
//! - CSS Text Module Level 4 text wrap mode:
//!   <https://www.w3.org/TR/css-text-4/#propdef-text-wrap-mode>
//!
//! Browser implementation references:
//! - Blink white-space decomposition helpers:
//!   <https://chromium.googlesource.com/chromium/src/+/HEAD/third_party/blink/renderer/core/css/white_space.h>
//! - Blink white-space behavior tests:
//!   <https://chromium.googlesource.com/chromium/src/+/HEAD/third_party/blink/renderer/core/css/white_space_test.cc>
//! - Gecko longhand definitions for `white-space-collapse` and `text-wrap-mode`:
//!   <https://searchfox.org/firefox-main/source/servo/components/style/properties/longhands.toml#7501>
//! - Gecko `white-space` shorthand definition:
//!   <https://searchfox.org/firefox-main/source/servo/components/style/properties/shorthands.toml#962>

use ui_dom::{DomAttribute, DomAttributeValue, DomIntrinsic, DomNode, DomPointerUpdate};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_close, assert_load_root_stylesheet,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, theme_stylesheet_origin,
};

#[derive(Clone, Debug)]
struct WhiteSpaceTextLayout {
    block_height: f32,
    text_height: f32,
    line_height: f32,
    max_content_width: f32,
}

#[test]
// CSS Text 4: `normal` collapses collapsible white-space sequences.
fn normal_collapses_repeated_spaces_to_single_space() {
    let repeated = layout_for_text_with_white_space("Alpha    Beta", "normal", 300.0);
    let collapsed = layout_for_text_with_white_space("Alpha Beta", "normal", 300.0);

    assert_close(
        repeated.max_content_width,
        collapsed.max_content_width,
        "normal collapsed max-content width",
    );
    assert_close(
        repeated.text_height,
        repeated.line_height,
        "normal collapsed single-line height",
    );
}

#[test]
// CSS Text 4: `pre` preserves spaces and segment breaks, and disables soft
// wrapping.
fn pre_preserves_repeated_spaces_and_segment_breaks() {
    let normal = layout_for_text_with_white_space("Alpha    Beta", "normal", 300.0);
    let pre_spaces = layout_for_text_with_white_space("Alpha    Beta", "pre", 300.0);
    let pre_breaks = layout_for_text_with_white_space("Alpha\nBeta", "pre", 300.0);

    assert!(
        pre_spaces.max_content_width > normal.max_content_width,
        "pre should preserve repeated-space advance: pre={} normal={}",
        pre_spaces.max_content_width,
        normal.max_content_width,
    );
    assert_close(
        pre_breaks.text_height,
        pre_breaks.line_height * 2.0,
        "pre explicit segment-break height",
    );
}

#[test]
// CSS Text 4: `nowrap` collapses like `normal` but suppresses soft wrapping.
fn nowrap_collapses_spaces_without_soft_wrapping() {
    let normal = layout_for_text_with_white_space("Alpha Beta Gamma Delta", "normal", 54.0);
    let nowrap = layout_for_text_with_white_space("Alpha Beta Gamma Delta", "nowrap", 54.0);
    let nowrap_repeated = layout_for_text_with_white_space("Alpha    Beta", "nowrap", 300.0);
    let nowrap_collapsed = layout_for_text_with_white_space("Alpha Beta", "nowrap", 300.0);

    assert!(
        normal.block_height > normal.line_height,
        "normal should soft-wrap under a narrow width",
    );
    assert_close(
        nowrap.block_height,
        nowrap.line_height,
        "nowrap one-line height",
    );
    assert_close(
        nowrap_repeated.max_content_width,
        nowrap_collapsed.max_content_width,
        "nowrap collapsed max-content width",
    );
}

#[test]
// Blink and Gecko apply `white-space:nowrap` by disabling auto-wrap while
// keeping normal white-space collapsing. The retained live-attribute path
// should therefore shrink a previously wrapped block back to one line.
fn live_white_space_change_to_nowrap_reduces_used_height_to_one_line() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, block, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text_node);
    let baseline_intrinsic = assert_text_intrinsic(&root, text_node);

    assert_set_attr_str(&mut root, block, "white-space", "nowrap");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let nowrap_layout = assert_node_layout(&root, text_node);

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline normal white-space should wrap under a narrow width",
    );
    assert_close(
        nowrap_layout.height_px,
        baseline_intrinsic.line_height_px,
        "live white-space change to nowrap should shrink back to one line",
    );
}

#[test]
// CSS Text 4 plus the persisted stylesheet contract: selector-authored
// `white-space: nowrap` must apply on first layout and still hold after a
// later style recompute on the same node.
fn stylesheet_white_space_nowrap_stays_single_line_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .nowrap-block {
                display: block;
                white-space: nowrap;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, block, "class", "nowrap-block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let intrinsic = assert_text_intrinsic(&root, text_node);
    let first_layout = assert_node_layout(&root, text_node);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_layout = assert_node_layout(&root, text_node);

    assert_close(
        first_layout.height_px,
        intrinsic.line_height_px,
        "stylesheet nowrap initial one-line height",
    );
    assert_close(
        second_layout.height_px,
        intrinsic.line_height_px,
        "stylesheet nowrap one-line height after recompute",
    );
}

#[test]
// Persisted stylesheet coverage should also prove fallback when the class goes
// away. Removing `white-space: nowrap` must restore the default wrapping
// contract under the same constrained width.
fn removing_stylesheet_white_space_nowrap_restores_wrapping() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .nowrap-block {
                display: block;
                white-space: nowrap;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, block, "class", "nowrap-block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let intrinsic = assert_text_intrinsic(&root, text_node);
    let nowrap_layout = assert_node_layout(&root, text_node);

    assert_set_attr_str(&mut root, block, "class", "unset");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let fallback_layout = assert_node_layout(&root, text_node);

    assert_close(
        nowrap_layout.height_px,
        intrinsic.line_height_px,
        "stylesheet nowrap baseline one-line height",
    );
    assert!(
        fallback_layout.height_px > intrinsic.line_height_px,
        "removing the stylesheet nowrap class should restore wrapping under the same width",
    );
}

#[test]
// Persisted stylesheet `white-space: pre` should preserve spaces and keep the
// text on one overflowing line even after a later style recompute.
fn stylesheet_white_space_pre_preserves_spaces_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .pre-block {
                display: block;
                white-space: pre;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let baseline_block = assert_append_div_to(&mut root, container);
    let baseline_text = assert_append_text_to(&mut root, baseline_block, "Alpha    Beta");
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, baseline_block, "display", "block");
    assert_set_attr_str(&mut root, block, "class", "pre-block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, baseline_text);
    let first_intrinsic = assert_text_intrinsic(&root, text_node);
    let first_layout = assert_node_layout(&root, text_node);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text_node);
    let second_layout = assert_node_layout(&root, text_node);

    assert!(
        first_intrinsic.max_content_width_px > baseline_intrinsic.max_content_width_px,
        "stylesheet pre should preserve repeated-space advance width",
    );
    assert_close(
        first_layout.height_px,
        first_intrinsic.line_height_px,
        "stylesheet pre initial one-line height",
    );
    assert_close(
        second_intrinsic.max_content_width_px,
        first_intrinsic.max_content_width_px,
        "stylesheet pre preserved width after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet pre one-line height after recompute",
    );
}

#[test]
// Blink and Gecko apply `white-space:pre` by preserving ASCII spaces and
// disabling soft wrapping. The retained live-attribute path should therefore
// widen max-content width while collapsing a previously wrapped normal block
// back to one overflowing line.
fn live_white_space_change_to_pre_preserves_spaces_and_disables_soft_wrap() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, block, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text_node);
    let baseline_intrinsic = assert_text_intrinsic(&root, text_node);

    assert_set_attr_str(&mut root, block, "white-space", "pre");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let pre_layout = assert_node_layout(&root, text_node);
    let pre_intrinsic = assert_text_intrinsic(&root, text_node);

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline normal white-space should wrap under the narrow width",
    );
    assert!(
        pre_intrinsic.max_content_width_px > baseline_intrinsic.max_content_width_px,
        "live change to pre should preserve repeated-space advance width",
    );
    assert_close(
        pre_layout.height_px,
        pre_intrinsic.line_height_px,
        "live change to pre should disable soft wrapping and keep one-line used height",
    );
}

#[test]
// CSS Text 4: `pre-wrap` preserves white-space while allowing soft wrapping.
fn pre_wrap_preserves_spaces_and_allows_soft_wrapping() {
    let normal = layout_for_text_with_white_space("Alpha    Beta", "normal", 300.0);
    let pre_wrap_spaces = layout_for_text_with_white_space("Alpha    Beta", "pre-wrap", 300.0);
    let pre_wrap_wrapped =
        layout_for_text_with_white_space("Alpha Beta Gamma Delta", "pre-wrap", 54.0);

    assert!(
        pre_wrap_spaces.max_content_width > normal.max_content_width,
        "pre-wrap should preserve repeated-space advance",
    );
    assert!(
        pre_wrap_wrapped.block_height > pre_wrap_wrapped.line_height,
        "pre-wrap should soft-wrap under a narrow width",
    );
}

#[test]
// CSS Text 4 and the engine white-space decomposition helpers distinguish
// `pre-wrap` from `pre-line`: `pre-line` must preserve explicit segment breaks
// while collapsing repeated spaces. A live retained style change should reduce
// preserved-space width without losing the explicit line break.
fn live_white_space_change_to_pre_line_collapses_spaces_but_keeps_explicit_breaks() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta\nGamma");

    assert_set_attr_str(&mut root, container, "width", "300px");
    assert_set_attr_str(&mut root, block, "display", "block");
    assert_set_attr_str(&mut root, block, "white-space", "pre-wrap");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let pre_wrap_layout = assert_node_layout(&root, text_node);
    let pre_wrap_intrinsic = assert_text_intrinsic(&root, text_node);

    assert_set_attr_str(&mut root, block, "white-space", "pre-line");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let pre_line_layout = assert_node_layout(&root, text_node);
    let pre_line_intrinsic = assert_text_intrinsic(&root, text_node);

    assert!(
        pre_wrap_intrinsic.max_content_width_px > pre_line_intrinsic.max_content_width_px,
        "live change to pre-line should collapse repeated-space advance width",
    );
    assert_close(
        pre_line_layout.height_px,
        pre_wrap_layout.height_px,
        "live change to pre-line should keep the explicit segment-break line count",
    );
}

#[test]
// Persisted stylesheet `white-space: pre-line` should collapse repeated spaces
// while preserving explicit breaks, and that decomposition should survive a
// later style recompute.
fn stylesheet_white_space_pre_line_preserves_breaks_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .pre-line-block {
                display: block;
                white-space: pre-line;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let baseline_block = assert_append_div_to(&mut root, container);
    let baseline_text = assert_append_text_to(&mut root, baseline_block, "Alpha Beta\nGamma");
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta\nGamma");

    assert_set_attr_str(&mut root, container, "width", "300px");
    assert_set_attr_str(&mut root, baseline_block, "display", "block");
    assert_set_attr_str(&mut root, baseline_block, "white-space", "pre-line");
    assert_set_attr_str(&mut root, block, "class", "pre-line-block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, baseline_text);
    let first_intrinsic = assert_text_intrinsic(&root, text_node);
    let first_layout = assert_node_layout(&root, text_node);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text_node);
    let second_layout = assert_node_layout(&root, text_node);

    assert_close(
        first_intrinsic.max_content_width_px,
        baseline_intrinsic.max_content_width_px,
        "stylesheet pre-line should collapse repeated-space width",
    );
    assert_close(
        first_layout.height_px,
        first_intrinsic.line_height_px * 2.0,
        "stylesheet pre-line should preserve the explicit segment break",
    );
    assert_close(
        second_intrinsic.max_content_width_px,
        first_intrinsic.max_content_width_px,
        "stylesheet pre-line width after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet pre-line height after recompute",
    );
}

#[test]
// CSS Text 4: `pre-line` preserves segment breaks while collapsing other
// white-space sequences.
fn pre_line_preserves_segment_breaks_and_collapses_spaces() {
    let repeated = layout_for_text_with_white_space("Alpha    Beta\nGamma", "pre-line", 300.0);
    let collapsed = layout_for_text_with_white_space("Alpha Beta\nGamma", "pre-line", 300.0);

    assert_close(
        repeated.max_content_width,
        collapsed.max_content_width,
        "pre-line collapsed max-content width",
    );
    assert_close(
        repeated.text_height,
        repeated.line_height * 2.0,
        "pre-line explicit segment-break height",
    );
}

#[test]
// CSS Text 4: `break-spaces` preserves spaces and allows wrapping. The
// trailing-space break-position delta from `pre-wrap` is tracked below as a
// staged proof because the retained layout seam does not expose trailing
// advance boxes directly yet.
fn break_spaces_preserves_spaces_and_allows_soft_wrapping() {
    let normal = layout_for_text_with_white_space("Alpha    Beta", "normal", 300.0);
    let break_spaces = layout_for_text_with_white_space("Alpha    Beta", "break-spaces", 300.0);
    let wrapped = layout_for_text_with_white_space("Alpha Beta Gamma Delta", "break-spaces", 54.0);

    assert!(
        break_spaces.max_content_width > normal.max_content_width,
        "break-spaces should preserve repeated-space advance",
    );
    assert!(
        wrapped.block_height > wrapped.line_height,
        "break-spaces should soft-wrap under a narrow width",
    );
}

#[test]
// Blink's `break-spaces` implementation preserves white-space and creates
// break opportunities after preserved spaces. The retained live-attribute path
// should therefore widen max-content width relative to `normal` when repeated
// spaces are introduced through `white-space: break-spaces`.
fn live_white_space_change_to_break_spaces_increases_preserved_space_width() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta");

    assert_set_attr_str(&mut root, container, "width", "300px");
    assert_set_attr_str(&mut root, block, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let normal_intrinsic = assert_text_intrinsic(&root, text_node);

    assert_set_attr_str(&mut root, block, "white-space", "break-spaces");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let break_spaces_intrinsic = assert_text_intrinsic(&root, text_node);

    assert!(
        break_spaces_intrinsic.max_content_width_px > normal_intrinsic.max_content_width_px,
        "live change to break-spaces should preserve repeated-space advance width",
    );
}

#[test]
// Persisted stylesheet `white-space: break-spaces` should preserve spaces and
// keep doing so after a later style recompute.
fn stylesheet_white_space_break_spaces_preserves_space_width_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .break-spaces-block {
                display: block;
                white-space: break-spaces;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let baseline_block = assert_append_div_to(&mut root, container);
    let baseline_text = assert_append_text_to(&mut root, baseline_block, "Alpha    Beta");
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, "Alpha    Beta");

    assert_set_attr_str(&mut root, container, "width", "300px");
    assert_set_attr_str(&mut root, baseline_block, "display", "block");
    assert_set_attr_str(&mut root, block, "class", "break-spaces-block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, baseline_text);
    let first_intrinsic = assert_text_intrinsic(&root, text_node);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text_node);

    assert!(
        first_intrinsic.max_content_width_px > baseline_intrinsic.max_content_width_px,
        "stylesheet break-spaces should preserve repeated-space advance width",
    );
    assert_close(
        second_intrinsic.max_content_width_px,
        first_intrinsic.max_content_width_px,
        "stylesheet break-spaces preserved width after recompute",
    );
}

#[test]
#[ignore = "ui-dom currently proves the shared preserve-and-wrap slice; enable when retained text layout exposes trailing-space break positions"]
fn break_spaces_preserves_trailing_space_break_positions() {
    let _ = layout_for_text_with_white_space("Alpha   Beta   ", "break-spaces", 54.0);
}

fn layout_for_text_with_white_space(
    text: &str,
    white_space: &str,
    width_px: f32,
) -> WhiteSpaceTextLayout {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_div_to(&mut root, container);
    let text_node = assert_append_text_to(&mut root, block, text);

    assert_set_attr_str(&mut root, container, "width", &format!("{width_px}px"));
    assert_set_attr_str(&mut root, block, "display", "block");
    assert_set_attr_str(&mut root, block, "white-space", white_space);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.node(block)
            .and_then(|node| node.presented_attr(&DomAttribute::WhiteSpace)),
        Some(&DomAttributeValue::String(white_space.to_string())),
    );

    let block_layout = assert_node_layout(&root, block);
    let text_layout = assert_node_layout(&root, text_node);
    let intrinsic = assert_text_intrinsic(&root, text_node);

    WhiteSpaceTextLayout {
        block_height: block_layout.height_px,
        text_height: text_layout.height_px,
        line_height: intrinsic.line_height_px,
        max_content_width: intrinsic.max_content_width_px,
    }
}

fn assert_text_intrinsic(
    root: &ui_dom::RootDom,
    text: ui_dom::DomNodeSlotKey,
) -> ui_dom::TextIntrinsic {
    match root.node(text).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("white-space proof node should keep text intrinsic state"),
    }
}
