#![allow(rustdoc::bare_urls)]

//! Replaced-element layout proofs.
//!
//! Spec references:
//! - CSS 2.1 10.6.2 replaced elements in normal flow:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-replaced-height>
//! - CSS 2.1 10.3.2 replaced elements in normal flow:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-replaced-width>

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_close, assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    img_under_document_setup,
};

#[test]
// ui-dom invariant: placeholder img intrinsic ratio transfers authored width
// into used height during the real update path.
fn img_with_only_width_uses_intrinsic_aspect_ratio_for_height() {
    let (mut world, mut root, img) = img_under_document_setup();

    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_set_attr_str(&mut root, img, "width", "320px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let layout = assert_node_layout(&root, img);

    assert_close(layout.width_px, 320.0, "img used width");
    assert_close(layout.height_px, 180.0, "img used height");
}

#[test]
// ui-dom invariant: placeholder img intrinsic ratio transfers authored height
// into used width during the real update path.
fn img_with_only_height_uses_intrinsic_aspect_ratio_for_width() {
    let (mut world, mut root, img) = img_under_document_setup();

    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_set_attr_str(&mut root, img, "height", "45px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let layout = assert_node_layout(&root, img);

    assert_close(layout.width_px, 80.0, "img used width");
    assert_close(layout.height_px, 45.0, "img used height");
}
