//! Computed `align-items`/`align-self` keyword vocabulary (PLAN.md S1.1, flex
//! group).
//!
//! This file owns the `AlignItemsKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set mirrors `domains/layout/flex.rs::align_items_from_str`'s
//!   closed set (the presentation registry's `ALIGN_ITEMS_KEYWORDS`)
//! - `auto` is **not** a member: the `align_self` field stores `None` for
//!   `auto` (inherit the container's `align-items`), exactly the flex solver's
//!   own `Option<FlexCrossAxisAlignment>` shape (`flex.rs::item_align_self`)
//! - **boundary note:** the flex solver uses its own `FlexCrossAxisAlignment`
//!   (private `enum`, identical variant set). The step-3.3 projection maps
//!   `AlignItemsKw` → `FlexCrossAxisAlignment`.

use crate::domains::style::Keyword;

/// Computed `align-items`/`align-self` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlignItemsKw {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
}

impl AlignItemsKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            AlignItemsKw::Start => "start",
            AlignItemsKw::End => "end",
            AlignItemsKw::FlexStart => "flex-start",
            AlignItemsKw::FlexEnd => "flex-end",
            AlignItemsKw::Center => "center",
            AlignItemsKw::Stretch => "stretch",
        }
    }

    /// Maps the admitted `align-items`/`align-self` keyword; `None` for any
    /// other (including `auto`, which the `align_self` field stores as `None`).
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Start => Some(AlignItemsKw::Start),
            Keyword::End => Some(AlignItemsKw::End),
            Keyword::FlexStart => Some(AlignItemsKw::FlexStart),
            Keyword::FlexEnd => Some(AlignItemsKw::FlexEnd),
            Keyword::Center => Some(AlignItemsKw::Center),
            Keyword::Stretch => Some(AlignItemsKw::Stretch),
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
            (AlignItemsKw::Start, Keyword::Start),
            (AlignItemsKw::End, Keyword::End),
            (AlignItemsKw::FlexStart, Keyword::FlexStart),
            (AlignItemsKw::FlexEnd, Keyword::FlexEnd),
            (AlignItemsKw::Center, Keyword::Center),
            (AlignItemsKw::Stretch, Keyword::Stretch),
        ];
        for (value, keyword) in variants {
            assert_eq!(AlignItemsKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn auto_is_not_a_member_and_is_rejected() {
        assert_eq!(AlignItemsKw::from_keyword(Keyword::Auto), None);
    }
}
