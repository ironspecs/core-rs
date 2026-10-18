#![allow(rustdoc::bare_urls)]

//! Replaced-element layout proofs.
//!
//! Spec references:
//! - CSS 2.1 10.6.2 replaced element height from the intrinsic ratio:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-replaced-height>
//! - CSS 2.1 10.3.2 replaced element width from the intrinsic ratio:
//!   <https://www.w3.org/TR/CSS21/visudet.html#inline-replaced-width>
//! - CSS Images 3 5.3 default object size (300x150) for replaced content
//!   with no intrinsic dimensions:
//!   <https://www.w3.org/TR/css-images-3/#default-object-size>
//!
//! Browser-backed subset pinned here: only the aspect-ratio transfer math of
//! CSS 2.1 10.3.2/10.6.2 (one authored dimension resolves through the
//! element's intrinsic ratio). No WPT or browser test covers the placeholder
//! ratio itself: a real `img` with no intrinsic dimensions falls back to the
//! 300x150 default object size (CSS Images 3 5.3, `LayoutReplaced::kDefaultWidth`
//! in Blink's `layout_replaced.cc` and the `GetIntrinsicRatio` image handling
//! in Gecko's `nsImageFrame.cpp`), while `ui-dom`
//! deliberately seeds its unloaded `img` placeholder with a 16:9 intrinsic
//! ratio as a crate-owned invariant. The cited CSS 2.1 sections govern the
//! transfer formulas only; they do not govern the 16:9 ratio.

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_close, assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    assert_style_set, img_under_document_setup,
};

#[test]
// Browser-backed: CSS 2.1 10.6.2 transfers an authored width through the
// element's intrinsic ratio. Crate-owned: the placeholder's 16:9 ratio (see
// module header), which a browser would replace with the 300x150 default
// object size for an image with no intrinsic dimensions.
fn img_with_only_width_uses_intrinsic_aspect_ratio_for_height() {
    let (mut world, mut root, img) = img_under_document_setup();

    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_style_set(&mut root, img, "width", "320px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let layout = assert_node_layout(&root, img);

    assert_close(layout.width_px, 320.0, "img used width");
    assert_close(layout.height_px, 180.0, "img used height");
}

#[test]
// Browser-backed: CSS 2.1 10.3.2 transfers an authored height through the
// element's intrinsic ratio. Crate-owned: the placeholder's 16:9 ratio (see
// module header).
fn img_with_only_height_uses_intrinsic_aspect_ratio_for_width() {
    let (mut world, mut root, img) = img_under_document_setup();

    assert_set_attr_str(&mut root, img, "src", "memory://a.png");
    assert_style_set(&mut root, img, "height", "45px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let layout = assert_node_layout(&root, img);

    assert_close(layout.width_px, 80.0, "img used width");
    assert_close(layout.height_px, 45.0, "img used height");
}
