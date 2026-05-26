use std::sync::Arc;

use slotmap::new_key_type;

use crate::domains::text_layout::{TextAlign, TextFontFamilyId, TextLanguageId, TextWrapMode};

new_key_type! {
    pub(crate) struct TextRunId;
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TextRunSignature {
    pub(crate) text: Arc<str>,
    pub(crate) language_hint_id: TextLanguageId,
    pub(crate) font_family_id: TextFontFamilyId,
}

impl TextRunSignature {
    #[must_use]
    pub(crate) fn from_text(text: Arc<str>) -> Self {
        Self {
            language_hint_id: TextLanguageId::from_text_hint(text.as_ref()),
            font_family_id: TextFontFamilyId::UiSans,
            text,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRunPaintOp {
    /// Stable identity of the banked text run.
    pub(crate) text_run_id: TextRunId,
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Requested font size in pixels.
    pub font_size_px: f32,
    /// Requested line height in pixels.
    pub line_height_px: f32,
    /// Requested wrapping strategy.
    pub wrap_mode: TextWrapMode,
    /// Requested line alignment.
    pub alignment: TextAlign,
    /// Shader tint applied during composition.
    pub color_rgba: [f32; 4],
}
