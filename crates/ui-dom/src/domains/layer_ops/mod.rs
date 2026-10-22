//! Shared retained layer-op vocabulary for `ui-dom`.
//!
//! This module owns the retained op payloads shared between paint planning and
//! retained paint execution. It intentionally stays narrow and free of DOM
//! planning logic.

mod icon;
mod image_paint_id;
mod layer_paint_plan;
mod rect;
mod svg_paint_id;
mod svg_paint_op;
mod text_run;
mod text_run_signature;

pub(crate) use crate::domains::text_layout::{FontFaceId, TextAlign, TextLanguageId, TextWrapMode};
pub use icon::IconPaintOp;
pub use image_paint_id::ImagePaintId;
pub use layer_paint_plan::LayerPaintPlan;
pub use rect::RectPaintOp;
pub use svg_paint_id::SvgPaintId;
pub use svg_paint_op::SvgPaintOp;
pub(crate) use text_run::TextRunId;
pub use text_run::TextRunPaintOp;
pub(crate) use text_run_signature::TextRunSignature;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePathOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Placeholder stroke color.
    pub rgba: [f32; 4],
}

/// Flat retained image placeholder geometry and authored image identity.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePaintOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Stable retained image placeholder identity.
    pub image_id: ImagePaintId,
}

fn stable_placeholder_id(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
