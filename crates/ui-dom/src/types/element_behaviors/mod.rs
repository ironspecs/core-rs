//! Element behavior definitions that sit between tags/attrs and lifecycle execution.

mod activation;
mod behavior_table;
mod button;
mod checkbox;
mod controls;
mod dom_event_dispatch_outcome;
mod intrinsic_size;
mod paint;
mod present_options;
mod range;
mod select;
mod text_entry;

pub(crate) use activation::{
    ActiveRangeDragSession, PressedPointerClickTarget, dispatch_click_and_apply_after_click,
    pressed_pointer_click_target_from_raw_target,
};
pub use activation::{
    DomElementAfterClick, DomElementAfterFocus, DomElementOnActivation, DomElementOnBlur,
    DomElementOnKeyboard, DomElementOnPointer,
};
pub(crate) use behavior_table::{behavior_row, resolve_kind};
pub(crate) use controls::count_unicode_scalars;
pub(crate) use dom_event_dispatch_outcome::DomEventDispatchOutcome;
pub use intrinsic_size::DomElementIntrinsicKind;
pub(crate) use intrinsic_size::{
    commit_intrinsic_after_style_recompute, compute_intrinsic_on_create,
    preview_intrinsic_after_attr_write,
};
#[cfg(test)]
pub(crate) use intrinsic_size::{input_default_width_sample, measured_text_width_px};
pub(crate) use paint::{ElementPaintContext, append_specialized_element_paint_ops};
pub use present_options::DomElementPresentOptionsBehavior;
pub(crate) use present_options::{
    PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX, PRESENTED_OPTIONS_ROLE_ROW,
    PRESENTED_OPTIONS_ROLE_SCROLL, PRESENTED_OPTIONS_ROLE_SHELL, PRESENTED_OPTIONS_STATE_CLOSING,
    PRESENTED_OPTIONS_STATE_OPEN, PRESENTED_OPTIONS_VERTICAL_PADDING_PX, PresentedOptionRecord,
    PresentedOptionsOwnerKind, PresentedOptionsPopupAnchor,
    choose_presented_options_highlighted_index, collect_direct_child_element_keys,
    collect_presented_option_records_from_owner_source, measure_presented_options_popup_height_px,
    presented_options_popup_anchor_for_owner, presented_options_popup_state,
    presented_options_role_value, presented_options_row_content_height_px,
    presented_options_row_is_highlighted, presented_options_row_source_index,
    set_select_source_option_selected_attrs, string_attr_value,
};
pub(crate) use range::{apply_default_range_drag_for_point, finish_range_drag_change_if_dirty};
pub(crate) use text_entry::{
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input,
    ensure_text_selection_for_target,
};
