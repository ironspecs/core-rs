//! Computed `justify-items` keyword vocabulary (grid T2, `GRID_STEP3_DESIGN.md`
//! D1).
//!
//! This file owns the `JustifyItemsKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here (the inline-axis cell alignment is
//! D5, in `grid.rs`).
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `justify-items` set the presentation registry
//!   admits (`JUSTIFY_ITEMS_KEYWORDS` in `presentation_property.rs`): `normal |
//!   stretch | start | end | center` — no `flex-start`/`flex-end` (CSS Grid
//!   §10.2's `<self-position>` has no flex-* forms for grid items)
//! - `Normal` is the CSS initial value and is `initial()`

use crate::domains::style::Keyword;

/// Computed `justify-items` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustifyItemsKw {
    Normal,
    Stretch,
    Start,
    End,
    Center,
}

impl JustifyItemsKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            JustifyItemsKw::Normal => "normal",
            JustifyItemsKw::Stretch => "stretch",
            JustifyItemsKw::Start => "start",
            JustifyItemsKw::End => "end",
            JustifyItemsKw::Center => "center",
        }
    }

    /// Maps the admitted `justify-items` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Normal => Some(JustifyItemsKw::Normal),
            Keyword::Stretch => Some(JustifyItemsKw::Stretch),
            Keyword::Start => Some(JustifyItemsKw::Start),
            Keyword::End => Some(JustifyItemsKw::End),
            Keyword::Center => Some(JustifyItemsKw::Center),
            _ => None,
        }
    }

    /// The CSS initial value: `normal`.
    #[must_use]
    pub fn initial() -> Self {
        Self::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant() {
        let variants = [
            (JustifyItemsKw::Normal, Keyword::Normal),
            (JustifyItemsKw::Stretch, Keyword::Stretch),
            (JustifyItemsKw::Start, Keyword::Start),
            (JustifyItemsKw::End, Keyword::End),
            (JustifyItemsKw::Center, Keyword::Center),
        ];
        for (value, keyword) in variants {
            assert_eq!(JustifyItemsKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn initial_is_normal() {
        assert_eq!(JustifyItemsKw::initial(), JustifyItemsKw::Normal);
    }

    #[test]
    fn rejects_flex_and_auto_keywords() {
        assert_eq!(JustifyItemsKw::from_keyword(Keyword::FlexStart), None);
        assert_eq!(JustifyItemsKw::from_keyword(Keyword::Auto), None);
    }
}
