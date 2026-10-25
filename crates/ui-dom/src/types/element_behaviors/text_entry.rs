//! Text-edit default-behavior hooks (PR 5, E6-a).
//!
//! This file owns the text-edit control kinds' behavior: the textarea keyboard
//! handler (`text_edit_on_keyboard`), the shared text-entry behavior
//! (`apply_default_text_behavior_and_dispatch_input_from_keyboard_input`), the
//! text caret maintenance (`ensure_text_selection_for_target`), and the pure
//! text-edit helpers (`default_text_behavior_result` and its leaves). The
//! text-input keyboard handler itself is shared with the other input kinds in
//! `controls.rs`; only the textarea-specific keyboard and pointer-down paths
//! live here.

use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

use crate::domains::visitor::event_target::DomPointEventTarget;
use crate::element_controls::resolved_text_edit_kind;
use crate::types::primitives::PointPx;
use crate::types::{
    DomAttribute, DomAttributeValue, DomElementEditKind, DomError, DomNode, DomNodeSlotKey,
    DomSelection, RootDom, TextSelection, TextSelectionDirection,
};

use super::controls::{count_unicode_scalars, presented_options_keyboard_navigation};
use super::popup::reopen_presented_options_for_text_input_value_change;

#[cfg(test)]
use crate::types::{DomTag, RootDomContext};

/// The textarea row's keyboard hook.
pub(crate) fn text_edit_on_keyboard(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
    input: &KeyboardInput,
    shift_key: bool,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    if input.state != ButtonState::Pressed || !event_stayed_unblocked {
        return Ok(false);
    }
    if presented_options_keyboard_navigation(root, target_key, input, event_stayed_unblocked)? {
        return Ok(true);
    }
    ensure_text_selection_for_target(root, target_key);
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input(root, input, shift_key)
}

/// The textarea row's pointer-down hook: place the live caret.
///
/// `#[allow(clippy::unnecessary_wraps)]`: the hook must share the table's
/// `PointerDownHook` signature (`Result<(), DomError>`), so a failure-free
/// body still wraps in `Ok(())` for the fn-pointer table.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn text_edit_pointer_down(
    root: &mut RootDom,
    target: DomPointEventTarget,
    _root_point: PointPx,
) -> Result<(), DomError> {
    ensure_text_selection_for_target(root, target.target_key);
    Ok(())
}

/// Places (or keeps) the live text caret for an editable element.
pub(crate) fn ensure_text_selection_for_target(root: &mut RootDom, target_key: DomNodeSlotKey) {
    let Some(element) = root.node(target_key).and_then(DomNode::as_element) else {
        root.set_selection(DomSelection::None);
        return;
    };
    let current_value = match element.attrs.attributes.get(&DomAttribute::Value) {
        Some(DomAttributeValue::String(value)) => value.clone(),
        None => String::new(),
        Some(_) => panic!("editable element value attribute should stay string"),
    };
    match resolved_text_edit_kind(element) {
        DomElementEditKind::None => {
            root.set_selection(DomSelection::None);
        }
        DomElementEditKind::SingleLine | DomElementEditKind::MultiLine => {
            if matches!(
                root.selection(),
                DomSelection::TextSelected(selection) if selection.target_element == target_key
            ) {
                return;
            }
            let value_len = count_unicode_scalars(&current_value);
            root.set_selection(DomSelection::TextSelected(TextSelection {
                target_element: target_key,
                selection_start: value_len,
                selection_end: value_len,
                selection_direction: TextSelectionDirection::None,
            }));
        }
    }
}

/// The text-input behavior: runs the default text edit for one keyboard input
/// and dispatches the resulting input event.
pub(crate) fn apply_default_text_behavior_and_dispatch_input_from_keyboard_input(
    root: &mut RootDom,
    input: &KeyboardInput,
    shift_key: bool,
) -> Result<bool, DomError> {
    if input.state != ButtonState::Pressed {
        return Ok(false);
    }
    let Some(target_key) = root.focused_element_key() else {
        return Ok(false);
    };
    let element = root
        .node(target_key)
        .and_then(DomNode::as_element)
        .ok_or(DomError::InvalidAttributeTarget)?;
    let edit_kind = resolved_text_edit_kind(element);
    if matches!(edit_kind, DomElementEditKind::None)
        || matches!(
            element.attrs.attributes.get(&DomAttribute::Disabled),
            Some(DomAttributeValue::Bool(true))
        )
    {
        return Ok(false);
    }
    let readonly = matches!(
        element.attrs.attributes.get(&DomAttribute::ReadOnly),
        Some(DomAttributeValue::Bool(true))
    );
    let selection = match root.selection() {
        DomSelection::TextSelected(selection) if selection.target_element == target_key => {
            selection
        }
        _ => panic!("focused editable element should own live text selection"),
    };
    let value = match element.attrs.attributes.get(&DomAttribute::Value) {
        Some(DomAttributeValue::String(value)) => value.clone(),
        None => panic!("editable element value attribute should exist"),
        Some(_) => panic!("editable element value attribute should stay string"),
    };
    let Some(behavior) = default_text_behavior_result(
        &value,
        edit_kind,
        selection.selection_start,
        selection.selection_end,
        selection.selection_direction,
        input,
        shift_key,
    ) else {
        return Ok(false);
    };
    match behavior {
        DefaultTextBehaviorResult::SetSelection {
            selection_start,
            selection_end,
            selection_direction,
        } => {
            root.set_selection(DomSelection::TextSelected(TextSelection {
                target_element: target_key,
                selection_start,
                selection_end,
                selection_direction,
            }));
            Ok(true)
        }
        DefaultTextBehaviorResult::ReplaceValue {
            next_value,
            next_caret_offset,
        } => {
            if readonly {
                return Ok(false);
            }
            assert!(
                next_caret_offset <= count_unicode_scalars(&next_value),
                "default text behavior should keep the caret within the next value",
            );
            root.set_attr(
                target_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(next_value.clone())),
            )?;
            root.set_selection(DomSelection::TextSelected(TextSelection {
                target_element: target_key,
                selection_start: next_caret_offset,
                selection_end: next_caret_offset,
                selection_direction: TextSelectionDirection::None,
            }));
            let _ = root.dispatch_input_event_to_target_key(target_key, next_value)?;
            reopen_presented_options_for_text_input_value_change(root, target_key)?;
            Ok(true)
        }
    }
}

enum DefaultTextBehaviorResult {
    SetSelection {
        selection_start: usize,
        selection_end: usize,
        selection_direction: TextSelectionDirection,
    },
    ReplaceValue {
        next_value: String,
        next_caret_offset: usize,
    },
}

#[allow(clippy::too_many_lines)]
fn default_text_behavior_result(
    value: &str,
    edit_kind: DomElementEditKind,
    selection_start: usize,
    selection_end: usize,
    selection_direction: TextSelectionDirection,
    input: &KeyboardInput,
    shift_key: bool,
) -> Option<DefaultTextBehaviorResult> {
    let scalar_count = count_unicode_scalars(value);
    assert!(
        selection_start <= selection_end,
        "selection start should not exceed selection end",
    );
    assert!(
        selection_start <= scalar_count,
        "selection start should stay within the current value",
    );
    assert!(
        selection_end <= scalar_count,
        "selection end should stay within the current value",
    );
    let selection_is_collapsed = selection_start == selection_end;
    assert!(
        !selection_is_collapsed || selection_direction == TextSelectionDirection::None,
        "collapsed text selection should keep direction none",
    );
    assert!(
        selection_is_collapsed || selection_direction != TextSelectionDirection::None,
        "extended text selection should keep a forward or backward direction",
    );
    let replacement_text = if matches!(edit_kind, DomElementEditKind::MultiLine)
        && matches!(input.logical_key, Key::Enter)
    {
        Some("\n")
    } else if matches!(input.logical_key, Key::Character(_) | Key::Space)
        && let Some(text) = input.text.as_ref()
        && !text.is_empty()
    {
        Some(text.as_str())
    } else {
        None
    };
    let (anchor_offset, focus_offset) = match selection_direction {
        TextSelectionDirection::None => (selection_end, selection_end),
        TextSelectionDirection::Forward => (selection_start, selection_end),
        TextSelectionDirection::Backward => (selection_end, selection_start),
    };
    if let Some(replacement_text) = replacement_text {
        let (next_value, next_caret_offset) = replaced_unicode_scalar_selection(
            value,
            selection_start,
            selection_end,
            replacement_text,
        );
        return Some(DefaultTextBehaviorResult::ReplaceValue {
            next_value,
            next_caret_offset,
        });
    }
    if !selection_is_collapsed && matches!(input.logical_key, Key::Backspace | Key::Delete) {
        let (next_value, next_caret_offset) =
            replaced_unicode_scalar_selection(value, selection_start, selection_end, "");
        return Some(DefaultTextBehaviorResult::ReplaceValue {
            next_value,
            next_caret_offset,
        });
    }
    match input.logical_key {
        Key::Backspace => {
            if focus_offset == 0 {
                return None;
            }
            let delete_start = byte_index_for_unicode_scalar_offset(value, focus_offset - 1);
            let delete_end = byte_index_for_unicode_scalar_offset(value, focus_offset);
            let mut next_value = String::with_capacity(value.len() - (delete_end - delete_start));
            next_value.push_str(&value[..delete_start]);
            next_value.push_str(&value[delete_end..]);
            Some(DefaultTextBehaviorResult::ReplaceValue {
                next_value,
                next_caret_offset: focus_offset - 1,
            })
        }
        Key::Delete => {
            if focus_offset == scalar_count {
                return None;
            }
            let delete_start = byte_index_for_unicode_scalar_offset(value, focus_offset);
            let delete_end = byte_index_for_unicode_scalar_offset(value, focus_offset + 1);
            let mut next_value = String::with_capacity(value.len() - (delete_end - delete_start));
            next_value.push_str(&value[..delete_start]);
            next_value.push_str(&value[delete_end..]);
            Some(DefaultTextBehaviorResult::ReplaceValue {
                next_value,
                next_caret_offset: focus_offset,
            })
        }
        Key::ArrowLeft if shift_key && focus_offset > 0 => {
            let next_focus_offset = focus_offset - 1;
            let next_selection_direction = match next_focus_offset.cmp(&anchor_offset) {
                std::cmp::Ordering::Equal => TextSelectionDirection::None,
                std::cmp::Ordering::Greater => TextSelectionDirection::Forward,
                std::cmp::Ordering::Less => TextSelectionDirection::Backward,
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: next_focus_offset.min(anchor_offset),
                selection_end: next_focus_offset.max(anchor_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::ArrowLeft if !selection_is_collapsed => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start,
                selection_end: selection_start,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::ArrowLeft if focus_offset > 0 => Some(DefaultTextBehaviorResult::SetSelection {
            selection_start: focus_offset - 1,
            selection_end: focus_offset - 1,
            selection_direction: TextSelectionDirection::None,
        }),
        Key::ArrowRight if shift_key && focus_offset < scalar_count => {
            let next_focus_offset = focus_offset + 1;
            let next_selection_direction = match next_focus_offset.cmp(&anchor_offset) {
                std::cmp::Ordering::Equal => TextSelectionDirection::None,
                std::cmp::Ordering::Greater => TextSelectionDirection::Forward,
                std::cmp::Ordering::Less => TextSelectionDirection::Backward,
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: next_focus_offset.min(anchor_offset),
                selection_end: next_focus_offset.max(anchor_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::ArrowRight if !selection_is_collapsed => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: selection_end,
                selection_end,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::ArrowRight if focus_offset < scalar_count => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: focus_offset + 1,
                selection_end: focus_offset + 1,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::Home if shift_key && focus_offset > 0 => {
            let next_focus_offset = 0;
            let next_selection_direction = if anchor_offset == 0 {
                TextSelectionDirection::None
            } else {
                TextSelectionDirection::Backward
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: 0,
                selection_end: anchor_offset.max(next_focus_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::Home if !selection_is_collapsed || focus_offset > 0 => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: 0,
                selection_end: 0,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::End if shift_key && focus_offset < scalar_count => {
            let next_focus_offset = scalar_count;
            let next_selection_direction = if anchor_offset == scalar_count {
                TextSelectionDirection::None
            } else {
                TextSelectionDirection::Forward
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: anchor_offset.min(next_focus_offset),
                selection_end: next_focus_offset,
                selection_direction: next_selection_direction,
            })
        }
        Key::End if !selection_is_collapsed || focus_offset < scalar_count => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: scalar_count,
                selection_end: scalar_count,
                selection_direction: TextSelectionDirection::None,
            })
        }
        _ => None,
    }
}

fn byte_index_for_unicode_scalar_offset(value: &str, scalar_offset: usize) -> usize {
    if scalar_offset == count_unicode_scalars(value) {
        return value.len();
    }
    value
        .char_indices()
        .nth(scalar_offset)
        .map(|(byte_index, _)| byte_index)
        .expect("unicode scalar offset should stay within the current value")
}

fn replaced_unicode_scalar_selection(
    value: &str,
    selection_start: usize,
    selection_end: usize,
    replacement: &str,
) -> (String, usize) {
    let replace_start = byte_index_for_unicode_scalar_offset(value, selection_start);
    let replace_end = byte_index_for_unicode_scalar_offset(value, selection_end);
    let mut next_value =
        String::with_capacity(value.len() - (replace_end - replace_start) + replacement.len());
    next_value.push_str(&value[..replace_start]);
    next_value.push_str(replacement);
    next_value.push_str(&value[replace_end..]);
    (
        next_value,
        selection_start + count_unicode_scalars(replacement),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_text_selection_places_caret_at_end_of_value() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input");
        root.append_child(root.root, input).expect("append");
        root.set_attr_str(input, "value", "hello").expect("value");

        ensure_text_selection_for_target(&mut root, input);

        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: input,
                selection_start: 5,
                selection_end: 5,
                selection_direction: TextSelectionDirection::None,
            })
        );
    }

    #[test]
    fn text_edit_pointer_down_places_the_caret() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        root.append_child(root.root, textarea).expect("append");
        root.set_attr_str(textarea, "value", "ab").expect("value");

        text_edit_pointer_down(
            &mut root,
            DomPointEventTarget {
                target_key: textarea,
                subtree_point: PointPx {
                    x_px: 0.0,
                    y_px: 0.0,
                },
            },
            PointPx {
                x_px: 0.0,
                y_px: 0.0,
            },
        )
        .expect("pointer down");

        assert_eq!(
            root.selection(),
            DomSelection::TextSelected(TextSelection {
                target_element: textarea,
                selection_start: 2,
                selection_end: 2,
                selection_direction: TextSelectionDirection::None,
            })
        );
    }
}
