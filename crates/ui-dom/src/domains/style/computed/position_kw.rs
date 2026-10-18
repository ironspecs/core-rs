//! Computed `position` keyword vocabulary (PLAN.md S1.1, box group).
//!
//! This file owns the `PositionKw` enum and its keyword mapping. It is a plain
//! `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3 resolver;
//! no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `position` set admitted by the presentation
//!   registry (`presentation_property.rs::POSITION_KEYWORDS`)
//! - **boundary note:** it mirrors `domains/layout/style.rs::PositionMode` (a
//!   `pub(super)` solver type, not visible to this sibling subtree). The
//!   step-3.3 layout projection maps `PositionKw` → `PositionMode`; the variant
//!   sets are identical.

use crate::domains::style::Keyword;

/// Computed `position` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionKw {
    Static,
    Relative,
    Absolute,
    Fixed,
}

impl PositionKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            PositionKw::Static => "static",
            PositionKw::Relative => "relative",
            PositionKw::Absolute => "absolute",
            PositionKw::Fixed => "fixed",
        }
    }

    /// Maps the admitted `position` keyword; `None` for any other keyword.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Static => Some(PositionKw::Static),
            Keyword::Relative => Some(PositionKw::Relative),
            Keyword::Absolute => Some(PositionKw::Absolute),
            Keyword::Fixed => Some(PositionKw::Fixed),
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
            (PositionKw::Static, Keyword::Static),
            (PositionKw::Relative, Keyword::Relative),
            (PositionKw::Absolute, Keyword::Absolute),
            (PositionKw::Fixed, Keyword::Fixed),
        ];
        for (value, keyword) in variants {
            assert_eq!(PositionKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(PositionKw::from_keyword(Keyword::Ltr), None);
        assert_eq!(PositionKw::from_keyword(Keyword::Block), None);
    }
}
