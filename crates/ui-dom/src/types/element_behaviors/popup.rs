//! The presented-options popup lifecycle (PR 5, E6-b).
//!
//! This file owns the popup *state machine + commit + seams + queries*: the
//! open/close/move/highlight state machine, the commit-and-dispatch logic, the
//! facade seam entry points (keyboard/click/blur/focus/reopen), and the popup
//! query accessors. The former `RootDom` popup methods re-homed here as free
//! functions over `&mut RootDom` (E6-c's evidence-gated narrowing was
//! REJECTED — the rows are document-wide). DOM construction lives in
//! `popup_shell.rs`; the definitions/records/measurement live in
//! `present_options.rs`.
//!
//! Invariants and rules:
//!
//! - select and datalist share the open/close/move machinery, parameterized by
//!   `PresentedOptionsOwnerKind`; the kind-specific differences are triaged:
//!   select commits set the source `<option>` `selected` attrs (a (b) extra),
//!   and the keyboard maps differ (select has Space + open-on-commit; text
//!   input commits on Enter only — a (c) semantic difference that stays
//!   bespoke).
//! - the popup shell opens with state `open` + opacity 1 and closes to state
//!   `closing` + opacity 0; the transition-end tick (still in `root_dom.rs`)
//!   queues a closing shell for layer removal via `presented_options_popup_state`.
//! - query accessors read role/state attributes set by `popup_shell.rs`'s
//!   construction; they are the single vocabulary the paint skip-special-case
//!   and the record collector share.

use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomNode, DomNodeSlotKey, DomTag, RootDom,
};

use super::popup_shell::{
    append_presented_options_popup_shell_and_rows, set_presented_options_popup_anchor_attrs,
    set_presented_options_row_highlight_attrs,
};
use super::present_options::{
    DomElementPresentOptionsBehavior, PRESENTED_OPTIONS_ROLE_ROW, PRESENTED_OPTIONS_ROLE_SCROLL,
    PRESENTED_OPTIONS_ROLE_SHELL, PRESENTED_OPTIONS_STATE_CLOSING, PRESENTED_OPTIONS_STATE_OPEN,
    PresentedOptionsOwnerKind, choose_presented_options_highlighted_index,
    collect_direct_child_element_keys, collect_presented_option_records_from_owner_source,
    measure_presented_options_popup_height_px, presented_options_popup_anchor_for_owner,
    presented_options_popup_state, presented_options_role_value,
    presented_options_row_is_highlighted, presented_options_row_source_index,
    set_select_source_option_selected_attrs, string_attr_value,
};

/// The select row's keyboard seam (the former
/// `RootDom::apply_select_keyboard_default_for_owner`).
pub(crate) fn apply_select_keyboard_default_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    input: &KeyboardInput,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    if input.state != ButtonState::Pressed || !event_stayed_unblocked {
        return Ok(false);
    }
    if resolve_presented_options_owner_kind(root, owner_key)
        != Some(PresentedOptionsOwnerKind::Select)
    {
        return Ok(false);
    }
    match input.logical_key {
        Key::ArrowDown => move_presented_options_highlight_or_open_popup(root, owner_key, true),
        Key::ArrowUp => move_presented_options_highlight_or_open_popup(root, owner_key, false),
        Key::Enter | Key::Space => {
            commit_highlighted_presented_option_or_open_popup(root, owner_key)
        }
        Key::Escape => close_presented_options_popup_for_owner(root, owner_key, false),
        _ => Ok(false),
    }
}

/// The text-input datalist keyboard seam (the former
/// `RootDom::apply_presented_options_text_input_keyboard_navigation_for_owner`).
pub(crate) fn apply_presented_options_text_input_keyboard_navigation_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    input: &KeyboardInput,
    event_stayed_unblocked: bool,
) -> Result<bool, DomError> {
    if input.state != ButtonState::Pressed || !event_stayed_unblocked {
        return Ok(false);
    }
    if resolve_presented_options_owner_kind(root, owner_key)
        != Some(PresentedOptionsOwnerKind::TextInput)
    {
        return Ok(false);
    }
    match input.logical_key {
        Key::ArrowDown => move_presented_options_highlight_or_open_popup(root, owner_key, true),
        Key::ArrowUp => move_presented_options_highlight_or_open_popup(root, owner_key, false),
        Key::Enter => commit_highlighted_presented_option_for_owner(root, owner_key),
        Key::Escape => close_presented_options_popup_for_owner(root, owner_key, false),
        _ => Ok(false),
    }
}

/// The presented-options click seam (the former
/// `RootDom::apply_presented_options_click_default_for_target`).
///
/// Named note (back-off (c), frozen behavior): this datalist/text-input click
/// commit path is currently driven only by its unit test. The E6-a lifecycle
/// re-shaped `commit_after_click_after_dispatch` onto the behavior table's
/// `on_click_commit` rows, and the text-input row's commit hook is `None`, so
/// the pre-E6 `DomElementOnActivation::PresentedOptions → click commit` wire
/// is not re-connected. It relocates byte-identically here (the migration
/// freezes E6-a behavior); re-wiring it is a separate, reviewed follow-up.
#[allow(dead_code)]
pub(crate) fn apply_presented_options_click_default_for_target(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if commit_presented_option_hit_row(root, target_key)? {
        return Ok(true);
    }
    let Some(owner_key) = resolve_presented_options_owner_key_from_target(root, target_key) else {
        return Ok(false);
    };
    if presented_options_popup_shell_key(root, owner_key).is_some() {
        return close_presented_options_popup_for_owner(root, owner_key, false);
    }
    open_presented_options_popup_for_owner(root, owner_key)
}

/// The select popup-row click commit seam (the former
/// `RootDom::apply_select_popup_row_click_default_for_target`). It is the first
/// commit step of the generic activation lifecycle: a popup `<option>` row
/// resolves to no control kind, so the lifecycle checks this target shape first.
pub(crate) fn apply_select_popup_row_click_default_for_target(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if resolve_presented_options_row_key_from_target(root, target_key).is_none() {
        return Ok(false);
    }
    commit_select_presented_option_hit_row(root, target_key)
}

/// The select click seam (the former
/// `RootDom::apply_select_click_default_for_target`).
pub(crate) fn apply_select_click_default_for_target(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if commit_select_presented_option_hit_row(root, target_key)? {
        return Ok(true);
    }
    let Some(owner_key) = resolve_select_owner_key_from_target(root, target_key) else {
        return Ok(false);
    };
    if presented_options_popup_shell_key(root, owner_key).is_some() {
        return close_presented_options_popup_for_owner(root, owner_key, false);
    }
    open_select_popup_for_owner(root, owner_key)
}

/// The select blur seam (the former
/// `RootDom::close_select_popup_for_blurred_owner`).
pub(crate) fn close_select_popup_for_blurred_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if resolve_presented_options_owner_kind(root, owner_key)
        != Some(PresentedOptionsOwnerKind::Select)
    {
        return Ok(false);
    }
    close_presented_options_popup_for_owner(root, owner_key, false)
}

/// The text-input focus seam (the former
/// `RootDom::open_presented_options_for_focused_text_input_owner`).
pub(crate) fn open_presented_options_for_focused_text_input_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if resolve_presented_options_owner_kind(root, owner_key)
        != Some(PresentedOptionsOwnerKind::TextInput)
    {
        return Ok(false);
    }
    open_presented_options_popup_for_owner(root, owner_key)
}

/// The text-input blur seam (the former
/// `RootDom::close_presented_options_for_blurred_owner`).
pub(crate) fn close_presented_options_for_blurred_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    close_presented_options_popup_for_owner(root, owner_key, false)
}

/// The text-input value-change reopen seam (the former
/// `RootDom::reopen_presented_options_for_text_input_value_change`).
pub(crate) fn reopen_presented_options_for_text_input_value_change(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    if resolve_presented_options_owner_kind(root, owner_key)
        != Some(PresentedOptionsOwnerKind::TextInput)
    {
        return Ok(());
    }
    let _ = open_presented_options_popup_for_owner(root, owner_key)?;
    Ok(())
}

/// Resolves an owner's presented-options kind, or `None` when the element is
/// neither a select nor a datalist-backed text input.
fn resolve_presented_options_owner_kind(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<PresentedOptionsOwnerKind> {
    let element = root.node(owner_key)?.as_element()?;
    if element.tag == DomTag::Select {
        return Some(PresentedOptionsOwnerKind::Select);
    }
    match element.behavior().present_options_behavior {
        DomElementPresentOptionsBehavior::None => None,
        DomElementPresentOptionsBehavior::TextInput => {
            resolve_presented_options_datalist_key_for_owner(root, owner_key)
                .map(|_| PresentedOptionsOwnerKind::TextInput)
        }
    }
}

/// Opens (or reopens) the presented-options popup for any owner kind.
fn open_presented_options_popup_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(owner_kind) = resolve_presented_options_owner_kind(root, owner_key) else {
        return Ok(false);
    };
    remove_other_presented_options_popups_immediately(root, owner_key)?;
    replace_presented_options_popup_for_owner_kind(root, owner_kind, owner_key)
}

/// Opens the select popup for a select owner.
fn open_select_popup_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if resolve_select_owner_key_from_target(root, owner_key) != Some(owner_key) {
        return Ok(false);
    }
    remove_other_presented_options_popups_immediately(root, owner_key)?;
    replace_presented_options_popup_for_owner_kind(
        root,
        PresentedOptionsOwnerKind::Select,
        owner_key,
    )
}

/// Closes an owner's popup. `immediate` removes the shell now; otherwise the
/// shell transitions to `closing` + opacity 0 and awaits transition-end.
fn close_presented_options_popup_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    immediate: bool,
) -> Result<bool, DomError> {
    let Some(shell_key) = presented_options_popup_shell_key(root, owner_key) else {
        return Ok(false);
    };
    if immediate {
        root.remove_node(shell_key)?;
        return Ok(true);
    }
    if presented_options_popup_state(root, shell_key) == Some(PRESENTED_OPTIONS_STATE_CLOSING) {
        return Ok(true);
    }
    set_presented_options_popup_state(root, shell_key, PRESENTED_OPTIONS_STATE_CLOSING)?;
    set_presented_options_popup_opacity(root, shell_key, 0.0)?;
    Ok(true)
}

/// Returns the owner's popup shell key (the `popup-shell` role child), if open.
pub(crate) fn presented_options_popup_shell_key(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    collect_direct_child_element_keys(root, owner_key)
        .into_iter()
        .find(|key| presented_options_role_value(root, *key) == Some(PRESENTED_OPTIONS_ROLE_SHELL))
}

/// Returns the owner's currently highlighted popup row key.
fn presented_options_highlighted_row_key(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    presented_options_popup_row_keys(root, owner_key)
        .into_iter()
        .find(|row_key| presented_options_row_is_highlighted(root, *row_key))
}

/// Returns the owner's popup row keys in document order.
pub(crate) fn presented_options_popup_row_keys(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Vec<DomNodeSlotKey> {
    let Some(scroll_key) = presented_options_popup_scroll_key(root, owner_key) else {
        return Vec::new();
    };
    collect_direct_child_element_keys(root, scroll_key)
        .into_iter()
        .filter(|key| presented_options_role_value(root, *key) == Some(PRESENTED_OPTIONS_ROLE_ROW))
        .collect()
}

/// Moves the highlight, or opens the popup when it is closed/closing.
fn move_presented_options_highlight_or_open_popup(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    move_forward: bool,
) -> Result<bool, DomError> {
    if presented_options_popup_shell_key(root, owner_key).is_some()
        && !presented_options_popup_is_closing_for_owner(root, owner_key)
    {
        return move_presented_options_popup_highlight(root, owner_key, move_forward);
    }
    open_presented_options_popup_for_owner(root, owner_key)
}

/// Commits the highlighted option, or opens the popup when it is
/// closed/closing (select's Enter/Space: commit-or-open).
fn commit_highlighted_presented_option_or_open_popup(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if presented_options_popup_shell_key(root, owner_key).is_some()
        && !presented_options_popup_is_closing_for_owner(root, owner_key)
    {
        return commit_highlighted_presented_option_for_owner(root, owner_key);
    }
    open_presented_options_popup_for_owner(root, owner_key)
}

/// Rebuilds the popup for one owner kind: collect records, choose the
/// highlight, close the stale shell, build the new one, and mark it open.
fn replace_presented_options_popup_for_owner_kind(
    root: &mut RootDom,
    owner_kind: PresentedOptionsOwnerKind,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let popup_anchor = presented_options_popup_anchor_for_owner(root, owner_key)?;
    let option_records =
        collect_presented_option_records_from_owner_source(root, owner_kind, owner_key)?;
    if option_records.is_empty() {
        let _ = close_presented_options_popup_for_owner(root, owner_key, true)?;
        return Ok(false);
    }
    let previous_highlighted_index =
        highlighted_presented_options_source_index_for_owner(root, owner_key);
    let _ = close_presented_options_popup_for_owner(root, owner_key, true)?;
    let highlighted_index = choose_presented_options_highlighted_index(
        root,
        owner_kind,
        owner_key,
        &option_records,
        previous_highlighted_index,
    );
    let popup_shell_key = append_presented_options_popup_shell_and_rows(
        root,
        owner_key,
        popup_anchor,
        &option_records,
        highlighted_index,
    )?;
    update_presented_options_popup_box_attrs_from_owner(root, owner_key)?;
    set_presented_options_popup_state(root, popup_shell_key, PRESENTED_OPTIONS_STATE_OPEN)?;
    set_presented_options_popup_opacity(root, popup_shell_key, 1.0)?;
    Ok(true)
}

/// Re-applies the shell/scroll geometry from the owner's current box (called
/// after the owner moves, before the popup is marked open).
fn update_presented_options_popup_box_attrs_from_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let Some(shell_key) = presented_options_popup_shell_key(root, owner_key) else {
        return Ok(());
    };
    let Some(scroll_key) = presented_options_popup_scroll_key(root, owner_key) else {
        return Ok(());
    };
    let popup_anchor = presented_options_popup_anchor_for_owner(root, owner_key)?;
    let popup_height_px = measure_presented_options_popup_height_px(
        root,
        owner_key,
        presented_options_popup_row_keys(root, owner_key).len(),
    )?;
    set_presented_options_popup_anchor_attrs(root, shell_key, popup_anchor)?;
    root.style_mut(shell_key).set(
        "width",
        &(format!("{}px", popup_anchor.owner_box.width_px.max(1.0))),
    )?;
    root.style_mut(shell_key)
        .set("height", &(format!("{popup_height_px}px")))?;
    root.style_mut(scroll_key).set(
        "width",
        &(format!("{}px", popup_anchor.owner_box.width_px.max(1.0))),
    )?;
    root.style_mut(scroll_key)
        .set("height", &(format!("{popup_height_px}px")))?;
    Ok(())
}

/// Closes every other owner's open popup immediately (only one popup at a time).
fn remove_other_presented_options_popups_immediately(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    for other_owner_key in owner_keys_with_presented_options_popup(root) {
        if other_owner_key != owner_key {
            let _ = close_presented_options_popup_for_owner(root, other_owner_key, true)?;
        }
    }
    Ok(())
}

/// Commits the presented-option row under `target_key` (datalist or select
/// owner), resolving the owner kind and dispatching its events.
#[allow(dead_code)]
fn commit_presented_option_hit_row(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(row_key) = resolve_presented_options_row_key_from_target(root, target_key) else {
        return Ok(false);
    };
    let Some(owner_key) = resolve_presented_options_owner_key_from_target(root, target_key) else {
        return Ok(false);
    };
    let Some(source_index) = presented_options_row_source_index(root, row_key) else {
        return Ok(false);
    };
    let Some(owner_kind) = resolve_presented_options_owner_kind(root, owner_key) else {
        return Ok(false);
    };
    let option_records =
        collect_presented_option_records_from_owner_source(root, owner_kind, owner_key)?;
    let Some(option_record) = option_records.get(source_index) else {
        return Ok(false);
    };
    apply_presented_option_to_owner_and_dispatch_events(
        root,
        owner_key,
        option_record.source_key,
        option_record.value.clone(),
    )
}

/// Commits a select-only popup row under `target_key` (the select popup row
/// click path; source `<option>` `selected` attrs are set on commit).
fn commit_select_presented_option_hit_row(
    root: &mut RootDom,
    target_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(row_key) = resolve_presented_options_row_key_from_target(root, target_key) else {
        return Ok(false);
    };
    let Some(owner_key) = resolve_select_owner_key_from_target(root, target_key) else {
        return Ok(false);
    };
    let Some(source_index) = presented_options_row_source_index(root, row_key) else {
        return Ok(false);
    };
    let option_records = collect_presented_option_records_from_owner_source(
        root,
        PresentedOptionsOwnerKind::Select,
        owner_key,
    )?;
    let Some(option_record) = option_records.get(source_index) else {
        return Ok(false);
    };
    apply_select_option_to_owner_and_dispatch_events(
        root,
        owner_key,
        option_record.source_key,
        option_record.value.clone(),
    )
}

/// Commits the currently highlighted option for an open popup.
fn commit_highlighted_presented_option_for_owner(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    let Some(source_index) = highlighted_presented_options_source_index_for_owner(root, owner_key)
    else {
        return Ok(false);
    };
    let Some(owner_kind) = resolve_presented_options_owner_kind(root, owner_key) else {
        return Ok(false);
    };
    let option_records =
        collect_presented_option_records_from_owner_source(root, owner_kind, owner_key)?;
    let Some(option_record) = option_records.get(source_index) else {
        return Ok(false);
    };
    apply_presented_option_to_owner_and_dispatch_events(
        root,
        owner_key,
        option_record.source_key,
        option_record.value.clone(),
    )
}

/// Applies a committed option to its owner and dispatches input/change, then
/// closes the popup. Select owners route through the select-specific path.
fn apply_presented_option_to_owner_and_dispatch_events(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    source_key: DomNodeSlotKey,
    value: String,
) -> Result<bool, DomError> {
    let Some(owner_kind) = resolve_presented_options_owner_kind(root, owner_key) else {
        return Ok(false);
    };
    match owner_kind {
        PresentedOptionsOwnerKind::Select => {
            return apply_select_option_to_owner_and_dispatch_events(
                root, owner_key, source_key, value,
            );
        }
        PresentedOptionsOwnerKind::TextInput => {
            root.set_attr(
                owner_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(value.clone())),
            )?;
            let _ = root.dispatch_input_event_to_target_key(owner_key, value.clone())?;
            let _ = root.dispatch_change_event_to_target_key(owner_key, value)?;
        }
    }
    let _ = close_presented_options_popup_for_owner(root, owner_key, false)?;
    Ok(true)
}

/// Applies a committed select option: set the source `<option>` `selected`
/// attrs, write the owner value, dispatch input/change, and close the popup.
fn apply_select_option_to_owner_and_dispatch_events(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    source_key: DomNodeSlotKey,
    value: String,
) -> Result<bool, DomError> {
    set_select_source_option_selected_attrs(root, owner_key, source_key)?;
    root.set_attr(
        owner_key,
        DomAttribute::Value,
        Some(DomAttributeValue::String(value.clone())),
    )?;
    let _ = root.dispatch_input_event_to_target_key(owner_key, value.clone())?;
    let _ = root.dispatch_change_event_to_target_key(owner_key, value)?;
    let _ = close_presented_options_popup_for_owner(root, owner_key, false)?;
    Ok(true)
}

/// Moves the highlight one row forward/back within an open popup.
fn move_presented_options_popup_highlight(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    move_forward: bool,
) -> Result<bool, DomError> {
    let row_keys = presented_options_popup_row_keys(root, owner_key);
    if row_keys.is_empty() {
        return Ok(false);
    }
    let current_index = row_keys
        .iter()
        .position(|row_key| presented_options_row_is_highlighted(root, *row_key))
        .unwrap_or(0);
    let next_index = if move_forward {
        (current_index + 1).min(row_keys.len() - 1)
    } else {
        current_index.saturating_sub(1)
    };
    highlight_presented_options_popup_row_at_index(root, owner_key, next_index)?;
    Ok(true)
}

/// Highlights one row and clears the rest.
fn highlight_presented_options_popup_row_at_index(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    highlighted_index: usize,
) -> Result<(), DomError> {
    let row_keys = presented_options_popup_row_keys(root, owner_key);
    for (row_index, row_key) in row_keys.into_iter().enumerate() {
        set_presented_options_row_highlight_attrs(root, row_key, row_index == highlighted_index)?;
    }
    Ok(())
}

/// Walks up from `target_key` to the nearest presented-options owner (select or
/// datalist-backed text input).
#[allow(dead_code)]
fn resolve_presented_options_owner_key_from_target(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let mut current_key = Some(target_key);
    while let Some(key) = current_key {
        let node = root.node(key)?;
        if let Some(element) = node.as_element()
            && (element.tag == DomTag::Select
                || element.behavior().present_options_behavior
                    != DomElementPresentOptionsBehavior::None)
        {
            return Some(key);
        }
        current_key = node.parent_key();
    }
    None
}

/// Walks up from `target_key` to the nearest select owner.
fn resolve_select_owner_key_from_target(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let mut current_key = Some(target_key);
    while let Some(key) = current_key {
        let node = root.node(key)?;
        if node
            .as_element()
            .is_some_and(|element| element.tag == DomTag::Select)
        {
            return Some(key);
        }
        current_key = node.parent_key();
    }
    None
}

/// Walks up from `target_key` to the nearest `popup-row` role element.
fn resolve_presented_options_row_key_from_target(
    root: &RootDom,
    target_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let mut current_key = Some(target_key);
    while let Some(key) = current_key {
        if presented_options_role_value(root, key) == Some(PRESENTED_OPTIONS_ROLE_ROW) {
            return Some(key);
        }
        current_key = root.node(key).and_then(DomNode::parent_key);
    }
    None
}

/// Resolves a text input owner's datalist key via its `list` attribute + id.
pub(crate) fn resolve_presented_options_datalist_key_for_owner(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let owner = root.node(owner_key)?.as_element()?;
    let list_id = owner
        .attr(&DomAttribute::List)
        .and_then(string_attr_value)?;
    let (list_key, list_node) = root.get_element_by_id(list_id)?;
    let list_element = list_node.as_element()?;
    if list_element.tag != DomTag::Datalist {
        return None;
    }
    Some(list_key)
}

/// Returns every owner key that currently has an open popup.
fn owner_keys_with_presented_options_popup(root: &RootDom) -> Vec<DomNodeSlotKey> {
    root.nodes
        .iter()
        .filter_map(|(key, node)| {
            let element = node.as_element()?;
            if element.tag != DomTag::Select
                && element.behavior().present_options_behavior
                    == DomElementPresentOptionsBehavior::None
            {
                return None;
            }
            presented_options_popup_shell_key(root, key).map(|_| key)
        })
        .collect()
}

/// Returns the owner's popup scroll container key.
fn presented_options_popup_scroll_key(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let shell_key = presented_options_popup_shell_key(root, owner_key)?;
    collect_direct_child_element_keys(root, shell_key)
        .into_iter()
        .find(|key| presented_options_role_value(root, *key) == Some(PRESENTED_OPTIONS_ROLE_SCROLL))
}

/// Whether the owner's popup is currently in the `closing` state.
fn presented_options_popup_is_closing_for_owner(root: &RootDom, owner_key: DomNodeSlotKey) -> bool {
    presented_options_popup_shell_key(root, owner_key)
        .and_then(|shell_key| presented_options_popup_state(root, shell_key))
        .is_some_and(|state| state == PRESENTED_OPTIONS_STATE_CLOSING)
}

/// The source index of the owner's currently highlighted row, if any.
fn highlighted_presented_options_source_index_for_owner(
    root: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Option<usize> {
    let row_key = presented_options_highlighted_row_key(root, owner_key)?;
    presented_options_row_source_index(root, row_key)
}

/// Writes the shell's `PresentedOptionsState` attribute.
fn set_presented_options_popup_state(
    root: &mut RootDom,
    shell_key: DomNodeSlotKey,
    state: &str,
) -> Result<(), DomError> {
    root.set_attr(
        shell_key,
        DomAttribute::PresentedOptionsState,
        Some(DomAttributeValue::String(state.to_string())),
    )
}

/// Writes the shell's opacity inline style.
fn set_presented_options_popup_opacity(
    root: &mut RootDom,
    shell_key: DomNodeSlotKey,
    opacity: f32,
) -> Result<(), DomError> {
    root.style_mut(shell_key)
        .set("opacity", &format!("{opacity}"))
}

#[cfg(test)]
use crate::domains::style::{DeclValue, PresentationProperty};
#[cfg(test)]
use crate::types::{
    DomSelection, DomTimingFunction, DomTransition, DomTransitionConfig, RootDomContext,
};

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{
            ButtonState,
            keyboard::{Key, KeyCode, KeyboardInput},
        },
        prelude::Entity,
    };

    struct PresentedOptionsFixture {
        owner_key: DomNodeSlotKey,
        #[allow(dead_code)]
        option_keys: [DomNodeSlotKey; 3],
    }

    fn append_select_with_options(root: &mut RootDom) -> PresentedOptionsFixture {
        let owner_key = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let third_option = root.create_element(DomTag::Option).expect("third option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        let third_text = root.create_text_node("gamma").expect("third text");

        root.append_child(root.document_node().0, owner_key)
            .expect("attach select");
        for (option_key, text_key, value) in [
            (first_option, first_text, "alpha"),
            (second_option, second_text, "beta"),
            (third_option, third_text, "gamma"),
        ] {
            root.append_child(owner_key, option_key)
                .expect("attach option");
            root.append_child(option_key, text_key)
                .expect("attach option text");
            root.set_attr(
                option_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(value.to_string())),
            )
            .expect("set option value");
        }
        let root_key = root.root;
        set_box(root, root_key, 0.0, 0.0, 240.0, 240.0);
        set_box(root, owner_key, 0.0, 0.0, 120.0, 28.0);

        PresentedOptionsFixture {
            owner_key,
            option_keys: [first_option, second_option, third_option],
        }
    }

    fn append_input_with_datalist_options(root: &mut RootDom) -> PresentedOptionsFixture {
        let owner_key = root.create_element(DomTag::Input).expect("input");
        let list_key = root.create_element(DomTag::Datalist).expect("datalist");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let third_option = root.create_element(DomTag::Option).expect("third option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        let third_text = root.create_text_node("gamma").expect("third text");

        root.append_child(root.document_node().0, owner_key)
            .expect("attach input");
        root.append_child(root.document_node().0, list_key)
            .expect("attach datalist");
        root.set_attr(
            owner_key,
            DomAttribute::List,
            Some(DomAttributeValue::String("pet-list".to_string())),
        )
        .expect("set list attr");
        root.set_attr(
            list_key,
            DomAttribute::Id,
            Some(DomAttributeValue::String("pet-list".to_string())),
        )
        .expect("set datalist id");
        for (option_key, text_key, value) in [
            (first_option, first_text, "alpha"),
            (second_option, second_text, "beta"),
            (third_option, third_text, "gamma"),
        ] {
            root.append_child(list_key, option_key)
                .expect("attach option");
            root.append_child(option_key, text_key)
                .expect("attach option text");
            root.set_attr(
                option_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(value.to_string())),
            )
            .expect("set option value");
        }
        let root_key = root.root;
        set_box(root, root_key, 0.0, 0.0, 240.0, 240.0);
        set_box(root, owner_key, 0.0, 0.0, 120.0, 28.0);

        PresentedOptionsFixture {
            owner_key,
            option_keys: [first_option, second_option, third_option],
        }
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
        root.nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::Value))
            .and_then(|value| match value {
                DomAttributeValue::String(value) => Some(value.clone()),
                DomAttributeValue::Bool(_)
                | DomAttributeValue::Integer(_)
                | DomAttributeValue::Float(_)
                | DomAttributeValue::Color(_) => None,
            })
            .expect("element value should stay string")
    }

    fn popup_state(root: &RootDom, owner_key: DomNodeSlotKey) -> Option<&str> {
        presented_options_popup_shell_key(root, owner_key)
            .and_then(|shell_key| root.nodes.get(shell_key))
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::PresentedOptionsState))
            .and_then(|value| match value {
                DomAttributeValue::String(value) => Some(value.as_str()),
                DomAttributeValue::Bool(_)
                | DomAttributeValue::Integer(_)
                | DomAttributeValue::Float(_)
                | DomAttributeValue::Color(_) => None,
            })
    }

    fn set_box(
        root: &mut RootDom,
        key: DomNodeSlotKey,
        left_px: f32,
        top_px: f32,
        width_px: f32,
        height_px: f32,
    ) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.box_model.border_box.left_px = left_px;
                document.base.box_model.border_box.top_px = top_px;
                document.base.box_model.border_box.width_px = width_px;
                document.base.box_model.border_box.height_px = height_px;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.box_model.border_box.left_px = left_px;
                fragment.base.box_model.border_box.top_px = top_px;
                fragment.base.box_model.border_box.width_px = width_px;
                fragment.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Element(element) => {
                element.base.box_model.border_box.left_px = left_px;
                element.base.box_model.border_box.top_px = top_px;
                element.base.box_model.border_box.width_px = width_px;
                element.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Text(text) => {
                text.base.box_model.border_box.left_px = left_px;
                text.base.box_model.border_box.top_px = top_px;
                text.base.box_model.border_box.width_px = width_px;
                text.base.box_model.border_box.height_px = height_px;
            }
        }
    }

    #[test]
    fn opening_select_presented_options_keeps_focus_on_the_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");

        let handled = apply_select_keyboard_default_for_owner(
            &mut root,
            fixture.owner_key,
            &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
            true,
        )
        .expect("select presented-options open should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert!(presented_options_popup_shell_key(&root, fixture.owner_key).is_some());
    }

    #[test]
    fn opening_input_datalist_presented_options_keeps_focus_on_the_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_input_with_datalist_options(&mut root);
        root.focus(fixture.owner_key).expect("input should focus");

        let handled = apply_presented_options_text_input_keyboard_navigation_for_owner(
            &mut root,
            fixture.owner_key,
            &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
            true,
        )
        .expect("input presented-options open should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert!(presented_options_popup_shell_key(&root, fixture.owner_key).is_some());
    }

    #[test]
    fn moving_presented_options_highlight_does_not_change_owner_focus() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        open_select_popup_for_owner(&mut root, fixture.owner_key).expect("open should succeed");

        let initial_highlight = presented_options_highlighted_row_key(&root, fixture.owner_key)
            .expect("open popup should keep one highlighted row");

        let handled = apply_select_keyboard_default_for_owner(
            &mut root,
            fixture.owner_key,
            &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
            true,
        )
        .expect("highlight move should succeed");

        let popup_row_keys = presented_options_popup_row_keys(&root, fixture.owner_key);
        let moved_highlight = presented_options_highlighted_row_key(&root, fixture.owner_key)
            .expect("open popup should keep one highlighted row");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_ne!(moved_highlight, initial_highlight);
        assert_eq!(moved_highlight, popup_row_keys[1]);
    }

    #[test]
    fn clicking_presented_option_row_commits_without_moving_focus_off_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        open_select_popup_for_owner(&mut root, fixture.owner_key).expect("open should succeed");

        let second_row_key = presented_options_popup_row_keys(&root, fixture.owner_key)
            .get(1)
            .copied()
            .expect("popup should contain a second row");

        let handled = apply_select_click_default_for_target(&mut root, second_row_key)
            .expect("row click commit should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_eq!(element_value(&root, fixture.owner_key), "beta");
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn clicking_input_datalist_option_text_commits_without_moving_focus_off_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_input_with_datalist_options(&mut root);
        root.focus(fixture.owner_key).expect("input should focus");
        open_presented_options_popup_for_owner(&mut root, fixture.owner_key)
            .expect("open should succeed");

        let second_row_key = presented_options_popup_row_keys(&root, fixture.owner_key)
            .get(1)
            .copied()
            .expect("popup should contain a second row");
        let row_text_key = root
            .node(second_row_key)
            .and_then(DomNode::first_child_key_for_store)
            .expect("popup row should contain text");

        let handled = apply_presented_options_click_default_for_target(&mut root, row_text_key)
            .expect("row text click commit should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_eq!(element_value(&root, fixture.owner_key), "beta");
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn blurring_presented_options_owner_starts_popup_close_and_clears_focus() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        open_select_popup_for_owner(&mut root, fixture.owner_key).expect("open should succeed");

        root.blur();

        assert_eq!(root.focused_element_key(), None);
        assert_eq!(root.selection(), DomSelection::None);
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn transition_end_queues_closing_presented_options_popup_shell_for_removal() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        open_select_popup_for_owner(&mut root, fixture.owner_key).expect("open should succeed");
        let shell_key = presented_options_popup_shell_key(&root, fixture.owner_key)
            .expect("open popup should attach shell");

        close_presented_options_popup_for_owner(&mut root, fixture.owner_key, false)
            .expect("close should succeed");
        // Drive the transition-end path through the public surface: schedule an
        // opacity transition on the closing shell and advance past its duration.
        root.set_transition(
            shell_key,
            DomTransition::new(DomTransitionConfig {
                node_key: shell_key,
                property: PresentationProperty::Opacity,
                from: DeclValue::Number(0.0),
                to: DeclValue::Number(1.0),
                started_at_s: 0.0,
                duration_s: 0.1,
                delay_s: 0.0,
                timing: DomTimingFunction::Linear,
            }),
        )
        .expect("set transition");
        root.sync_transitions(0.2);

        assert_eq!(root.drain_pending_layer_remove_roots(), vec![shell_key]);
    }
}
