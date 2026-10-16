//! Element behavior definitions that sit between tags/attrs and lifecycle execution.

mod activation;
mod dom_event_dispatch_outcome;
mod intrinsic_size;
mod paint;
mod present_options;

pub(crate) use activation::{ActiveRangeDragSession, PressedPointerClickTarget};
pub use activation::{
    DomElementAfterClick, DomElementAfterFocus, DomElementOnActivation, DomElementOnBlur,
    DomElementOnKeyboard, DomElementOnPointer,
};
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
