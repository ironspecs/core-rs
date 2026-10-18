//! Computed `flex-basis` keyword vocabulary (PLAN.md S1.1, flex group).
//!
//! This file owns the `FlexBasisKw` enum and its keyword mapping. It is a plain
//! `Copy` enum plus a `from_keyword()` helper for the step-3.3 resolver; no
//! resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - `Auto` is the absent default; `Content` is the `content` keyword;
//!   `Length(px)` is a resolved length. Today's `0`/`Zero` shorthand case
//!   resolves to `Length(0.0)` (`flex.rs::parse_flex_basis_source` returns
//!   `Zero`; the step-3.3 resolver collapses `Zero` → `Length(0.0)`).
//! - **boundary note:** the flex solver uses its own `FlexBasisSource`
//!   (`domains/layout/flex.rs`, private `enum` with an extra `Zero` variant).
//!   The step-3.3 projection maps `FlexBasisKw` → `FlexBasisSource`
//!   (`Length(0.0)` → `Zero`).

use crate::domains::style::Keyword;

/// Computed `flex-basis` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexBasisKw {
    Auto,
    Content,
    Length(f32),
}

impl FlexBasisKw {
    /// Maps the admitted keyword forms (`auto`/`content`); lengths are produced
    /// by the resolver from a `DeclValue::Length`, not from a `Keyword`.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Auto => Some(FlexBasisKw::Auto),
            Keyword::Content => Some(FlexBasisKw::Content),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_auto_and_content_keywords_only() {
        assert_eq!(
            FlexBasisKw::from_keyword(Keyword::Auto),
            Some(FlexBasisKw::Auto)
        );
        assert_eq!(
            FlexBasisKw::from_keyword(Keyword::Content),
            Some(FlexBasisKw::Content)
        );
        assert_eq!(FlexBasisKw::from_keyword(Keyword::Stretch), None);
        assert_eq!(FlexBasisKw::from_keyword(Keyword::None), None);
    }
}
