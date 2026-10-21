//! Computed `justify-self` keyword vocabulary (grid T2, `GRID_STEP3_DESIGN.md`
//! D1).
//!
//! This file owns the `JustifySelfKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here (the inline-axis cell alignment is
//! D5, in `grid.rs`).
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `justify-self` set the presentation registry
//!   admits (`JUSTIFY_SELF_KEYWORDS` in `presentation_property.rs`): `auto |
//!   normal | stretch | start | end | center` — the `justify-items` set plus
//!   `auto`, with no `flex-*` forms (CSS Grid §10.2's `<self-position>`)
//! - `Auto` is the CSS initial value (`auto` — inherit the container's
//!   `justify-items`) and is `initial()`

use crate::domains::style::Keyword;

/// Computed `justify-self` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustifySelfKw {
    Auto,
    Normal,
    Stretch,
    Start,
    End,
    Center,
}

impl JustifySelfKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            JustifySelfKw::Auto => "auto",
            JustifySelfKw::Normal => "normal",
            JustifySelfKw::Stretch => "stretch",
            JustifySelfKw::Start => "start",
            JustifySelfKw::End => "end",
            JustifySelfKw::Center => "center",
        }
    }

    /// Maps the admitted `justify-self` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Auto => Some(JustifySelfKw::Auto),
            Keyword::Normal => Some(JustifySelfKw::Normal),
            Keyword::Stretch => Some(JustifySelfKw::Stretch),
            Keyword::Start => Some(JustifySelfKw::Start),
            Keyword::End => Some(JustifySelfKw::End),
            Keyword::Center => Some(JustifySelfKw::Center),
            _ => None,
        }
    }

    /// The CSS initial value: `auto`.
    #[must_use]
    pub fn initial() -> Self {
        Self::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant() {
        let variants = [
            (JustifySelfKw::Auto, Keyword::Auto),
            (JustifySelfKw::Normal, Keyword::Normal),
            (JustifySelfKw::Stretch, Keyword::Stretch),
            (JustifySelfKw::Start, Keyword::Start),
            (JustifySelfKw::End, Keyword::End),
            (JustifySelfKw::Center, Keyword::Center),
        ];
        for (value, keyword) in variants {
            assert_eq!(JustifySelfKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn initial_is_auto() {
        assert_eq!(JustifySelfKw::initial(), JustifySelfKw::Auto);
    }

    #[test]
    fn rejects_flex_keywords() {
        assert_eq!(JustifySelfKw::from_keyword(Keyword::FlexStart), None);
        assert_eq!(JustifySelfKw::from_keyword(Keyword::FlexEnd), None);
    }
}
