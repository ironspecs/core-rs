//! Inherited text computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedTextGroup` struct: the computed values of the
//! inherited `text-align`, `direction`, `white-space`, `overflow-wrap`, and
//! `word-break` longhands. It is plain `Copy` data plus `initial()`; resolution
//! lives in the step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - each field reuses the crate's shared text vocabulary enums from
//!   `src/types/white_space.rs` (`DomTextAlign`, `DomTextDirection`,
//!   `DomWhiteSpace`, `DomOverflowWrap`, `DomWordBreak`) rather than defining
//!   duplicates (widened to `pub` in 3.3a as part of the computed read surface)
//! - `initial()` mirrors the `#[default]` variants of those enums, which is
//!   exactly what today's inherited-style context falls back to via
//!   `unwrap_or_default()` in
//!   `src/domains/visitor/inherited_style_context.rs` (`effective_text_align`,
//!   `effective_text_direction`, `effective_white_space`,
//!   `effective_overflow_wrap`, `effective_word_break`)

use crate::types::{DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak};

/// Computed values of the inherited text longhands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputedTextGroup {
    /// `DomTextAlign::Start` is the crate default (`white_space.rs` `#[default]`).
    pub align: DomTextAlign,
    /// `DomTextDirection::Ltr` is the crate default.
    pub direction: DomTextDirection,
    /// `DomWhiteSpace::Normal` is the crate default.
    pub white_space: DomWhiteSpace,
    /// `DomOverflowWrap::Normal` is the crate default.
    pub overflow_wrap: DomOverflowWrap,
    /// `DomWordBreak::Normal` is the crate default.
    pub word_break: DomWordBreak,
}

impl ComputedTextGroup {
    /// The no-parent text seed: every field at its crate default.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            align: DomTextAlign::Start,
            direction: DomTextDirection::Ltr,
            white_space: DomWhiteSpace::Normal,
            overflow_wrap: DomOverflowWrap::Normal,
            word_break: DomWordBreak::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_mirrors_the_inherited_context_defaults() {
        let text = ComputedTextGroup::initial();
        assert_eq!(text.align, DomTextAlign::Start);
        assert_eq!(text.direction, DomTextDirection::Ltr);
        assert_eq!(text.white_space, DomWhiteSpace::Normal);
        assert_eq!(text.overflow_wrap, DomOverflowWrap::Normal);
        assert_eq!(text.word_break, DomWordBreak::Normal);
    }

    #[test]
    fn initial_matches_enum_defaults_for_every_field() {
        let text = ComputedTextGroup::initial();
        assert_eq!(text.align, DomTextAlign::default());
        assert_eq!(text.direction, DomTextDirection::default());
        assert_eq!(text.white_space, DomWhiteSpace::default());
        assert_eq!(text.overflow_wrap, DomOverflowWrap::default());
        assert_eq!(text.word_break, DomWordBreak::default());
    }
}
