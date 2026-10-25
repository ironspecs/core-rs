//! Presented-options popup DOM construction (PR 5, E6-b).
//!
//! This file owns the popup *construction* concern: building the
//! shell/scroll/rows DOM subtree and writing its role/state attributes, inline
//! styles, and layer ordering. The former `RootDom` methods
//! `append_presented_options_popup_shell_and_rows`,
//! `append_presented_options_row_element`, and the three `set_*_style_attrs`
//! setters re-homed here as free functions over `&mut RootDom` (E6-c's
//! evidence-gated narrowing was REJECTED — the rows are document-wide).
//!
//! Invariants and rules:
//!
//! - this file is pure construction: it takes a resolved `(owner_key,
//!   popup_anchor, option_records, highlighted_index)` and emits DOM — no
//!   open/close/commit state machine (that lives in `popup.rs`).
//! - select and datalist construction share this one builder: the two shells
//!   were already unified in E6-a via
//!   `collect_presented_option_records_from_owner_source` +
//!   `replace_presented_options_popup_for_owner_kind`; this file is that
//!   builder's construction half.
//! - the shell carries the `transition: opacity 120ms ease-out` inline style
//!   that drives the open/close transition; the `popup-row`/`popup-scroll`/
//!   `popup-shell` role values are the query keys `popup.rs` reads.

use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomNodeSlotKey, DomTag, LayerOrder, RootDom,
};

use super::present_options::{
    PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX, PRESENTED_OPTIONS_ROLE_ROW,
    PRESENTED_OPTIONS_ROLE_SCROLL, PRESENTED_OPTIONS_ROLE_SHELL, PRESENTED_OPTIONS_STATE_OPEN,
    PRESENTED_OPTIONS_VERTICAL_PADDING_PX, PresentedOptionRecord, PresentedOptionsPopupAnchor,
    measure_presented_options_popup_height_px, presented_options_row_content_height_px,
};

/// Builds and attaches the popup shell + scroll + one row per option record,
/// anchored under `owner_key`. Returns the shell key. This is the one
/// `build_options_popup` builder: select and datalist both land here.
pub(crate) fn append_presented_options_popup_shell_and_rows(
    root: &mut RootDom,
    owner_key: DomNodeSlotKey,
    popup_anchor: PresentedOptionsPopupAnchor,
    option_records: &[PresentedOptionRecord],
    highlighted_index: usize,
) -> Result<DomNodeSlotKey, DomError> {
    let popup_height_px =
        measure_presented_options_popup_height_px(root, owner_key, option_records.len())?;
    let popup_shell_key = root.create_element(DomTag::Div)?;
    let popup_scroll_key = root.create_element(DomTag::Div)?;
    root.set_layout_layer_order(popup_shell_key, LayerOrder::Raised)?;
    set_presented_options_popup_shell_style_attrs(
        root,
        popup_shell_key,
        popup_anchor,
        popup_height_px,
    )?;
    set_presented_options_popup_scroll_style_attrs(
        root,
        popup_scroll_key,
        popup_anchor.owner_box.width_px,
        popup_height_px,
    )?;
    root.append_child(popup_shell_key, popup_scroll_key)?;
    for (source_index, option_record) in option_records.iter().enumerate() {
        append_presented_options_row_element(
            root,
            popup_scroll_key,
            popup_anchor.owner_box.width_px,
            source_index,
            option_record,
            source_index == highlighted_index,
        )?;
    }
    root.append_child(owner_key, popup_shell_key)?;
    Ok(popup_shell_key)
}

/// Builds one popup row `<option>` with its label text, role/source-index/value
/// attributes, inline box styles, and highlight attributes.
fn append_presented_options_row_element(
    root: &mut RootDom,
    popup_scroll_key: DomNodeSlotKey,
    owner_width_px: f32,
    source_index: usize,
    option_record: &PresentedOptionRecord,
    highlighted: bool,
) -> Result<(), DomError> {
    let row_key = root.create_element(DomTag::Option)?;
    let text_key = root.create_text_node(option_record.label.clone())?;
    let row_content_height_px = presented_options_row_content_height_px();
    root.append_child(popup_scroll_key, row_key)?;
    root.append_child(row_key, text_key)?;
    root.style_mut(row_key).set("display", "block")?;
    root.set_attr(
        row_key,
        DomAttribute::PresentedOptionsRole,
        Some(DomAttributeValue::String(
            PRESENTED_OPTIONS_ROLE_ROW.to_string(),
        )),
    )?;
    root.set_attr(
        row_key,
        DomAttribute::PresentedOptionsSourceIndex,
        Some(DomAttributeValue::Integer(
            i64::try_from(source_index).expect("source index should fit into i64"),
        )),
    )?;
    root.set_attr(
        row_key,
        DomAttribute::Value,
        Some(DomAttributeValue::String(option_record.value.clone())),
    )?;
    root.style_mut(row_key)
        .set("width", &(format!("{}px", owner_width_px.max(1.0))))?;
    root.style_mut(row_key)
        .set("height", &(format!("{row_content_height_px}px")))?;
    root.style_mut(row_key)
        .set("line-height", &(format!("{row_content_height_px}px")))?;
    root.style_mut(row_key).set(
        "padding-left",
        &(format!("{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px")),
    )?;
    root.style_mut(row_key).set(
        "padding-right",
        &(format!("{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px")),
    )?;
    root.style_mut(row_key).set(
        "padding-top",
        &(format!("{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px")),
    )?;
    root.style_mut(row_key).set(
        "padding-bottom",
        &(format!("{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px")),
    )?;
    set_presented_options_row_highlight_attrs(root, row_key, highlighted)
}

/// Writes one row's highlight attribute and background-color style (the
/// highlight state machine and the initial-row construction both use this).
pub(crate) fn set_presented_options_row_highlight_attrs(
    root: &mut RootDom,
    row_key: DomNodeSlotKey,
    highlighted: bool,
) -> Result<(), DomError> {
    root.set_attr(
        row_key,
        DomAttribute::PresentedOptionsHighlighted,
        highlighted.then_some(DomAttributeValue::Bool(true)),
    )?;
    if highlighted {
        root.style_mut(row_key).set("background-color", "#273754")?;
    } else {
        root.style_mut(row_key).remove("background-color")?;
    }
    Ok(())
}

/// Writes the shell's display/position/geometry/transition/color/border styles
/// and its role + `open` state attributes.
fn set_presented_options_popup_shell_style_attrs(
    root: &mut RootDom,
    popup_shell_key: DomNodeSlotKey,
    popup_anchor: PresentedOptionsPopupAnchor,
    popup_height_px: f32,
) -> Result<(), DomError> {
    root.style_mut(popup_shell_key).set("display", "block")?;
    root.style_mut(popup_shell_key)
        .set("position", "absolute")?;
    set_presented_options_popup_anchor_attrs(root, popup_shell_key, popup_anchor)?;
    root.style_mut(popup_shell_key).set(
        "width",
        &(format!("{}px", popup_anchor.owner_box.width_px.max(1.0))),
    )?;
    root.style_mut(popup_shell_key)
        .set("height", &(format!("{popup_height_px}px")))?;
    root.style_mut(popup_shell_key)
        .set("transition", "opacity 120ms ease-out")?;
    root.style_mut(popup_shell_key)
        .set("background-color", "#141922")?;
    root.style_mut(popup_shell_key)
        .set("border-color", "#394254")?;
    root.style_mut(popup_shell_key).set("border-top", "1px")?;
    root.style_mut(popup_shell_key).set("border-right", "1px")?;
    root.style_mut(popup_shell_key)
        .set("border-bottom", "1px")?;
    root.style_mut(popup_shell_key).set("border-left", "1px")?;
    root.set_attr(
        popup_shell_key,
        DomAttribute::PresentedOptionsRole,
        Some(DomAttributeValue::String(
            PRESENTED_OPTIONS_ROLE_SHELL.to_string(),
        )),
    )?;
    root.set_attr(
        popup_shell_key,
        DomAttribute::PresentedOptionsState,
        Some(DomAttributeValue::String(
            PRESENTED_OPTIONS_STATE_OPEN.to_string(),
        )),
    )
}

/// Writes the shell's absolute-position left/top from the owner's border box.
pub(crate) fn set_presented_options_popup_anchor_attrs(
    root: &mut RootDom,
    popup_shell_key: DomNodeSlotKey,
    popup_anchor: PresentedOptionsPopupAnchor,
) -> Result<(), DomError> {
    root.style_mut(popup_shell_key)
        .set("left", &(format!("{}px", popup_anchor.owner_box.left_px)))?;
    root.style_mut(popup_shell_key).set(
        "top",
        &(format!(
            "{}px",
            popup_anchor.owner_box.top_px + popup_anchor.owner_box.height_px
        )),
    )
}

/// Writes the scroll container's display/overflow/size styles and role.
fn set_presented_options_popup_scroll_style_attrs(
    root: &mut RootDom,
    popup_scroll_key: DomNodeSlotKey,
    owner_width_px: f32,
    popup_height_px: f32,
) -> Result<(), DomError> {
    root.style_mut(popup_scroll_key).set("display", "block")?;
    root.style_mut(popup_scroll_key).set("overflow", "scroll")?;
    root.style_mut(popup_scroll_key)
        .set("width", &(format!("{}px", owner_width_px.max(1.0))))?;
    root.style_mut(popup_scroll_key)
        .set("height", &(format!("{popup_height_px}px")))?;
    root.set_attr(
        popup_scroll_key,
        DomAttribute::PresentedOptionsRole,
        Some(DomAttributeValue::String(
            PRESENTED_OPTIONS_ROLE_SCROLL.to_string(),
        )),
    )
}
