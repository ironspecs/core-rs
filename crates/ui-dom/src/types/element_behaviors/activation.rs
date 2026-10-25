//! Activation behavior definitions and the generic activation lifecycle.
//!
//! This file owns two layers:
//!
//! 1. the per-element activation *definitions* — the keyboard, pointer, focus,
//!    blur, and activation descriptors plus the retained pressed-pointer-click
//!    and range-drag session state;
//! 2. the *generic* click activation lifecycle (press → dispatch →
//!    commit/revert), extracted from `RootDom` as free functions over the
//!    `&mut RootDom` memory shape (PR 5, E6-a). The lifecycle invokes the
//!    per-kind row hooks from `behavior_table` at its seams.
//!
//! Invariants and rules:
//!
//! - the lifecycle is control-AGNOSTIC: it resolves the target's control kind,
//!   looks up the row, and invokes the row's hook at each seam; the per-kind
//!   work lives in the rows, not here.
//! - the select popup-row commit stays the first commit step here because it
//!   is a target-shape check (popup `<option>` rows resolve to no control
//!   kind), exactly as it was before the extraction.

use crate::domains::layout::node_box_from_ref;
use crate::element_controls::{ResolvedElementControl, resolved_element_control};
use crate::types::dom_event::{
    DomEvent, DomEventDetail, DomEventName, DomModifierState, DomMouseButton, DomMouseEvent,
};
use crate::types::primitives::PointPx;
use crate::types::{
    DomAttribute, DomAttributeValue, DomElementNode, DomError, DomNode, DomNodeSlotKey, RootDom,
    RootDomMetricName,
};

use super::behavior_table::{behavior_row, resolve_kind_for_element};
use super::dom_event_dispatch_outcome::DomEventDispatchOutcome;
use super::popup::apply_select_popup_row_click_default_for_target;

#[cfg(test)]
use crate::types::{DomPointerUpdate, DomTag, RootDomContext};

/// Selects which behavior gets first chance at keyboard interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnKeyboard {
    #[default]
    None,
    Button,
    InputControl,
    /// Runs the built-in select keyboard behavior for opening and committing the picker.
    Select,
    TextEdit,
    PresentedOptions,
}

/// Selects which behavior gets first chance at pointer interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnPointer {
    #[default]
    None,
    InputControl,
    TextEdit,
}

/// Selects the post-click lifecycle for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterClick {
    #[default]
    None,
    Activate,
}

/// Selects the committed activation action for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnActivation {
    #[default]
    None,
    /// Runs the built-in select activation behavior after an uncanceled click.
    Select,
    Toggle,
    PresentedOptions,
}

/// Selects the work that runs after one element becomes focused.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterFocus {
    #[default]
    None,
    PresentedOptions,
}

/// Selects which behavior runs when one element loses focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnBlur {
    #[default]
    None,
    /// Runs the built-in select blur behavior for closing the picker.
    Select,
    PresentedOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PressedPointerClickTarget {
    pub(crate) target_key: DomNodeSlotKey,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveRangeDragSession {
    pub(crate) target_key: DomNodeSlotKey,
    pub(crate) root_to_subtree_point_offset: PointPx,
    pub(crate) focus_entry_value: String,
    pub(crate) dirty: bool,
}

/// The pre-dispatch state the generic activation lifecycle carries so a
/// canceled click can revert the pre-applied toggle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationPreDispatchState {
    Checkbox { checked_before: bool },
}

/// Resolves the committed activation action for one element.
///
/// This is the pure resolution of `after_click`/`on_activation` (with the
/// checkbox refinement) into the activation enum the lifecycle dispatches on.
pub(crate) fn resolved_on_activation_for_element(
    element: &DomElementNode,
) -> DomElementOnActivation {
    if element.behavior().after_click != DomElementAfterClick::Activate {
        return DomElementOnActivation::None;
    }
    match element.behavior().on_activation {
        DomElementOnActivation::None => DomElementOnActivation::None,
        DomElementOnActivation::Select => DomElementOnActivation::Select,
        DomElementOnActivation::PresentedOptions => DomElementOnActivation::PresentedOptions,
        DomElementOnActivation::Toggle => {
            if matches!(
                resolved_element_control(element),
                Some(ResolvedElementControl::Checkbox(_))
            ) {
                DomElementOnActivation::Toggle
            } else {
                DomElementOnActivation::None
            }
        }
    }
}

pub(crate) fn target_has_disabled_attr(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        return Err(DomError::InvalidAttributeTarget);
    };
    Ok(matches!(
        element.attrs.attributes.get(&DomAttribute::Disabled),
        Some(DomAttributeValue::Bool(true))
    ))
}

pub(crate) fn keyboard_activation_click_point_for_target(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Result<PointPx, DomError> {
    let node = root.node(target_key).ok_or(DomError::InvalidNodeKey)?;
    let border_box = node_box_from_ref(node).border_box;
    Ok(PointPx {
        x_px: border_box.left_px + border_box.width_px * 0.5,
        y_px: border_box.top_px + border_box.height_px * 0.5,
    })
}

pub(crate) fn pressed_pointer_click_target_from_raw_target(
    root: &RootDom,
    target_key: Option<DomNodeSlotKey>,
) -> Option<PressedPointerClickTarget> {
    let target_key = target_key?;
    let _element = root.node(target_key).and_then(DomNode::as_element)?;
    if target_has_disabled_attr(root, target_key).ok()? {
        return None;
    }
    Some(PressedPointerClickTarget { target_key })
}

/// The generic click activation lifecycle: press → dispatch → commit/revert.
pub(crate) fn dispatch_click_and_apply_after_click(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    point: PointPx,
) -> Result<bool, DomError> {
    let pre_activation_state = apply_after_click_before_dispatch(root, target_key)?;
    let outcome = dispatch_click_event_to_target_key(root, target_key, point)?;
    let default_prevented = outcome.default_prevented();
    if let Some(event) = outcome.into_sink_eligible_bubbled_event() {
        root.push_emitted_event(event);
    }
    if default_prevented {
        revert_after_click_after_canceled_dispatch(root, target_key, pre_activation_state)?;
        return Ok(false);
    }
    commit_after_click_after_dispatch(root, target_key)
}

fn dispatch_click_event_to_target_key(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    point: PointPx,
) -> Result<DomEventDispatchOutcome, DomError> {
    let outcome = root.dispatch_event_to_target_key_and_finish(
        target_key,
        DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 0,
                client_x: point.x_px,
                client_y: point.y_px,
                modifiers: DomModifierState::default(),
            }),
        ),
    )?;
    if matches!(outcome, DomEventDispatchOutcome::Dispatched(_)) {
        root.increment_metric(RootDomMetricName::ClickDispatched);
    }
    Ok(outcome)
}

fn apply_after_click_before_dispatch(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<Option<ActivationPreDispatchState>, DomError> {
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        return Ok(None);
    };
    match resolved_on_activation_for_element(element) {
        DomElementOnActivation::None
        | DomElementOnActivation::Select
        | DomElementOnActivation::PresentedOptions => Ok(None),
        DomElementOnActivation::Toggle => {
            let Some(ResolvedElementControl::Checkbox(control)) = resolved_element_control(element)
            else {
                return Ok(None);
            };
            if control.disabled {
                return Ok(None);
            }
            root.set_attr(
                target_key,
                DomAttribute::Checked,
                Some(DomAttributeValue::Bool(!control.checked)),
            )?;
            root.increment_metric(RootDomMetricName::ActivationPreApplied);
            Ok(Some(ActivationPreDispatchState::Checkbox {
                checked_before: control.checked,
            }))
        }
    }
}

fn revert_after_click_after_canceled_dispatch(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    pre_activation_state: Option<ActivationPreDispatchState>,
) -> Result<(), DomError> {
    let Some(pre_activation_state) = pre_activation_state else {
        return Ok(());
    };
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        return Ok(());
    };
    if resolved_on_activation_for_element(element) != DomElementOnActivation::Toggle {
        return Ok(());
    }
    match pre_activation_state {
        ActivationPreDispatchState::Checkbox { checked_before } => {
            root.set_attr(
                target_key,
                DomAttribute::Checked,
                Some(DomAttributeValue::Bool(checked_before)),
            )?;
            root.increment_metric(RootDomMetricName::ActivationCanceled);
        }
    }
    Ok(())
}

/// The commit step of the generic activation lifecycle.
///
/// The select popup-row commit runs first because it is a target-shape check
/// (the popup's `<option>` rows resolve to no control kind), exactly as it was
/// before the extraction. The per-kind commit is then the row's
/// `on_click_commit` hook; a kind with no hook completes the activation as a
/// no-op (today's `DomElementOnActivation::None => Ok(true)`).
pub(crate) fn commit_after_click_after_dispatch(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if apply_select_popup_row_click_default_for_target(root, target_key)? {
        return Ok(true);
    }
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        return Ok(false);
    };
    let Some(kind) = resolve_kind_for_element(element) else {
        return Ok(true);
    };
    match behavior_row(kind).on_click_commit {
        Some(hook) => hook(root, target_key),
        None => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_on_activation_maps_checkbox_to_toggle_and_select_to_select() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");
        let select = root.create_element(DomTag::Select).expect("select");
        let button = root.create_element(DomTag::Button).expect("button");

        let checkbox_element = root
            .node(checkbox)
            .and_then(DomNode::as_element)
            .expect("element");
        assert_eq!(
            resolved_on_activation_for_element(checkbox_element),
            DomElementOnActivation::Toggle
        );

        let select_element = root
            .node(select)
            .and_then(DomNode::as_element)
            .expect("element");
        assert_eq!(
            resolved_on_activation_for_element(select_element),
            DomElementOnActivation::Select
        );

        let button_element = root
            .node(button)
            .and_then(DomNode::as_element)
            .expect("element");
        assert_eq!(
            resolved_on_activation_for_element(button_element),
            DomElementOnActivation::None
        );
    }

    #[test]
    fn checkbox_click_lifecycle_flips_checked_and_emits_click_input_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.append_child(root.root, checkbox).expect("append");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");

        let committed = dispatch_click_and_apply_after_click(
            &mut root,
            checkbox,
            PointPx {
                x_px: 9.0,
                y_px: 9.0,
            },
        )
        .expect("click");
        assert!(committed);

        let element = root
            .node(checkbox)
            .and_then(DomNode::as_element)
            .expect("element");
        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Checked),
            Some(&DomAttributeValue::Bool(true))
        );

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::Click,
                DomEventName::Input,
                DomEventName::Change,
            ]
        );
    }

    #[test]
    fn disabled_target_never_becomes_a_pressed_click_target() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.set_attr(
            button,
            DomAttribute::Disabled,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("disabled");

        assert_eq!(
            pressed_pointer_click_target_from_raw_target(&root, Some(button)),
            None
        );
    }
}
