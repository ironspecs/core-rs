//! Activation behavior definitions and lifecycle execution for DOM elements.

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::{Entity, World},
};

use crate::domains::layout::node_box_from_ref;
use crate::domains::visitor::event_target::{
    DomPointEventTarget, dispatch_event_to_target_and_finish, topmost_event_target_from_point,
};
use crate::element_controls::{
    NumericStepDirection, ResolvedElementControl, range_value_string_from_point,
    resolved_control_part_from_point, resolved_element_control, resolved_text_edit_kind,
};
use crate::types::dom_event::{DomEvent, DomEventDetail, DomEventName, DomMouseEvent};
use crate::types::primitives::PointPx;
use crate::types::{
    DomAttribute, DomAttributeValue, DomElementEditKind, DomError, DomNode, DomNodeSlotKey,
    DomSelection, RootDom, RootDomMetricName, TextSelection, TextSelectionDirection,
};

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

pub(crate) enum DomEventDispatchOutcome {
    NotDispatched,
    Dispatched(DomEvent),
}

impl DomEventDispatchOutcome {
    #[must_use]
    pub(crate) fn default_prevented(&self) -> bool {
        match self {
            Self::NotDispatched => false,
            Self::Dispatched(event) => event.default_prevented,
        }
    }

    #[must_use]
    pub(crate) fn into_sink_eligible_event(self) -> Option<DomEvent> {
        match self {
            Self::Dispatched(event) if !event.default_prevented && !event.propagation.stopped => {
                Some(event)
            }
            Self::NotDispatched | Self::Dispatched(_) => None,
        }
    }

    #[must_use]
    pub(crate) fn into_sink_eligible_bubbled_event(self) -> Option<DomEvent> {
        self.into_sink_eligible_event()
            .filter(crate::types::DomEvent::bubbles)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationPreDispatchState {
    Checkbox { checked_before: bool },
}

impl RootDom {
    pub(crate) fn point_event_target_key_from_attached_point(
        &self,
        world: &World,
        parent: Entity,
        point: PointPx,
    ) -> Result<Option<DomNodeSlotKey>, DomError> {
        topmost_event_target_from_point(self, world, parent, point)
    }

    pub(crate) fn dispatch_event_to_target_key_and_finish(
        &mut self,
        target_key: DomNodeSlotKey,
        event: DomEvent,
    ) -> Result<DomEventDispatchOutcome, DomError> {
        if !event.bubbles() && !self.target_has_exact_listener(target_key, event.name)? {
            return Ok(DomEventDispatchOutcome::NotDispatched);
        }
        Ok(DomEventDispatchOutcome::Dispatched(
            dispatch_event_to_target_and_finish(self, target_key, event)?,
        ))
    }

    pub(crate) fn apply_default_behavior_from_keyboard_input(
        &mut self,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        let Some(target_key) = self.focused_element_key() else {
            return Ok(false);
        };
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(false);
        };
        match element.behavior().on_keyboard {
            DomElementOnKeyboard::None => Ok(false),
            DomElementOnKeyboard::Button => {
                self.apply_button_on_keyboard(target_key, input, event_stayed_unblocked)
            }
            DomElementOnKeyboard::InputControl => self.apply_input_control_on_keyboard(
                target_key,
                input,
                shift_key,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::Select => self.apply_select_keyboard_default_for_owner(
                target_key,
                input,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::TextEdit => self.apply_text_edit_on_keyboard(
                target_key,
                input,
                shift_key,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::PresentedOptions => self
                .apply_presented_options_text_input_keyboard_navigation_for_owner(
                    target_key,
                    input,
                    event_stayed_unblocked,
                ),
        }
    }

    pub(crate) fn apply_after_focus(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().after_focus {
            DomElementAfterFocus::None => {}
            DomElementAfterFocus::PresentedOptions => {
                let _ = self.open_presented_options_for_focused_text_input_owner(target_key)?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply_on_blur(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().on_blur {
            DomElementOnBlur::None => {}
            DomElementOnBlur::Select => {
                let _ = self.close_select_popup_for_blurred_owner(target_key)?;
            }
            DomElementOnBlur::PresentedOptions => {
                let _ = self.close_presented_options_for_blurred_owner(target_key)?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply_pointer_down_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
        default_target: Option<DomPointEventTarget>,
    ) -> Result<(), DomError> {
        self.pressed_pointer_click_target =
            self.pressed_pointer_click_target_from_raw_target(raw_target_key);
        let Some(target) = default_target else {
            return Ok(());
        };
        self.apply_on_pointer_down(target, point)
    }

    pub(crate) fn apply_pointer_up_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
    ) -> Result<(), DomError> {
        self.finish_range_drag_change_if_dirty()?;
        let pressed_click_target = self.pressed_pointer_click_target.take();
        let Some(pressed_click_target) = pressed_click_target else {
            return Ok(());
        };
        if raw_target_key != Some(pressed_click_target.target_key) {
            return Ok(());
        }
        let _ =
            self.dispatch_click_and_apply_after_click(pressed_click_target.target_key, point)?;
        Ok(())
    }

    pub(crate) fn apply_default_range_drag_for_point(
        &mut self,
        point: PointPx,
    ) -> Result<(), DomError> {
        let Some(session) = self.active_range_drag_session.clone() else {
            return Ok(());
        };
        let Some(ResolvedElementControl::Range(control)) =
            self.resolved_element_control_for_key(session.target_key)
        else {
            self.active_range_drag_session = None;
            return Ok(());
        };
        if control.disabled {
            self.active_range_drag_session = None;
            return Ok(());
        }
        let subtree_point = range_drag_subtree_point(&session, point);
        let next_value =
            self.range_value_string_for_point(session.target_key, subtree_point, control)?;
        let changed = self.set_control_value_and_dispatch_input(session.target_key, next_value)?;
        self.mark_active_range_drag_dirty(session.target_key, changed);
        Ok(())
    }

    fn apply_button_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state == ButtonState::Released {
            if !event_stayed_unblocked {
                return Ok(false);
            }
            return self.apply_button_keyboard_click_on_keyup(target_key, input);
        }
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        self.apply_button_keyboard_click_on_keydown(target_key, input)
    }

    fn apply_input_control_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state == ButtonState::Released {
            if !event_stayed_unblocked {
                return Ok(false);
            }
            return self.apply_input_keyboard_click_on_keyup(target_key, input);
        }
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if let Some(applied) = self.apply_input_keyboard_step_on_keydown(target_key, input)? {
            return Ok(applied);
        }
        if self.apply_presented_options_text_input_keyboard_navigation_for_owner(
            target_key,
            input,
            event_stayed_unblocked,
        )? {
            return Ok(true);
        }
        self.ensure_text_selection_for_target(target_key);
        self.apply_default_text_behavior_and_dispatch_input_from_keyboard_input(input, shift_key)
    }

    fn apply_text_edit_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.apply_presented_options_text_input_keyboard_navigation_for_owner(
            target_key,
            input,
            event_stayed_unblocked,
        )? {
            return Ok(true);
        }
        self.ensure_text_selection_for_target(target_key);
        self.apply_default_text_behavior_and_dispatch_input_from_keyboard_input(input, shift_key)
    }

    fn apply_on_pointer_down(
        &mut self,
        target: DomPointEventTarget,
        root_point: PointPx,
    ) -> Result<(), DomError> {
        let target_key = target.target_key;
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().on_pointer {
            DomElementOnPointer::None => Ok(()),
            DomElementOnPointer::TextEdit => {
                self.ensure_text_selection_for_target(target_key);
                Ok(())
            }
            DomElementOnPointer::InputControl => {
                let Some(control) = self.resolved_element_control_for_key(target_key) else {
                    return Ok(());
                };
                if matches!(control, ResolvedElementControl::TextInput) {
                    self.ensure_text_selection_for_target(target_key);
                    return Ok(());
                }
                let Some(control_part) =
                    self.resolved_control_part_for_key(target_key, target.subtree_point)?
                else {
                    return Ok(());
                };
                match (control, control_part) {
                    (
                        ResolvedElementControl::Number(control),
                        crate::element_controls::ResolvedElementControlPart::NumberDecrementButton,
                    ) if !control.disabled && !control.readonly => self
                        .set_control_value_and_dispatch_input_change(
                            target_key,
                            control
                                .numeric
                                .stepped_value_string(NumericStepDirection::Decrement),
                        )
                        .map(|_| ()),
                    (
                        ResolvedElementControl::Number(control),
                        crate::element_controls::ResolvedElementControlPart::NumberIncrementButton,
                    ) if !control.disabled && !control.readonly => self
                        .set_control_value_and_dispatch_input_change(
                            target_key,
                            control
                                .numeric
                                .stepped_value_string(NumericStepDirection::Increment),
                        )
                        .map(|_| ()),
                    (ResolvedElementControl::Range(control), _) if !control.disabled => {
                        self.start_range_drag_session(
                            target_key,
                            root_point,
                            target.subtree_point,
                        )?;
                        let next_value = self.range_value_string_for_point(
                            target_key,
                            target.subtree_point,
                            control,
                        )?;
                        let changed =
                            self.set_control_value_and_dispatch_input(target_key, next_value)?;
                        self.mark_active_range_drag_dirty(target_key, changed);
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    pub(crate) fn ensure_text_selection_for_target(&mut self, target_key: DomNodeSlotKey) {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            self.set_selection(DomSelection::None);
            return;
        };
        let current_value = match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            None => String::new(),
            Some(_) => panic!("editable element value attribute should stay string"),
        };
        match resolved_text_edit_kind(element) {
            DomElementEditKind::None => {
                self.set_selection(DomSelection::None);
            }
            DomElementEditKind::SingleLine | DomElementEditKind::MultiLine => {
                if matches!(
                    self.selection(),
                    DomSelection::TextSelected(selection) if selection.target_element == target_key
                ) {
                    return;
                }
                let value_len = count_unicode_scalars(&current_value);
                self.set_selection(DomSelection::TextSelected(TextSelection {
                    target_element: target_key,
                    selection_start: value_len,
                    selection_end: value_len,
                    selection_direction: TextSelectionDirection::None,
                }));
            }
        }
    }

    fn apply_input_keyboard_step_on_keydown(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<Option<bool>, DomError> {
        let Some(control) = self.resolved_element_control_for_key(target_key) else {
            return Ok(Some(false));
        };
        match control {
            ResolvedElementControl::Checkbox(_) | ResolvedElementControl::Textarea => {
                Ok(Some(false))
            }
            ResolvedElementControl::Number(control) => {
                let Some(next_value) =
                    numeric_keyboard_value_string(control.numeric, &input.logical_key)
                else {
                    return Ok(None);
                };
                if control.disabled || control.readonly {
                    return Ok(Some(false));
                }
                self.set_control_value_and_dispatch_input_change(target_key, next_value)
                    .map(Some)
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
                self.set_control_value_and_dispatch_input_change(target_key, next_value)
                    .map(Some)
            }
            ResolvedElementControl::TextInput => Ok(None),
        }
    }

    fn apply_button_keyboard_click_on_keydown(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if input.repeat
            || !matches!(input.logical_key, Key::Enter)
            || self.target_has_disabled_attr(target_key)?
        {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn apply_button_keyboard_click_on_keyup(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if !matches!(input.logical_key, Key::Space) || self.target_has_disabled_attr(target_key)? {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn apply_input_keyboard_click_on_keyup(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if !matches!(input.logical_key, Key::Space) {
            return Ok(false);
        }
        let Some(ResolvedElementControl::Checkbox(control)) =
            self.resolved_element_control_for_key(target_key)
        else {
            return Ok(false);
        };
        if control.disabled {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn pressed_pointer_click_target_from_raw_target(
        &self,
        target_key: Option<DomNodeSlotKey>,
    ) -> Option<PressedPointerClickTarget> {
        let target_key = target_key?;
        let _element = self.node(target_key).and_then(DomNode::as_element)?;
        if self.target_has_disabled_attr(target_key).ok()? {
            return None;
        }
        Some(PressedPointerClickTarget { target_key })
    }

    fn dispatch_click_and_apply_after_click(
        &mut self,
        target_key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<bool, DomError> {
        let pre_activation_state = self.apply_after_click_before_dispatch(target_key)?;
        let outcome = self.dispatch_click_event_to_target_key(target_key, point)?;
        let default_prevented = outcome.default_prevented();
        if let Some(event) = outcome.into_sink_eligible_bubbled_event() {
            self.push_emitted_event(event);
        }
        if default_prevented {
            self.revert_after_click_after_canceled_dispatch(target_key, pre_activation_state)?;
            return Ok(false);
        }
        self.commit_after_click_after_dispatch(target_key)
    }

    fn dispatch_click_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<DomEventDispatchOutcome, DomError> {
        let outcome = self.dispatch_event_to_target_key_and_finish(
            target_key,
            DomEvent::new(
                DomEventName::Click,
                DomEventDetail::Mouse(DomMouseEvent {
                    button: Some(crate::types::dom_event::DomMouseButton::Primary),
                    buttons: 0,
                    client_x: point.x_px,
                    client_y: point.y_px,
                    modifiers: crate::types::dom_event::DomModifierState::default(),
                }),
            ),
        )?;
        if matches!(outcome, DomEventDispatchOutcome::Dispatched(_)) {
            self.increment_metric(RootDomMetricName::ClickDispatched);
        }
        Ok(outcome)
    }

    fn apply_after_click_before_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<Option<ActivationPreDispatchState>, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(None);
        };
        match Self::resolved_on_activation_for_element(element) {
            DomElementOnActivation::None
            | DomElementOnActivation::Select
            | DomElementOnActivation::PresentedOptions => Ok(None),
            DomElementOnActivation::Toggle => {
                let Some(ResolvedElementControl::Checkbox(control)) =
                    resolved_element_control(element)
                else {
                    return Ok(None);
                };
                if control.disabled {
                    return Ok(None);
                }
                self.set_attr(
                    target_key,
                    DomAttribute::Checked,
                    Some(DomAttributeValue::Bool(!control.checked)),
                )?;
                self.increment_metric(RootDomMetricName::ActivationPreApplied);
                Ok(Some(ActivationPreDispatchState::Checkbox {
                    checked_before: control.checked,
                }))
            }
        }
    }

    fn revert_after_click_after_canceled_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
        pre_activation_state: Option<ActivationPreDispatchState>,
    ) -> Result<(), DomError> {
        let Some(pre_activation_state) = pre_activation_state else {
            return Ok(());
        };
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        if Self::resolved_on_activation_for_element(element) != DomElementOnActivation::Toggle {
            return Ok(());
        }
        match pre_activation_state {
            ActivationPreDispatchState::Checkbox { checked_before } => {
                self.set_attr(
                    target_key,
                    DomAttribute::Checked,
                    Some(DomAttributeValue::Bool(checked_before)),
                )?;
                self.increment_metric(RootDomMetricName::ActivationCanceled);
            }
        }
        Ok(())
    }

    fn commit_after_click_after_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.apply_select_popup_row_click_default_for_target(target_key)? {
            return Ok(true);
        }
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(false);
        };
        match Self::resolved_on_activation_for_element(element) {
            DomElementOnActivation::None => Ok(true),
            DomElementOnActivation::Select => {
                self.apply_select_click_default_for_target(target_key)
            }
            DomElementOnActivation::Toggle => {
                let Some(ResolvedElementControl::Checkbox(control)) =
                    resolved_element_control(element)
                else {
                    return Ok(true);
                };
                if control.disabled {
                    return Ok(false);
                }
                let Some(current_value) = self.control_value_event_string(target_key)? else {
                    return Ok(false);
                };
                let _ =
                    self.dispatch_input_event_to_target_key(target_key, current_value.clone())?;
                let _ = self.dispatch_change_event_to_target_key(target_key, current_value)?;
                self.increment_metric(RootDomMetricName::ActivationCommitted);
                Ok(true)
            }
            DomElementOnActivation::PresentedOptions => {
                self.apply_presented_options_click_default_for_target(target_key)
            }
        }
    }

    fn resolved_on_activation_for_element(
        element: &crate::types::DomElementNode,
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

    fn target_has_disabled_attr(&self, target_key: DomNodeSlotKey) -> Result<bool, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Err(DomError::InvalidAttributeTarget);
        };
        Ok(matches!(
            element.attrs.attributes.get(&DomAttribute::Disabled),
            Some(DomAttributeValue::Bool(true))
        ))
    }

    fn keyboard_activation_click_point_for_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Result<PointPx, DomError> {
        let node = self.node(target_key).ok_or(DomError::InvalidNodeKey)?;
        let border_box = node_box_from_ref(node).border_box;
        Ok(PointPx {
            x_px: border_box.left_px + border_box.width_px * 0.5,
            y_px: border_box.top_px + border_box.height_px * 0.5,
        })
    }

    fn resolved_element_control_for_key(
        &self,
        key: DomNodeSlotKey,
    ) -> Option<ResolvedElementControl> {
        let element = self.node(key)?.as_element()?;
        resolved_element_control(element)
    }

    fn resolved_control_part_for_key(
        &self,
        key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<Option<crate::element_controls::ResolvedElementControlPart>, DomError> {
        let Some(element) = self.node(key).and_then(DomNode::as_element) else {
            return Ok(None);
        };
        Ok(resolved_control_part_from_point(
            element,
            node_box_from_ref(self.node(key).ok_or(DomError::InvalidNodeKey)?),
            point,
        ))
    }

    fn range_value_string_for_point(
        &self,
        key: DomNodeSlotKey,
        point: PointPx,
        control: crate::element_controls::ResolvedRangeControl,
    ) -> Result<String, DomError> {
        Ok(range_value_string_from_point(
            node_box_from_ref(self.node(key).ok_or(DomError::InvalidNodeKey)?),
            control,
            point,
        ))
    }

    fn start_range_drag_session(
        &mut self,
        target_key: DomNodeSlotKey,
        root_point: PointPx,
        subtree_point: PointPx,
    ) -> Result<(), DomError> {
        let focus_entry_value = self
            .control_value_event_string(target_key)?
            .unwrap_or_else(|| String::from("0"));
        self.active_range_drag_session = Some(ActiveRangeDragSession {
            target_key,
            root_to_subtree_point_offset: PointPx {
                x_px: subtree_point.x_px - root_point.x_px,
                y_px: subtree_point.y_px - root_point.y_px,
            },
            focus_entry_value,
            dirty: false,
        });
        Ok(())
    }

    fn mark_active_range_drag_dirty(&mut self, target_key: DomNodeSlotKey, changed: bool) {
        if !changed {
            return;
        }
        if let Some(session) = self.active_range_drag_session.as_mut()
            && session.target_key == target_key
        {
            session.dirty = true;
        }
    }

    fn finish_range_drag_change_if_dirty(&mut self) -> Result<(), DomError> {
        let Some(session) = self.active_range_drag_session.take() else {
            return Ok(());
        };
        if !session.dirty {
            return Ok(());
        }
        let Some(current_value) = self.control_value_event_string(session.target_key)? else {
            return Ok(());
        };
        if current_value != session.focus_entry_value {
            let _ = self.dispatch_change_event_to_target_key(session.target_key, current_value)?;
        }
        Ok(())
    }

    fn set_control_value_and_dispatch_input_change(
        &mut self,
        target_key: DomNodeSlotKey,
        next_value: String,
    ) -> Result<bool, DomError> {
        let changed = self.set_control_value_and_dispatch_input(target_key, next_value.clone())?;
        if !changed {
            return Ok(false);
        }
        let _ = self.dispatch_change_event_to_target_key(target_key, next_value)?;
        Ok(true)
    }

    fn set_control_value_and_dispatch_input(
        &mut self,
        target_key: DomNodeSlotKey,
        next_value: String,
    ) -> Result<bool, DomError> {
        let current_value = self
            .control_value_event_string(target_key)?
            .unwrap_or_default();
        if current_value == next_value {
            return Ok(false);
        }
        self.set_attr(
            target_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(next_value.clone())),
        )?;
        if self.focused_element_key() == Some(target_key)
            && matches!(
                self.node(target_key)
                    .and_then(DomNode::as_element)
                    .map(resolved_text_edit_kind),
                Some(DomElementEditKind::SingleLine | DomElementEditKind::MultiLine)
            )
        {
            let value_len = count_unicode_scalars(&next_value);
            self.set_selection(DomSelection::TextSelected(TextSelection {
                target_element: target_key,
                selection_start: value_len,
                selection_end: value_len,
                selection_direction: TextSelectionDirection::None,
            }));
        }
        let _ = self.dispatch_input_event_to_target_key(target_key, next_value)?;
        Ok(true)
    }

    fn control_value_event_string(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Result<Option<String>, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
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
}

fn numeric_keyboard_value_string(
    numeric: crate::element_controls::ResolvedNumericControl,
    logical_key: &Key,
) -> Option<String> {
    let key_name = match logical_key {
        Key::ArrowDown => "ArrowDown",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageDown => "PageDown",
        Key::PageUp => "PageUp",
        _ => return None,
    };
    numeric.keyboard_step_value_string(key_name)
}

fn range_drag_subtree_point(session: &ActiveRangeDragSession, root_point: PointPx) -> PointPx {
    PointPx {
        x_px: root_point.x_px + session.root_to_subtree_point_offset.x_px,
        y_px: root_point.y_px + session.root_to_subtree_point_offset.y_px,
    }
}

fn count_unicode_scalars(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
use crate::domains::hit_test::hit_test_from_dom_subtree_cluster_local;
#[cfg(test)]
use crate::domains::layer_hit::topmost_attached_layer_hit_from_root_point;
#[cfg(test)]
use crate::domains::paint::build_layer_paint_plan;
#[cfg(test)]
use crate::domains::visitor::inherited_style::InheritedStyleContext;
#[cfg(test)]
use crate::types::{DomPointerState, DomPointerUpdate, DomRect, DomTag, RootDomContext};
#[cfg(test)]
use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{image::Image, prelude::*};

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn append_select(root: &mut RootDom) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(root.root, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn append_select_to_parent(root: &mut RootDom, parent: DomNodeSlotKey) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(parent, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn attach_root_with_size(
        root: &mut RootDom,
        world: &mut World,
        width_px: f32,
        height_px: f32,
    ) -> Entity {
        install_direct_gpu_support(world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(width_px, height_px)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(world, parent, 0.0)
            .expect("attach root");
        root.update(world, 0.0, DomPointerUpdate::Unchanged)
            .expect("present once");
        parent
    }

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

    fn input_value(root: &RootDom, input: DomNodeSlotKey) -> String {
        let element = root
            .node(input)
            .and_then(DomNode::as_element)
            .expect("input element should exist");
        match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            other => panic!("expected input string value, got {other:?}"),
        }
    }

    struct SelectPopupProof {
        shell_entity: Entity,
        shell_box: DomRect,
        row_keys: Vec<DomNodeSlotKey>,
        popup_scroll_key: DomNodeSlotKey,
        popup_scroll_entity: Entity,
    }

    fn select_popup_proof(root: &RootDom, select: DomNodeSlotKey) -> SelectPopupProof {
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_box = node_box_from_ref(root.node(shell_key).expect("shell")).border_box;
        let row_keys = root.presented_options_popup_row_keys(select);
        let first_row_key = row_keys
            .first()
            .copied()
            .expect("popup should contain at least one row");
        let popup_scroll_key = root
            .node(first_row_key)
            .and_then(DomNode::parent_key)
            .expect("popup row should have a scroll parent");
        let popup_scroll_entity = root
            .layer_entity(popup_scroll_key)
            .expect("popup scroll layer should have a Bevy entity");
        SelectPopupProof {
            shell_entity,
            shell_box,
            row_keys,
            popup_scroll_key,
            popup_scroll_entity,
        }
    }

    fn assert_popup_surface_order_and_text(
        root: &mut RootDom,
        world: &World,
        parent: Entity,
        proof: &SelectPopupProof,
    ) {
        assert!(
            proof.shell_box.height_px >= 56.0,
            "select popup shell should keep enough height for two rows, got {}",
            proof.shell_box.height_px
        );
        assert_eq!(proof.row_keys.len(), 2);
        let ordered_surface_entities = world
            .resource::<LayerPoolRenderJobQueue>()
            .ordered_surface_entities_for_test(parent.into())
            .expect("root update should publish retained surface order");
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.shell_entity.into()),
            Some(1),
            "popup shell should be raised above the document layer"
        );
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.popup_scroll_entity.into()),
            Some(2),
            "popup scroll child should render after its raised shell"
        );
        assert_popup_text_runs_inside_scroll_surface(root, proof.popup_scroll_key);
    }

    fn assert_popup_text_runs_inside_scroll_surface(
        root: &mut RootDom,
        popup_scroll_key: DomNodeSlotKey,
    ) {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let popup_scroll_plan = build_layer_paint_plan(
            root,
            &mut layer_pool,
            popup_scroll_key,
            (120.0, 56.0),
            &InheritedStyleContext::default(),
        )
        .expect("popup scroll paint plan");
        assert_eq!(popup_scroll_plan.text_runs.len(), 2);
        assert_close(popup_scroll_plan.text_runs[0].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[0].position_px[1], 5.0);
        assert_close(popup_scroll_plan.text_runs[0].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[0].line_height_px, 18.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[1], 33.0);
        assert_close(popup_scroll_plan.text_runs[1].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[1].line_height_px, 18.0);
        for text_run in &popup_scroll_plan.text_runs {
            assert!(
                text_run.size_px[0] > 0.0 && text_run.size_px[1] > 0.0,
                "popup option text run should paint with non-zero size: {text_run:?}"
            );
            assert!(
                text_run.position_px[0] >= 0.0
                    && text_run.position_px[0] < 120.0
                    && text_run.position_px[1] >= 0.0
                    && text_run.position_px[1] < 56.0,
                "popup option text run should paint inside the scroll surface: {text_run:?}"
            );
        }
    }

    fn assert_second_popup_row_mouse_click_commits(
        root: &mut RootDom,
        world: &mut World,
        parent: Entity,
        select: DomNodeSlotKey,
        proof: &SelectPopupProof,
    ) {
        let second_row_point = PointPx {
            x_px: 48.0,
            y_px: 130.0,
        };
        let second_row_layer_hit =
            topmost_attached_layer_hit_from_root_point(root, world, second_row_point)
                .expect("popup row layer hit should resolve");
        let second_row_subtree_hit = second_row_layer_hit.and_then(|layer_hit| {
            hit_test_from_dom_subtree_cluster_local(
                root,
                layer_hit.node_key,
                layer_hit.point,
                layer_hit.effective_rect,
            )
            .expect("popup row subtree hit should resolve")
        });
        let root_box = node_box_from_ref(root.node(root.root).expect("root")).border_box;
        assert_eq!(
            root.point_event_target_key_from_attached_point(world, parent, second_row_point)
                .expect("popup row hit target should resolve"),
            Some(proof.row_keys[1]),
            "pointer hit target should be the popup row under the cursor; root box: {root_box:?}; shell box: {:?}; layer hit: {second_row_layer_hit:?}; subtree hit: {second_row_subtree_hit:?}",
            proof.shell_box,
        );
        let down_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, true),
            )
            .expect("second row down");
        let target_after_down = root
            .point_event_target_key_from_attached_point(world, parent, second_row_point)
            .expect("target after row down should resolve");
        assert_eq!(
            target_after_down,
            Some(proof.row_keys[1]),
            "popup row should remain the hit target after pointer down; down events: {down_events:?}",
        );
        let up_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, false),
            )
            .expect("second row up");
        assert_eq!(
            input_value(root, select),
            "beta",
            "mouse click on a raised popup row should commit the row value; down events: {down_events:?}; up events: {up_events:?}",
        );
    }

    #[test]
    fn pointer_click_opens_select_presented_options_after_click_activation() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let select = append_select(&mut root);
        root.set_attr_str(select, "margin-left", "12px")
            .expect("select margin left");
        root.set_attr_str(select, "margin-top", "40px")
            .expect("select margin top");
        root.set_border_box_for_test(root.root, 0.0, 0.0, 180.0, 180.0);
        let mut world = World::new();
        attach_root_with_size(&mut root, &mut world, 180.0, 180.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 17.0,
            y_px: 45.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let up_events = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        assert_eq!(
            event_names(&up_events),
            vec![DomEventName::PointerUp, DomEventName::Click]
        );
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        assert!(
            root.layer_entity(shell_key).is_some(),
            "select popup shell should materialize as a layer"
        );
        assert!(
            root.raised_layer_roots().contains(&shell_key),
            "select popup shell should use raised layer ordering"
        );
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_transform = world
            .get::<Transform>(shell_entity)
            .expect("select popup shell transform");
        assert_close(shell_transform.translation.x, 12.0);
        assert_close(shell_transform.translation.y, -68.0);
    }

    #[test]
    fn pointer_click_anchors_select_popup_to_nested_owner_box() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        root.append_child(root.root, wrapper)
            .expect("append wrapper");
        root.set_attr_str(wrapper, "display", "block")
            .expect("wrapper display");
        root.set_attr_str(wrapper, "margin-left", "31px")
            .expect("wrapper margin left");
        root.set_attr_str(wrapper, "margin-top", "53px")
            .expect("wrapper margin top");
        root.set_attr_str(wrapper, "padding-left", "9px")
            .expect("wrapper padding left");
        root.set_attr_str(wrapper, "padding-top", "7px")
            .expect("wrapper padding top");
        root.set_attr_str(root.root, "width", "240px")
            .expect("root width");
        root.set_attr_str(root.root, "height", "240px")
            .expect("root height");
        let select = append_select_to_parent(&mut root, wrapper);
        let mut world = World::new();
        let parent = attach_root_with_size(&mut root, &mut world, 240.0, 240.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 45.0,
            y_px: 65.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        let proof = select_popup_proof(&root, select);
        let shell_transform = world
            .get::<Transform>(proof.shell_entity)
            .expect("select popup shell transform");
        assert_popup_surface_order_and_text(&mut root, &world, parent, &proof);
        assert_close(shell_transform.translation.x, 40.0);
        assert_close(shell_transform.translation.y, -88.0);
        assert_second_popup_row_mouse_click_commits(&mut root, &mut world, parent, select, &proof);
    }
}
