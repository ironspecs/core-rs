//! Bevy keyboard-to-DOM dispatch helpers.
//!
//! This module converts Bevy keyboard input into `DomEvent` payloads and
//! routes them through the focused DOM target in `RootDom`.

use bevy::input::{
    ButtonInput, ButtonState,
    keyboard::{Key, KeyCode, KeyboardInput},
};

use crate::types::{
    DomError, DomEvent, DomEventDetail, DomEventName, DomKeyboardEvent, DomModifierState,
    DomSystemModifierState, RootDom,
};

#[cfg(test)]
use crate::types::{
    DomAttribute, DomAttributeValue, DomEventListenerContext, DomEventListenerOptions,
    DomEventPhase, DomNode, DomNodeSlotKey, DomPointerUpdate, DomSelection, DomTag, RootDomContext,
    TextSelection, TextSelectionDirection,
};

/// Dispatches one Bevy keyboard input through the focused DOM element and
/// runs root-owned default keyboard behavior for accepted events.
///
/// Routing starts from the focused element when present. If no element is
/// focused, the event targets the document root.
///
/// Returns whether the keyboard event finished unblocked and stayed
/// sink-eligible. Accepted events may also move the focused text caret,
/// mutate the focused text value, synthesize activation clicks, and emit
/// value-change events when default behavior performs a real edit or
/// committed activation.
pub fn dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
    root_dom: &mut RootDom,
    input: &KeyboardInput,
    keycodes: &ButtonInput<KeyCode>,
) -> Result<bool, DomError> {
    let event = dom_event_from_keyboard_input(input, keycodes);
    let target_key = root_dom.focused_element_key().unwrap_or(root_dom.root);
    let accepted =
        root_dom.dispatch_event_to_target_key_and_record_if_bubbled(target_key, event)?;
    let shift_key = keycodes.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let _ = root_dom.apply_default_behavior_from_keyboard_input(input, shift_key, accepted)?;
    Ok(accepted)
}

fn dom_event_from_keyboard_input(
    input: &KeyboardInput,
    keycodes: &ButtonInput<KeyCode>,
) -> DomEvent {
    DomEvent::new(
        dom_event_name_from_button_state(input.state),
        DomEventDetail::Keyboard(DomKeyboardEvent {
            key: dom_key_from_keyboard_input(input),
            code: dom_code_from_keyboard_input(input),
            repeat: input.repeat,
            modifiers: dom_modifiers_from_keyboard_input(input, keycodes),
        }),
    )
}

fn dom_event_name_from_button_state(state: ButtonState) -> DomEventName {
    match state {
        ButtonState::Pressed => DomEventName::KeyDown,
        ButtonState::Released => DomEventName::KeyUp,
    }
}

fn dom_key_from_keyboard_input(input: &KeyboardInput) -> String {
    if matches!(input.logical_key, Key::Character(_) | Key::Space)
        && let Some(text) = input.text.as_ref()
        && !text.is_empty()
    {
        return text.as_str().to_owned();
    }
    key_name_from_logical_key(&input.logical_key)
}

fn key_name_from_logical_key(key: &Key) -> String {
    match key {
        Key::Character(value) => value.as_str().to_owned(),
        Key::Enter => "Enter".to_owned(),
        Key::Escape => "Escape".to_owned(),
        Key::Tab => "Tab".to_owned(),
        Key::Space => " ".to_owned(),
        Key::ArrowDown => "ArrowDown".to_owned(),
        Key::ArrowUp => "ArrowUp".to_owned(),
        Key::ArrowLeft => "ArrowLeft".to_owned(),
        Key::ArrowRight => "ArrowRight".to_owned(),
        Key::Backspace => "Backspace".to_owned(),
        Key::Delete => "Delete".to_owned(),
        Key::Home => "Home".to_owned(),
        Key::End => "End".to_owned(),
        Key::PageDown => "PageDown".to_owned(),
        Key::PageUp => "PageUp".to_owned(),
        Key::Shift => "Shift".to_owned(),
        Key::Control => "Control".to_owned(),
        Key::Alt => "Alt".to_owned(),
        Key::Meta | Key::Super => "Meta".to_owned(),
        _ => "Unidentified".to_owned(),
    }
}

fn dom_code_from_keyboard_input(input: &KeyboardInput) -> String {
    format!("{:?}", input.key_code)
}

fn dom_modifiers_from_keyboard_input(
    input: &KeyboardInput,
    keycodes: &ButtonInput<KeyCode>,
) -> DomModifierState {
    let mut modifiers = DomModifierState {
        alt_key: keycodes.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        ctrl_key: keycodes.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        system: DomSystemModifierState {
            meta_key: keycodes.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]),
            shift_key: keycodes.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        },
    };

    // Reconcile the current key event into modifier state so callers do not
    // depend on Bevy input-system ordering for accurate modifier flags.
    let modifier_patch = match input.key_code {
        KeyCode::AltLeft | KeyCode::AltRight => Some(ModifierKind::Alt),
        KeyCode::ControlLeft | KeyCode::ControlRight => Some(ModifierKind::Control),
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Some(ModifierKind::Shift),
        KeyCode::SuperLeft | KeyCode::SuperRight => Some(ModifierKind::Meta),
        _ => None,
    };
    if let Some(modifier_patch) = modifier_patch {
        let pressed = input.state == ButtonState::Pressed;
        match modifier_patch {
            ModifierKind::Alt => modifiers.alt_key = pressed,
            ModifierKind::Control => modifiers.ctrl_key = pressed,
            ModifierKind::Shift => modifiers.system.shift_key = pressed,
            ModifierKind::Meta => modifiers.system.meta_key = pressed,
        }
    }
    modifiers
}

#[derive(Clone, Copy)]
enum ModifierKind {
    Alt,
    Control,
    Shift,
    Meta,
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use bevy::{
        ecs::entity::Entity,
        input::{
            ButtonInput, ButtonState,
            keyboard::{Key, KeyCode, KeyboardInput},
        },
        prelude::World,
    };

    use super::*;

    #[test]
    fn dispatches_keyboard_event_along_focused_path() {
        let _guard = keyboard_dispatch_test_guard();
        clear_keyboard_dispatch_log();

        let mut root = RootDom::new(RootDomContext::new_cpu());
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Button).expect("child");
        root.append_child(root.root, parent).expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.focus(child).expect("focus child");

        root.nodes
            .get_mut(root.root)
            .expect("root node")
            .add_event_listener(
                DomEventName::KeyDown,
                record_root_capture,
                Some(capture_options()),
            );
        root.nodes
            .get_mut(parent)
            .expect("parent node")
            .add_event_listener(
                DomEventName::KeyDown,
                record_parent_capture,
                Some(capture_options()),
            );
        root.nodes
            .get_mut(child)
            .expect("child node")
            .add_event_listener(
                DomEventName::KeyDown,
                record_target_capture,
                Some(capture_options()),
            );
        root.nodes
            .get_mut(child)
            .expect("child node")
            .add_event_listener(DomEventName::KeyDown, record_target_bubble, None);
        root.nodes
            .get_mut(parent)
            .expect("parent node")
            .add_event_listener(DomEventName::KeyDown, record_parent_bubble, None);

        let input = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch should succeed");

        assert!(accepted);
        assert_eq!(
            keyboard_dispatch_log_snapshot(),
            vec![
                "root:capture",
                "parent:capture",
                "target:capture",
                "target:bubble",
                "parent:bubble",
            ]
        );
    }

    #[test]
    fn falls_back_to_document_root_when_focus_is_absent() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let root_key = root.root;
        root.nodes
            .get_mut(root_key)
            .expect("root node")
            .add_event_listener(DomEventName::KeyDown, prevent_default_listener, None);

        let input = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch should succeed");

        assert!(!accepted);
    }

    #[test]
    fn returns_error_for_stale_focus_target() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let child = root.create_element(DomTag::Button).expect("child");
        root.append_child(root.root, child).expect("append child");
        root.focus(child).expect("focus child");
        let removed = root.nodes.remove(child);
        assert!(removed.is_some(), "child should remove");

        let input = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        let result = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        );
        assert_eq!(
            result.expect_err("stale focus should fail"),
            DomError::InvalidNodeKey
        );
    }

    #[test]
    fn maps_keyboard_payload_fields() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");

        root.nodes
            .get_mut(button)
            .expect("button")
            .add_event_listener(
                DomEventName::KeyDown,
                assert_keyboard_payload_listener,
                None,
            );
        let mut keycodes = ButtonInput::default();
        keycodes.press(KeyCode::ControlLeft);
        keycodes.press(KeyCode::ShiftLeft);
        keycodes.press(KeyCode::AltLeft);
        keycodes.press(KeyCode::SuperLeft);
        let mut input = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("ignored".into()),
        );
        input.text = Some("A".into());
        input.repeat = true;

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root, &input, &keycodes,
        )
        .expect("dispatch");
        assert!(accepted);
    }

    #[test]
    fn enter_key_payload_stays_enter_when_text_payload_exists() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");

        root.nodes
            .get_mut(button)
            .expect("button")
            .add_event_listener(
                DomEventName::KeyDown,
                assert_enter_keyboard_payload_listener,
                None,
            );

        let mut input = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        input.text = Some("\n".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
    }

    #[test]
    fn released_state_dispatches_key_up_events() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");
        root.nodes
            .get_mut(button)
            .expect("button")
            .add_event_listener(DomEventName::KeyUp, prevent_default_listener, None);

        let input = keyboard_input(ButtonState::Released, KeyCode::Enter, Key::Enter);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(!accepted, "keyup listener should receive released event");
    }

    #[test]
    fn button_enter_keydown_dispatches_click() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");

        let input = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::FocusIn,
                DomEventName::KeyDown,
                DomEventName::Click
            ],
        );
    }

    #[test]
    fn button_space_keyup_dispatches_click() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");

        let keydown = keyboard_input(ButtonState::Pressed, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keydown,
            &ButtonInput::default(),
        )
        .expect("keydown dispatch");
        assert!(accepted);

        let mut world = World::new();
        let keydown_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("keydown drain");
        assert_eq!(
            keydown_drained
                .iter()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );

        let keyup = keyboard_input(ButtonState::Released, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keyup,
            &ButtonInput::default(),
        )
        .expect("keyup dispatch");
        assert!(accepted);

        let keyup_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("keyup drain");
        assert_eq!(
            keyup_drained
                .iter()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            vec![DomEventName::KeyUp, DomEventName::Click],
        );
    }

    #[test]
    fn accepted_keyboard_event_drains_on_next_update() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append");
        root.focus(button).expect("focus");

        let input = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted, "accepted keydown should stay drainable");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued keyboard event should drain");

        assert_eq!(drained.len(), 2, "focusin and keydown should both drain");
        assert_eq!(drained[1].name, DomEventName::KeyDown);
        assert_eq!(drained[1].target_key, Some(button));
    }

    #[test]
    fn accepted_printable_keydown_inserts_text_and_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "é").expect("value");
        root.focus(input).expect("focus");

        let mut input_event = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        input_event.text = Some("a".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "éa");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 2,
                selection_end: 2,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::FocusIn,
                DomEventName::KeyDown,
                DomEventName::Input
            ],
        );
    }

    #[test]
    fn accepted_backspace_deletes_previous_unicode_scalar_and_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "a🙂").expect("value");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::Backspace, Key::Backspace);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "a");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 1,
                selection_end: 1,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(drained[2].name, DomEventName::Input);
    }

    #[test]
    fn accepted_delete_deletes_next_unicode_scalar_and_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.focus(input).expect("focus");
        root.set_attr_str(input, "value", "a🙂").expect("value");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::Delete, Key::Delete);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "🙂");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 0,
                selection_end: 0,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(drained[2].name, DomEventName::Input);
    }

    #[test]
    fn keyup_never_mutates_or_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.focus(input).expect("focus");

        let mut input_event = keyboard_input(
            ButtonState::Released,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        input_event.text = Some("a".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyUp],
        );
    }

    #[test]
    fn prevented_keydown_blocks_default_text_behavior_and_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.focus(input).expect("focus");
        root.nodes
            .get_mut(input)
            .expect("input node")
            .add_event_listener(DomEventName::KeyDown, prevent_default_listener, None);

        let mut input_event = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        input_event.text = Some("a".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(!accepted);
        assert_eq!(element_value(&root, input), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn],
        );
    }

    #[test]
    fn readonly_input_does_not_mutate_or_drain_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.set_attr(
            input,
            DomAttribute::ReadOnly,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("readonly attr");
        root.focus(input).expect("focus");

        let mut input_event = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        input_event.text = Some("a".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn enter_in_single_line_input_leaves_value_unchanged() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn arrow_left_moves_the_collapsed_caret_without_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "é🙂").expect("value");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::ArrowLeft, Key::ArrowLeft);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "é🙂");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 1,
                selection_end: 1,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn arrow_right_moves_the_collapsed_caret_without_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "é🙂").expect("value");
        root.focus(input).expect("focus");
        let home_input = keyboard_input(ButtonState::Pressed, KeyCode::Home, Key::Home);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &home_input,
            &ButtonInput::default(),
        )
        .expect("dispatch home");
        assert!(accepted);

        let input_event =
            keyboard_input(ButtonState::Pressed, KeyCode::ArrowRight, Key::ArrowRight);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "é🙂");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 1,
                selection_end: 1,
                selection_direction: TextSelectionDirection::None,
            })
        );
    }

    #[test]
    fn home_and_end_move_the_collapsed_caret_to_value_edges() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "é🙂z").expect("value");
        root.focus(input).expect("focus");

        let home_input = keyboard_input(ButtonState::Pressed, KeyCode::Home, Key::Home);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &home_input,
            &ButtonInput::default(),
        )
        .expect("dispatch home");
        assert!(accepted);
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 0,
                selection_end: 0,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let end_input = keyboard_input(ButtonState::Pressed, KeyCode::End, Key::End);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &end_input,
            &ButtonInput::default(),
        )
        .expect("dispatch end");
        assert!(accepted);
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 3,
                selection_end: 3,
                selection_direction: TextSelectionDirection::None,
            })
        );
    }

    #[test]
    fn shift_arrow_left_extends_selection_without_draining_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::ArrowLeft, Key::ArrowLeft);
        let mut keycodes = ButtonInput::default();
        keycodes.press(KeyCode::ShiftLeft);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &keycodes,
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 4,
                selection_end: 5,
                selection_direction: TextSelectionDirection::Backward,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn readonly_input_allows_caret_movement_without_value_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.set_attr(
            input,
            DomAttribute::ReadOnly,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("readonly attr");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::ArrowLeft, Key::ArrowLeft);

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 4,
                selection_end: 4,
                selection_direction: TextSelectionDirection::None,
            })
        );
    }

    #[test]
    fn stopped_input_from_default_text_behavior_blocks_input_drain_but_keeps_mutation() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.focus(input).expect("focus");
        root.nodes
            .get_mut(input)
            .expect("input node")
            .add_event_listener(
                DomEventName::Input,
                |context: &mut DomEventListenerContext<'_>| {
                    context.stop_propagation();
                },
                None,
            );

        let mut input_event = keyboard_input(
            ButtonState::Pressed,
            KeyCode::KeyA,
            Key::Character("a".into()),
        );
        input_event.text = Some("a".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "a");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn enter_text_payload_does_not_insert_text_for_single_line_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");
        root.focus(input).expect("focus");

        let mut input_event = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        input_event.text = Some("\n".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, input), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn enter_in_textarea_inserts_line_break_and_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, textarea).expect("append");
        root.set_attr_str(textarea, "value", "hello")
            .expect("value");
        root.focus(textarea).expect("focus");

        let mut input_event = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        input_event.text = Some("\n".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, textarea), "hello\n");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: textarea,
                selection_start: 6,
                selection_end: 6,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::FocusIn,
                DomEventName::KeyDown,
                DomEventName::Input,
            ],
        );
    }

    #[test]
    fn enter_replaces_extended_selection_in_textarea_and_drains_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, textarea).expect("append");
        root.set_attr_str(textarea, "value", "hello")
            .expect("value");
        root.focus(textarea).expect("focus");
        let extend_left = keyboard_input(ButtonState::Pressed, KeyCode::ArrowLeft, Key::ArrowLeft);
        let mut keycodes = ButtonInput::default();
        keycodes.press(KeyCode::ShiftLeft);
        for _ in 0..4 {
            let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
                &mut root,
                &extend_left,
                &keycodes,
            )
            .expect("shift+arrow left should extend");
            assert!(accepted);
        }
        let mut setup_world = World::new();
        let _ = root
            .update(&mut setup_world, 0.0, DomPointerUpdate::Unchanged)
            .expect("selection setup events should drain");

        let mut input_event = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        input_event.text = Some("\n".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, textarea), "h\n");
        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: textarea,
                selection_start: 2,
                selection_end: 2,
                selection_direction: TextSelectionDirection::None,
            })
        );

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::KeyDown, DomEventName::Input],
        );
    }

    #[test]
    fn readonly_textarea_enter_does_not_mutate_or_drain_input() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, textarea).expect("append");
        root.set_attr_str(textarea, "value", "hello")
            .expect("value");
        root.set_attr(
            textarea,
            DomAttribute::ReadOnly,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("readonly attr");
        root.focus(textarea).expect("focus");

        let mut input_event = keyboard_input(ButtonState::Pressed, KeyCode::Enter, Key::Enter);
        input_event.text = Some("\n".into());

        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");
        assert!(accepted);
        assert_eq!(element_value(&root, textarea), "hello");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");

        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );
    }

    #[test]
    fn checkbox_space_keyup_toggles_checked_and_drains_click_input_and_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "type", "checkbox").expect("type");
        root.style_mut(input).set("width", "24px").expect("width");
        root.style_mut(input).set("height", "24px").expect("height");
        root.focus(input).expect("focus");

        let keydown_event = keyboard_input(ButtonState::Pressed, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keydown_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");

        assert!(accepted);
        assert!(!element_checked(&root, input));

        let mut world = World::new();
        let keydown_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");
        assert_eq!(
            keydown_drained
                .iter()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );

        let keyup_event = keyboard_input(ButtonState::Released, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keyup_event,
            &ButtonInput::default(),
        )
        .expect("keyup dispatch");

        assert!(accepted);
        assert!(element_checked(&root, input));

        let keyup_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("keyup drain");
        assert_eq!(
            keyup_drained
                .iter()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            vec![
                DomEventName::KeyUp,
                DomEventName::Click,
                DomEventName::Input,
                DomEventName::Change,
            ],
        );
    }

    #[test]
    fn prevented_checkbox_space_keyup_does_not_toggle_or_emit_click() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "type", "checkbox").expect("type");
        root.style_mut(input).set("width", "24px").expect("width");
        root.style_mut(input).set("height", "24px").expect("height");
        root.add_event_listener(input, DomEventName::KeyUp, prevent_default_listener, None)
            .expect("keyup listener");
        root.focus(input).expect("focus");

        let keydown_event = keyboard_input(ButtonState::Pressed, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keydown_event,
            &ButtonInput::default(),
        )
        .expect("keydown dispatch");
        assert!(accepted);
        assert!(!element_checked(&root, input));

        let mut world = World::new();
        let keydown_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("keydown drain");
        assert_eq!(
            keydown_drained
                .iter()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            vec![DomEventName::FocusIn, DomEventName::KeyDown],
        );

        let keyup_event = keyboard_input(ButtonState::Released, KeyCode::Space, Key::Space);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &keyup_event,
            &ButtonInput::default(),
        )
        .expect("keyup dispatch");
        assert!(!accepted);
        assert!(!element_checked(&root, input));

        let keyup_drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("keyup drain");
        assert!(keyup_drained.is_empty());
    }

    #[test]
    fn number_arrow_up_steps_value_and_drains_input_and_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "type", "number").expect("type");
        root.set_attr_str(input, "value", "2").expect("value");
        root.focus(input).expect("focus");

        let input_event = keyboard_input(ButtonState::Pressed, KeyCode::ArrowUp, Key::ArrowUp);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");

        assert!(accepted);
        assert_eq!(element_value(&root, input), "3");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::FocusIn,
                DomEventName::KeyDown,
                DomEventName::Input,
                DomEventName::Change,
            ],
        );
    }

    #[test]
    fn range_arrow_right_steps_value_and_drains_input_and_change() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "type", "range").expect("type");
        root.set_attr_str(input, "min", "0").expect("min");
        root.set_attr_str(input, "max", "20").expect("max");
        root.set_attr_str(input, "value", "10").expect("value");
        root.focus(input).expect("focus");

        let input_event =
            keyboard_input(ButtonState::Pressed, KeyCode::ArrowRight, Key::ArrowRight);
        let accepted = dispatch_bevy_keyboard_input_from_focus_and_apply_default_behavior(
            &mut root,
            &input_event,
            &ButtonInput::default(),
        )
        .expect("dispatch");

        assert!(accepted);
        assert_eq!(element_value(&root, input), "11");

        let mut world = World::new();
        let drained = root
            .update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("queued events should drain");
        assert_eq!(
            drained.iter().map(|event| event.name).collect::<Vec<_>>(),
            vec![
                DomEventName::FocusIn,
                DomEventName::KeyDown,
                DomEventName::Input,
                DomEventName::Change,
            ],
        );
    }

    fn keyboard_input(state: ButtonState, key_code: KeyCode, logical_key: Key) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn element_value(root: &RootDom, key: DomNodeSlotKey) -> String {
        let element = root
            .nodes
            .get(key)
            .and_then(DomNode::as_element)
            .expect("value target should stay an element");
        match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            None => panic!("editable value attribute should exist"),
            Some(_) => panic!("editable value attribute should stay string"),
        }
    }

    fn element_checked(root: &RootDom, key: DomNodeSlotKey) -> bool {
        let element = root
            .nodes
            .get(key)
            .and_then(DomNode::as_element)
            .expect("checked target should stay an element");
        matches!(
            element.attrs.attributes.get(&DomAttribute::Checked),
            Some(DomAttributeValue::Bool(true))
        )
    }

    fn capture_options() -> DomEventListenerOptions {
        DomEventListenerOptions {
            capture: true,
            once: false,
            passive: false,
        }
    }

    fn prevent_default_listener(context: &mut DomEventListenerContext<'_>) {
        context.prevent_default();
    }

    fn assert_keyboard_payload_listener(context: &mut DomEventListenerContext<'_>) {
        if let DomEventDetail::Keyboard(keyboard) = &context.event().detail {
            assert_eq!(keyboard.key, "A");
            assert_eq!(keyboard.code, "KeyA");
            assert!(keyboard.repeat);
            assert!(keyboard.modifiers.ctrl_key);
            assert!(keyboard.modifiers.alt_key);
            assert!(keyboard.modifiers.system.shift_key);
            assert!(keyboard.modifiers.system.meta_key);
            return;
        }
        panic!("expected keyboard event detail");
    }

    fn assert_enter_keyboard_payload_listener(context: &mut DomEventListenerContext<'_>) {
        if let DomEventDetail::Keyboard(keyboard) = &context.event().detail {
            assert_eq!(keyboard.key, "Enter");
            assert_eq!(keyboard.code, "Enter");
            return;
        }
        panic!("expected keyboard event detail");
    }

    fn keyboard_dispatch_log() -> &'static Mutex<Vec<&'static str>> {
        static LOG: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
        LOG.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn keyboard_dispatch_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn keyboard_dispatch_test_guard() -> std::sync::MutexGuard<'static, ()> {
        match keyboard_dispatch_test_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn clear_keyboard_dispatch_log() {
        keyboard_dispatch_log()
            .lock()
            .expect("dispatch log should lock")
            .clear();
    }

    fn keyboard_dispatch_log_snapshot() -> Vec<&'static str> {
        keyboard_dispatch_log()
            .lock()
            .expect("dispatch log should lock")
            .clone()
    }

    fn push_keyboard_dispatch_log(entry: &'static str) {
        keyboard_dispatch_log()
            .lock()
            .expect("dispatch log should lock")
            .push(entry);
    }

    fn record_root_capture(context: &mut DomEventListenerContext<'_>) {
        assert_eq!(context.phase(), DomEventPhase::Capturing);
        push_keyboard_dispatch_log("root:capture");
    }

    fn record_parent_capture(context: &mut DomEventListenerContext<'_>) {
        assert_eq!(context.phase(), DomEventPhase::Capturing);
        push_keyboard_dispatch_log("parent:capture");
    }

    fn record_target_capture(context: &mut DomEventListenerContext<'_>) {
        assert_eq!(context.phase(), DomEventPhase::AtTarget);
        push_keyboard_dispatch_log("target:capture");
    }

    fn record_target_bubble(context: &mut DomEventListenerContext<'_>) {
        assert_eq!(context.phase(), DomEventPhase::AtTarget);
        push_keyboard_dispatch_log("target:bubble");
    }

    fn record_parent_bubble(context: &mut DomEventListenerContext<'_>) {
        assert_eq!(context.phase(), DomEventPhase::Bubbling);
        push_keyboard_dispatch_log("parent:bubble");
    }
}
