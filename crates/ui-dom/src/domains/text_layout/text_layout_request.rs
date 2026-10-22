//! Text layout request for `ui-dom`.
//!
//! This file owns the immutable `TextLayoutRequest` value that carries text,
//! font, white-space, wrap, and alignment inputs into the layout engine. It is
//! a plain data carrier with two constructors and no layout logic.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextLayoutRequest` struct and its `impl` block
//! - `for_text` derives the language hint from the text sample and carries the
//!   default (unstyled) family/weight/style/direction
//! - `for_text_with_white_space` resolves wrap mode from CSS layout policy
//! - the carried `family` is the RESOLVED `TextFamilyList` (self-owned names),
//!   not an arena key: the render world reads it from the extracted record and
//!   has no document interner (text seam S2, Role A — `TextFontFamilyId`'s
//!   request-carrier role is replaced; the enum survives for Role B, the
//!   render-world registration grouping key)

use crate::domains::style::computed::TextFamilyList;
use crate::types::{DomOverflowWrap, DomTextDirection, DomWhiteSpace, DomWordBreak};

use super::{
    FontStyleKw, FontWeightKw, TextAlign, TextLanguageId, TextWrapMode,
    text_wrap_mode_for_css_layout,
};

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) white_space: DomWhiteSpace,
    pub(crate) word_break: DomWordBreak,
    pub(crate) family: TextFamilyList,
    pub(crate) language_hint_id: TextLanguageId,
    pub(crate) weight: FontWeightKw,
    pub(crate) style: FontStyleKw,
    pub(crate) direction: DomTextDirection,
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
            family: TextFamilyList::default(),
            language_hint_id: TextLanguageId::from_text_hint(text),
            weight: FontWeightKw::NORMAL,
            style: FontStyleKw::NORMAL,
            direction: DomTextDirection::Ltr,
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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn request_constructors_capture_text_and_wrap_policy() {
        let request = TextLayoutRequest::for_text("abc", 14.0);
        assert_eq!(request.text, "abc");
        assert_eq!(request.white_space, DomWhiteSpace::Normal);
        assert_eq!(request.word_break, DomWordBreak::Normal);
        assert_eq!(request.family, TextFamilyList::default());
        assert_eq!(request.language_hint_id, TextLanguageId::English);
        assert_eq!(request.weight, FontWeightKw::NORMAL);
        assert_eq!(request.style, FontStyleKw::Normal);
        assert_eq!(request.direction, DomTextDirection::Ltr);
        assert_eq!(request.font_size_px, 14.0);
        assert_eq!(request.line_height_px, None);
        assert_eq!(request.width_px, None);
        assert_eq!(request.wrap_mode, TextWrapMode::Word);
        assert_eq!(request.alignment, TextAlign::Left);

        let explicit_white_space = DomWhiteSpace::Pre;
        let request =
            TextLayoutRequest::for_text_with_white_space("abc", 14.0, explicit_white_space);
        assert_eq!(request.text, "abc");
        assert_eq!(request.white_space, explicit_white_space);
        assert_eq!(request.wrap_mode, TextWrapMode::None);
    }
}
