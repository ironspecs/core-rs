#![allow(rustdoc::bare_urls)]

//! Text-driven auto-size layout proofs.
//!
//! Spec references:
//! - CSS 2.1 10.3.3 block-level, non-replaced elements in normal flow:
//!   <https://www.w3.org/TR/CSS21/visudet.html#blockwidth>
//! - CSS 2.1 10.6.3 block formatting context height calculation:
//!   <https://www.w3.org/TR/CSS21/visudet.html#normal-block>

use ui_dom::{DomIntrinsic, DomNode, DomPointerUpdate, node_box};

use crate::layout_proofs::support::{
    assert_append_button_to, assert_append_div_to, assert_append_text_to, assert_close,
    assert_load_root_stylesheet, assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup, fixed_popup_with_text_rows_setup,
    theme_stylesheet_origin,
};

const MENU_POPUP_LEFT_PX: &str = "16px";
const MENU_POPUP_TOP_PX: &str = "12px";
const MENU_POPUP_WIDTH_PX: &str = "180px";
const MENU_POPUP_MAX_HEIGHT_PX: &str = "160px";
const MENU_CONTENT_WIDTH_PX: &str = "172px";
const MENU_CONTENT_PADDING_PX: &str = "4px";
const ROW_MARGIN_LEFT_RIGHT_PX: &str = "2px";
const ROW_MARGIN_TOP_BOTTOM_PX: &str = "1px";
const ROW_PADDING_LEFT_RIGHT_PX: &str = "2px";
const ROW_PADDING_TOP_BOTTOM_PX: &str = "1px";

#[test]
// CSS 2.1 10.3.3 and 10.6.3: a block with only a text child and no authored
// width or height should use the available width and the text line height, and
// the text node should inherit that width-constrained layout box.
fn text_only_block_auto_sizes_from_available_width_and_text_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Open Right Drawer");

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert_close(row_layout.width_px, 180.0, "text-only block width");
    assert_close(
        row_layout.height_px,
        text_intrinsic.line_height_px,
        "text-only block height",
    );
    assert_close(
        text_layout.width_px,
        row_layout.width_px,
        "text-only text layout width",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "text-only text height",
    );
}

#[test]
// Menu-row invariant: in a fixed-width content box, a block row with text and
// only stylesheet-like margins and padding should fill the available width and
// auto-height to text line height plus vertical padding.
fn menu_like_text_row_auto_height_comes_from_text_and_padding() {
    let (mut world, mut root, popup, content, first_row, first_text, second_row, second_text) =
        fixed_popup_with_text_rows_setup();

    assert_menu_like_popup_style(&mut root, popup, content);
    assert_menu_like_row_style(&mut root, first_row);
    assert_menu_like_row_style(&mut root, second_row);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let popup_layout = assert_node_layout(&root, popup);
    let content_layout = assert_node_layout(&root, content);
    let content_box = node_box(&root, content).expect("menu-like content should have a box");
    let first_row_layout = assert_node_layout(&root, first_row);
    let second_row_layout = assert_node_layout(&root, second_row);
    let first_text_intrinsic = assert_text_intrinsic(&root, first_text);
    let second_text_intrinsic = assert_text_intrinsic(&root, second_text);
    let first_row_expected_height = first_text_intrinsic.line_height_px + 2.0;
    let second_row_expected_height = second_text_intrinsic.line_height_px + 2.0;
    let expected_content_height = first_row_expected_height + second_row_expected_height + 11.0;

    assert_close(popup_layout.width_px, 180.0, "menu-like popup width");
    assert_close(
        content_layout.width_px,
        180.0,
        "menu-like content layout width",
    );
    assert_close(
        content_box.border_box.width_px,
        180.0,
        "menu-like content border-box width",
    );
    assert_close(
        first_row_layout.width_px,
        168.0,
        "menu-like first row width",
    );
    assert_close(
        second_row_layout.width_px,
        168.0,
        "menu-like second row width",
    );
    assert_close(
        first_row_layout.height_px,
        first_row_expected_height,
        "menu-like first row height",
    );
    assert_close(
        second_row_layout.height_px,
        second_row_expected_height,
        "menu-like second row height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "menu-like popup content height",
    );
}

#[test]
// Width-constrained Japanese text should grow in height from the shared
// paragraph layout path, not remain at a single intrinsic line.
fn japanese_text_block_grows_in_height_when_width_is_constrained() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(
        &mut root,
        row,
        "長い日本語の段落テキストです。さらに長い説明文です。",
    );

    assert_set_attr_str(&mut root, container, "width", "36px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        row_layout.height_px > text_intrinsic.line_height_px,
        "Japanese row should wrap and grow taller than one line",
    );
    assert!(
        text_layout.height_px > text_intrinsic.line_height_px,
        "Japanese text layout should grow taller than one line",
    );
}

#[test]
// CSS Text 3 `word-break: keep-all` suppresses implicit letter-unit breaks
// within Japanese text while keeping the same containing-width contract. Gecko
// wires `KeepAll` into its line breaker, and Blink tracks it as a dedicated
// `keep_all` path in inline breaking.
fn word_break_keep_all_suppresses_japanese_intra_word_wrap_in_normal_flow() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "窓ぎわのトットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");
    assert_set_attr_str(&mut root, row, "word-break", "keep-all");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after keep-all update")
        .snapshot();

    assert_close(
        row_layout.height_px,
        baseline_intrinsic.line_height_px,
        "word-break:keep-all row should stay on one overflowing line",
    );
    assert_close(
        text_layout.height_px,
        baseline_intrinsic.line_height_px,
        "word-break:keep-all text should stay on one overflowing line",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "word-break:keep-all snapshot available width",
    );
}

#[test]
// Gecko and Blink remeasure live text after a `word-break: keep-all` change.
// The retained path should therefore pull a previously wrapped Japanese
// heading back onto one overflowing line without changing the width contract.
fn word_break_keep_all_live_attr_restores_single_line_japanese_heading() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "窓ぎわのトットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, row, "word-break", "keep-all");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let keep_all_layout = assert_node_layout(&root, text);
    let keep_all_snapshot = root
        .node(text)
        .expect("text node should exist after live keep-all update")
        .snapshot();

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline Japanese heading should wrap before keep-all",
    );
    assert_close(
        keep_all_layout.height_px,
        baseline_intrinsic.line_height_px,
        "live keep-all should restore one-line Japanese height",
    );
    assert_close(
        keep_all_snapshot.layout.available_width_px,
        constrained_width_px,
        "live keep-all should keep the descendant width contract",
    );
}

#[test]
// CSS Text 3 says `keep-all` only suppresses implicit breaks; explicit break
// markers such as U+200B still remain. This keeps the implementation honest
// against the tempting but wrong `nowrap` shortcut.
fn word_break_keep_all_preserves_explicit_zero_width_space_breaks() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "窓ぎわの\u{200b}トットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");
    assert_set_attr_str(&mut root, row, "word-break", "keep-all");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        text_layout.height_px > text_intrinsic.line_height_px,
        "word-break:keep-all should still honor explicit zero-width-space breaks",
    );
}

#[test]
// `word-break` is inherited in CSS Text 3. A descendant Japanese heading under
// ancestor-authored `keep-all` should therefore stay on one overflowing line
// under the same constrained width contract.
fn inherited_word_break_keep_all_suppresses_descendant_japanese_wrap() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "窓ぎわのトットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, container, "word-break", "keep-all");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited keep-all update")
        .snapshot();

    assert_close(
        text_layout.height_px,
        baseline_intrinsic.line_height_px,
        "inherited keep-all should suppress descendant Japanese wrapping",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "inherited keep-all should keep the descendant width contract",
    );
}

#[test]
// Updating an ancestor from normal to inherited `keep-all` should remeasure a
// wrapped descendant Japanese heading back to one line under the same
// available width, not leave the old wrapped geometry in place.
fn live_inherited_word_break_keep_all_restores_single_line_descendant() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "窓ぎわのトットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "word-break", "keep-all");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let keep_all_layout = assert_node_layout(&root, text);
    let keep_all_snapshot = root
        .node(text)
        .expect("text node should exist after live inherited keep-all update")
        .snapshot();

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline descendant heading should wrap before inherited keep-all",
    );
    assert_close(
        keep_all_layout.height_px,
        baseline_intrinsic.line_height_px,
        "live inherited keep-all should restore one-line descendant height",
    );
    assert_close(
        keep_all_snapshot.layout.available_width_px,
        constrained_width_px,
        "live inherited keep-all should keep the descendant width contract",
    );
}

#[test]
// Width-constrained Thai text should grow in height from the same shared
// paragraph layout path so line breaking stays consistent with render.
fn thai_text_block_grows_in_height_when_width_is_constrained() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม");

    assert_set_attr_str(&mut root, container, "width", "36px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        row_layout.height_px > text_intrinsic.line_height_px,
        "Thai row should wrap and grow taller than one line",
    );
    assert!(
        text_layout.height_px > text_intrinsic.line_height_px,
        "Thai text layout should grow taller than one line",
    );
}

#[test]
// CSS 2.1 10.3.3 plus the existing retained content-box contract: a padded
// block should give its text child only the content-box width as available
// inline space, so wrapping must use the inner width rather than the border
// box width.
fn padded_text_block_wraps_from_content_box_available_width() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "72px");
    assert_set_attr_str(&mut root, row, "display", "block");
    assert_set_attr_str(&mut root, row, "padding-left", "10px");
    assert_set_attr_str(&mut root, row, "padding-right", "10px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert_close(
        row_layout.width_px,
        72.0,
        "padded text block border-box width",
    );
    assert_close(
        text_layout.width_px,
        52.0,
        "text child width should use the padded content-box width",
    );
    assert!(
        text_layout.height_px > text_intrinsic.line_height_px,
        "text child should wrap when only the padded content box is available",
    );
}

#[test]
// Blink NGLineBreaker and Gecko nsTextFrame both keep default wrapping on the
// normal word path and only use glyph fallback for explicit break-word style
// cases. A narrow block containing `Alpha Beta` therefore should resolve to two
// wrapped lines, not a column of individual letters.
fn default_text_wrap_keeps_ascii_words_intact_in_normal_flow() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (text_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_snapshot = root
        .node(text)
        .expect("text node should exist after update")
        .snapshot();
    let text_layout = assert_node_layout(&root, text);

    assert_close(
        text_snapshot.layout.available_width_px,
        text_layout.width_px,
        "text snapshot available width",
    );
    assert_close(
        text_snapshot.layout.used_width_px,
        text_layout.width_px,
        "text snapshot used width",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px * 2.0,
        "normal wrap should produce exactly two text lines",
    );
}

#[test]
// Blink and Gecko distinguish `overflow-wrap:anywhere` from `break-word`, and
// that authored wrap policy must remain available through persisted stylesheets.
fn stylesheet_overflow_wrap_anywhere_rewraps_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .wrap-anywhere {
                display: block;
                overflow-wrap: anywhere;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let baseline_block = assert_append_button_to(&mut root, container);
    let baseline_text = assert_append_text_to(
        &mut root,
        baseline_block,
        "Supercalifragilisticexpialidocious",
    );
    let block = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, block, "Supercalifragilisticexpialidocious");

    assert_set_attr_str(&mut root, container, "width", "36px");
    assert_set_attr_str(&mut root, baseline_block, "display", "block");
    assert_set_attr_str(&mut root, block, "class", "wrap-anywhere");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, baseline_text);
    let intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_layout = assert_node_layout(&root, text);

    assert!(
        intrinsic.min_content_width_px < baseline_intrinsic.min_content_width_px,
        "stylesheet anywhere should reduce min-content width below normal wrapping: anywhere={} normal={}",
        intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
    );
    assert!(
        first_layout.height_px > intrinsic.line_height_px,
        "stylesheet anywhere should wrap the unbreakable word under a narrow width",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet anywhere wrap height after recompute",
    );
}

#[test]
// Blink NG and Gecko both map `word-break: break-all` to the glyph-breaking
// path. That behavior should be reachable through stylesheets and survive a
// later style recompute.
fn stylesheet_word_break_break_all_rewraps_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .break-all {
                display: block;
                word-break: break-all;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, block, "UnbreakableASCIIWord");

    assert_set_attr_str(&mut root, container, "width", "36px");
    assert_set_attr_str(&mut root, block, "class", "break-all");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_layout = assert_node_layout(&root, text);

    assert!(
        first_layout.height_px > intrinsic.line_height_px,
        "stylesheet break-all should wrap the unbreakable word under a narrow width",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet break-all wrap height after recompute",
    );
}

#[test]
// Blink NG and Gecko keep `word-break: keep-all` on the dedicated non-breaking
// CJK path across style recomputes. The stylesheet path should therefore keep a
// constrained Japanese heading on one line even after an unrelated update.
fn stylesheet_word_break_keep_all_suppresses_japanese_wrap_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .keep-all {
                display: block;
                word-break: keep-all;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, block, "窓ぎわのトットちゃん");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.max_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, block, "class", "keep-all");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_layout = assert_node_layout(&root, text);

    assert_close(
        first_layout.height_px,
        baseline_intrinsic.line_height_px,
        "stylesheet keep-all should suppress implicit Japanese wraps",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet keep-all height after recompute",
    );
}

#[test]
// Blink NGLineBreaker and Gecko nsTextFrame both keep `overflow-wrap:
// break-word` on the normal min-content path while still allowing emergency
// fallback wrapping during layout overflow. That behavior should remain
// available through persisted stylesheets and survive recompute.
fn stylesheet_overflow_wrap_break_word_rewraps_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .break-word {
                display: block;
                overflow-wrap: break-word;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let baseline_block = assert_append_button_to(&mut root, container);
    let baseline_text = assert_append_text_to(&mut root, baseline_block, "AlphaBeta");
    let block = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, block, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, baseline_text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, baseline_block, "display", "block");
    assert_set_attr_str(&mut root, block, "class", "break-word");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);
    let first_snapshot = root
        .node(text)
        .expect("text node should exist after stylesheet break-word update")
        .snapshot();

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text);
    let second_layout = assert_node_layout(&root, text);

    assert_close(
        first_intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
        "stylesheet break-word should preserve min-content width on the normal path",
    );
    assert!(
        first_layout.height_px > baseline_intrinsic.line_height_px,
        "stylesheet break-word should still rewrap used height under a narrow width",
    );
    assert_close(
        first_snapshot.layout.available_width_px,
        constrained_width_px,
        "stylesheet break-word should keep the text width contract",
    );
    assert_close(
        second_intrinsic.min_content_width_px,
        first_intrinsic.min_content_width_px,
        "stylesheet break-word min-content width after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet break-word wrapped height after recompute",
    );
}

#[test]
// Line height is part of the recently widened text style surface. A class-set
// line height should drive used height through retained layout and still apply
// after a later style recompute.
fn stylesheet_line_height_scales_wrapped_text_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .tall-lines {
                display: block;
                line-height: 24px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let block = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, block, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, block, "class", "tall-lines");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, block, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text);
    let second_layout = assert_node_layout(&root, text);

    assert_close(
        first_intrinsic.line_height_px,
        24.0,
        "stylesheet line-height intrinsic value",
    );
    assert!(
        first_layout.height_px > first_intrinsic.line_height_px,
        "stylesheet line-height block should still wrap under the narrow width",
    );
    assert_close(
        second_intrinsic.line_height_px,
        24.0,
        "stylesheet line-height intrinsic value after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet line-height wrapped height after recompute",
    );
}

#[test]
// Inherited `line-height` should also remain available through persisted
// stylesheets. A class-authored ancestor line height must remeasure the
// descendant text and keep doing so after a later recompute.
fn stylesheet_inherited_line_height_updates_descendant_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .tall-line-container {
                line-height: 28px;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Open Right Drawer");

    assert_set_attr_str(&mut root, container, "class", "tall-line-container");
    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text);
    let second_layout = assert_node_layout(&root, text);

    assert_close(
        first_intrinsic.line_height_px,
        28.0,
        "stylesheet inherited line-height intrinsic value",
    );
    assert_close(
        first_layout.height_px,
        28.0,
        "stylesheet inherited line-height first text height",
    );
    assert_close(
        second_intrinsic.line_height_px,
        first_intrinsic.line_height_px,
        "stylesheet inherited line-height intrinsic value after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet inherited line-height text height after recompute",
    );
}

#[test]
// Blink and Gecko keep `white-space:nowrap` on one overflowing line even when
// the containing block is narrower than the text run. The retained path should
// therefore keep one-line used height under the constrained width contract.
fn nowrap_text_stays_single_line_when_width_is_constrained() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (text_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");
    assert_set_attr_str(&mut root, row, "white-space", "nowrap");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);

    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "nowrap text should keep one-line used height under constrained width",
    );
}

#[test]
// Blink and Gecko treat `word-break:break-all` as explicit glyph breaking. A
// constrained unbreakable ASCII word should therefore grow taller than one line
// through the retained normal-flow path.
fn word_break_break_all_wraps_unbreakable_ascii_word_in_normal_flow() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (text_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");
    assert_set_attr_str(&mut root, row, "word-break", "break-all");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);

    assert!(
        text_layout.height_px > text_intrinsic.line_height_px,
        "word-break:break-all should grow used height for a constrained unbreakable ASCII word",
    );
}

#[test]
// Blink and Gecko switch `word-break:break-all` onto character-breaking. The
// retained live-attribute path should therefore rewrap a previously
// single-line overflowing ASCII word once the word-break policy changes.
fn word_break_break_all_live_attr_rewraps_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (text_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, row, "word-break", "break-all");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let break_all_layout = assert_node_layout(&root, text);
    let break_all_snapshot = root
        .node(text)
        .expect("text node should exist after break-all update")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        text_intrinsic.line_height_px,
        "baseline unbreakable word should stay on one overflowing line before break-all",
    );
    assert!(
        break_all_layout.height_px > baseline_layout.height_px,
        "word-break:break-all should rewrap used height after a live attribute change",
    );
    assert_close(
        break_all_snapshot.layout.available_width_px,
        constrained_width_px,
        "word-break:break-all snapshot available width",
    );
}

#[test]
// Blink and Gecko treat `overflow-wrap:anywhere` as a real min-content break
// opportunity. The retained intrinsic path should therefore shrink the text
// node's min-content width while keeping the same source text and max-content
// contract.
fn overflow_wrap_anywhere_reduces_retained_text_min_content_width() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");

    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, text);

    assert_set_attr_str(&mut root, row, "overflow-wrap", "anywhere");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let anywhere_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        anywhere_intrinsic.min_content_width_px < baseline_intrinsic.min_content_width_px,
        "overflow-wrap:anywhere should reduce retained min-content width for an unbreakable ASCII word",
    );
    assert_close(
        anywhere_intrinsic.max_content_width_px,
        baseline_intrinsic.max_content_width_px,
        "overflow-wrap:anywhere should keep retained max-content width",
    );
}

#[test]
// The live retained layout path should also reflect `overflow-wrap:anywhere`
// under constrained width, not just the intrinsic min-content metric. A
// previously overflowing unbreakable ASCII word should grow used height after
// the wrap-policy change while keeping the same containing width contract.
fn overflow_wrap_anywhere_live_attr_rewraps_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (text_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, row, "overflow-wrap", "anywhere");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let anywhere_layout = assert_node_layout(&root, text);
    let anywhere_snapshot = root
        .node(text)
        .expect("text node should exist after anywhere update")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        text_intrinsic.line_height_px,
        "baseline unbreakable word should stay on one overflowing line",
    );
    assert!(
        anywhere_layout.height_px > baseline_layout.height_px,
        "overflow-wrap:anywhere should rewrap used height under constrained width",
    );
    assert_close(
        anywhere_snapshot.layout.available_width_px,
        constrained_width_px,
        "overflow-wrap:anywhere snapshot available width",
    );
}

#[test]
// Blink and Gecko keep `overflow-wrap:break-word` off the min-content path.
// The retained intrinsic contract should therefore leave min-content width on
// the normal word-wrap measurement even though the live layout path may use
// emergency glyph fallback after overflow.
fn overflow_wrap_break_word_preserves_retained_text_min_content_width() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");

    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_text_intrinsic(&root, text);

    assert_set_attr_str(&mut root, row, "overflow-wrap", "break-word");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let break_word_intrinsic = assert_text_intrinsic(&root, text);

    assert_close(
        break_word_intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
        "overflow-wrap:break-word should preserve retained min-content width",
    );
    assert_close(
        break_word_intrinsic.max_content_width_px,
        baseline_intrinsic.max_content_width_px,
        "overflow-wrap:break-word should keep retained max-content width",
    );
}

#[test]
// Blink NGLineBreaker and Gecko nsTextFrame both keep `break-word` off the
// intrinsic min-content path while still allowing emergency fallback wrapping
// during live layout overflow. The retained DOM path should therefore grow used
// height under constraint without changing the min-content contract.
fn overflow_wrap_break_word_live_attr_rewraps_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, row, "overflow-wrap", "break-word");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let break_word_layout = assert_node_layout(&root, text);
    let break_word_intrinsic = assert_text_intrinsic(&root, text);
    let break_word_snapshot = root
        .node(text)
        .expect("text node should exist after break-word update")
        .snapshot();

    assert_close(
        break_word_intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
        "overflow-wrap:break-word should keep min-content width during live rewrap",
    );
    assert!(
        break_word_layout.height_px > baseline_layout.height_px,
        "overflow-wrap:break-word should rewrap used height after a live attribute change",
    );
    assert_close(
        break_word_snapshot.layout.available_width_px,
        constrained_width_px,
        "overflow-wrap:break-word snapshot available width",
    );
}

#[test]
// The retained text layout contract must remeasure after text mutation without
// changing the containing width contract. A longer replacement string should
// grow used height while the text node snapshot keeps the same available and
// used width as its constrained block parent.
fn text_content_update_rewraps_height_without_changing_width_contract() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha");

    assert_set_attr_str(&mut root, container, "width", "60px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);
    let baseline_snapshot = root
        .node(text)
        .expect("text node should exist before text mutation")
        .snapshot();

    root.set_text_content(text, "Alpha Beta Gamma Delta")
        .expect("text content update should succeed");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grown_layout = assert_node_layout(&root, text);
    let grown_snapshot = root
        .node(text)
        .expect("text node should exist after text mutation")
        .snapshot();

    assert!(
        grown_layout.height_px > baseline_layout.height_px,
        "longer text content should grow retained text height when the width is constrained",
    );
    assert_close(
        baseline_snapshot.layout.available_width_px,
        60.0,
        "baseline text snapshot available width",
    );
    assert_close(
        grown_snapshot.layout.available_width_px,
        60.0,
        "grown text snapshot available width",
    );
    assert_close(
        grown_snapshot.layout.used_width_px,
        60.0,
        "grown text snapshot used width",
    );
}

#[test]
// The retained text mutation contract should be symmetric. Replacing a wrapped
// longer string with a short string should shrink used height back to one line
// while keeping the same available and used width contract.
fn text_content_update_can_shrink_height_back_to_one_line() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta Gamma Delta");

    assert_set_attr_str(&mut root, container, "width", "60px");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);
    let baseline_intrinsic = assert_text_intrinsic(&root, text);

    root.set_text_content(text, "Alpha")
        .expect("text content shrink update should succeed");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let shrunk_layout = assert_node_layout(&root, text);
    let shrunk_snapshot = root
        .node(text)
        .expect("text node should exist after shrinking text")
        .snapshot();

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline longer text should start wrapped under the constrained width",
    );
    assert_close(
        shrunk_layout.height_px,
        baseline_intrinsic.line_height_px,
        "shorter replacement text should shrink back to one line",
    );
    assert_close(
        shrunk_snapshot.layout.available_width_px,
        60.0,
        "shrunk text snapshot available width",
    );
    assert_close(
        shrunk_snapshot.layout.used_width_px,
        60.0,
        "shrunk text snapshot used width",
    );
}

#[test]
// Blink and Gecko treat `line-height` as an inherited text measurement input.
// The retained path should therefore carry an ancestor-authored line-height
// into descendant text intrinsic metrics and one-line used height.
fn inherited_line_height_updates_descendant_text_intrinsic_and_used_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");

    assert_set_attr_str(&mut root, container, "width", "180px");
    assert_set_attr_str(&mut root, container, "line-height", "2");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let row_layout = assert_node_layout(&root, row);
    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited line-height update")
        .snapshot();

    assert_close(
        text_intrinsic.line_height_px,
        28.0,
        "inherited line-height should update descendant text intrinsic line-height",
    );
    assert_close(
        row_layout.height_px,
        text_intrinsic.line_height_px,
        "one-line row height should track inherited text line-height",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "one-line text height should track inherited text line-height",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        180.0,
        "inherited line-height should not change the text width contract",
    );
}

#[test]
// Blink and Gecko treat `white-space` as inherited text layout policy. A
// descendant text node under an ancestor-authored `nowrap` should therefore
// keep one-line used height even when the containing block is narrower than
// the text run.
fn inherited_white_space_nowrap_keeps_descendant_single_line_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, container, "white-space", "nowrap");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited nowrap update")
        .snapshot();

    assert_close(
        text_layout.height_px,
        baseline_intrinsic.line_height_px,
        "inherited nowrap should keep descendant text on one overflowing line",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "inherited nowrap should keep the descendant width contract",
    );
}

#[test]
// Blink and Gecko remeasure descendant text after a live inherited
// `white-space` change. Updating an ancestor from the default wrap policy to
// `nowrap` should therefore collapse a previously wrapped descendant back to a
// single overflowing line without changing the width contract.
fn live_inherited_white_space_nowrap_reduces_descendant_used_height_to_one_line() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "white-space", "nowrap");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let nowrap_layout = assert_node_layout(&root, text);
    let nowrap_snapshot = root
        .node(text)
        .expect("text node should exist after live inherited nowrap update")
        .snapshot();

    assert!(
        baseline_layout.height_px > baseline_intrinsic.line_height_px,
        "baseline descendant text should wrap under the constrained width",
    );
    assert_close(
        nowrap_layout.height_px,
        baseline_intrinsic.line_height_px,
        "live inherited nowrap should shrink descendant text back to one line",
    );
    assert_close(
        nowrap_snapshot.layout.available_width_px,
        constrained_width_px,
        "live inherited nowrap should keep the descendant width contract",
    );
}

#[test]
// Blink NGLineBreaker and Gecko nsTextFrame both honor inherited
// `overflow-wrap:anywhere` when shaping descendant text. The retained path
// should therefore rewrap a descendant unbreakable word under a constrained
// width even when the policy is authored on an ancestor.
fn inherited_overflow_wrap_anywhere_rewraps_descendant_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, container, "overflow-wrap", "anywhere");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited anywhere update")
        .snapshot();

    assert!(
        text_layout.height_px > baseline_intrinsic.line_height_px,
        "inherited overflow-wrap:anywhere should rewrap descendant used height",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "inherited anywhere should keep the descendant width contract",
    );
}

#[test]
// Persisted stylesheet `overflow-wrap:anywhere` on an ancestor should stay
// available through inheritance and still rewrap the descendant text after a
// later style recompute.
fn stylesheet_inherited_overflow_wrap_anywhere_rewraps_descendant_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .anywhere-container {
                overflow-wrap: anywhere;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(&mut root, container, "class", "anywhere-container");
    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_layout = assert_node_layout(&root, text);
    let first_snapshot = root
        .node(text)
        .expect("text node should exist after stylesheet inherited anywhere update")
        .snapshot();

    assert_set_attr_str(&mut root, container, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_layout = assert_node_layout(&root, text);

    assert!(
        first_layout.height_px > baseline_intrinsic.line_height_px,
        "stylesheet inherited anywhere should rewrap descendant used height",
    );
    assert_close(
        first_snapshot.layout.available_width_px,
        constrained_width_px,
        "stylesheet inherited anywhere should keep the descendant width contract",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet inherited anywhere height after recompute",
    );
}

#[test]
// Removing an ancestor stylesheet class should also restore the baseline
// inherited wrap policy. The descendant should fall back from inherited
// `overflow-wrap:anywhere` to the one-line overflowing contract.
fn removing_stylesheet_inherited_overflow_wrap_anywhere_restores_baseline_wrap_policy() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .anywhere-container {
                overflow-wrap: anywhere;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(&mut root, container, "class", "anywhere-container");
    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let anywhere_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "class", "unset");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let fallback_layout = assert_node_layout(&root, text);

    assert!(
        anywhere_layout.height_px > baseline_intrinsic.line_height_px,
        "stylesheet inherited anywhere should wrap before the class is removed",
    );
    assert_close(
        fallback_layout.height_px,
        baseline_intrinsic.line_height_px,
        "removing the inherited anywhere class should restore the one-line baseline contract",
    );
}

#[test]
// Blink and Gecko remeasure descendant text after a live inherited
// `overflow-wrap:anywhere` change. Updating an ancestor should therefore
// rewrap a previously overflowing descendant word under the same constrained
// width contract.
fn live_inherited_overflow_wrap_anywhere_rewraps_descendant_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "overflow-wrap", "anywhere");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let anywhere_layout = assert_node_layout(&root, text);
    let anywhere_snapshot = root
        .node(text)
        .expect("text node should exist after live inherited anywhere update")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        baseline_intrinsic.line_height_px,
        "baseline descendant word should stay on one overflowing line before inherited anywhere",
    );
    assert!(
        anywhere_layout.height_px > baseline_layout.height_px,
        "live inherited anywhere should rewrap descendant used height",
    );
    assert_close(
        anywhere_snapshot.layout.available_width_px,
        constrained_width_px,
        "live inherited anywhere should keep the descendant width contract",
    );
}

#[test]
// Blink NGLineBreaker and Gecko nsTextFrame both keep inherited
// `overflow-wrap:break-word` off the min-content path while still allowing
// emergency fallback wrapping during layout overflow. The retained path should
// therefore grow descendant used height under constraint without changing the
// baseline min-content contract.
fn inherited_overflow_wrap_break_word_rewraps_descendant_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, container, "overflow-wrap", "break-word");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited break-word update")
        .snapshot();

    assert_close(
        text_intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
        "inherited break-word should keep descendant min-content width on the normal path",
    );
    assert!(
        text_layout.height_px > baseline_intrinsic.line_height_px,
        "inherited break-word should rewrap descendant used height under constraint",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "inherited break-word should keep the descendant width contract",
    );
}

#[test]
// Persisted stylesheet `overflow-wrap:break-word` on an ancestor should stay
// available through inheritance and keep the descendant min-content width on
// the normal path while still rewrapping used height after recompute.
fn stylesheet_inherited_overflow_wrap_break_word_rewraps_descendant_after_recompute() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        r"
            .break-word-container {
                overflow-wrap: break-word;
            }
        ",
    );

    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(&mut root, container, "class", "break-word-container");
    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let first_intrinsic = assert_text_intrinsic(&root, text);
    let first_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "color", "#ffffff");
    assert_root_dom_update(&mut world, &mut root, 1.0, DomPointerUpdate::Unchanged);

    let second_intrinsic = assert_text_intrinsic(&root, text);
    let second_layout = assert_node_layout(&root, text);

    assert_close(
        first_intrinsic.min_content_width_px,
        baseline_intrinsic.min_content_width_px,
        "stylesheet inherited break-word should keep descendant min-content width on the normal path",
    );
    assert!(
        first_layout.height_px > baseline_intrinsic.line_height_px,
        "stylesheet inherited break-word should rewrap descendant used height",
    );
    assert_close(
        second_intrinsic.min_content_width_px,
        first_intrinsic.min_content_width_px,
        "stylesheet inherited break-word min-content width after recompute",
    );
    assert_close(
        second_layout.height_px,
        first_layout.height_px,
        "stylesheet inherited break-word height after recompute",
    );
}

#[test]
// Blink and Gecko treat `word-break` as inherited text break policy. A
// descendant unbreakable ASCII word under ancestor-authored `break-all` should
// therefore wrap by glyph under constrained width on the retained path.
fn inherited_word_break_break_all_rewraps_descendant_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, container, "word-break", "break-all");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_snapshot = root
        .node(text)
        .expect("text node should exist after inherited break-all update")
        .snapshot();

    assert!(
        text_layout.height_px > baseline_intrinsic.line_height_px,
        "inherited word-break:break-all should rewrap descendant used height",
    );
    assert_close(
        text_snapshot.layout.available_width_px,
        constrained_width_px,
        "inherited break-all should keep the descendant width contract",
    );
}

#[test]
// Blink and Gecko remeasure descendant text after a live inherited
// `word-break:break-all` change. Updating an ancestor should therefore rewrap a
// previously overflowing descendant word under the same constrained width
// contract.
fn live_inherited_word_break_break_all_rewraps_descendant_used_height_under_constraint() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "AlphaBeta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "word-break", "break-all");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let break_all_layout = assert_node_layout(&root, text);
    let break_all_snapshot = root
        .node(text)
        .expect("text node should exist after live inherited break-all update")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        baseline_intrinsic.line_height_px,
        "baseline descendant word should stay on one overflowing line before inherited break-all",
    );
    assert!(
        break_all_layout.height_px > baseline_layout.height_px,
        "live inherited break-all should rewrap descendant used height",
    );
    assert_close(
        break_all_snapshot.layout.available_width_px,
        constrained_width_px,
        "live inherited break-all should keep the descendant width contract",
    );
}

#[test]
// Blink and Gecko remeasure descendant text after a live inherited
// `line-height` change. Updating an ancestor should therefore rescale the
// descendant wrapped used height without changing the available inline width.
fn live_inherited_line_height_change_scales_descendant_wrapped_height_without_changing_available_width()
 {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, container, "line-height", "2");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let tall_layout = assert_node_layout(&root, text);
    let tall_intrinsic = assert_text_intrinsic(&root, text);
    let tall_snapshot = root
        .node(text)
        .expect("text node should exist after live inherited line-height change")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        baseline_intrinsic.line_height_px * 2.0,
        "baseline descendant text should wrap to exactly two lines",
    );
    assert_close(
        tall_intrinsic.line_height_px,
        28.0,
        "live inherited line-height should update descendant text intrinsic line-height",
    );
    assert_close(
        tall_layout.height_px,
        tall_intrinsic.line_height_px * 2.0,
        "live inherited line-height should scale descendant wrapped height",
    );
    assert_close(
        tall_snapshot.layout.available_width_px,
        constrained_width_px,
        "live inherited line-height should keep the descendant width contract",
    );
}

#[test]
// Blink and Gecko apply inherited `white-space:pre` by preserving repeated
// spaces and disabling soft wrapping for descendant text. The retained path
// should therefore widen descendant max-content width and keep one-line used
// height even under a narrow container.
fn inherited_white_space_pre_preserves_spaces_and_disables_soft_wrap() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha    Beta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);

    assert_set_attr_str(&mut root, container, "width", "54px");
    assert_set_attr_str(&mut root, container, "white-space", "pre");
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let text_layout = assert_node_layout(&root, text);
    let text_intrinsic = assert_text_intrinsic(&root, text);

    assert!(
        text_intrinsic.max_content_width_px > baseline_intrinsic.max_content_width_px,
        "inherited pre should preserve repeated-space advance width",
    );
    assert_close(
        text_layout.height_px,
        text_intrinsic.line_height_px,
        "inherited pre should disable descendant soft wrapping",
    );
}

#[test]
// Blink and Gecko remeasure wrapped text after a live `line-height` change
// without changing the available inline width. The retained path should keep
// the same two-line wrap while scaling used height to the new line box size.
fn live_line_height_change_scales_wrapped_text_height_without_changing_available_width() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let row = assert_append_button_to(&mut root, container);
    let text = assert_append_text_to(&mut root, row, "Alpha Beta");
    let baseline_intrinsic = assert_text_intrinsic(&root, text);
    let constrained_width_px = (baseline_intrinsic.min_content_width_px - 1.0).max(1.0);

    assert_set_attr_str(
        &mut root,
        container,
        "width",
        &format!("{constrained_width_px}px"),
    );
    assert_set_attr_str(&mut root, row, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_layout = assert_node_layout(&root, text);

    assert_set_attr_str(&mut root, row, "line-height", "2");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let tall_layout = assert_node_layout(&root, text);
    let tall_intrinsic = assert_text_intrinsic(&root, text);
    let tall_snapshot = root
        .node(text)
        .expect("text node should exist after live line-height change")
        .snapshot();

    assert_close(
        baseline_layout.height_px,
        baseline_intrinsic.line_height_px * 2.0,
        "baseline constrained text should wrap to exactly two lines",
    );
    assert_close(
        tall_intrinsic.line_height_px,
        28.0,
        "live line-height change should update text intrinsic line-height",
    );
    assert_close(
        tall_layout.height_px,
        tall_intrinsic.line_height_px * 2.0,
        "live line-height change should scale the wrapped two-line used height",
    );
    assert_close(
        tall_snapshot.layout.available_width_px,
        constrained_width_px,
        "live line-height change should keep the text available-width contract",
    );
}

fn assert_menu_like_popup_style(
    root: &mut ui_dom::RootDom,
    popup: ui_dom::DomNodeSlotKey,
    content: ui_dom::DomNodeSlotKey,
) {
    assert_set_attr_str(root, popup, "position", "fixed");
    assert_set_attr_str(root, popup, "left", MENU_POPUP_LEFT_PX);
    assert_set_attr_str(root, popup, "top", MENU_POPUP_TOP_PX);
    assert_set_attr_str(root, popup, "width", MENU_POPUP_WIDTH_PX);
    assert_set_attr_str(root, popup, "max-height", MENU_POPUP_MAX_HEIGHT_PX);
    assert_set_attr_str(root, popup, "overflow", "scroll");
    assert_set_attr_str(root, content, "width", MENU_CONTENT_WIDTH_PX);
    assert_set_attr_str(root, content, "padding-left", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-right", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-top", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-bottom", MENU_CONTENT_PADDING_PX);
}

fn assert_menu_like_row_style(root: &mut ui_dom::RootDom, row: ui_dom::DomNodeSlotKey) {
    assert_set_attr_str(root, row, "display", "block");
    assert_set_attr_str(root, row, "margin-left", ROW_MARGIN_LEFT_RIGHT_PX);
    assert_set_attr_str(root, row, "margin-right", ROW_MARGIN_LEFT_RIGHT_PX);
    assert_set_attr_str(root, row, "margin-top", ROW_MARGIN_TOP_BOTTOM_PX);
    assert_set_attr_str(root, row, "margin-bottom", ROW_MARGIN_TOP_BOTTOM_PX);
    assert_set_attr_str(root, row, "padding-left", ROW_PADDING_LEFT_RIGHT_PX);
    assert_set_attr_str(root, row, "padding-right", ROW_PADDING_LEFT_RIGHT_PX);
    assert_set_attr_str(root, row, "padding-top", ROW_PADDING_TOP_BOTTOM_PX);
    assert_set_attr_str(root, row, "padding-bottom", ROW_PADDING_TOP_BOTTOM_PX);
}

fn assert_text_intrinsic(
    root: &ui_dom::RootDom,
    text: ui_dom::DomNodeSlotKey,
) -> ui_dom::TextIntrinsic {
    match root.node(text).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("text auto-size proof node should keep text intrinsic state"),
    }
}
