//! Computed `flex-wrap` keyword vocabulary (PLAN.md S1.1, flex group).
//!
//! This file owns the `FlexWrapKw` enum and its keyword mapping. It is a plain
//! `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `flex-wrap` set admitted by the presentation
//!   registry (`presentation_property.rs::FLEX_WRAP_KEYWORDS`)
//! - **boundary note:** the flex solver uses its own `FlexWrapMode`
//!   (`domains/layout/flex_wrap_mode.rs`, `pub(crate)` solver machinery with the
//!   same three variants). The step-3.3 projection maps `FlexWrapKw` →
//!   `FlexWrapMode`.

use crate::domains::style::Keyword;

/// Computed `flex-wrap` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlexWrapKw {
    NoWrap,
    Wrap,
    WrapReverse,
}

impl FlexWrapKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            FlexWrapKw::NoWrap => "nowrap",
            FlexWrapKw::Wrap => "wrap",
            FlexWrapKw::WrapReverse => "wrap-reverse",
        }
    }

    /// Maps the admitted `flex-wrap` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Nowrap => Some(FlexWrapKw::NoWrap),
            Keyword::Wrap => Some(FlexWrapKw::Wrap),
            Keyword::WrapReverse => Some(FlexWrapKw::WrapReverse),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant() {
        let variants = [
            (FlexWrapKw::NoWrap, Keyword::Nowrap),
            (FlexWrapKw::Wrap, Keyword::Wrap),
            (FlexWrapKw::WrapReverse, Keyword::WrapReverse),
        ];
        for (value, keyword) in variants {
            assert_eq!(FlexWrapKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(FlexWrapKw::from_keyword(Keyword::Row), None);
        assert_eq!(FlexWrapKw::from_keyword(Keyword::Column), None);
    }
}
