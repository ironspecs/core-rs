//! Text layout request for `ui-dom`.
//!
//! This file owns the immutable `TextLayoutRequest` value that carries text,
//! font, white-space, wrap, and alignment inputs into the layout engine. It is
//! a plain data carrier with two constructors and no layout logic.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextLayoutRequest` struct and its `impl` block
//! - `for_text` derives the language hint from the text sample
//! - `for_text_with_white_space` resolves wrap mode from CSS layout policy

use crate::types::{DomOverflowWrap, DomWhiteSpace, DomWordBreak};

use super::{
    TextAlign, TextFontFamilyId, TextLanguageId, TextWrapMode, text_wrap_mode_for_css_layout,
};

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) white_space: DomWhiteSpace,
    pub(crate) word_break: DomWordBreak,
    pub(crate) font_family_id: TextFontFamilyId,
    pub(crate) language_hint_id: TextLanguageId,
    pub(crate) font_size_px: f32,
    pub(crate) line_height_px: Option<f32>,
    pub(crate) width_px: Option<f32>,
    pub(crate) wrap_mode: TextWrapMode,
    pub(crate) alignment: TextAlign,
}

impl<'a> TextLayoutRequest<'a> {
    #[must_use]
    pub(crate) fn for_text(text: &'a str, font_size_px: f32) -> Self {
        Self {
            text,
            white_space: DomWhiteSpace::Normal,
            word_break: DomWordBreak::Normal,
            font_family_id: TextFontFamilyId::UiSans,
            language_hint_id: TextLanguageId::from_text_hint(text),
            font_size_px,
            line_height_px: None,
            width_px: None,
            wrap_mode: TextWrapMode::Word,
            alignment: TextAlign::Left,
        }
    }

    #[must_use]
    pub(crate) fn for_text_with_white_space(
        text: &'a str,
        font_size_px: f32,
        white_space: DomWhiteSpace,
    ) -> Self {
        Self {
            white_space,
            word_break: DomWordBreak::Normal,
            wrap_mode: text_wrap_mode_for_css_layout(
                white_space,
                DomOverflowWrap::Normal,
                DomWordBreak::Normal,
            ),
            ..Self::for_text(text, font_size_px)
        }
    }
}
