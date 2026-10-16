//! Shared inherited-style context for retained visitors in `ui-dom`.
//!
//! This file owns the resolved inherited values that style, layout, and paint
//! visitors read when a node has no authored override. Its purpose is to keep
//! the inherited value set explicit and cheap to clone without owning the
//! ancestor climber that seeds it.
//!
//! Invariants and rules:
//!
//! - this file owns only the `InheritedStyleContext` struct and its `impl` block
//! - every field is an `Option` so "not inherited yet" stays distinguishable
//!   from an authored default
//! - effective accessors fall back to each property's default, never error

use crate::domains::text_layout::DEFAULT_TEXT_FONT_SIZE_PX;
use crate::types::{
    DomAttributeValue, DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct InheritedStyleContext {
    pub(crate) color: Option<DomAttributeValue>,
    pub(crate) font_size_px: Option<f32>,
    pub(crate) line_height_px: Option<f32>,
    pub(crate) text_align: Option<DomTextAlign>,
    pub(crate) text_direction: Option<DomTextDirection>,
    pub(crate) white_space: Option<DomWhiteSpace>,
    pub(crate) overflow_wrap: Option<DomOverflowWrap>,
    pub(crate) word_break: Option<DomWordBreak>,
}

impl InheritedStyleContext {
    pub(crate) fn effective_font_size_px(&self) -> f32 {
        self.font_size_px.unwrap_or(DEFAULT_TEXT_FONT_SIZE_PX)
    }

    pub(crate) fn effective_white_space(&self) -> DomWhiteSpace {
        self.white_space.unwrap_or_default()
    }

    pub(crate) fn effective_overflow_wrap(&self) -> DomOverflowWrap {
        self.overflow_wrap.unwrap_or_default()
    }

    pub(crate) fn effective_word_break(&self) -> DomWordBreak {
        self.word_break.unwrap_or_default()
    }

    pub(crate) fn effective_line_height_px(&self) -> Option<f32> {
        self.line_height_px
    }

    pub(crate) fn effective_text_align(&self) -> DomTextAlign {
        self.text_align.unwrap_or_default()
    }

    pub(crate) fn effective_text_direction(&self) -> DomTextDirection {
        self.text_direction.unwrap_or_default()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn effective_methods_return_seeded_values() {
        let context = InheritedStyleContext {
            font_size_px: Some(20.0),
            line_height_px: Some(28.0),
            text_align: Some(DomTextAlign::End),
            text_direction: Some(DomTextDirection::Rtl),
            white_space: Some(DomWhiteSpace::PreWrap),
            overflow_wrap: Some(DomOverflowWrap::Anywhere),
            word_break: Some(DomWordBreak::KeepAll),
            ..InheritedStyleContext::default()
        };

        assert_eq!(context.effective_font_size_px(), 20.0);
        assert_eq!(context.effective_line_height_px(), Some(28.0));
        assert_eq!(context.effective_text_align(), DomTextAlign::End);
        assert_eq!(context.effective_text_direction(), DomTextDirection::Rtl);
        assert_eq!(context.effective_white_space(), DomWhiteSpace::PreWrap);
        assert_eq!(context.effective_overflow_wrap(), DomOverflowWrap::Anywhere);
        assert_eq!(context.effective_word_break(), DomWordBreak::KeepAll);
    }
}
