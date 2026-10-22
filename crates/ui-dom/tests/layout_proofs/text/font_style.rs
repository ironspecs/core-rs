//! Font-style consumption proofs (text seam S2): the `font-family` list
//! grammar's wire round-trip and the `font-weight` → cosmic attrs mapping.
//!
//! These pins witness the S2 surface the off-spec registry rows' rewrite relies
//! on: the parsed family list serializes per CSS rules (quoted names re-quoted,
//! generics bare, malformed lists drop + count), and a resolved weight reaches
//! the shaper's attrs (700 vs 400 select different variable-font instances, so
//! the measured width differs).
//!
//! Text intrinsics for a fixed font are bit-identical, so the width-difference
//! assertions compare exact `f32` literals (deliberate exact checks).
#![allow(clippy::float_cmp)]

use ui_dom::{DomIntrinsic, DomNode, DomNodeSlotKey, DomPointerUpdate, RootDom};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_text_to, assert_decl_drop, assert_load_root_stylesheet,
    assert_root_dom_update, assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    theme_stylesheet_origin,
};

/// Clones one text node's retained intrinsic metrics.
fn text_intrinsic(root: &RootDom, key: DomNodeSlotKey) -> ui_dom::TextIntrinsic {
    match root.node(key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("font-style proof text node should keep text intrinsic state"),
    }
}

#[test]
fn font_family_list_parses_quoted_and_generic() {
    // Comma-split with a quoted name (re-quoted on the wire), a generic
    // (emitted bare), and an unquoted name.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { font-family: \"My Font\", sans-serif, ui-sans }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").font_family(),
        Some("\"My Font\", sans-serif, ui-sans".to_string())
    );

    // Single-quoted names strip and re-quote identically.
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let target = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, target, "class", "x");
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".x { font-family: 'My Font' }",
    );
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    assert_eq!(
        root.computed_style(target).expect("target").font_family(),
        Some("\"My Font\"".to_string())
    );

    // Malformed lists drop + count: empty list, empty item, unterminated quote.
    assert_decl_drop(".x { font-family: , }", 0, 1);
    assert_decl_drop(".x { font-family: Arial,, sans-serif }", 0, 1);
    assert_decl_drop(".x { font-family: \"Arial }", 0, 1);
}

#[test]
fn font_weight_maps_to_cosmic_attrs() {
    // 700 vs the 400 initial selects a different variable-font axis instance →
    // distinct measured widths (the attrs weight reaches the shaper).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), ".w { font-weight: 700 }");
    let document_root = root.document_node().0;
    let plain = assert_append_div_to(&mut root, document_root);
    let plain_text = assert_append_text_to(&mut root, plain, "Hello world");
    let bold = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, bold, "class", "w");
    let bold_text = assert_append_text_to(&mut root, bold, "Hello world");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let plain = text_intrinsic(&root, plain_text);
    let bold = text_intrinsic(&root, bold_text);
    assert!(
        plain.min_content_width_px != bold.min_content_width_px
            || plain.max_content_width_px != bold.max_content_width_px,
        "font-weight 700 should change the measured intrinsic width (attrs weight)"
    );
}

#[test]
fn font_family_weight_style_flow_to_the_request() {
    // A styled node's resolved family/weight/style reach the request: the three
    // longhands resolve in the computed group, and the combined
    // family+weight consumption changes the measured width (the request
    // carries them into the shaper).
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(
        &root,
        theme_stylesheet_origin(),
        ".styled { font-family: Source Han Sans; font-weight: bold; font-style: italic }",
    );
    let document_root = root.document_node().0;
    let plain = assert_append_div_to(&mut root, document_root);
    let plain_text = assert_append_text_to(&mut root, plain, "Hello world");
    let styled = assert_append_div_to(&mut root, document_root);
    assert_set_attr_str(&mut root, styled, "class", "styled");
    let styled_text = assert_append_text_to(&mut root, styled, "Hello world");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let view = root.computed_style(styled).expect("styled container");
    assert_eq!(view.font_family(), Some("\"Source Han Sans\"".to_string()));
    assert_eq!(view.as_font().weight, 700);
    assert_eq!(view.as_font().style, ui_dom::Keyword::Italic);

    let plain = text_intrinsic(&root, plain_text);
    let styled = text_intrinsic(&root, styled_text);
    assert!(
        plain.min_content_width_px != styled.min_content_width_px
            || plain.max_content_width_px != styled.max_content_width_px,
        "the resolved family+weight should change the measured intrinsic width"
    );
}
