//! Element behavior definitions that sit between tags/attrs and lifecycle execution.

mod activation;
mod intrinsic_size;
mod paint;
mod present_options;

pub(crate) use activation::{
    ActiveRangeDragSession, DomEventDispatchOutcome, PressedPointerClickTarget,
};
pub use activation::{
    DomElementAfterClick, DomElementAfterFocus, DomElementOnActivation, DomElementOnBlur,
    DomElementOnKeyboard, DomElementOnPointer,
};
pub use intrinsic_size::DomElementIntrinsicKind;
pub(crate) use intrinsic_size::{
    commit_intrinsic_after_style_recompute, compute_intrinsic_on_create,
    preview_intrinsic_after_attr_write,
};
#[cfg(test)]
pub(crate) use intrinsic_size::{input_default_width_sample, measured_text_width_px};
pub(crate) use paint::{ElementPaintContext, append_specialized_element_paint_ops};
pub use present_options::DomElementPresentOptionsBehavior;
