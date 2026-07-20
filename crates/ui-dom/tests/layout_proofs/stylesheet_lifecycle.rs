#![allow(rustdoc::bare_urls)]

//! Stylesheet lifecycle layout proofs.
//!
//! These proofs pin the frame-pipeline ordering invariant that style
//! resolution for newly inserted or restyled content runs before that
//! content's first layout and paint, for every stylesheet/DOM ordering a
//! consumer can produce. The claimed behavior is the browser one:
//!
//! - HTML Standard, "update the rendering": style and layout run as ordered
//!   steps of one rendering update, so newly inserted content is never
//!   presented with unresolved style:
//!   <https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>
//! - Gecko flushes pending restyles before reflow from the same notification
//!   flush (`RestyleManager::ProcessPendingRestyles` in
//!   `layout/base/RestyleManager.cpp`, called from `PresShell` flush in
//!   `layout/base/PresShell.cpp`).
//! - Blink orders the lifecycle identically: `Document::UpdateStyleAndLayout`
//!   runs style recalc before layout (`core/dom/document.cc`,
//!   `core/dom/document_lifecycle.cc`, and
//!   `core/css/style_recalc/style_recalc.cc` for the recalc itself).
//!
//! No single WPT file isolates these engine-internal scheduling guarantees;
//! the browser-backed subset pinned here is the externally observable half:
//! retained layout after the first update reflects the stylesheet for every
//! load/attach/append ordering, without any later interaction trigger.

use ui_dom::{DomPointerUpdate, DomTag, RootDomContext, RootDomFragment};

use crate::layout_proofs::support::{
    assert_close, assert_load_root_stylesheet, assert_node_layout, assert_root_dom_update,
    assert_set_attr_str, attached_gpu_root_dom_default_viewport_setup,
    attached_gpu_root_dom_setup_with_context, theme_stylesheet_origin,
};

const CHIP_CSS: &str = ".chip { width: 100px; height: 40px; }";
const CHIP_WIDTH_PX: f32 = 100.0;

// A stylesheet loaded before `RootDom::new` must still style content that is
// appended after attach: the append marks the enclosing layer root
// style-dirty, so the very next update resolves style before layout.
#[test]
fn stylesheet_loaded_before_root_construction_applies_on_first_update_after_append() {
    let context = RootDomContext::new_gpu();
    context
        .load_stylesheet(theme_stylesheet_origin(), CHIP_CSS)
        .expect("stylesheet should load");
    let (mut world, mut root) = attached_gpu_root_dom_setup_with_context(context, 320.0, 320.0);

    let document_root = root.document_node().0;
    let chip = root.create_element(DomTag::Div).expect("div should insert");
    // The class is authored before the append on purpose: setting a
    // style-affecting attribute on an already-connected node marks the layer
    // root style-dirty through the attr path, which would mask the insertion
    // invalidation this proof pins.
    assert_set_attr_str(&mut root, chip, "class", "chip");
    root.append_child(document_root, chip)
        .expect("div should append");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let chip_layout = assert_node_layout(&root, chip);
    assert_close(
        chip_layout.width_px,
        CHIP_WIDTH_PX,
        "pre-construction stylesheet width must apply on the first update after append",
    );
}

// The reported first-render bug: attach, present one frame, then append
// content and update. The appended content must come up styled, not fall
// back to unstyled auto sizing until a later hover or attr change.
#[test]
fn appended_content_is_styled_on_the_update_after_append() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), CHIP_CSS);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let document_root = root.document_node().0;
    let chip = root.create_element(DomTag::Div).expect("div should insert");
    // Class authored before append; see the masking note in
    // `stylesheet_loaded_before_root_construction_applies_on_first_update_after_append`.
    assert_set_attr_str(&mut root, chip, "class", "chip");
    root.append_child(document_root, chip)
        .expect("div should append");
    assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    let chip_layout = assert_node_layout(&root, chip);
    assert_close(
        chip_layout.width_px,
        CHIP_WIDTH_PX,
        "content appended after the first presented frame must be styled on its first update",
    );
}

// Fragment imports take the same insertion invalidation path: imported
// content must be restyled (and repainted, via the layer-root repaint the
// import now propagates) on the update after `append_fragment`.
#[test]
fn fragment_imported_content_is_styled_on_the_update_after_import() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    assert_load_root_stylesheet(&root, theme_stylesheet_origin(), CHIP_CSS);
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let mut fragment = RootDomFragment::new();
    let fragment_root = fragment.document_fragment_key();
    let chip = fragment
        .create_element(DomTag::Div)
        .expect("div should insert");
    fragment
        .append_child(fragment_root, chip)
        .expect("div should append");
    fragment
        .set_attr_str(chip, "class", "chip")
        .expect("class should apply");

    let document_root = root.document_node().0;
    root.append_fragment(document_root, fragment)
        .expect("fragment should import");
    assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    let chip_layout = assert_node_layout(&root, chip);
    assert_close(
        chip_layout.width_px,
        CHIP_WIDTH_PX,
        "fragment-imported content must be styled on its first update",
    );
}

// Negative ordering: a stylesheet removed before `RootDom::new` must stay
// absent, so appended content keeps unstyled auto sizing instead of
// resurrecting unloaded rules.
#[test]
fn stylesheet_unloaded_before_root_construction_stays_unapplied() {
    let context = RootDomContext::new_gpu();
    context
        .load_stylesheet(theme_stylesheet_origin(), CHIP_CSS)
        .expect("stylesheet should load");
    let unloaded_rule_count = context.unload_stylesheet(&theme_stylesheet_origin().key);
    assert_eq!(
        unloaded_rule_count, 1,
        "the theme stylesheet should unload its one rule"
    );
    let (mut world, mut root) = attached_gpu_root_dom_setup_with_context(context, 320.0, 320.0);

    let document_root = root.document_node().0;
    let chip = root.create_element(DomTag::Div).expect("div should insert");
    assert_set_attr_str(&mut root, chip, "class", "chip");
    root.append_child(document_root, chip)
        .expect("div should append");
    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let chip_layout = assert_node_layout(&root, chip);
    assert_close(
        chip_layout.width_px,
        320.0,
        "a stylesheet unloaded before root construction must not style appended content",
    );
}
