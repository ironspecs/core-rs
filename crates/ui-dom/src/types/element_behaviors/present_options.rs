//! Presented-options behavior definitions for DOM elements.
//!
//! This file owns the presented-options behavior *definitions*: the select
//! and text-input datalist owner kinds, the popup row and anchor record
//! types, the popup role/state constants, and the popup measurement and
//! record-reading helpers. The presented-options lifecycle methods that
//! execute those definitions live on `RootDom` in `types/root_dom.rs`.

use crate::domains::layout::{layer_kind_for_node, node_box_from_ref};
use crate::numeric::usize_to_f32;
use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::primitives::DomRect;
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomLayerKind, DomNode, DomNodeSlotKey, DomTag,
    RootDom,
};

use super::popup::{
    presented_options_popup_shell_key, resolve_presented_options_datalist_key_for_owner,
};

const PRESENTED_OPTIONS_ROW_HEIGHT_PX: f32 = 28.0;
pub(crate) const PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX: f32 = 8.0;
pub(crate) const PRESENTED_OPTIONS_VERTICAL_PADDING_PX: f32 = 5.0;
pub(crate) const PRESENTED_OPTIONS_ROLE_SHELL: &str = "popup-shell";
pub(crate) const PRESENTED_OPTIONS_ROLE_SCROLL: &str = "popup-scroll";
pub(crate) const PRESENTED_OPTIONS_ROLE_ROW: &str = "popup-row";
pub(crate) const PRESENTED_OPTIONS_STATE_OPEN: &str = "open";
pub(crate) const PRESENTED_OPTIONS_STATE_CLOSING: &str = "closing";

/// Selects whether a non-select element definition participates in presented options.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementPresentOptionsBehavior {
    #[default]
    None,
    TextInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresentedOptionsOwnerKind {
    Select,
    TextInput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresentedOptionRecord {
    pub(crate) source_key: DomNodeSlotKey,
    pub(crate) value: String,
    pub(crate) label: String,
    selected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PresentedOptionsPopupAnchor {
    pub(crate) owner_box: DomRect,
}

pub(crate) fn measure_presented_options_popup_height_px(
    root_dom: &RootDom,
    owner_key: DomNodeSlotKey,
    row_count: usize,
) -> Result<f32, DomError> {
    let popup_anchor = presented_options_popup_anchor_for_owner(root_dom, owner_key)?;
    let viewport_height_px = containing_layer_available_height_px(root_dom, owner_key)?;
    let available_height_px = (viewport_height_px
        - (popup_anchor.owner_box.top_px + popup_anchor.owner_box.height_px))
        .max(1.0);
    let total_height_px = usize_to_f32(row_count) * PRESENTED_OPTIONS_ROW_HEIGHT_PX;
    Ok(total_height_px.min(available_height_px).max(1.0))
}

pub(crate) fn presented_options_row_content_height_px() -> f32 {
    let content_height_px =
        PRESENTED_OPTIONS_ROW_HEIGHT_PX - (PRESENTED_OPTIONS_VERTICAL_PADDING_PX * 2.0);
    assert!(
        content_height_px > 0.0,
        "presented options vertical padding should leave positive row content height"
    );
    content_height_px
}

pub(crate) fn presented_options_popup_anchor_for_owner(
    root_dom: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<PresentedOptionsPopupAnchor, DomError> {
    let owner_node = root_dom.node(owner_key).ok_or(DomError::InvalidNodeKey)?;
    Ok(PresentedOptionsPopupAnchor {
        owner_box: node_box_from_ref(owner_node).border_box,
    })
}

fn containing_layer_available_height_px(
    root_dom: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<f32, DomError> {
    let layer_key = containing_layer_key_for_owner(root_dom, owner_key)?;
    let layer_node = root_dom.node(layer_key).ok_or(DomError::InvalidNodeKey)?;
    let available_height_px = layer_node.layout().available_height_px;
    if available_height_px > 0.0 {
        return Ok(available_height_px);
    }
    Ok(node_box_from_ref(layer_node).border_box.height_px.max(1.0))
}

fn containing_layer_key_for_owner(
    root_dom: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<DomNodeSlotKey, DomError> {
    let mut current_key = Some(owner_key);
    while let Some(key) = current_key {
        let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
        if key == root_dom.root || layer_kind_for_node(node) != DomLayerKind::None {
            return Ok(key);
        }
        current_key = node.parent_key();
    }
    Ok(root_dom.root)
}

pub(crate) fn presented_options_popup_state(
    root_dom: &RootDom,
    shell_key: DomNodeSlotKey,
) -> Option<&str> {
    root_dom
        .node(shell_key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::PresentedOptionsState))
        .and_then(string_attr_value)
}

pub(crate) fn presented_options_role_value(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Option<&str> {
    root_dom
        .node(key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::PresentedOptionsRole))
        .and_then(string_attr_value)
}

pub(crate) fn presented_options_row_is_highlighted(
    root_dom: &RootDom,
    row_key: DomNodeSlotKey,
) -> bool {
    matches!(
        root_dom
            .node(row_key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::PresentedOptionsHighlighted)),
        Some(DomAttributeValue::Bool(true))
    )
}

pub(crate) fn presented_options_row_source_index(
    root_dom: &RootDom,
    row_key: DomNodeSlotKey,
) -> Option<usize> {
    let value = root_dom
        .node(row_key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::PresentedOptionsSourceIndex))?;
    let DomAttributeValue::Integer(value) = value else {
        return None;
    };
    usize::try_from(*value).ok()
}

pub(crate) fn collect_direct_child_element_keys(
    root_dom: &RootDom,
    parent_key: DomNodeSlotKey,
) -> Vec<DomNodeSlotKey> {
    let Some(parent_node) = root_dom.node(parent_key) else {
        return Vec::new();
    };
    let mut child_keys = Vec::new();
    let mut current_child_key = parent_node.first_child_key_for_store();
    while let Some(child_key) = current_child_key {
        let child = root_dom
            .node(child_key)
            .expect("child key should stay live");
        current_child_key = child.next_sibling_key_for_store();
        if child.as_element().is_some() {
            child_keys.push(child_key);
        }
    }
    child_keys
}

pub(crate) fn string_attr_value(value: &DomAttributeValue) -> Option<&str> {
    match value {
        DomAttributeValue::String(value) => Some(value.as_str()),
        DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_)
        | DomAttributeValue::Color(_) => None,
    }
}

pub(crate) fn collect_presented_option_records_from_owner_source(
    root_dom: &RootDom,
    owner_kind: PresentedOptionsOwnerKind,
    owner_key: DomNodeSlotKey,
) -> Result<Vec<PresentedOptionRecord>, DomError> {
    let source_root_key = match owner_kind {
        PresentedOptionsOwnerKind::Select => owner_key,
        PresentedOptionsOwnerKind::TextInput => {
            resolve_presented_options_datalist_key_for_owner(root_dom, owner_key)
                .expect("text-input presented options should resolve a datalist")
        }
    };
    let popup_shell_key = presented_options_popup_shell_key(root_dom, owner_key);
    let mut option_records = Vec::new();
    let mut node_stack = vec![source_root_key];
    while let Some(current_key) = node_stack.pop() {
        if Some(current_key) == popup_shell_key {
            continue;
        }
        let node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        let mut child_key = node.last_child_key_for_store();
        while let Some(current_child_key) = child_key {
            let child = root_dom
                .node(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child.previous_sibling_key_for_store();
            node_stack.push(current_child_key);
        }
        let Some(element) = node.as_element() else {
            continue;
        };
        if element.tag != DomTag::Option {
            continue;
        }
        let label = read_option_label(root_dom, current_key)?;
        let value = read_option_value(root_dom, element, current_key)?;
        let selected = matches!(
            element.attr(&DomAttribute::Selected),
            Some(DomAttributeValue::Bool(true))
        );
        option_records.push(PresentedOptionRecord {
            source_key: current_key,
            value,
            label,
            selected,
        });
    }
    Ok(option_records)
}

pub(crate) fn select_face_label_for_owner_safe(
    root_dom: &RootDom,
    owner_key: DomNodeSlotKey,
) -> Result<String, DomError> {
    let Some(owner) = root_dom.node(owner_key).and_then(DomNode::as_element) else {
        return Err(DomError::InvalidNodeKey);
    };
    if owner.tag != DomTag::Select {
        return Err(DomError::InvalidAttributeTarget);
    }
    let option_records = collect_presented_option_records_from_owner_source(
        root_dom,
        PresentedOptionsOwnerKind::Select,
        owner_key,
    )?;
    if let Some(option_record) = option_records.iter().find(|record| record.selected) {
        return Ok(option_record.label.clone());
    }
    let value = owner
        .attr(&DomAttribute::Value)
        .and_then(string_attr_value)
        .unwrap_or("");
    if let Some(option_record) = option_records
        .iter()
        .find(|record| record.value.as_str() == value)
    {
        return Ok(option_record.label.clone());
    }
    Ok(value.to_string())
}

fn read_option_label(root_dom: &RootDom, option_key: DomNodeSlotKey) -> Result<String, DomError> {
    let option = root_dom
        .node(option_key)
        .and_then(DomNode::as_element)
        .ok_or(DomError::InvalidNodeKey)?;
    if let Some(label) = option
        .attr(&DomAttribute::Label)
        .and_then(string_attr_value)
    {
        return Ok(label.to_string());
    }
    collect_subtree_text_content(root_dom, option_key)
}

fn read_option_value(
    root_dom: &RootDom,
    option: &crate::types::DomElementNode,
    option_key: DomNodeSlotKey,
) -> Result<String, DomError> {
    if let Some(value) = option
        .attr(&DomAttribute::Value)
        .and_then(string_attr_value)
    {
        return Ok(value.to_string());
    }
    read_option_label(root_dom, option_key)
}

fn collect_subtree_text_content(
    root_dom: &RootDom,
    subtree_root_key: DomNodeSlotKey,
) -> Result<String, DomError> {
    let mut text_content = String::new();
    let mut node_stack = vec![subtree_root_key];
    while let Some(current_key) = node_stack.pop() {
        let node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        if let Some(text) = node.as_text() {
            text_content.push_str(text.data());
        }
        let mut child_key = node.last_child_key_for_store();
        while let Some(current_child_key) = child_key {
            let child = root_dom
                .node(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child.previous_sibling_key_for_store();
            node_stack.push(current_child_key);
        }
    }
    Ok(text_content)
}

pub(crate) fn choose_presented_options_highlighted_index(
    root_dom: &RootDom,
    owner_kind: PresentedOptionsOwnerKind,
    owner_key: DomNodeSlotKey,
    option_records: &[PresentedOptionRecord],
    previous_highlighted_index: Option<usize>,
) -> usize {
    if let Some(previous_index) = previous_highlighted_index
        && previous_index < option_records.len()
    {
        return previous_index;
    }
    if let Some(selected_index) = option_records.iter().position(|record| record.selected) {
        return selected_index;
    }
    if owner_kind == PresentedOptionsOwnerKind::TextInput
        && let Some(owner) = root_dom.node(owner_key).and_then(DomNode::as_element)
        && let Some(value) = owner.attr(&DomAttribute::Value).and_then(string_attr_value)
        && let Some(exact_match_index) = option_records
            .iter()
            .position(|record| record.value == value)
    {
        return exact_match_index;
    }
    0
}

pub(crate) fn set_select_source_option_selected_attrs(
    root_dom: &mut RootDom,
    owner_key: DomNodeSlotKey,
    selected_source_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let option_records = collect_presented_option_records_from_owner_source(
        root_dom,
        PresentedOptionsOwnerKind::Select,
        owner_key,
    )?;
    for option_record in option_records {
        root_dom.set_attr(
            option_record.source_key,
            DomAttribute::Selected,
            Some(DomAttributeValue::Bool(
                option_record.source_key == selected_source_key,
            )),
        )?;
    }
    Ok(())
}
