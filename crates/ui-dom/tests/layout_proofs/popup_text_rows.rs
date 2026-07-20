#![allow(rustdoc::bare_urls)]

//! Popup-menu proofs for text-driven rows.
//!
//! Spec references:
//! - CSS 2.2 10.6.6: block roots with non-visible overflow route auto-height
//!   through block formatting context root sizing.
//!   <https://www.w3.org/TR/CSS22/visudet.html#block-root-margin>
//! - CSS 2.2 10.6.7: auto height for block formatting context roots.
//!   <https://www.w3.org/TR/CSS22/visudet.html#root-height>
//! - CSS 2.2 10.7: `max-height` clamps only after the tentative used height is
//!   computed.
//!   <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
//! - CSS 2.1 conformance tests for `max-height` with overflow separate the
//!   below-max and overflowing cases.
//!   <https://www.w3.org/Style/CSS/Test/CSS2.1/20100127/html4/max-height-108.htm>
//!   <https://www.w3.org/Style/CSS/Test/CSS2.1/20100127/html4/max-height-109.htm>

use ui_dom::{
    DomAttribute, DomAttributeNode, DomAttributeValue, DomIntrinsic, DomNode, DomNodeSlotKey,
    DomPointerUpdate, RootDom, node_box,
};

use crate::layout_proofs::support::{
    assert_append_button_to, assert_append_div_to, assert_append_text_to, assert_close,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup,
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

struct PopupTextRowsScenario {
    world: bevy::prelude::World,
    root: RootDom,
    popup: DomNodeSlotKey,
    content: DomNodeSlotKey,
    row_keys: Vec<DomNodeSlotKey>,
    text_keys: Vec<DomNodeSlotKey>,
}

#[test]
// CSS 2.1 10.6.3 (normal-block auto height) and 10.7 (#min-max-heights):
// while content height stays under `max-height`, the used height is the
// content height and the clamp must not engage.
fn fixed_popup_text_rows_keep_auto_height_below_max_height() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6",
    ];
    let mut scenario = fixed_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);
    let popup_node = scenario
        .root
        .node(scenario.popup)
        .expect("popup root should remain in the dom");

    assert!(
        expected_content_height < 160.0,
        "test oracle requires text rows to stay below the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        expected_content_height,
        "below-max text-row popup height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "below-max text-row popup content height",
    );
    let popup_element = popup_node
        .as_element()
        .expect("popup root should stay as an element");
    assert!(
        popup_element.attr(&DomAttribute::Height).is_none(),
        "text-row popup should rely on auto height rather than authored height",
    );
    assert_eq!(
        popup_element.attr(&DomAttribute::MaxHeight),
        Some(&DomAttributeValue::String(
            MENU_POPUP_MAX_HEIGHT_PX.to_owned()
        )),
    );
}

#[test]
// CSS 2.1 10.7 (#min-max-heights) and CSS2.1-suite `max-height-108.htm`:
// once the computed content height exceeds `max-height`, the used height is
// `max-height`, while the retained content height stays at the full extent
// for scroll measurement (CSS 2.1 11.1.1 #overflow).
fn fixed_popup_text_rows_clamp_only_after_content_exceeds_max_height() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6", "Scene 7",
        "Scene 8", "Scene 9",
    ];
    let mut scenario = fixed_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);

    assert!(
        expected_content_height > 160.0,
        "test oracle requires text rows to exceed the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        160.0,
        "overflowing text-row popup used height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "overflowing text-row popup content height",
    );
}

#[test]
// CSS 2.1 10.6.3 plus 10.7: auto height recomputes from live content after
// row removal, so the clamp disengages and the used height returns to the
// content height.
fn fixed_popup_text_rows_shrink_back_below_max_height_after_row_removal() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6", "Scene 7",
        "Scene 8", "Scene 9",
    ];
    let mut scenario = fixed_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let initial_popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    assert_close(
        initial_popup_layout.height_px,
        160.0,
        "overflowing text-row popup initial used height",
    );

    for &row in scenario.row_keys.iter().skip(7) {
        scenario
            .root
            .remove_node(row)
            .expect("text-row popup shrink proof should remove trailing rows");
    }

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        16.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let remaining_row_keys = &scenario.row_keys[..7];
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, remaining_row_keys);

    assert!(
        expected_content_height < 160.0,
        "test oracle requires the remaining text rows to fit below the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        expected_content_height,
        "text-row popup should shrink back to auto height after content falls below max-height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "text-row popup should keep live content height after row removal",
    );
}

#[test]
// CSS 2.1 10.6.3: with no in-flow children left, the auto content height
// collapses to zero and only the authored vertical padding remains.
fn fixed_popup_text_rows_shrink_to_padding_when_all_rows_are_removed() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6", "Scene 7",
        "Scene 8", "Scene 9",
    ];
    let mut scenario = fixed_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    for &row in &scenario.row_keys {
        scenario
            .root
            .remove_node(row)
            .expect("text-row popup shrink proof should remove every row");
    }

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        16.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &[]);

    assert_close(
        popup_layout.height_px,
        expected_content_height,
        "text-row popup should shrink to content padding after all rows are removed",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "text-row popup content height should shrink to content padding after all rows are removed",
    );
}

#[test]
// CSS 2.1 10.6.4 (absolutely positioned auto height) and 10.7: below
// `max-height`, an absolutely positioned popup sizes to its in-flow content.
fn absolute_popup_text_rows_keep_auto_height_below_max_height() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6",
    ];
    let mut scenario = absolute_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);

    assert!(
        expected_content_height < 160.0,
        "test oracle requires text rows to stay below the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        expected_content_height,
        "absolute text-row popup height below max-height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "absolute text-row popup content height below max-height",
    );
}

#[test]
// CSS 2.1 10.7 and CSS2.1-suite `max-height-109.htm`: growth that pushes
// content past `max-height` engages the clamp on the next layout pass.
fn fixed_popup_text_rows_clamp_after_growing_past_max_height() {
    let labels = [
        "Scene 0", "Scene 1", "Scene 2", "Scene 3", "Scene 4", "Scene 5", "Scene 6",
    ];
    let mut scenario = fixed_popup_text_rows_scenario(&labels);

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let initial_popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let initial_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);
    assert!(
        initial_content_height < 160.0,
        "test oracle requires the initial text rows to fit below the authored max-height; got {initial_content_height}",
    );
    assert_close(
        initial_popup_layout.height_px,
        initial_content_height,
        "initial text-row popup height before growth",
    );

    append_menu_like_text_row(
        &mut scenario.root,
        scenario.content,
        &mut scenario.row_keys,
        &mut scenario.text_keys,
        "Scene 7",
    );
    append_menu_like_text_row(
        &mut scenario.root,
        scenario.content,
        &mut scenario.row_keys,
        &mut scenario.text_keys,
        "Scene 8",
    );
    append_menu_like_text_row(
        &mut scenario.root,
        scenario.content,
        &mut scenario.row_keys,
        &mut scenario.text_keys,
        "Scene 9",
    );

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        16.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);

    assert!(
        expected_content_height > 160.0,
        "test oracle requires the grown text rows to exceed the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        160.0,
        "text-row popup should clamp after growth exceeds max-height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "text-row popup content height after growth exceeds max-height",
    );
}

#[test]
// CSS Text 3 soft wrapping plus CSS 2.1 10.7: wrapped rows grow the content
// height, and the resulting over-max content engages the `max-height` clamp.
fn wrapped_text_rows_can_drive_popup_clamp_height() {
    let labels = [
        "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
        "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
        "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
        "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
        "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
    ];
    let mut scenario = fixed_popup_text_rows_scenario_with_content_width(&labels, "36px");

    assert_root_dom_update(
        &mut scenario.world,
        &mut scenario.root,
        0.0,
        DomPointerUpdate::Unchanged,
    );

    let popup_layout = assert_node_layout(&scenario.root, scenario.popup);
    let first_row_layout = assert_node_layout(&scenario.root, scenario.row_keys[0]);
    let first_text_intrinsic = assert_text_intrinsic(&scenario.root, scenario.text_keys[0]);
    let expected_content_height =
        popup_content_height_from_boxes(&scenario.root, scenario.content, &scenario.row_keys);

    assert!(
        first_row_layout.height_px > first_text_intrinsic.line_height_px,
        "wrapped popup row should grow taller than one text line",
    );
    assert!(
        expected_content_height > 160.0,
        "test oracle requires wrapped rows to exceed the authored max-height; got {expected_content_height}",
    );
    assert_close(
        popup_layout.height_px,
        160.0,
        "wrapped text-row popup should clamp at max-height",
    );
    assert_close(
        popup_layout.content_height_px,
        expected_content_height,
        "wrapped text-row popup should retain full content height for scrolling",
    );
}

fn fixed_popup_text_rows_scenario(labels: &[&str]) -> PopupTextRowsScenario {
    popup_text_rows_scenario_with_position_and_content_width(labels, "fixed", MENU_CONTENT_WIDTH_PX)
}

fn absolute_popup_text_rows_scenario(labels: &[&str]) -> PopupTextRowsScenario {
    let mut scenario = popup_text_rows_scenario_with_position_and_content_width(
        labels,
        "absolute",
        MENU_CONTENT_WIDTH_PX,
    );
    let document_root = scenario.root.document_node().0;
    let fixed_layer = assert_append_div_to(&mut scenario.root, document_root);
    scenario
        .root
        .append_child(fixed_layer, scenario.popup)
        .expect("absolute popup should move under a fixed containing layer");
    assert_set_attr_str(&mut scenario.root, fixed_layer, "position", "fixed");
    assert_set_attr_str(&mut scenario.root, fixed_layer, "top", "0px");
    assert_set_attr_str(&mut scenario.root, fixed_layer, "left", "0px");
    assert_set_attr_str(&mut scenario.root, fixed_layer, "right", "0px");
    assert_set_attr_str(&mut scenario.root, fixed_layer, "bottom", "0px");
    scenario
}

fn fixed_popup_text_rows_scenario_with_content_width(
    labels: &[&str],
    content_width_px: &str,
) -> PopupTextRowsScenario {
    popup_text_rows_scenario_with_position_and_content_width(labels, "fixed", content_width_px)
}

fn popup_text_rows_scenario_with_position_and_content_width(
    labels: &[&str],
    popup_position: &str,
    content_width_px: &str,
) -> PopupTextRowsScenario {
    let (world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let popup = assert_append_div_to(&mut root, document_root);
    let content = assert_append_div_to(&mut root, popup);
    let mut row_keys = Vec::with_capacity(labels.len());
    let mut text_keys = Vec::with_capacity(labels.len());
    for &label in labels {
        append_menu_like_text_row(&mut root, content, &mut row_keys, &mut text_keys, label);
    }
    assert_menu_like_popup_style(&mut root, popup, content, popup_position, content_width_px);
    PopupTextRowsScenario {
        world,
        root,
        popup,
        content,
        row_keys,
        text_keys,
    }
}

fn append_menu_like_text_row(
    root: &mut RootDom,
    content: DomNodeSlotKey,
    row_keys: &mut Vec<DomNodeSlotKey>,
    text_keys: &mut Vec<DomNodeSlotKey>,
    label: &str,
) {
    let row = assert_append_button_to(root, content);
    let text = assert_append_text_to(root, row, label);
    assert_menu_like_row_style(root, row);
    row_keys.push(row);
    text_keys.push(text);
}

fn assert_menu_like_popup_style(
    root: &mut RootDom,
    popup: DomNodeSlotKey,
    content: DomNodeSlotKey,
    popup_position: &str,
    content_width_px: &str,
) {
    assert_set_attr_str(root, popup, "position", popup_position);
    assert_set_attr_str(root, popup, "left", MENU_POPUP_LEFT_PX);
    assert_set_attr_str(root, popup, "top", MENU_POPUP_TOP_PX);
    assert_set_attr_str(root, popup, "width", MENU_POPUP_WIDTH_PX);
    assert_set_attr_str(root, popup, "max-height", MENU_POPUP_MAX_HEIGHT_PX);
    assert_set_attr_str(root, popup, "overflow", "scroll");
    assert_set_attr_str(root, content, "width", content_width_px);
    assert_set_attr_str(root, content, "padding-left", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-right", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-top", MENU_CONTENT_PADDING_PX);
    assert_set_attr_str(root, content, "padding-bottom", MENU_CONTENT_PADDING_PX);
}

fn assert_menu_like_row_style(root: &mut RootDom, row: DomNodeSlotKey) {
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

fn popup_content_height_from_boxes(
    root: &RootDom,
    content: DomNodeSlotKey,
    row_keys: &[DomNodeSlotKey],
) -> f32 {
    let content_box = node_box(root, content).expect("popup content should expose a box");
    let mut height_px = content_box.padding.top_px + content_box.padding.bottom_px;
    let mut previous_margin_bottom_px: f32 = 0.0;
    for &row in row_keys {
        let row_box = node_box(root, row).expect("popup row should expose a box");
        let collapsed_margin_px = previous_margin_bottom_px.max(row_box.margin.top_px);
        height_px += collapsed_margin_px + row_box.border_box.height_px;
        previous_margin_bottom_px = row_box.margin.bottom_px;
    }
    height_px += previous_margin_bottom_px;
    height_px
}

fn assert_text_intrinsic(root: &RootDom, text: DomNodeSlotKey) -> ui_dom::TextIntrinsic {
    match root.node(text).map(DomNode::intrinsic) {
        Some(DomIntrinsic::Text(intrinsic)) => intrinsic.clone(),
        _ => panic!("popup text-row proof node should keep text intrinsic state"),
    }
}
