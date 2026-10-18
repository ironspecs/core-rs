//! Computed `align-content` keyword vocabulary (PLAN.md S1.1, flex group).
//!
//! This file owns the `AlignContentKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the align-items set plus the three distribution
//!   keywords (`SpaceBetween`, `SpaceAround`, `SpaceEvenly`), mirroring
//!   `domains/layout/flex.rs::align_content_from_str`'s closed set (the
//!   presentation registry's `ALIGN_CONTENT_KEYWORDS`)
//! - **boundary note:** the flex solver uses its own `FlexLineCrossAxisAlignment`
//!   (private `enum`, identical variant set). The step-3.3 projection maps
//!   `AlignContentKw` → `FlexLineCrossAxisAlignment`.

use crate::domains::style::Keyword;

/// Computed `align-content` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlignContentKw {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl AlignContentKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            AlignContentKw::Start => "start",
            AlignContentKw::End => "end",
            AlignContentKw::FlexStart => "flex-start",
            AlignContentKw::FlexEnd => "flex-end",
            AlignContentKw::Center => "center",
            AlignContentKw::Stretch => "stretch",
            AlignContentKw::SpaceBetween => "space-between",
            AlignContentKw::SpaceAround => "space-around",
            AlignContentKw::SpaceEvenly => "space-evenly",
        }
    }

    /// Maps the admitted `align-content` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Start => Some(AlignContentKw::Start),
            Keyword::End => Some(AlignContentKw::End),
            Keyword::FlexStart => Some(AlignContentKw::FlexStart),
            Keyword::FlexEnd => Some(AlignContentKw::FlexEnd),
            Keyword::Center => Some(AlignContentKw::Center),
            Keyword::Stretch => Some(AlignContentKw::Stretch),
            Keyword::SpaceBetween => Some(AlignContentKw::SpaceBetween),
            Keyword::SpaceAround => Some(AlignContentKw::SpaceAround),
            Keyword::SpaceEvenly => Some(AlignContentKw::SpaceEvenly),
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
            (AlignContentKw::Start, Keyword::Start),
            (AlignContentKw::End, Keyword::End),
            (AlignContentKw::FlexStart, Keyword::FlexStart),
            (AlignContentKw::FlexEnd, Keyword::FlexEnd),
            (AlignContentKw::Center, Keyword::Center),
            (AlignContentKw::Stretch, Keyword::Stretch),
            (AlignContentKw::SpaceBetween, Keyword::SpaceBetween),
            (AlignContentKw::SpaceAround, Keyword::SpaceAround),
            (AlignContentKw::SpaceEvenly, Keyword::SpaceEvenly),
        ];
        for (value, keyword) in variants {
            assert_eq!(AlignContentKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(AlignContentKw::from_keyword(Keyword::Row), None);
        assert_eq!(AlignContentKw::from_keyword(Keyword::Auto), None);
    }
}
