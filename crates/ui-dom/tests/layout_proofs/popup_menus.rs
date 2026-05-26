#![allow(rustdoc::bare_urls)]

//! Popup-menu layout proofs.
//!
//! Spec references:
//! - CSS 2.1 10.6.3 block formatting context height calculation:
//!   <https://www.w3.org/TR/CSS21/visudet.html#normal-block>
//! - CSS 2.1 10.7 minimum and maximum heights:
//!   <https://www.w3.org/TR/CSS21/visudet.html#min-max-heights>
//! - CSS 2.1 11.1.1 overflow:
//!   <https://www.w3.org/TR/CSS21/visufx.html#overflow>

use ui_dom::DomPointerUpdate;

use crate::layout_proofs::support::{
    assert_close, assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    fixed_layer_with_absolute_popup_menu_setup, fixed_popup_menu_with_items_setup,
};

const POPUP_TOP_PX: &str = "12px";
const POPUP_LEFT_PX: &str = "16px";
const POPUP_WIDTH_PX: &str = "120px";
const POPUP_MAX_HEIGHT_PX: &str = "50px";
const POPUP_ITEM_HEIGHT_PX: &str = "20px";

#[test]
// Popup menu invariant: auto-height should match the sum of menu items when
// the content stays below the authored max-height.
fn fixed_popup_menu_auto_height_matches_items_below_max_height() {
    let (mut world, mut root, popup, items) = fixed_popup_menu_with_items_setup(2);

    assert_fixed_popup_menu_style(&mut root, popup);
    assert_popup_menu_item_sizes(&mut root, &items);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let popup_layout = assert_node_layout(&root, popup);

    assert_close(popup_layout.height_px, 40.0, "fixed popup menu height");
    assert_close(
        popup_layout.content_height_px,
        40.0,
        "fixed popup menu content height",
    );
}

#[test]
// Popup menu invariant: max-height should clamp the popup viewport height
// while retained content height still tracks all menu items for scrolling.
fn fixed_popup_menu_clamps_to_max_height_and_retains_item_height() {
    let (mut world, mut root, popup, items) = fixed_popup_menu_with_items_setup(3);

    assert_fixed_popup_menu_style(&mut root, popup);
    assert_popup_menu_item_sizes(&mut root, &items);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let popup_layout = assert_node_layout(&root, popup);

    assert_close(popup_layout.height_px, 50.0, "fixed popup menu height");
    assert_close(
        popup_layout.content_height_px,
        60.0,
        "fixed popup menu content height",
    );
}

#[test]
// Popup menu invariant: absolutely positioned popup menus should use the same
// auto-height plus max-height behavior as fixed menus when they are positioned
// against a screen-sized containing layer.
fn absolute_popup_menu_clamps_to_max_height_and_retains_item_height() {
    let (mut world, mut root, fixed_layer, popup, items) =
        fixed_layer_with_absolute_popup_menu_setup(3);

    assert_set_attr_str(&mut root, fixed_layer, "position", "fixed");
    assert_set_attr_str(&mut root, fixed_layer, "top", "0px");
    assert_set_attr_str(&mut root, fixed_layer, "left", "0px");
    assert_set_attr_str(&mut root, fixed_layer, "right", "0px");
    assert_set_attr_str(&mut root, fixed_layer, "bottom", "0px");
    assert_absolute_popup_menu_style(&mut root, popup);
    assert_popup_menu_item_sizes(&mut root, &items);

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let popup_layout = assert_node_layout(&root, popup);

    assert_close(popup_layout.height_px, 50.0, "absolute popup menu height");
    assert_close(
        popup_layout.content_height_px,
        60.0,
        "absolute popup menu content height",
    );
}

fn assert_fixed_popup_menu_style(root: &mut ui_dom::RootDom, popup: ui_dom::DomNodeSlotKey) {
    assert_set_attr_str(root, popup, "position", "fixed");
    assert_positioned_popup_menu_style(root, popup);
}

fn assert_absolute_popup_menu_style(root: &mut ui_dom::RootDom, popup: ui_dom::DomNodeSlotKey) {
    assert_set_attr_str(root, popup, "position", "absolute");
    assert_positioned_popup_menu_style(root, popup);
}

fn assert_positioned_popup_menu_style(root: &mut ui_dom::RootDom, popup: ui_dom::DomNodeSlotKey) {
    assert_set_attr_str(root, popup, "top", POPUP_TOP_PX);
    assert_set_attr_str(root, popup, "left", POPUP_LEFT_PX);
    assert_set_attr_str(root, popup, "width", POPUP_WIDTH_PX);
    assert_set_attr_str(root, popup, "overflow", "scroll");
    assert_set_attr_str(root, popup, "max-height", POPUP_MAX_HEIGHT_PX);
}

fn assert_popup_menu_item_sizes(root: &mut ui_dom::RootDom, items: &[ui_dom::DomNodeSlotKey]) {
    for &item in items {
        assert_set_attr_str(root, item, "width", POPUP_WIDTH_PX);
        assert_set_attr_str(root, item, "height", POPUP_ITEM_HEIGHT_PX);
    }
}
