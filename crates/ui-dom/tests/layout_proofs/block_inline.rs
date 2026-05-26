#![allow(rustdoc::bare_urls)]

//! Block and inline flow layout proofs.
//!
//! Spec references:
//! - CSS 2.1 9.4.1 block formatting contexts:
//!   <https://www.w3.org/TR/CSS21/visuren.html#block-formatting>
//! - CSS 2.1 9.4.2 inline formatting contexts:
//!   <https://www.w3.org/TR/CSS21/visuren.html#inline-formatting>
//! - CSS 2.1 9.2.2 inline-level elements and inline boxes:
//!   <https://www.w3.org/TR/CSS21/visuren.html#inline-boxes>
//! - CSS 2.1 10.3.1 inline, non-replaced elements:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-width>
//! - CSS 2.1 10.6.1 inline, non-replaced height and 10.8 line height:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-non-replaced>
//!   <https://www.w3.org/TR/CSS21/visudet.html#line-height>
//! - CSS 2.1 10.3.10 inline-block, non-replaced elements in normal flow:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inlineblock-width>
//! - CSS 2.1 10.6.3 block formatting context height calculation:
//!   <https://www.w3.org/TR/CSS21/visudet.html#normal-block>
//!
//! Closest working examples used as oracles:
//! - WPT `block-formatting-contexts-001.xht`
//! - WPT `css/CSS2/visuren/inline-formatting-context-001.xht`
//! - WPT `inline-block-001.xht`
//!
//! Browser implementation references:
//! - Gecko `nsInlineFrame::AddInline{Min,Pref}ISize`, `nsInlineFrame::Reflow`,
//!   `nsLineLayout::ReflowFrame`, and `nsTextFrame::AddInline{Min,Pref}ISize`:
//!   <https://github.com/mozilla-firefox/firefox/blob/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/generic/nsInlineFrame.cpp>
//!   <https://github.com/mozilla-firefox/firefox/blob/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/generic/nsLineLayout.cpp>
//!   <https://github.com/mozilla-firefox/firefox/blob/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/generic/nsTextFrame.cpp>
//! - Blink `InlineNode::ComputeMinMaxSizes` and `LineBreaker` min/max-content
//!   modes for inline text and atomic inline descendants:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/inline/inline_node.cc>
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/inline/line_breaker.cc>

use ui_dom::{DomIntrinsic, DomPointerUpdate};

use crate::layout_proofs::support::{
    assert_append_button_to, assert_append_div_to, assert_append_span_to, assert_append_text_to,
    assert_close, assert_node_border_box_screen_left_px, assert_node_border_box_screen_top_px,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup,
};

#[test]
// CSS 2.1 9.4.1 and WPT block-formatting-contexts-001.xht: in-flow block
// children are laid out vertically one after another from the top of the
// containing block.
fn block_children_stack_vertically_from_the_top_of_the_parent() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let first_child = assert_append_div_to(&mut root, parent);
    let second_child = assert_append_div_to(&mut root, parent);

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, first_child, "width", "180px");
    assert_set_attr_str(&mut root, first_child, "height", "20px");
    assert_set_attr_str(&mut root, second_child, "width", "180px");
    assert_set_attr_str(&mut root, second_child, "height", "30px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    assert_close(
        assert_node_border_box_screen_top_px(&root, first_child),
        0.0,
        "first block child screen top",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second_child),
        20.0,
        "second block child screen top",
    );
    assert_close(parent_layout.height_px, 50.0, "parent auto height");
    assert_close(
        parent_layout.content_height_px,
        50.0,
        "parent retained content height",
    );
}

#[test]
// CSS 2.1 9.4.2 and WPT inline-formatting-context-001.xht: inline-level
// boxes in a block container share the same line box and advance horizontally.
// The containing block height comes from the tallest box in that line.
fn inline_block_siblings_share_a_line_and_use_the_tallest_line_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let first_chip = assert_append_div_to(&mut root, parent);
    let second_chip = assert_append_div_to(&mut root, parent);

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_inline_block_size(&mut root, first_chip, "40px", "12px");
    assert_set_inline_block_size(&mut root, second_chip, "50px", "30px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    assert_close(
        assert_node_border_box_screen_left_px(&root, first_chip),
        0.0,
        "first inline-block screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, first_chip),
        0.0,
        "first inline-block screen top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, second_chip),
        40.0,
        "second inline-block screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, second_chip),
        0.0,
        "second inline-block screen top",
    );
    assert_close(parent_layout.height_px, 30.0, "parent line-box height");
    assert_close(
        parent_layout.content_height_px,
        30.0,
        "parent retained line-box height",
    );
}

#[test]
// CSS 2.1 9.4.1 and 9.4.2: once a block child appears after inline content,
// the next block starts below the completed line box instead of overlapping it.
fn block_child_after_an_inline_line_starts_below_the_line_box() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let first_chip = assert_append_div_to(&mut root, parent);
    let second_chip = assert_append_div_to(&mut root, parent);
    let block_child = assert_append_div_to(&mut root, parent);

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_inline_block_size(&mut root, first_chip, "40px", "12px");
    assert_set_inline_block_size(&mut root, second_chip, "50px", "30px");
    assert_set_attr_str(&mut root, block_child, "width", "180px");
    assert_set_attr_str(&mut root, block_child, "height", "20px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    assert_close(
        assert_node_border_box_screen_left_px(&root, block_child),
        0.0,
        "block child screen left after inline line",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, block_child),
        30.0,
        "block child screen top after inline line",
    );
    assert_close(parent_layout.height_px, 50.0, "parent height");
    assert_close(
        parent_layout.content_height_px,
        50.0,
        "parent retained content height",
    );
}

#[test]
// CSS 2.1 9.4.1 and 9.4.2: normal-flow children are positioned from the
// parent's content box, not from its border edge, so authored padding shifts
// the starting left/top position for both block stacking and inline lines.
fn normal_flow_children_start_inside_the_parent_padding_box() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let parent = assert_append_div_to(&mut root, document_root);
    let block_child = assert_append_div_to(&mut root, parent);
    let inline_child = assert_append_div_to(&mut root, parent);

    assert_set_attr_str(&mut root, parent, "width", "180px");
    assert_set_attr_str(&mut root, parent, "padding-left", "7px");
    assert_set_attr_str(&mut root, parent, "padding-top", "5px");
    assert_set_attr_str(&mut root, block_child, "width", "173px");
    assert_set_attr_str(&mut root, block_child, "height", "20px");
    assert_set_inline_block_size(&mut root, inline_child, "40px", "12px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let parent_layout = assert_node_layout(&root, parent);
    assert_close(
        assert_node_border_box_screen_left_px(&root, block_child),
        7.0,
        "padded block child screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, block_child),
        5.0,
        "padded block child screen top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, inline_child),
        7.0,
        "padded inline child screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, inline_child),
        25.0,
        "padded inline child screen top",
    );
    assert_close(parent_layout.height_px, 37.0, "padded parent height");
}

#[test]
// CSS 2.1 9.2.2, 9.4.2, and 10.3.1: a non-replaced inline element sizes to
// its rendered inline contents rather than filling the containing block width.
fn inline_container_shrink_wraps_inline_block_children() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let inline_container = assert_append_div_to(&mut root, wrapper);
    let first_chip = assert_append_div_to(&mut root, inline_container);
    let second_chip = assert_append_div_to(&mut root, inline_container);

    assert_set_attr_str(&mut root, wrapper, "width", "180px");
    assert_set_attr_str(&mut root, inline_container, "display", "inline");
    assert_set_inline_block_size(&mut root, first_chip, "40px", "12px");
    assert_set_inline_block_size(&mut root, second_chip, "50px", "30px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let inline_layout = assert_node_layout(&root, inline_container);
    assert_close(
        inline_layout.width_px,
        90.0,
        "inline container shrink-wrap width",
    );
    assert_close(inline_layout.height_px, 30.0, "inline container height");
    assert_close(
        assert_node_border_box_screen_left_px(&root, second_chip),
        40.0,
        "second chip screen left inside inline container",
    );
}

#[test]
// CSS 2.1 9.4.2: an inline element stays in the surrounding inline formatting
// context, so a preceding inline-level sibling advances the inline container's
// start position instead of forcing it onto a new block row.
fn inline_container_stays_on_the_same_line_as_a_preceding_inline_sibling() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let leading_chip = assert_append_div_to(&mut root, wrapper);
    let inline_container = assert_append_div_to(&mut root, wrapper);
    let first_chip = assert_append_div_to(&mut root, inline_container);
    let second_chip = assert_append_div_to(&mut root, inline_container);

    assert_set_attr_str(&mut root, wrapper, "width", "180px");
    assert_set_inline_block_size(&mut root, leading_chip, "20px", "10px");
    assert_set_attr_str(&mut root, inline_container, "display", "inline");
    assert_set_inline_block_size(&mut root, first_chip, "40px", "12px");
    assert_set_inline_block_size(&mut root, second_chip, "50px", "30px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let wrapper_layout = assert_node_layout(&root, wrapper);
    assert_close(
        assert_node_border_box_screen_left_px(&root, inline_container),
        20.0,
        "inline container screen left after leading sibling",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, inline_container),
        0.0,
        "inline container screen top after leading sibling",
    );
    assert_close(wrapper_layout.height_px, 30.0, "wrapper line-box height");
}

#[test]
// CSS 2.1 9.2.2, 9.4.2, and 10.3.1: a non-replaced inline span with no
// authored width/height contributes the rendered text content size to the line
// box. Gecko's nsInlineFrame/nsTextFrame path and Blink's InlineNode/LineBreaker
// path both derive inline min/max content from measured text runs before final
// inline placement.
fn span_text_child_without_authored_size_uses_text_intrinsic_dimensions() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let span = assert_append_span_to(&mut root, wrapper);
    let text = assert_append_text_to(&mut root, span, "Status Ready");

    assert_set_attr_str(&mut root, wrapper, "width", "220px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let wrapper_layout = assert_node_layout(&root, wrapper);
    let span_layout = assert_node_layout(&root, span);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert_close(
        assert_node_border_box_screen_left_px(&root, span),
        0.0,
        "span screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, span),
        0.0,
        "span screen top",
    );
    assert_close(
        assert_node_border_box_screen_left_px(&root, text),
        0.0,
        "span text screen left",
    );
    assert_close(
        assert_node_border_box_screen_top_px(&root, text),
        0.0,
        "span text screen top",
    );
    assert_close(
        text_layout.width_px,
        text_intrinsic.max_content_width_px,
        "span text child width should use measured max-content width",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "span text child height should stay one measured line",
    );
    assert_close(
        span_layout.width_px,
        text_intrinsic.max_content_width_px,
        "span width should come from its text child",
    );
    assert_close(
        span_layout.height_px,
        text_intrinsic.line_height_px,
        "span height should come from its text child line box",
    );
    assert_close(
        wrapper_layout.height_px,
        text_intrinsic.line_height_px,
        "wrapper line box height should come from the span text",
    );
}

#[test]
// CSS 2.1 10.3.1 and 10.6.1: `width` and `height` do not apply to
// non-replaced inline elements. A span with authored width/height should still
// size from its rendered text, and the text child should not inherit the
// ignored authored width as a wrapping constraint.
fn span_text_child_ignores_authored_inline_width_and_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let span = assert_append_span_to(&mut root, wrapper);
    let text = assert_append_text_to(&mut root, span, "Status Ready");

    assert_set_attr_str(&mut root, wrapper, "width", "220px");
    assert_set_attr_str(&mut root, span, "width", "20px");
    assert_set_attr_str(&mut root, span, "height", "40px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let span_layout = assert_node_layout(&root, span);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        text_intrinsic.max_content_width_px > 20.0,
        "proof requires text wider than ignored authored width; got {}",
        text_intrinsic.max_content_width_px,
    );
    assert!(
        (text_intrinsic.line_height_px - 40.0).abs() > 0.001,
        "proof requires text line height different from ignored authored height; got {}",
        text_intrinsic.line_height_px,
    );
    assert_close(
        text_layout.width_px,
        text_intrinsic.max_content_width_px,
        "text child should ignore span authored width",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "text child should ignore span authored height",
    );
    assert_close(
        span_layout.width_px,
        text_intrinsic.max_content_width_px,
        "span width should ignore authored inline width",
    );
    assert_close(
        span_layout.height_px,
        text_intrinsic.line_height_px,
        "span height should ignore authored inline height",
    );
}

#[test]
// CSS 2.1 9.2.2 and 10.3.10 plus WPT inline-block-001.xht: an inline-block is
// an atomic inline-level box whose inner block formatting context shrink-wraps
// to its contents, then adds authored padding around that content box.
fn inline_block_text_chip_shrink_wraps_text_width_plus_padding() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let chip = assert_append_div_to(&mut root, wrapper);
    let text = assert_append_text_to(&mut root, chip, "Add Scene");

    assert_set_attr_str(&mut root, wrapper, "width", "180px");
    assert_set_attr_str(&mut root, chip, "display", "inline-block");
    assert_set_attr_str(&mut root, chip, "padding-left", "16px");
    assert_set_attr_str(&mut root, chip, "padding-right", "16px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let chip_layout = assert_node_layout(&root, chip);
    let text_layout = assert_node_layout(&root, text);
    let text_max_content_width_px = assert_text_max_content_width_px(&root, text);

    assert!(
        chip_layout.width_px < 180.0,
        "inline-block chip should shrink-wrap instead of filling the wrapper: chip={chip_layout:?}",
    );
    assert_close(
        text_layout.width_px,
        text_max_content_width_px,
        "inline-block text child max-content width",
    );
    assert_close(
        chip_layout.width_px,
        text_layout.width_px + 32.0,
        "inline-block chip width including horizontal padding",
    );
}

#[test]
// CSS 2.1 10.3.9 and 10.3.5: an auto-width non-replaced inline-block
// calculates shrink-to-fit from preferred content widths before laying out its
// children. The text child must therefore receive a max-content width in a
// roomy containing block instead of being wrapped by a provisional zero width.
fn inline_block_button_auto_width_uses_text_max_content_before_child_layout() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let wrapper = assert_append_div_to(&mut root, document_root);
    let button = assert_append_button_to(&mut root, wrapper);
    let text = assert_append_text_to(&mut root, button, "Save Camera Shot");

    assert_set_attr_str(&mut root, wrapper, "width", "220px");
    assert_set_attr_str(&mut root, button, "display", "inline-block");
    assert_set_attr_str(&mut root, button, "padding-left", "12px");
    assert_set_attr_str(&mut root, button, "padding-right", "12px");
    assert_set_attr_str(&mut root, button, "border-left", "1px");
    assert_set_attr_str(&mut root, button, "border-right", "1px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let button_layout = assert_node_layout(&root, button);
    let text_layout = assert_node_layout(&root, text);
    let text_max_content_width_px = assert_text_max_content_width_px(&root, text);

    assert_close(
        text_layout.width_px,
        text_max_content_width_px,
        "button text child max-content width",
    );
    assert_close(
        button_layout.width_px,
        text_max_content_width_px + 26.0,
        "inline-block button width including horizontal padding and border",
    );
}

fn assert_text_max_content_width_px(root: &ui_dom::RootDom, key: ui_dom::DomNodeSlotKey) -> f32 {
    assert_text_intrinsic(root, key).max_content_width_px
}

fn assert_text_intrinsic(
    root: &ui_dom::RootDom,
    key: ui_dom::DomNodeSlotKey,
) -> &ui_dom::TextIntrinsic {
    let node = root.node(key).expect("text node should exist");
    let DomIntrinsic::Text(intrinsic) = node.intrinsic() else {
        panic!("layout proof target should be a measured text node");
    };
    intrinsic
}

fn assert_set_inline_block_size(
    root: &mut ui_dom::RootDom,
    key: ui_dom::DomNodeSlotKey,
    width: &str,
    height: &str,
) {
    assert_set_attr_str(root, key, "display", "inline-block");
    assert_set_attr_str(root, key, "width", width);
    assert_set_attr_str(root, key, "height", height);
}
