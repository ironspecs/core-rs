//! Computed `box-sizing` keyword vocabulary (PLAN.md S1.1, box group).
//!
//! This file owns the `BoxSizingKw` enum and its keyword mapping. It is a plain
//! `Copy` enum plus `name()`/`from_keyword()` helpers for the step-3.3 resolver;
//! no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `box-sizing` set admitted by the presentation
//!   registry (`presentation_property.rs::BOX_SIZING_KEYWORDS`)
//! - `ContentBox` is the `Default` and the CSS UI 4 §3.1 initial value: the
//!   specified `width`/`height` names the content box, with padding and border
//!   added outside (the crate's single sizing model before `border-box` landed)

use crate::domains::style::Keyword;

/// Computed `box-sizing` value (CSS UI 4 §3.1).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BoxSizingKw {
    /// Specified size is the content box; padding and border are outside it.
    #[default]
    ContentBox,
    /// Specified size is the border box; padding and border are inside it.
    BorderBox,
}

impl BoxSizingKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BoxSizingKw::ContentBox => "content-box",
            BoxSizingKw::BorderBox => "border-box",
        }
    }

    /// Maps the admitted `box-sizing` keyword; `None` for any other keyword.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::ContentBox => Some(BoxSizingKw::ContentBox),
            Keyword::BorderBox => Some(BoxSizingKw::BorderBox),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant_and_defaults_to_content_box() {
        let variants = [
            (BoxSizingKw::ContentBox, Keyword::ContentBox),
            (BoxSizingKw::BorderBox, Keyword::BorderBox),
        ];
        for (value, keyword) in variants {
            assert_eq!(BoxSizingKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
        assert_eq!(BoxSizingKw::default(), BoxSizingKw::ContentBox);
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(BoxSizingKw::from_keyword(Keyword::Isolate), None);
        assert_eq!(BoxSizingKw::from_keyword(Keyword::Auto), None);
    }
}
