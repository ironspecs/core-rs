//! Element behavior definitions that sit between tags/attrs and lifecycle execution.

mod activation;
mod behavior_table;
mod button;
mod checkbox;
mod control_kind;
mod controls;
mod dom_event_dispatch_outcome;
mod intrinsic_size;
mod paint;
mod popup;
mod popup_shell;
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
#[cfg(test)]
pub(crate) use popup::presented_options_popup_row_keys;
pub(crate) use popup::presented_options_popup_shell_key;
pub use present_options::DomElementPresentOptionsBehavior;
pub(crate) use present_options::{PRESENTED_OPTIONS_STATE_CLOSING, presented_options_popup_state};
pub(crate) use range::{apply_default_range_drag_for_point, finish_range_drag_change_if_dirty};
pub(crate) use text_entry::{
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input,
    ensure_text_selection_for_target,
};
