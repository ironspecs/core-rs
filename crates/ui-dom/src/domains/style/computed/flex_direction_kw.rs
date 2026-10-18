//! Computed `flex-direction` keyword vocabulary (PLAN.md S1.1, flex group).
//!
//! This file owns the `FlexDirectionKw` enum and its keyword mapping. It is a
//! plain `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3
//! resolver; no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `flex-direction` set admitted by the
//!   presentation registry (`presentation_property.rs::FLEX_DIRECTION_KEYWORDS`)
//! - **boundary note:** the flex solver derives direction as a `(FlexAxis,
//!   reversed)` pair (`domains/layout/flex.rs::flex_direction_from_str`). The
//!   step-3.3 projection maps `FlexDirectionKw` onto that pair.

use crate::domains::style::Keyword;

/// Computed `flex-direction` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlexDirectionKw {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirectionKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            FlexDirectionKw::Row => "row",
            FlexDirectionKw::RowReverse => "row-reverse",
            FlexDirectionKw::Column => "column",
            FlexDirectionKw::ColumnReverse => "column-reverse",
        }
    }

    /// Maps the admitted `flex-direction` keyword; `None` for any other.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Row => Some(FlexDirectionKw::Row),
            Keyword::RowReverse => Some(FlexDirectionKw::RowReverse),
            Keyword::Column => Some(FlexDirectionKw::Column),
            Keyword::ColumnReverse => Some(FlexDirectionKw::ColumnReverse),
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
            (FlexDirectionKw::Row, Keyword::Row),
            (FlexDirectionKw::RowReverse, Keyword::RowReverse),
            (FlexDirectionKw::Column, Keyword::Column),
            (FlexDirectionKw::ColumnReverse, Keyword::ColumnReverse),
        ];
        for (value, keyword) in variants {
            assert_eq!(FlexDirectionKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(FlexDirectionKw::from_keyword(Keyword::Block), None);
        assert_eq!(FlexDirectionKw::from_keyword(Keyword::Wrap), None);
    }
}
