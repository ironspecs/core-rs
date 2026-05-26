use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

use crate::domains::layout::{layer_kind_for_node, node_box_from_ref};
use crate::numeric::usize_to_f32;
use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::primitives::DomRect;
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomLayerKind, DomNode, DomNodeSlotKey, DomTag,
    LayerOrder, RootDom,
};

const PRESENTED_OPTIONS_ROW_HEIGHT_PX: f32 = 28.0;
const PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX: f32 = 8.0;
const PRESENTED_OPTIONS_VERTICAL_PADDING_PX: f32 = 5.0;
const PRESENTED_OPTIONS_ROLE_SHELL: &str = "popup-shell";
const PRESENTED_OPTIONS_ROLE_SCROLL: &str = "popup-scroll";
const PRESENTED_OPTIONS_ROLE_ROW: &str = "popup-row";
const PRESENTED_OPTIONS_STATE_OPEN: &str = "open";
const PRESENTED_OPTIONS_STATE_CLOSING: &str = "closing";

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
struct PresentedOptionRecord {
    source_key: DomNodeSlotKey,
    value: String,
    label: String,
    selected: bool,
}

#[derive(Clone, Copy, Debug)]
struct PresentedOptionsPopupAnchor {
    owner_box: DomRect,
}

impl RootDom {
    pub(crate) fn apply_select_keyboard_default_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::Select)
        {
            return Ok(false);
        }
        match input.logical_key {
            Key::ArrowDown => self.move_presented_options_highlight_or_open_popup(owner_key, true),
            Key::ArrowUp => self.move_presented_options_highlight_or_open_popup(owner_key, false),
            Key::Enter | Key::Space => {
                self.commit_highlighted_presented_option_or_open_popup(owner_key)
            }
            Key::Escape => self.close_presented_options_popup_for_owner(owner_key, false),
            _ => Ok(false),
        }
    }

    pub(crate) fn apply_presented_options_text_input_keyboard_navigation_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(false);
        }
        match input.logical_key {
            Key::ArrowDown => self.move_presented_options_highlight_or_open_popup(owner_key, true),
            Key::ArrowUp => self.move_presented_options_highlight_or_open_popup(owner_key, false),
            Key::Enter => self.commit_highlighted_presented_option_for_owner(owner_key),
            Key::Escape => self.close_presented_options_popup_for_owner(owner_key, false),
            _ => Ok(false),
        }
    }

    pub(crate) fn apply_presented_options_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.commit_presented_option_hit_row(target_key)? {
            return Ok(true);
        }
        let Some(owner_key) = self.resolve_presented_options_owner_key_from_target(target_key)
        else {
            return Ok(false);
        };
        if self.presented_options_popup_shell_key(owner_key).is_some() {
            return self.close_presented_options_popup_for_owner(owner_key, false);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    pub(crate) fn apply_select_popup_row_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self
            .resolve_presented_options_row_key_from_target(target_key)
            .is_none()
        {
            return Ok(false);
        }
        self.commit_select_presented_option_hit_row(target_key)
    }

    pub(crate) fn apply_select_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.commit_select_presented_option_hit_row(target_key)? {
            return Ok(true);
        }
        let Some(owner_key) = self.resolve_select_owner_key_from_target(target_key) else {
            return Ok(false);
        };
        if self.presented_options_popup_shell_key(owner_key).is_some() {
            return self.close_presented_options_popup_for_owner(owner_key, false);
        }
        self.open_select_popup_for_owner(owner_key)
    }

    pub(crate) fn close_select_popup_for_blurred_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::Select)
        {
            return Ok(false);
        }
        self.close_presented_options_popup_for_owner(owner_key, false)
    }

    pub(crate) fn open_presented_options_for_focused_text_input_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(false);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    pub(crate) fn close_presented_options_for_blurred_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        self.close_presented_options_popup_for_owner(owner_key, false)
    }

    pub(crate) fn reopen_presented_options_for_text_input_value_change(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(());
        }
        let _ = self.open_presented_options_popup_for_owner(owner_key)?;
        Ok(())
    }

    pub(crate) fn resolve_presented_options_owner_kind(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<PresentedOptionsOwnerKind> {
        let element = self.node(owner_key)?.as_element()?;
        if element.tag == DomTag::Select {
            return Some(PresentedOptionsOwnerKind::Select);
        }
        match element.behavior().present_options_behavior {
            DomElementPresentOptionsBehavior::None => None,
            DomElementPresentOptionsBehavior::TextInput => self
                .resolve_presented_options_datalist_key_for_owner(owner_key)
                .map(|_| PresentedOptionsOwnerKind::TextInput),
        }
    }

    pub(crate) fn open_presented_options_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        self.remove_other_presented_options_popups_immediately(owner_key)?;
        self.replace_presented_options_popup_for_owner_kind(owner_kind, owner_key)
    }

    pub(crate) fn open_select_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_select_owner_key_from_target(owner_key) != Some(owner_key) {
            return Ok(false);
        }
        self.remove_other_presented_options_popups_immediately(owner_key)?;
        self.replace_presented_options_popup_for_owner_kind(
            PresentedOptionsOwnerKind::Select,
            owner_key,
        )
    }

    pub(crate) fn close_presented_options_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        immediate: bool,
    ) -> Result<bool, DomError> {
        let Some(shell_key) = self.presented_options_popup_shell_key(owner_key) else {
            return Ok(false);
        };
        if immediate {
            self.remove_node(shell_key)?;
            return Ok(true);
        }
        if presented_options_popup_state(self, shell_key) == Some(PRESENTED_OPTIONS_STATE_CLOSING) {
            return Ok(true);
        }
        self.set_presented_options_popup_state(shell_key, PRESENTED_OPTIONS_STATE_CLOSING)?;
        self.set_presented_options_popup_opacity(shell_key, 0.0)?;
        Ok(true)
    }

    pub(crate) fn presented_options_popup_shell_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        collect_direct_child_element_keys(self, owner_key)
            .into_iter()
            .find(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_SHELL)
            })
    }

    pub(crate) fn presented_options_highlighted_row_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        self.presented_options_popup_row_keys(owner_key)
            .into_iter()
            .find(|row_key| presented_options_row_is_highlighted(self, *row_key))
    }

    pub(crate) fn presented_options_popup_row_keys(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Vec<DomNodeSlotKey> {
        let Some(scroll_key) = self.presented_options_popup_scroll_key(owner_key) else {
            return Vec::new();
        };
        collect_direct_child_element_keys(self, scroll_key)
            .into_iter()
            .filter(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_ROW)
            })
            .collect()
    }

    fn move_presented_options_highlight_or_open_popup(
        &mut self,
        owner_key: DomNodeSlotKey,
        move_forward: bool,
    ) -> Result<bool, DomError> {
        if self.presented_options_popup_shell_key(owner_key).is_some()
            && !self.presented_options_popup_is_closing_for_owner(owner_key)
        {
            return self.move_presented_options_popup_highlight(owner_key, move_forward);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    fn commit_highlighted_presented_option_or_open_popup(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.presented_options_popup_shell_key(owner_key).is_some()
            && !self.presented_options_popup_is_closing_for_owner(owner_key)
        {
            return self.commit_highlighted_presented_option_for_owner(owner_key);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    fn replace_presented_options_popup_for_owner_kind(
        &mut self,
        owner_kind: PresentedOptionsOwnerKind,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let popup_anchor = presented_options_popup_anchor_for_owner(self, owner_key)?;
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        if option_records.is_empty() {
            let _ = self.close_presented_options_popup_for_owner(owner_key, true)?;
            return Ok(false);
        }
        let previous_highlighted_index =
            self.highlighted_presented_options_source_index_for_owner(owner_key);
        let _ = self.close_presented_options_popup_for_owner(owner_key, true)?;
        let highlighted_index = choose_presented_options_highlighted_index(
            self,
            owner_kind,
            owner_key,
            &option_records,
            previous_highlighted_index,
        );
        let popup_shell_key = self.append_presented_options_popup_shell_and_rows(
            owner_key,
            popup_anchor,
            &option_records,
            highlighted_index,
        )?;
        self.update_presented_options_popup_box_attrs_from_owner(owner_key)?;
        self.set_presented_options_popup_state(popup_shell_key, PRESENTED_OPTIONS_STATE_OPEN)?;
        self.set_presented_options_popup_opacity(popup_shell_key, 1.0)?;
        Ok(true)
    }

    fn append_presented_options_popup_shell_and_rows(
        &mut self,
        owner_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
        option_records: &[PresentedOptionRecord],
        highlighted_index: usize,
    ) -> Result<DomNodeSlotKey, DomError> {
        let popup_height_px =
            measure_presented_options_popup_height_px(self, owner_key, option_records.len())?;
        let popup_shell_key = self.create_element(DomTag::Div)?;
        let popup_scroll_key = self.create_element(DomTag::Div)?;
        self.set_layout_layer_order(popup_shell_key, LayerOrder::Raised)?;
        self.set_presented_options_popup_shell_style_attrs(
            popup_shell_key,
            popup_anchor,
            popup_height_px,
        )?;
        self.set_presented_options_popup_scroll_style_attrs(
            popup_scroll_key,
            popup_anchor.owner_box.width_px,
            popup_height_px,
        )?;
        self.append_child(popup_shell_key, popup_scroll_key)?;
        for (source_index, option_record) in option_records.iter().enumerate() {
            self.append_presented_options_row_element(
                popup_scroll_key,
                popup_anchor.owner_box.width_px,
                source_index,
                option_record,
                source_index == highlighted_index,
            )?;
        }
        self.append_child(owner_key, popup_shell_key)?;
        Ok(popup_shell_key)
    }

    fn append_presented_options_row_element(
        &mut self,
        popup_scroll_key: DomNodeSlotKey,
        owner_width_px: f32,
        source_index: usize,
        option_record: &PresentedOptionRecord,
        highlighted: bool,
    ) -> Result<(), DomError> {
        let row_key = self.create_element(DomTag::Option)?;
        let text_key = self.create_text_node(option_record.label.clone())?;
        let row_content_height_px = presented_options_row_content_height_px();
        self.append_child(popup_scroll_key, row_key)?;
        self.append_child(row_key, text_key)?;
        self.set_attr_str(row_key, "display", "block")?;
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_ROW.to_string(),
            )),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsSourceIndex,
            Some(DomAttributeValue::Integer(
                i64::try_from(source_index).expect("source index should fit into i64"),
            )),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(option_record.value.clone())),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                owner_width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!(
                "{row_content_height_px}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::LineHeight,
            Some(DomAttributeValue::String(format!(
                "{row_content_height_px}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingLeft,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingRight,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingTop,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingBottom,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px"
            ))),
        )?;
        self.set_presented_options_row_highlight_attrs(row_key, highlighted)
    }

    fn set_presented_options_popup_shell_style_attrs(
        &mut self,
        popup_shell_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
        popup_height_px: f32,
    ) -> Result<(), DomError> {
        self.set_attr_str(popup_shell_key, "display", "block")?;
        self.set_attr_str(popup_shell_key, "position", "absolute")?;
        self.set_presented_options_popup_anchor_attrs(popup_shell_key, popup_anchor)?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr_str(popup_shell_key, "transition", "opacity 120ms ease-out")?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Custom("background-color".to_string()),
            Some(DomAttributeValue::String("#141922".to_string())),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Custom("border-color".to_string()),
            Some(DomAttributeValue::String("#394254".to_string())),
        )?;
        self.set_attr_str(popup_shell_key, "border-top", "1px")?;
        self.set_attr_str(popup_shell_key, "border-right", "1px")?;
        self.set_attr_str(popup_shell_key, "border-bottom", "1px")?;
        self.set_attr_str(popup_shell_key, "border-left", "1px")?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_SHELL.to_string(),
            )),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::PresentedOptionsState,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_STATE_OPEN.to_string(),
            )),
        )
    }

    fn set_presented_options_popup_anchor_attrs(
        &mut self,
        popup_shell_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
    ) -> Result<(), DomError> {
        self.set_attr(
            popup_shell_key,
            DomAttribute::Left,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.left_px
            ))),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Top,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.top_px + popup_anchor.owner_box.height_px
            ))),
        )
    }

    fn set_presented_options_popup_scroll_style_attrs(
        &mut self,
        popup_scroll_key: DomNodeSlotKey,
        owner_width_px: f32,
        popup_height_px: f32,
    ) -> Result<(), DomError> {
        self.set_attr_str(popup_scroll_key, "display", "block")?;
        self.set_attr_str(popup_scroll_key, "overflow", "scroll")?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                owner_width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_SCROLL.to_string(),
            )),
        )
    }

    fn update_presented_options_popup_box_attrs_from_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        let Some(shell_key) = self.presented_options_popup_shell_key(owner_key) else {
            return Ok(());
        };
        let Some(scroll_key) = self.presented_options_popup_scroll_key(owner_key) else {
            return Ok(());
        };
        let popup_anchor = presented_options_popup_anchor_for_owner(self, owner_key)?;
        let popup_height_px = measure_presented_options_popup_height_px(
            self,
            owner_key,
            self.presented_options_popup_row_keys(owner_key).len(),
        )?;
        self.set_presented_options_popup_anchor_attrs(shell_key, popup_anchor)?;
        self.set_attr(
            shell_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            shell_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr(
            scroll_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            scroll_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        Ok(())
    }

    fn remove_other_presented_options_popups_immediately(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        for other_owner_key in self.owner_keys_with_presented_options_popup() {
            if other_owner_key != owner_key {
                let _ = self.close_presented_options_popup_for_owner(other_owner_key, true)?;
            }
        }
        Ok(())
    }

    fn commit_presented_option_hit_row(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(row_key) = self.resolve_presented_options_row_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(owner_key) = self.resolve_presented_options_owner_key_from_target(target_key)
        else {
            return Ok(false);
        };
        let Some(source_index) = presented_options_row_source_index(self, row_key) else {
            return Ok(false);
        };
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_presented_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn commit_select_presented_option_hit_row(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(row_key) = self.resolve_presented_options_row_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(owner_key) = self.resolve_select_owner_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(source_index) = presented_options_row_source_index(self, row_key) else {
            return Ok(false);
        };
        let option_records = collect_presented_option_records_from_owner_source(
            self,
            PresentedOptionsOwnerKind::Select,
            owner_key,
        )?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_select_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn commit_highlighted_presented_option_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(source_index) =
            self.highlighted_presented_options_source_index_for_owner(owner_key)
        else {
            return Ok(false);
        };
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_presented_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn apply_presented_option_to_owner_and_dispatch_events(
        &mut self,
        owner_key: DomNodeSlotKey,
        source_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        match owner_kind {
            PresentedOptionsOwnerKind::Select => {
                return self.apply_select_option_to_owner_and_dispatch_events(
                    owner_key, source_key, value,
                );
            }
            PresentedOptionsOwnerKind::TextInput => {
                self.set_attr(
                    owner_key,
                    DomAttribute::Value,
                    Some(DomAttributeValue::String(value.clone())),
                )?;
                let _ = self.dispatch_input_event_to_target_key(owner_key, value.clone())?;
                let _ = self.dispatch_change_event_to_target_key(owner_key, value)?;
            }
        }
        let _ = self.close_presented_options_popup_for_owner(owner_key, false)?;
        Ok(true)
    }

    fn apply_select_option_to_owner_and_dispatch_events(
        &mut self,
        owner_key: DomNodeSlotKey,
        source_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        set_select_source_option_selected_attrs(self, owner_key, source_key)?;
        self.set_attr(
            owner_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(value.clone())),
        )?;
        let _ = self.dispatch_input_event_to_target_key(owner_key, value.clone())?;
        let _ = self.dispatch_change_event_to_target_key(owner_key, value)?;
        let _ = self.close_presented_options_popup_for_owner(owner_key, false)?;
        Ok(true)
    }

    fn move_presented_options_popup_highlight(
        &mut self,
        owner_key: DomNodeSlotKey,
        move_forward: bool,
    ) -> Result<bool, DomError> {
        let row_keys = self.presented_options_popup_row_keys(owner_key);
        if row_keys.is_empty() {
            return Ok(false);
        }
        let current_index = row_keys
            .iter()
            .position(|row_key| presented_options_row_is_highlighted(self, *row_key))
            .unwrap_or(0);
        let next_index = if move_forward {
            (current_index + 1).min(row_keys.len() - 1)
        } else {
            current_index.saturating_sub(1)
        };
        self.highlight_presented_options_popup_row_at_index(owner_key, next_index)?;
        Ok(true)
    }

    fn highlight_presented_options_popup_row_at_index(
        &mut self,
        owner_key: DomNodeSlotKey,
        highlighted_index: usize,
    ) -> Result<(), DomError> {
        let row_keys = self.presented_options_popup_row_keys(owner_key);
        for (row_index, row_key) in row_keys.into_iter().enumerate() {
            self.set_presented_options_row_highlight_attrs(
                row_key,
                row_index == highlighted_index,
            )?;
        }
        Ok(())
    }

    fn set_presented_options_row_highlight_attrs(
        &mut self,
        row_key: DomNodeSlotKey,
        highlighted: bool,
    ) -> Result<(), DomError> {
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsHighlighted,
            highlighted.then_some(DomAttributeValue::Bool(true)),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Custom("background-color".to_string()),
            highlighted.then(|| DomAttributeValue::String("#273754".to_string())),
        )
    }

    fn resolve_presented_options_owner_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            let node = self.node(key)?;
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

    fn resolve_select_owner_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            let node = self.node(key)?;
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

    fn resolve_presented_options_row_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            if presented_options_role_value(self, key) == Some(PRESENTED_OPTIONS_ROLE_ROW) {
                return Some(key);
            }
            current_key = self.node(key).and_then(DomNode::parent_key);
        }
        None
    }

    fn resolve_presented_options_datalist_key_for_owner(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let owner = self.node(owner_key)?.as_element()?;
        let list_id = owner
            .attr(&DomAttribute::List)
            .and_then(string_attr_value)?;
        let (list_key, list_node) = self.get_element_by_id(list_id)?;
        let list_element = list_node.as_element()?;
        if list_element.tag != DomTag::Datalist {
            return None;
        }
        Some(list_key)
    }

    fn owner_keys_with_presented_options_popup(&self) -> Vec<DomNodeSlotKey> {
        self.nodes
            .iter()
            .filter_map(|(key, node)| {
                let element = node.as_element()?;
                if element.tag != DomTag::Select
                    && element.behavior().present_options_behavior
                        == DomElementPresentOptionsBehavior::None
                {
                    return None;
                }
                self.presented_options_popup_shell_key(key).map(|_| key)
            })
            .collect()
    }

    fn presented_options_popup_scroll_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let shell_key = self.presented_options_popup_shell_key(owner_key)?;
        collect_direct_child_element_keys(self, shell_key)
            .into_iter()
            .find(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_SCROLL)
            })
    }

    fn presented_options_popup_is_closing_for_owner(&self, owner_key: DomNodeSlotKey) -> bool {
        self.presented_options_popup_shell_key(owner_key)
            .and_then(|shell_key| presented_options_popup_state(self, shell_key))
            .is_some_and(|state| state == PRESENTED_OPTIONS_STATE_CLOSING)
    }

    fn highlighted_presented_options_source_index_for_owner(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<usize> {
        let row_key = self.presented_options_highlighted_row_key(owner_key)?;
        presented_options_row_source_index(self, row_key)
    }

    fn set_presented_options_popup_state(
        &mut self,
        shell_key: DomNodeSlotKey,
        state: &str,
    ) -> Result<(), DomError> {
        self.set_attr(
            shell_key,
            DomAttribute::PresentedOptionsState,
            Some(DomAttributeValue::String(state.to_string())),
        )
    }

    fn set_presented_options_popup_opacity(
        &mut self,
        shell_key: DomNodeSlotKey,
        opacity: f32,
    ) -> Result<(), DomError> {
        self.set_attr(
            shell_key,
            DomAttribute::Custom("opacity".to_string()),
            Some(DomAttributeValue::Float(opacity)),
        )
    }
}

fn measure_presented_options_popup_height_px(
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

fn presented_options_row_content_height_px() -> f32 {
    let content_height_px =
        PRESENTED_OPTIONS_ROW_HEIGHT_PX - (PRESENTED_OPTIONS_VERTICAL_PADDING_PX * 2.0);
    assert!(
        content_height_px > 0.0,
        "presented options vertical padding should leave positive row content height"
    );
    content_height_px
}

fn presented_options_popup_anchor_for_owner(
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

fn presented_options_popup_state(root_dom: &RootDom, shell_key: DomNodeSlotKey) -> Option<&str> {
    root_dom
        .node(shell_key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::PresentedOptionsState))
        .and_then(string_attr_value)
}

fn presented_options_role_value(root_dom: &RootDom, key: DomNodeSlotKey) -> Option<&str> {
    root_dom
        .node(key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attr(&DomAttribute::PresentedOptionsRole))
        .and_then(string_attr_value)
}

fn presented_options_row_is_highlighted(root_dom: &RootDom, row_key: DomNodeSlotKey) -> bool {
    matches!(
        root_dom
            .node(row_key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::PresentedOptionsHighlighted)),
        Some(DomAttributeValue::Bool(true))
    )
}

fn presented_options_row_source_index(
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

fn collect_direct_child_element_keys(
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

fn string_attr_value(value: &DomAttributeValue) -> Option<&str> {
    match value {
        DomAttributeValue::String(value) => Some(value.as_str()),
        DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_)
        | DomAttributeValue::Color(_) => None,
    }
}

fn collect_presented_option_records_from_owner_source(
    root_dom: &RootDom,
    owner_kind: PresentedOptionsOwnerKind,
    owner_key: DomNodeSlotKey,
) -> Result<Vec<PresentedOptionRecord>, DomError> {
    let source_root_key = match owner_kind {
        PresentedOptionsOwnerKind::Select => owner_key,
        PresentedOptionsOwnerKind::TextInput => root_dom
            .resolve_presented_options_datalist_key_for_owner(owner_key)
            .expect("text-input presented options should resolve a datalist"),
    };
    let popup_shell_key = root_dom.presented_options_popup_shell_key(owner_key);
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

fn choose_presented_options_highlighted_index(
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

fn set_select_source_option_selected_attrs(
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
