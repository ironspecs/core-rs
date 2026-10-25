//! Button default-behavior hooks (PR 5, E6-a).
//!
//! This file owns the `<button>` control kind's keyboard behavior: Enter on
//! keydown and Space on keyup both synthesize a click through the generic
//! activation lifecycle. The bodies are byte-identical to the former
//! `RootDom` methods (`apply_button_on_keyboard`,
//! `apply_button_keyboard_click_on_key{down,up}`), modulo `self → root`.

use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

use crate::types::{DomError, DomNodeSlotKey, RootDom};

use super::activation::{
    dispatch_click_and_apply_after_click, keyboard_activation_click_point_for_target,
    target_has_disabled_attr,
};

#[cfg(test)]
use crate::types::{
    DomAttribute, DomAttributeValue, DomEventName, DomPointerUpdate, DomTag, RootDomContext,
};

/// The button row's keyboard hook.
pub(crate) fn button_on_keyboard(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
    _shift_key: bool,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    if input.state == ButtonState::Released {
        if !event_stayed_unblocked {
            return Ok(false);
        }
        return button_keyboard_click_on_keyup(root, target_key, input);
    }
    if input.state != ButtonState::Pressed || !event_stayed_unblocked {
        return Ok(false);
    }
    button_keyboard_click_on_keydown(root, target_key, input)
}

fn button_keyboard_click_on_keydown(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
) -> Result<bool, DomError> {
    if input.repeat
        || !matches!(input.logical_key, Key::Enter)
        || target_has_disabled_attr(root, target_key)?
    {
        return Ok(false);
    }
    let point = keyboard_activation_click_point_for_target(root, target_key)?;
    dispatch_click_and_apply_after_click(root, target_key, point)
}

fn button_keyboard_click_on_keyup(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
) -> Result<bool, DomError> {
    if !matches!(input.logical_key, Key::Space) || target_has_disabled_attr(root, target_key)? {
        return Ok(false);
    }
    let point = keyboard_activation_click_point_for_target(root, target_key)?;
    dispatch_click_and_apply_after_click(root, target_key, point)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::KeyCode;

    fn keyboard_input(state: ButtonState, key_code: KeyCode, logical_key: Key) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: bevy::prelude::Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn button_enter_keydown_dispatches_click() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");

        let input = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        let applied = button_on_keyboard(&mut root, button, &input, false, true).expect("keyboard");
        assert!(applied);

        let mut world = bevy::prelude::World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::Click]
        );
    }

    #[test]
    fn disabled_button_keydown_does_not_dispatch_click() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.set_attr(
            button,
            DomAttribute::Disabled,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("disabled");

        let input = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        let applied = button_on_keyboard(&mut root, button, &input, false, true).expect("keyboard");
        assert!(!applied);
    }
}
