//! Form-control auto-size layout proofs.
//!
//! Spec references:
//! - CSS Form Control Styling Level 1 `field-sizing`:
//!   <https://drafts.csswg.org/css-forms-1/#field-sizing>
//! - HTML Living Standard rendering widgets section:
//!   <https://html.spec.whatwg.org/multipage/rendering.html#widgets>
//! - CSS Sizing Level 3 intrinsic sizes:
//!   <https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes>
//!
//! WPT references:
//! - `html/rendering/widgets/field-sizing-input-text.html`:
//!   <https://github.com/web-platform-tests/wpt/blob/18099a9f2e19ae86a7ae6c0b187f0bd03728fee6/html/rendering/widgets/field-sizing-input-text.html>
//! - `html/rendering/widgets/field-sizing-textarea.html`:
//!   <https://github.com/web-platform-tests/wpt/blob/18099a9f2e19ae86a7ae6c0b187f0bd03728fee6/html/rendering/widgets/field-sizing-textarea.html>
//! - `html/rendering/widgets/field-sizing-select.html`:
//!   <https://github.com/web-platform-tests/wpt/blob/18099a9f2e19ae86a7ae6c0b187f0bd03728fee6/html/rendering/widgets/field-sizing-select.html>
//!
//! Browser implementation references:
//! - Gecko text controls:
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/forms/nsTextControlFrame.cpp>
//! - Gecko combobox and list controls:
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/forms/nsComboboxControlFrame.cpp>
//!   <https://searchfox.org/firefox-main/rev/ad704963dac696aa26a7cb39eded9642c10c0ae0/layout/forms/nsListControlFrame.cpp>
//! - Blink default intrinsic control sizing is routed through
//!   `LayoutBox::DefaultIntrinsicContentInlineSize` and
//!   `LayoutBox::DefaultIntrinsicContentBlockSize`:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/layout_box.cc>
//! - Blink text controls use dedicated single-line and multiline layout objects:
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/forms/layout_text_control_single_line.h>
//!   <https://chromium.googlesource.com/chromium/src/+/c50f75313342e085919cf7525192f45f2ab649d1/third_party/blink/renderer/core/layout/forms/layout_text_control_multi_line.h>
//!
//! Browser contract pinned here:
//! - CSS Forms defines `field-sizing: fixed` as the initial value for elements
//!   with default preferred size, and says that branch uses the host-language
//!   default preferred intrinsic size.
//! - HTML defines `textarea` as an element with default preferred size and
//!   gives fixed intrinsic sizing rules based on effective width and height.
//! - WPT compares fixed/default form controls against `field-sizing: content`
//!   controls for input, textarea, and select.
//! - Gecko and Blink route text controls and select controls through dedicated
//!   control-sizing paths rather than ordinary child-flow sizing.
//!
//! Scope: these proofs pin the fixed/default preferred-size subset. They do not
//! attempt to match platform theme pixels, full `field-sizing: content`,
//! placeholder growth, scrollbar policy, or all select/listbox variants.

use bevy::input::{
    ButtonInput, ButtonState,
    keyboard::{Key, KeyCode, KeyboardInput},
};
use ui_dom::{
    DomAttribute, DomAttributeValue, DomIntrinsic, DomNode, DomPointerState, DomPointerUpdate,
    DomTag,
};

use crate::layout_proofs::support::{
    assert_append_div_to, assert_append_input_to, assert_append_textarea_to, assert_close,
    assert_node_layout, assert_root_dom_update, assert_set_attr_str,
    attached_gpu_root_dom_default_viewport_setup,
};

#[test]
// Browser contract: CSS Forms gives text controls the fixed field-sizing branch
// by default; WPT `field-sizing-input-text.html` expects the fixed input to keep
// a default face while `field-sizing: content` can shrink. Blink routes this
// through `TextFieldIntrinsicInlineSize` and `TextFieldIntrinsicBlockSize`;
// Gecko routes fixed text controls through
// `nsTextControlFrame::CalcIntrinsicSize`.
fn input_auto_sizes_from_default_preferred_size_intrinsic() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let input = assert_append_input_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(
        &mut root,
        input,
        "placeholder",
        "Longer placeholder text for intrinsic sizing",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let input_layout = assert_node_layout(&root, input);
    let input_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);

    assert!(
        input_layout.width_px > 0.0,
        "input width should be non-zero"
    );
    assert_close(
        input_layout.width_px,
        input_intrinsic.width_px,
        "input preferred width",
    );
    assert_close(
        input_layout.height_px,
        input_intrinsic.height_px,
        "input preferred height",
    );
}

#[test]
// Browser contract: HTML says a fixed textarea intrinsic size comes from its
// effective width and height; WPT `field-sizing-textarea.html` records the
// historic fixed default as about `20ch x 2lh`. Blink multiplies line height by
// rows in `TextAreaIntrinsicBlockSize`; Gecko uses
// `nsTextControlFrame::CalcIntrinsicSize`.
fn textarea_auto_sizes_from_default_preferred_size_intrinsic() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let textarea = assert_append_textarea_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let textarea_layout = assert_node_layout(&root, textarea);
    let textarea_intrinsic = assert_element_default_preferred_size_intrinsic(&root, textarea);

    assert!(
        textarea_layout.width_px > 0.0,
        "textarea width should be non-zero"
    );
    assert!(
        textarea_layout.height_px > textarea_intrinsic.line_height_px,
        "textarea height should span more than one line",
    );
    assert_close(
        textarea_layout.width_px,
        textarea_intrinsic.width_px,
        "textarea preferred width",
    );
    assert_close(
        textarea_layout.height_px,
        textarea_intrinsic.height_px,
        "textarea preferred height",
    );
}

#[test]
// Browser contract: WPT `field-sizing-select.html` distinguishes fixed select
// faces from content-sized dropdown/listbox sizing. Blink computes menulist and
// listbox intrinsic sizes on `LayoutBox`; Gecko keeps combobox and list controls
// in `nsComboboxControlFrame` and `nsListControlFrame`.
fn select_auto_sizes_from_default_preferred_size_intrinsic() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let select = root.create_element(DomTag::Select).expect("select");
    let option = root.create_element(DomTag::Option).expect("option");

    root.append_child(container, select).expect("append select");
    root.append_child(select, option).expect("append option");
    root.set_attr(
        option,
        DomAttribute::Value,
        Some(DomAttributeValue::String("cam-1".to_owned())),
    )
    .expect("option value");
    root.set_attr(
        option,
        DomAttribute::Label,
        Some(DomAttributeValue::String("Face Camera".to_owned())),
    )
    .expect("option label");
    root.set_attr(
        option,
        DomAttribute::Selected,
        Some(DomAttributeValue::Bool(true)),
    )
    .expect("option selected");

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(&mut root, select, "display", "block");
    assert_set_attr_str(&mut root, select, "line-height", "2");
    assert_set_attr_str(&mut root, select, "padding-top", "6px");
    assert_set_attr_str(&mut root, select, "padding-bottom", "6px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let select_layout = assert_node_layout(&root, select);
    let select_intrinsic = assert_element_default_preferred_size_intrinsic(&root, select);

    assert_close(
        select_intrinsic.line_height_px,
        28.0,
        "select preferred line-height",
    );
    assert_close(
        select_layout.height_px,
        select_intrinsic.height_px + 12.0,
        "select preferred height from line-height",
    );
}

#[test]
// Browser contract: select picker/list presentation is separate from the closed
// select face. WPT covers dropdown and listbox sizing separately, while Blink's
// `MenuListIntrinsicInlineSize`, `MenuListIntrinsicBlockSize`, and
// `ListBoxItemBlockSize` plus Gecko's combobox/list frames keep those paths
// distinct.
fn select_popup_does_not_replace_closed_face_intrinsic_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let select = root.create_element(DomTag::Select).expect("select");
    let option = root.create_element(DomTag::Option).expect("option");

    root.append_child(container, select).expect("append select");
    root.append_child(select, option).expect("append option");
    root.set_attr(
        option,
        DomAttribute::Value,
        Some(DomAttributeValue::String("cam-1".to_owned())),
    )
    .expect("option value");
    root.set_attr(
        option,
        DomAttribute::Label,
        Some(DomAttributeValue::String("Face Camera".to_owned())),
    )
    .expect("option label");
    root.set_attr(
        option,
        DomAttribute::Selected,
        Some(DomAttributeValue::Bool(true)),
    )
    .expect("option selected");

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(&mut root, select, "display", "block");
    assert_set_attr_str(&mut root, select, "width", "140px");
    assert_set_attr_str(&mut root, select, "line-height", "2");
    assert_set_attr_str(&mut root, select, "padding-top", "6px");
    assert_set_attr_str(&mut root, select, "padding-bottom", "6px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let closed_height_px = assert_node_layout(&root, select).height_px;

    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_update(5.0, 5.0, true));
    assert_root_dom_update(&mut world, &mut root, 0.0, pointer_update(5.0, 5.0, false));

    assert_close(
        assert_node_layout(&root, select).height_px,
        closed_height_px,
        "select height after popup opens",
    );
}

#[test]
// Browser contract: fixed text-control block size is derived from text metrics.
// Blink's `TextFieldIntrinsicBlockSize` uses the inner editor line height, and
// WPT `field-sizing-input-text.html` keeps fixed input height stable while
// content-sized width changes.
fn input_auto_sizes_from_authored_line_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let input = assert_append_input_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(&mut root, input, "display", "block");
    assert_set_attr_str(&mut root, input, "line-height", "2");
    assert_set_attr_str(&mut root, input, "padding-top", "6px");
    assert_set_attr_str(&mut root, input, "padding-bottom", "6px");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let input_layout = assert_node_layout(&root, input);
    let input_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);

    assert_close(
        input_intrinsic.line_height_px,
        28.0,
        "input preferred line-height",
    );
    assert_close(
        input_layout.height_px,
        input_intrinsic.height_px + 12.0,
        "input preferred height from line-height",
    );
}

fn pointer_update(x_px: f32, y_px: f32, primary_button_down: bool) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down,
        scroll_delta_y_px: 0.0,
    })
}

#[test]
// Browser contract: changing a text input value affects the content-sized path,
// not the fixed/default preferred face height. WPT compares fixed inputs with
// long values and placeholders against `field-sizing: content`; Blink and Gecko
// compute the fixed height from control metrics instead of value length.
fn input_keyboard_edit_keeps_default_preferred_size_height() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let input = assert_append_input_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(&mut root, input, "display", "block");
    assert_set_attr_str(&mut root, input, "padding-top", "6px");
    assert_set_attr_str(&mut root, input, "padding-bottom", "6px");
    assert_set_attr_str(&mut root, input, "placeholder", "Inspector value");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let baseline_layout = assert_node_layout(&root, input);
    let baseline_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);
    root.focus(input).expect("input should focus");

    let input_event = KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character("a".into()),
        state: ButtonState::Pressed,
        text: Some("a".into()),
        repeat: false,
        window: bevy::prelude::Entity::PLACEHOLDER,
    };
    let keycodes = ButtonInput::default();
    ui_dom::dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
        &mut root,
        &input_event,
        &keycodes,
    )
    .expect("keyboard edit should apply");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let input_layout = assert_node_layout(&root, input);
    let input_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);

    assert!(
        input_layout.height_px > 0.0,
        "input height should stay non-zero after keyboard edit",
    );
    assert_close(
        input_layout.height_px,
        input_intrinsic.height_px + 12.0,
        "input height after keyboard edit",
    );
    assert_close(
        input_intrinsic.width_px,
        baseline_intrinsic.width_px,
        "input preferred width after keyboard edit",
    );
    assert_close(
        input_layout.width_px,
        baseline_layout.width_px,
        "input layout width after keyboard edit",
    );
}

#[test]
// Browser contract: the fixed/default preferred-size branch must not widen a
// text input from the current value or placeholder content. That growth belongs
// to the content-sized path, not the host-language default face.
fn input_default_preferred_width_ignores_value_and_placeholder_growth() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let input = assert_append_input_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");
    assert_set_attr_str(&mut root, input, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);
    let baseline_layout = assert_node_layout(&root, input);

    assert_set_attr_str(
        &mut root,
        input,
        "placeholder",
        "Very long placeholder text that should not widen a fixed preferred input face",
    );
    assert_set_attr_str(
        &mut root,
        input,
        "value",
        "Very long current value that should not widen a fixed preferred input face",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grown_intrinsic = assert_element_default_preferred_size_intrinsic(&root, input);
    let grown_layout = assert_node_layout(&root, input);

    assert_close(
        grown_intrinsic.width_px,
        baseline_intrinsic.width_px,
        "fixed input preferred width after value and placeholder growth",
    );
    assert_close(
        grown_layout.width_px,
        baseline_layout.width_px,
        "fixed input layout width after value and placeholder growth",
    );
}

#[test]
// Browser contract: the fixed/default preferred-size branch must not widen or
// add rows to a textarea from current value or placeholder content. Fixed
// textarea geometry stays on the host default face until the content-sized path
// is explicitly selected.
fn textarea_default_preferred_size_ignores_value_and_placeholder_growth() {
    let (mut world, mut root) = attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = assert_append_div_to(&mut root, document_root);
    let textarea = assert_append_textarea_to(&mut root, container);

    assert_set_attr_str(&mut root, container, "display", "block");

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let baseline_intrinsic = assert_element_default_preferred_size_intrinsic(&root, textarea);
    let baseline_layout = assert_node_layout(&root, textarea);

    assert_set_attr_str(
        &mut root,
        textarea,
        "placeholder",
        "Line one of a much longer placeholder\nLine two of a much longer placeholder\nLine three of a much longer placeholder",
    );
    assert_set_attr_str(
        &mut root,
        textarea,
        "value",
        "Line one of a much longer current value\nLine two of a much longer current value\nLine three of a much longer current value",
    );

    assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let grown_intrinsic = assert_element_default_preferred_size_intrinsic(&root, textarea);
    let grown_layout = assert_node_layout(&root, textarea);

    assert_close(
        grown_intrinsic.width_px,
        baseline_intrinsic.width_px,
        "fixed textarea preferred width after value and placeholder growth",
    );
    assert_close(
        grown_intrinsic.height_px,
        baseline_intrinsic.height_px,
        "fixed textarea preferred height after value and placeholder growth",
    );
    assert_close(
        grown_layout.width_px,
        baseline_layout.width_px,
        "fixed textarea layout width after value and placeholder growth",
    );
    assert_close(
        grown_layout.height_px,
        baseline_layout.height_px,
        "fixed textarea layout height after value and placeholder growth",
    );
}

fn assert_element_default_preferred_size_intrinsic(
    root: &ui_dom::RootDom,
    node_key: ui_dom::DomNodeSlotKey,
) -> ui_dom::ElementDefaultPreferredSizeIntrinsic {
    match root.node(node_key).map(DomNode::intrinsic) {
        Some(DomIntrinsic::ElementDefaultPreferredSize(intrinsic)) => intrinsic.clone(),
        other => panic!("expected default preferred size intrinsic, got {other:?}"),
    }
}
