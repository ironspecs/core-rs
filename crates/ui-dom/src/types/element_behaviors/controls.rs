//! Shared input-control behavior helpers (PR 5, E6-a).
//!
//! This file owns the default-behavior helpers that are shared across the four
//! input control kinds (checkbox, number, range, text input). It is the shared
//! home called out by the extraction rule: "methods that turn out to be shared
//! helpers (used by multiple kinds) go to a shared file with a named note, not
//! duplicated."
//!
//! Invariants and rules:
//!
//! - these helpers take `&mut RootDom` exactly as their former `RootDom`
//!   methods did (stage-a context; the narrowed `BehaviorContext` is E6-c).
//! - `input_control_on_keyboard` and `input_control_pointer_down` are the
//!   former `apply_input_control_on_keyboard` / `apply_on_pointer_down`
//!   `InputControl` arms, byte-identical bodies modulo `self → root`; they
//!   branch on `ResolvedElementControl` internally (named note: the four input
//!   kinds share one keyboard/pointer dispatch shape today — splitting them
//!   per-kind is E6-c evidence work, not this stage).
//! - the presented-options thin hooks delegate to the popup methods that stay
//!   on `RootDom` (E6-b moves the popup itself).

use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

use crate::domains::layout::node_box_from_ref;
use crate::domains::visitor::event_target::DomPointEventTarget;
use crate::element_controls::{
    NumericStepDirection, ResolvedElementControl, ResolvedElementControlPart, ResolvedRangeControl,
    range_value_string_from_point, resolved_control_part_from_point, resolved_element_control,
    resolved_text_edit_kind,
};
use crate::types::primitives::PointPx;
use crate::types::{
    DomAttribute, DomAttributeValue, DomElementEditKind, DomError, DomNode, DomNodeSlotKey,
    DomSelection, RootDom, TextSelection, TextSelectionDirection,
};

use super::activation::{
    dispatch_click_and_apply_after_click, keyboard_activation_click_point_for_target,
};
use super::popup::{
    apply_presented_options_text_input_keyboard_navigation_for_owner,
    close_presented_options_for_blurred_owner, open_presented_options_for_focused_text_input_owner,
};
use super::range::{mark_active_range_drag_dirty, start_range_drag_session};
use super::text_entry::{
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input,
    ensure_text_selection_for_target,
};

#[cfg(test)]
use crate::types::{DomEventName, DomPointerUpdate, DomTag, RootDomContext};

/// The shared input-control keyboard handler (checkbox/number/range/text
/// input). Formerly `RootDom::apply_input_control_on_keyboard`.
pub(crate) fn input_control_on_keyboard(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
    shift_key: bool,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    if input.state == ButtonState::Released {
        if !event_stayed_unblocked {
            return Ok(false);
        }
        return apply_input_keyboard_click_on_keyup(root, target_key, input);
    }
    if input.state != ButtonState::Pressed || !event_stayed_unblocked {
        return Ok(false);
    }
    if let Some(applied) = apply_input_keyboard_step_on_keydown(root, target_key, input)? {
        return Ok(applied);
    }
    if presented_options_keyboard_navigation(root, target_key, input, event_stayed_unblocked)? {
        return Ok(true);
    }
    ensure_text_selection_for_target(root, target_key);
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input(root, input, shift_key)
}

/// The shared input-control pointer-down handler (the former
/// `apply_on_pointer_down` `InputControl` arm).
pub(crate) fn input_control_pointer_down(
    root: &mut RootDom,
    target: DomPointEventTarget,
    root_point: PointPx,
) -> Result<(), DomError> {
    let target_key = target.target_key;
    let Some(control) = resolved_element_control_for_key(root, target_key) else {
        return Ok(());
    };
    if matches!(control, ResolvedElementControl::TextInput) {
        ensure_text_selection_for_target(root, target_key);
        return Ok(());
    }
    let Some(control_part) = resolved_control_part_for_key(root, target_key, target.subtree_point)?
    else {
        return Ok(());
    };
    match (control, control_part) {
        (
            ResolvedElementControl::Number(control),
            ResolvedElementControlPart::NumberDecrementButton,
        ) if !control.disabled && !control.readonly => set_control_value_and_dispatch_input_change(
            root,
            target_key,
            control
                .numeric
                .stepped_value_string(NumericStepDirection::Decrement),
        )
        .map(|_| ()),
        (
            ResolvedElementControl::Number(control),
            ResolvedElementControlPart::NumberIncrementButton,
        ) if !control.disabled && !control.readonly => set_control_value_and_dispatch_input_change(
            root,
            target_key,
            control
                .numeric
                .stepped_value_string(NumericStepDirection::Increment),
        )
        .map(|_| ()),
        (ResolvedElementControl::Range(control), _) if !control.disabled => {
            start_range_drag_session(root, target_key, root_point, target.subtree_point)?;
            let next_value =
                range_value_string_for_point(root, target_key, target.subtree_point, control)?;
            let changed = set_control_value_and_dispatch_input(root, target_key, next_value)?;
            mark_active_range_drag_dirty(root, target_key, changed);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Steps a number/range control on a keyboard keydown, or returns `None` when
/// the key should fall through to text entry.
pub(crate) fn apply_input_keyboard_step_on_keydown(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
) -> Result<Option<bool>, DomError> {
    let Some(control) = resolved_element_control_for_key(root, target_key) else {
        return Ok(Some(false));
    };
    match control {
        ResolvedElementControl::Checkbox(_) | ResolvedElementControl::Textarea => Ok(Some(false)),
        ResolvedElementControl::Number(control) => {
            let Some(next_value) =
                numeric_keyboard_value_string(control.numeric, &input.logical_key)
            else {
                return Ok(None);
            };
            if control.disabled || control.readonly {
                return Ok(Some(false));
            }
            set_control_value_and_dispatch_input_change(root, target_key, next_value).map(Some)
        }
        ResolvedElementControl::Range(control) => {
            let Some(next_value) =
                numeric_keyboard_value_string(control.numeric, &input.logical_key)
            else {
                return Ok(Some(false));
            };
            if control.disabled {
                return Ok(Some(false));
            }
            set_control_value_and_dispatch_input_change(root, target_key, next_value).map(Some)
        }
        ResolvedElementControl::TextInput => Ok(None),
    }
}

/// The shared keyboard keyup handler (the former
/// `apply_input_keyboard_click_on_keyup`): only the checkbox has a keyup
/// commit; every other input kind returns `false` here.
pub(crate) fn apply_input_keyboard_click_on_keyup(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
) -> Result<bool, DomError> {
    if !matches!(input.logical_key, Key::Space) {
        return Ok(false);
    }
    let Some(ResolvedElementControl::Checkbox(control)) =
        resolved_element_control_for_key(root, target_key)
    else {
        return Ok(false);
    };
    if control.disabled {
        return Ok(false);
    }
    let point = keyboard_activation_click_point_for_target(root, target_key)?;
    dispatch_click_and_apply_after_click(root, target_key, point)
}

pub(crate) fn set_control_value_and_dispatch_input_change(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    next_value: String,
) -> Result<bool, DomError> {
    let changed = set_control_value_and_dispatch_input(root, target_key, next_value.clone())?;
    if !changed {
        return Ok(false);
    }
    let _ = root.dispatch_change_event_to_target_key(target_key, next_value)?;
    Ok(true)
}

pub(crate) fn set_control_value_and_dispatch_input(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    next_value: String,
) -> Result<bool, DomError> {
    let current_value = control_value_event_string(root, target_key)?.unwrap_or_default();
    if current_value == next_value {
        return Ok(false);
    }
    root.set_attr(
        target_key,
        DomAttribute::Value,
        Some(DomAttributeValue::String(next_value.clone())),
    )?;
    if root.focused_element_key() == Some(target_key)
        && matches!(
            root.node(target_key)
                .and_then(DomNode::as_element)
                .map(resolved_text_edit_kind),
            Some(DomElementEditKind::SingleLine | DomElementEditKind::MultiLine)
        )
    {
        let value_len = count_unicode_scalars(&next_value);
        root.set_selection(DomSelection::TextSelected(TextSelection {
            target_element: target_key,
            selection_start: value_len,
            selection_end: value_len,
            selection_direction: TextSelectionDirection::None,
        }));
    }
    let _ = root.dispatch_input_event_to_target_key(target_key, next_value)?;
    Ok(true)
}

pub(crate) fn control_value_event_string(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Result<Option<String>, DomError> {
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        return Err(DomError::InvalidAttributeTarget);
    };
    let value = match resolved_element_control(element) {
        Some(ResolvedElementControl::Checkbox(control)) => Some(control.checked.to_string()),
        Some(ResolvedElementControl::Number(control)) => {
            Some(format!("{}", control.numeric.display_value_px()))
        }
        Some(ResolvedElementControl::Range(control)) => {
            Some(format!("{}", control.numeric.display_value_px()))
        }
        Some(ResolvedElementControl::TextInput | ResolvedElementControl::Textarea) => {
            match element.attrs.attributes.get(&DomAttribute::Value) {
                Some(DomAttributeValue::String(value)) => Some(value.clone()),
                None => Some(String::new()),
                Some(_) => return Err(DomError::InvalidAttributeValue),
            }
        }
        None => None,
    };
    Ok(value)
}

pub(crate) fn resolved_element_control_for_key(
    root: &RootDom,
    key: DomNodeSlotKey,
) -> Option<ResolvedElementControl> {
    let element = root.node(key)?.as_element()?;
    resolved_element_control(element)
}

pub(crate) fn resolved_control_part_for_key(
    root: &RootDom,
    key: DomNodeSlotKey,
    point: PointPx,
) -> Result<Option<ResolvedElementControlPart>, DomError> {
    let Some(element) = root.node(key).and_then(DomNode::as_element) else {
        return Ok(None);
    };
    Ok(resolved_control_part_from_point(
        element,
        node_box_from_ref(root.node(key).ok_or(DomError::InvalidNodeKey)?),
        point,
    ))
}

pub(crate) fn range_value_string_for_point(
    root: &RootDom,
    key: DomNodeSlotKey,
    point: PointPx,
    control: ResolvedRangeControl,
) -> Result<String, DomError> {
    Ok(range_value_string_from_point(
        node_box_from_ref(root.node(key).ok_or(DomError::InvalidNodeKey)?),
        control,
        point,
    ))
}

/// Thin hook over the popup lifecycle (E6-b): keyboard navigation for a text
/// input with a presented-options (datalist) popup.
pub(crate) fn presented_options_keyboard_navigation(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    input: &KeyboardInput,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    apply_presented_options_text_input_keyboard_navigation_for_owner(
        root,
        owner_key,
        input,
        event_stayed_unblocked,
    )
}

/// Thin hook over the popup lifecycle (E6-b): open the presented-options popup
/// for a newly focused text input owner.
pub(crate) fn presented_options_focus(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let _ = open_presented_options_for_focused_text_input_owner(root, target_key)?;
    Ok(())
}

/// Thin hook over the popup lifecycle (E6-b): close the presented-options
/// popup for a blurred text input owner.
pub(crate) fn presented_options_blur(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let _ = close_presented_options_for_blurred_owner(root, target_key)?;
    Ok(())
}

/// Number of Unicode scalar values in a string. Named note: this duplicates
/// `domains::text_layout::count_unicode_scalars` byte-for-byte; the duplicate
/// existed in `root_dom.rs` before this extraction and moved here to keep the
/// shared input-control helpers self-contained. Dedup is a D1(a) alignment
/// follow-up, not this changeset.
pub(crate) fn count_unicode_scalars(value: &str) -> usize {
    value.chars().count()
}

fn numeric_keyboard_value_string(
    control: crate::element_controls::ResolvedNumericControl,
    logical_key: &Key,
) -> Option<String> {
    match logical_key {
        Key::ArrowDown => control.keyboard_step_value_string("ArrowDown"),
        Key::ArrowLeft => control.keyboard_step_value_string("ArrowLeft"),
        Key::ArrowUp => control.keyboard_step_value_string("ArrowUp"),
        Key::ArrowRight => control.keyboard_step_value_string("ArrowRight"),
        Key::Home => control.keyboard_step_value_string("Home"),
        Key::End => control.keyboard_step_value_string("End"),
        Key::PageDown => control.keyboard_step_value_string("PageDown"),
        Key::PageUp => control.keyboard_step_value_string("PageUp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element_value(root: &RootDom, key: DomNodeSlotKey) -> String {
        let element = root
            .node(key)
            .and_then(DomNode::as_element)
            .expect("value target should stay an element");
        match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            None => panic!("editable value attribute should exist"),
            Some(_) => panic!("editable value attribute should stay string"),
        }
    }

    #[test]
    fn control_value_event_string_reads_each_control_shape() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");
        root.set_attr(
            checkbox,
            DomAttribute::Checked,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("checked");
        let number = root.create_element(DomTag::Input).expect("number");
        root.set_attr_str(number, "type", "number").expect("type");
        root.set_attr_str(number, "value", "7").expect("value");
        let text = root.create_element(DomTag::Input).expect("text");
        root.set_attr_str(text, "value", "hi").expect("value");

        assert_eq!(
            control_value_event_string(&root, checkbox).expect("checkbox value"),
            Some("true".to_string())
        );
        assert_eq!(
            control_value_event_string(&root, number).expect("number value"),
            Some("7".to_string())
        );
        assert_eq!(
            control_value_event_string(&root, text).expect("text value"),
            Some("hi".to_string())
        );
    }

    #[test]
    fn set_control_value_and_dispatch_input_writes_value_and_emits_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let range = root.create_element(DomTag::Input).expect("range");
        root.append_child(root.root, range).expect("append");
        root.set_attr_str(range, "type", "range").expect("type");
        root.set_attr_str(range, "value", "10").expect("value");

        let changed = set_control_value_and_dispatch_input(&mut root, range, "11".to_string())
            .expect("set value");
        assert!(changed);
        assert_eq!(element_value(&root, range), "11");

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::Input]
        );
    }
}
