//! Computed `display` keyword vocabulary (PLAN.md S1.1, box group).
//!
//! This file owns the `DisplayKw` enum and its keyword mapping. It is a plain
//! `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3 resolver;
//! no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `display` set admitted by the presentation
//!   registry (`presentation_property.rs::DISPLAY_KEYWORDS`)
//! - **boundary note:** it mirrors `domains/layout/style.rs::LayoutDisplay` (a
//!   `pub(super)` solver type, not visible to this sibling subtree). The
//!   step-3.3 layout projection maps `DisplayKw` → `LayoutDisplay`; the variant
//!   sets are identical.

use crate::domains::style::Keyword;

/// Computed `display` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKw {
    None,
    Block,
    Inline,
    InlineBlock,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
}

impl DisplayKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            DisplayKw::None => "none",
            DisplayKw::Block => "block",
            DisplayKw::Inline => "inline",
            DisplayKw::InlineBlock => "inline-block",
            DisplayKw::Flex => "flex",
            DisplayKw::InlineFlex => "inline-flex",
            DisplayKw::Grid => "grid",
            DisplayKw::InlineGrid => "inline-grid",
        }
    }

    /// Maps the admitted `display` keyword; `None` for any other keyword.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::None => Some(DisplayKw::None),
            Keyword::Block => Some(DisplayKw::Block),
            Keyword::Inline => Some(DisplayKw::Inline),
            Keyword::InlineBlock => Some(DisplayKw::InlineBlock),
            Keyword::Flex => Some(DisplayKw::Flex),
            Keyword::InlineFlex => Some(DisplayKw::InlineFlex),
            Keyword::Grid => Some(DisplayKw::Grid),
            Keyword::InlineGrid => Some(DisplayKw::InlineGrid),
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
            (DisplayKw::None, Keyword::None),
            (DisplayKw::Block, Keyword::Block),
            (DisplayKw::Inline, Keyword::Inline),
            (DisplayKw::InlineBlock, Keyword::InlineBlock),
            (DisplayKw::Flex, Keyword::Flex),
            (DisplayKw::InlineFlex, Keyword::InlineFlex),
            (DisplayKw::Grid, Keyword::Grid),
            (DisplayKw::InlineGrid, Keyword::InlineGrid),
        ];
        for (value, keyword) in variants {
            assert_eq!(DisplayKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(DisplayKw::from_keyword(Keyword::Row), None);
        assert_eq!(DisplayKw::from_keyword(Keyword::Static), None);
    }
}
