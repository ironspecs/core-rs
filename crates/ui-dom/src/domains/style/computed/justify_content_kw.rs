//! Computed `justify-content` keyword vocabulary (PLAN.md S1.1, flex group).
//!
//! This file owns the `JustifyContentKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set mirrors `domains/layout/flex.rs::justify_content_from_str`'s
//!   closed set (the presentation registry's `JUSTIFY_CONTENT_KEYWORDS`)
//! - **boundary note:** the flex solver uses its own `FlexMainAxisAlignment`
//!   (private `enum`, identical variant set). The step-3.3 projection maps
//!   `JustifyContentKw` → `FlexMainAxisAlignment`.

use crate::domains::style::Keyword;

/// Computed `justify-content` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JustifyContentKw {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl JustifyContentKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            JustifyContentKw::Start => "start",
            JustifyContentKw::End => "end",
            JustifyContentKw::FlexStart => "flex-start",
            JustifyContentKw::FlexEnd => "flex-end",
            JustifyContentKw::Center => "center",
            JustifyContentKw::SpaceBetween => "space-between",
            JustifyContentKw::SpaceAround => "space-around",
            JustifyContentKw::SpaceEvenly => "space-evenly",
        }
    }

    /// Maps the admitted `justify-content` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Start => Some(JustifyContentKw::Start),
            Keyword::End => Some(JustifyContentKw::End),
            Keyword::FlexStart => Some(JustifyContentKw::FlexStart),
            Keyword::FlexEnd => Some(JustifyContentKw::FlexEnd),
            Keyword::Center => Some(JustifyContentKw::Center),
            Keyword::SpaceBetween => Some(JustifyContentKw::SpaceBetween),
            Keyword::SpaceAround => Some(JustifyContentKw::SpaceAround),
            Keyword::SpaceEvenly => Some(JustifyContentKw::SpaceEvenly),
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
            (JustifyContentKw::Start, Keyword::Start),
            (JustifyContentKw::End, Keyword::End),
            (JustifyContentKw::FlexStart, Keyword::FlexStart),
            (JustifyContentKw::FlexEnd, Keyword::FlexEnd),
            (JustifyContentKw::Center, Keyword::Center),
            (JustifyContentKw::SpaceBetween, Keyword::SpaceBetween),
            (JustifyContentKw::SpaceAround, Keyword::SpaceAround),
            (JustifyContentKw::SpaceEvenly, Keyword::SpaceEvenly),
        ];
        for (value, keyword) in variants {
            assert_eq!(JustifyContentKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(JustifyContentKw::from_keyword(Keyword::Stretch), None);
        assert_eq!(JustifyContentKw::from_keyword(Keyword::Row), None);
    }
}
