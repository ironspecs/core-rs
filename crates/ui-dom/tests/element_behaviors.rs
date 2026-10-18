//! Element-behavior contract tests for the `ui-dom` retained pipeline.
//!
//! These tests re-express activation-behavior assertions originally residing
//! in `crates/ui-dom/src/types/element_behaviors/activation.rs` through the
//! public `ui_dom` API and `RootDom::metrics()` / `history()` observability
//! surface.  They use the shared GPU fixture from `layout_proofs/support.rs`
//! and real DOM layout (via `node_box()`) instead of `set_border_box_for_test`.

#[allow(dead_code)]
#[path = "layout_proofs/support.rs"]
mod support;

use ui_dom::{
    DomAttribute, DomAttributeValue, DomElementNode, DomEvent, DomEventListenerContext,
    DomEventName, DomNode, DomNodeSlotKey, DomPointerState, DomPointerUpdate, DomTag, RootDom,
    RootDomMetricName, node_box,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pointer_update(x_px: f32, y_px: f32, primary_button_down: bool) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down,
        scroll_delta_y_px: 0.0,
    })
}

fn event_names(events: &[DomEvent]) -> Vec<DomEventName> {
    events.iter().map(|event| event.name).collect()
}

fn history_names(root: &RootDom) -> Vec<RootDomMetricName> {
    root.history().map(|entry| entry.name).collect()
}

fn metric_index(history: &[RootDomMetricName], metric_name: RootDomMetricName) -> usize {
    history
        .iter()
        .position(|name| *name == metric_name)
        .expect("metric should be present")
}

fn prevent_default(context: &mut DomEventListenerContext<'_>) {
    context.prevent_default();
}

fn stop_propagation(context: &mut DomEventListenerContext<'_>) {
    context.stop_propagation();
}

fn input_value(root: &RootDom, input: DomNodeSlotKey) -> String {
    let element = root
        .node(input)
        .and_then(DomNode::as_element)
        .expect("input element should exist");
    element_string_attr(element, &DomAttribute::Value)
        .unwrap_or_default()
        .to_string()
}

fn element_string_attr<'a>(element: &'a DomElementNode, name: &DomAttribute) -> Option<&'a str> {
    match element.attrs.attributes.get(name)? {
        DomAttributeValue::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn element_bool_attr(element: &DomElementNode, name: &DomAttribute) -> Option<bool> {
    match element.attrs.attributes.get(name)? {
        DomAttributeValue::Bool(value) => Some(*value),
        _ => None,
    }
}

/// Position of the center of an element's border box, as laid out.
fn element_center(root: &RootDom, key: DomNodeSlotKey) -> (f32, f32) {
    let box_model = node_box(root, key).expect("element should have a box model after layout");
    let bb = box_model.border_box;
    (
        bb.left_px + bb.width_px * 0.5,
        bb.top_px + bb.height_px * 0.5,
    )
}

/// Build a checkbox input as a child of the document root, run layout once,
/// and return `(world, root, input_key)` with metrics already reset.
fn checkbox_under_document_setup() -> (bevy::prelude::World, RootDom, DomNodeSlotKey) {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let input = root.create_element(DomTag::Input).expect("input");
    root.append_child(document_root, input).expect("append");
    root.set_attr_str(input, "type", "checkbox").expect("type");
    root.style_mut(input).set("width", "24px").expect("width");
    root.style_mut(input).set("height", "24px").expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    (world, root, input)
}

/// Build a number input inside a fixed-position offset layer as a child of the
/// document root, run layout once, and return `(world, root, input_key)` with
/// metrics already reset.
fn offset_number_input_setup() -> (bevy::prelude::World, RootDom, DomNodeSlotKey) {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let layer = root.create_element(DomTag::Div).expect("layer");
    let input = root.create_element(DomTag::Input).expect("number input");
    root.append_child(document_root, layer)
        .expect("append layer");
    root.append_child(layer, input).expect("append input");
    root.style_mut(layer)
        .set("position", "fixed")
        .expect("layer position");
    root.style_mut(layer)
        .set("left", "80px")
        .expect("layer left");
    root.style_mut(layer).set("top", "10px").expect("layer top");
    root.style_mut(layer)
        .set("width", "140px")
        .expect("layer width");
    root.style_mut(layer)
        .set("height", "36px")
        .expect("layer height");
    root.set_attr_str(input, "type", "number")
        .expect("input type");
    root.set_attr_str(input, "value", "5").expect("input value");
    root.style_mut(input)
        .set("width", "120px")
        .expect("input width");
    root.style_mut(input)
        .set("height", "24px")
        .expect("input height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    (world, root, input)
}

/// Build a range input inside a fixed-position offset layer as a child of the
/// document root, run layout once, and return `(world, root, input_key)` with
/// metrics already reset.
fn offset_range_input_setup() -> (bevy::prelude::World, RootDom, DomNodeSlotKey) {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let layer = root.create_element(DomTag::Div).expect("layer");
    let input = root.create_element(DomTag::Input).expect("range input");
    root.append_child(document_root, layer)
        .expect("append layer");
    root.append_child(layer, input).expect("append input");
    root.style_mut(layer)
        .set("position", "fixed")
        .expect("layer position");
    root.style_mut(layer)
        .set("left", "80px")
        .expect("layer left");
    root.style_mut(layer).set("top", "10px").expect("layer top");
    root.style_mut(layer)
        .set("width", "140px")
        .expect("layer width");
    root.style_mut(layer)
        .set("height", "36px")
        .expect("layer height");
    root.set_attr_str(input, "type", "range")
        .expect("input type");
    root.set_attr_str(input, "value", "0").expect("input value");
    root.style_mut(input)
        .set("width", "120px")
        .expect("input width");
    root.style_mut(input)
        .set("height", "24px")
        .expect("input height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    (world, root, input)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Re-expressed from activation.rs: `pointer_click_checks_then_unchecks_checkbox`.
#[test]
fn pointer_click_checks_then_unchecks_checkbox() {
    let (mut world, mut root, input) = checkbox_under_document_setup();
    let (cx, cy) = element_center(&root, input);

    // First click — check
    let down_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("pointer down");
    assert!(event_names(&down_events).contains(&DomEventName::PointerDown));

    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("pointer up");
    let checkbox_element = root
        .node(input)
        .and_then(DomNode::as_element)
        .expect("checkbox element");
    assert_eq!(
        element_bool_attr(checkbox_element, &DomAttribute::Checked),
        Some(true)
    );
    assert_eq!(
        event_names(&up_events),
        vec![
            DomEventName::PointerUp,
            DomEventName::Click,
            DomEventName::Input,
            DomEventName::Change,
        ]
    );
    let first_click_history = history_names(&root);
    assert!(
        metric_index(
            &first_click_history,
            RootDomMetricName::ActivationPreApplied
        ) < metric_index(&first_click_history, RootDomMetricName::ClickDispatched)
    );
    assert!(
        metric_index(&first_click_history, RootDomMetricName::ClickDispatched)
            < metric_index(&first_click_history, RootDomMetricName::ActivationCommitted)
    );

    // Second click — uncheck
    root.reset_metrics();
    let _ = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("second down");
    let second_up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("second up");
    let checkbox_element2 = root
        .node(input)
        .and_then(DomNode::as_element)
        .expect("checkbox element");
    assert_eq!(
        element_bool_attr(checkbox_element2, &DomAttribute::Checked),
        Some(false)
    );
    assert_eq!(
        event_names(&second_up_events),
        vec![
            DomEventName::PointerUp,
            DomEventName::Click,
            DomEventName::Input,
            DomEventName::Change,
        ]
    );

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}

/// Re-expressed from activation.rs: `pointer_click_emits_click_for_button`.
#[test]
fn pointer_click_emits_click_for_button() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let button = root.create_element(DomTag::Button).expect("button");
    root.append_child(document_root, button).expect("append");
    root.style_mut(button).set("width", "40px").expect("width");
    root.style_mut(button)
        .set("height", "40px")
        .expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let (cx, cy) = element_center(&root, button);

    let _ = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(
        event_names(&up_events),
        vec![DomEventName::PointerUp, DomEventName::Click]
    );
}

/// Re-expressed from activation.rs:
/// `pointer_click_on_disabled_button_does_not_focus_or_dispatch_click`.
#[test]
fn pointer_click_on_disabled_button_does_not_focus_or_dispatch_click() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let button = root.create_element(DomTag::Button).expect("button");
    root.append_child(document_root, button).expect("append");
    root.set_attr(
        button,
        DomAttribute::Disabled,
        Some(DomAttributeValue::Bool(true)),
    )
    .expect("disable button");
    root.style_mut(button).set("width", "40px").expect("width");
    root.style_mut(button)
        .set("height", "40px")
        .expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let (cx, cy) = element_center(&root, button);

    let down_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(root.focused_element_key(), None);
    assert!(!event_names(&down_events).contains(&DomEventName::Click));
    assert!(!event_names(&up_events).contains(&DomEventName::Click));
}

/// Re-expressed from activation.rs:
/// `pointer_click_on_pointer_events_none_button_does_not_focus_or_dispatch_click`.
#[test]
fn pointer_click_on_pointer_events_none_button_does_not_focus_or_dispatch_click() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let button = root.create_element(DomTag::Button).expect("button");
    root.append_child(document_root, button).expect("append");
    root.set_attr_str(button, "pointer-events", "none")
        .expect("button pointer-events");
    root.style_mut(button).set("width", "40px").expect("width");
    root.style_mut(button)
        .set("height", "40px")
        .expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    let (cx, cy) = element_center(&root, button);

    let down_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(root.focused_element_key(), None);
    assert!(!event_names(&down_events).contains(&DomEventName::Click));
    assert!(!event_names(&up_events).contains(&DomEventName::Click));
}

/// Re-expressed from activation.rs:
/// `pointer_click_emits_click_for_textarea_without_activation_lifecycle`.
#[test]
fn pointer_click_emits_click_for_textarea_without_activation_lifecycle() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let textarea = root.create_element(DomTag::Textarea).expect("textarea");
    root.append_child(document_root, textarea).expect("append");
    root.style_mut(textarea)
        .set("width", "40px")
        .expect("width");
    root.style_mut(textarea)
        .set("height", "40px")
        .expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    let (cx, cy) = element_center(&root, textarea);

    let _ = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(
        event_names(&up_events),
        vec![DomEventName::PointerUp, DomEventName::Click]
    );
    let history = history_names(&root);
    assert!(!history.contains(&RootDomMetricName::ActivationPreApplied));
    assert!(!history.contains(&RootDomMetricName::ActivationCanceled));
    assert!(!history.contains(&RootDomMetricName::ActivationCommitted));
}

/// Re-expressed from activation.rs:
/// `pointer_click_emits_click_for_text_input_without_activation_action`.
#[test]
fn pointer_click_emits_click_for_text_input_without_activation_action() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;

    let input = root.create_element(DomTag::Input).expect("input");
    root.append_child(document_root, input).expect("append");
    root.style_mut(input).set("width", "40px").expect("width");
    root.style_mut(input).set("height", "40px").expect("height");

    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    let (cx, cy) = element_center(&root, input);

    let _ = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(
        event_names(&up_events),
        vec![DomEventName::PointerUp, DomEventName::Click]
    );
    let history = history_names(&root);
    assert!(!history.contains(&RootDomMetricName::ActivationPreApplied));
    assert!(!history.contains(&RootDomMetricName::ActivationCanceled));
    assert!(!history.contains(&RootDomMetricName::ActivationCommitted));
}

/// Re-expressed from activation.rs:
/// `prevented_click_reverts_checkbox_pre_activation_and_skips_input_change`.
#[test]
fn prevented_click_reverts_checkbox_pre_activation_and_skips_input_change() {
    let (mut world, mut root, input) = checkbox_under_document_setup();
    root.add_event_listener(input, DomEventName::Click, prevent_default, None)
        .expect("click listener");

    let (cx, cy) = element_center(&root, input);

    let _ = root
        .update(&mut world, 0.0, pointer_update(cx, cy, true))
        .expect("down");
    let up_events = root
        .update(&mut world, 0.0, pointer_update(cx, cy, false))
        .expect("up");

    assert_eq!(event_names(&up_events), vec![DomEventName::PointerUp]);
    let checkbox_element = root
        .node(input)
        .and_then(DomNode::as_element)
        .expect("checkbox element");
    assert!(matches!(
        element_bool_attr(checkbox_element, &DomAttribute::Checked),
        Some(false) | None
    ));
    let history = history_names(&root);
    let pre_activation_index = metric_index(&history, RootDomMetricName::ActivationPreApplied);
    let click_dispatch_index = metric_index(&history, RootDomMetricName::ClickDispatched);
    let canceled_index = metric_index(&history, RootDomMetricName::ActivationCanceled);
    assert!(pre_activation_index < click_dispatch_index);
    assert!(click_dispatch_index < canceled_index);
    assert!(!history.contains(&RootDomMetricName::ActivationCommitted));

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}

/// Re-expressed from activation.rs:
/// `number_stepper_uses_subtree_local_point_inside_offset_layer`.
#[test]
fn number_stepper_uses_subtree_local_point_inside_offset_layer() {
    let (mut world, mut root, input) = offset_number_input_setup();

    // The layer is fixed at (80, 10). The input is 120×24 inside it.
    // The increment stepper button occupies the right ~30px of the input,
    // so a click near the right edge (x ≈ 190) in screen space hits it.
    // These are the same coordinates the original inline test used.
    let increment_x = 190.0;
    let increment_y = 20.0;

    let down_events = root
        .update(
            &mut world,
            0.0,
            pointer_update(increment_x, increment_y, true),
        )
        .expect("increment pointer down should step number");

    assert_eq!(input_value(&root, input), "6");
    assert_eq!(
        down_events
            .first()
            .expect("pointerdown should be emitted")
            .target_key,
        Some(input),
        "public pointer target should stay the input, not an internal stepper",
    );
    assert_eq!(root.focused_element_key(), Some(input));
    assert!(event_names(&down_events).contains(&DomEventName::Input));
    assert!(event_names(&down_events).contains(&DomEventName::Change));

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}

/// Re-expressed from activation.rs:
/// `pointerdown_prevent_default_blocks_number_stepper_default_action`.
#[test]
fn pointerdown_prevent_default_blocks_number_stepper_default_action() {
    let (mut world, mut root, input) = offset_number_input_setup();
    root.add_event_listener(input, DomEventName::PointerDown, prevent_default, None)
        .expect("pointerdown listener");

    let increment_x = 190.0;
    let increment_y = 20.0;

    let down_events = root
        .update(
            &mut world,
            0.0,
            pointer_update(increment_x, increment_y, true),
        )
        .expect("prevented pointer down should not step number");

    assert_eq!(input_value(&root, input), "5");
    assert_eq!(root.focused_element_key(), None);
    assert!(
        !event_names(&down_events).contains(&DomEventName::PointerDown),
        "prevented pointerdown should not be sink-eligible",
    );
    assert!(!event_names(&down_events).contains(&DomEventName::Input));
    assert!(!event_names(&down_events).contains(&DomEventName::Change));

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}

/// Re-expressed from activation.rs:
/// `pointerdown_stop_propagation_does_not_block_number_stepper_default_action`.
#[test]
fn pointerdown_stop_propagation_does_not_block_number_stepper_default_action() {
    let (mut world, mut root, input) = offset_number_input_setup();
    root.add_event_listener(input, DomEventName::PointerDown, stop_propagation, None)
        .expect("pointerdown listener");

    let increment_x = 190.0;
    let increment_y = 20.0;

    let down_events = root
        .update(
            &mut world,
            0.0,
            pointer_update(increment_x, increment_y, true),
        )
        .expect("stopped pointer down should still step number");

    assert_eq!(input_value(&root, input), "6");
    assert_eq!(root.focused_element_key(), Some(input));
    assert!(
        !event_names(&down_events).contains(&DomEventName::PointerDown),
        "stopped pointerdown should not be sink-eligible",
    );
    assert!(event_names(&down_events).contains(&DomEventName::Input));
    assert!(event_names(&down_events).contains(&DomEventName::Change));

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}

/// Re-expressed from activation.rs:
/// `range_drag_uses_subtree_local_points_inside_offset_layer`.
#[test]
fn range_drag_uses_subtree_local_points_inside_offset_layer() {
    let (mut world, mut root, input) = offset_range_input_setup();

    // The layer is fixed at (80, 10). The range input is 120×24 inside it.
    // Drag start at screen x=140 hits near the midpoint of the track and
    // produces value "50". Drag to x=188 produces value "100". These are
    // the same coordinates the original inline test used.
    let down_x = 140.0;
    let down_y = 20.0;
    let move_x = 188.0;

    let down_events = root
        .update(&mut world, 0.0, pointer_update(down_x, down_y, true))
        .expect("range pointer down should use local point");

    assert_eq!(input_value(&root, input), "50");
    assert_eq!(
        down_events
            .first()
            .expect("pointerdown should be emitted")
            .target_key,
        Some(input),
        "public pointer target should stay the range input",
    );

    let move_events = root
        .update(&mut world, 0.0, pointer_update(move_x, down_y, true))
        .expect("range pointer move should keep drag-local offset");

    assert_eq!(input_value(&root, input), "100");
    assert!(event_names(&move_events).contains(&DomEventName::Input));

    let up_events = root
        .update(&mut world, 0.0, pointer_update(move_x, down_y, false))
        .expect("range pointer up should finish drag");

    assert!(event_names(&up_events).contains(&DomEventName::Change));

    root.detach_from_bevy_parent_entity(&mut world)
        .expect("detach");
}
