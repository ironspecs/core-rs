//! Select default-behavior hooks (PR 5, E6-a).
//!
//! This file owns the `<select>` control kind's thin hooks over the popup
//! lifecycle that lives in `popup.rs` (E6-b). The select's keyboard,
//! click-commit, and blur handlers delegate to
//! `apply_select_keyboard_default_for_owner`,
//! `apply_select_click_default_for_target`, and
//! `close_select_popup_for_blurred_owner` respectively.

use bevy::input::keyboard::KeyboardInput;

use crate::types::{DomError, DomNodeSlotKey, RootDom};

use super::popup::{
    apply_select_click_default_for_target, apply_select_keyboard_default_for_owner,
    close_select_popup_for_blurred_owner,
};

#[cfg(test)]
use super::popup::presented_options_popup_shell_key;
#[cfg(test)]
use crate::types::{DomAttribute, DomAttributeValue, DomTag, RootDomContext};

/// The select row's keyboard hook.
pub(crate) fn select_on_keyboard(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
    _shift_key: bool,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    apply_select_keyboard_default_for_owner(root, target_key, input, event_stayed_unblocked)
}

/// The select row's click-commit hook.
pub(crate) fn select_click_commit(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    apply_select_click_default_for_target(root, target_key)
}

/// The select row's blur hook.
pub(crate) fn select_on_blur(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let _ = close_select_popup_for_blurred_owner(root, target_key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyCode},
    };

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

    fn append_select_with_one_option(root: &mut RootDom) -> DomNodeSlotKey {
        let owner_key = root.create_element(DomTag::Select).expect("select");
        let option = root.create_element(DomTag::Option).expect("option");
        let text = root.create_text_node("alpha").expect("text");
        root.append_child(root.root, owner_key)
            .expect("attach select");
        root.append_child(owner_key, option).expect("attach option");
        root.append_child(option, text).expect("attach text");
        root.set_attr(
            option,
            DomAttribute::Value,
            Some(DomAttributeValue::String("alpha".to_string())),
        )
        .expect("option value");
        root.set_border_box_for_test(root.root, 0.0, 0.0, 240.0, 240.0);
        root.set_border_box_for_test(owner_key, 0.0, 0.0, 120.0, 28.0);
        owner_key
    }

    #[test]
    fn select_arrow_down_opens_the_presented_options_popup() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let owner_key = append_select_with_one_option(&mut root);

        let input = keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown);
        let applied =
            select_on_keyboard(&mut root, owner_key, &input, false, true).expect("select keyboard");

        assert!(applied);
        assert!(presented_options_popup_shell_key(&root, owner_key).is_some());
    }

    #[test]
    fn select_blur_without_popup_is_a_no_op() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let owner_key = append_select_with_one_option(&mut root);

        select_on_blur(&mut root, owner_key).expect("select blur");
    }
}
