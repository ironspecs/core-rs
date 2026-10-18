//! Computed `overflow` keyword vocabulary (PLAN.md S1.1, box group).
//!
//! This file owns the `OverflowKw` enum, its keyword mapping, and its two
//! consumer-mirroring predicates. It is a plain `Copy` enum plus small helpers;
//! no resolution policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed consumed set `{Visible, Scroll, Auto, Hidden,
//!   Clip}` — exactly the five values read today by
//!   `domains/layout/style.rs` (clip gating), `src/types/root_dom.rs::
//!   node_allows_vertical_scroll_offset` (scroll gating), composite, and flex's
//!   automatic-minimum overflow handling
//! - `allows_vertical_scroll` is `true` for exactly `Scroll | Auto | Hidden`
//!   (today's `node_allows_vertical_scroll_offset` set)
//! - `creates_clip` is `self != Visible` (today's
//!   `overflow_creates_clipped_layer` rule)

use crate::domains::style::Keyword;

/// Computed `overflow` value: the closed consumed set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverflowKw {
    Visible,
    Scroll,
    Auto,
    Hidden,
    Clip,
}

impl OverflowKw {
    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            OverflowKw::Visible => "visible",
            OverflowKw::Scroll => "scroll",
            OverflowKw::Auto => "auto",
            OverflowKw::Hidden => "hidden",
            OverflowKw::Clip => "clip",
        }
    }

    /// Maps the admitted `overflow` keyword; `None` for any other keyword.
    #[must_use]
    pub fn from_keyword(keyword: Keyword) -> Option<Self> {
        match keyword {
            Keyword::Visible => Some(OverflowKw::Visible),
            Keyword::Scroll => Some(OverflowKw::Scroll),
            Keyword::Auto => Some(OverflowKw::Auto),
            Keyword::Hidden => Some(OverflowKw::Hidden),
            Keyword::Clip => Some(OverflowKw::Clip),
            _ => None,
        }
    }

    /// True for exactly the set that allows a vertical scroll offset today
    /// (`src/types/root_dom.rs::node_allows_vertical_scroll_offset`:
    /// `scroll | auto | hidden`).
    #[must_use]
    pub fn allows_vertical_scroll(self) -> bool {
        matches!(
            self,
            OverflowKw::Scroll | OverflowKw::Auto | OverflowKw::Hidden
        )
    }

    /// True for every non-`visible` value — today's clip rule
    /// (`domains/layout/style.rs::overflow_creates_clipped_layer`:
    /// `overflow != "visible"`).
    #[must_use]
    pub fn creates_clip(self) -> bool {
        self != OverflowKw::Visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant() {
        let variants = [
            (OverflowKw::Visible, Keyword::Visible),
            (OverflowKw::Scroll, Keyword::Scroll),
            (OverflowKw::Auto, Keyword::Auto),
            (OverflowKw::Hidden, Keyword::Hidden),
            (OverflowKw::Clip, Keyword::Clip),
        ];
        for (value, keyword) in variants {
            assert_eq!(OverflowKw::from_keyword(keyword), Some(value));
            assert!(!value.name().is_empty());
        }
    }

    #[test]
    fn scroll_set_and_clip_rule_match_todays_consumers() {
        assert!(!OverflowKw::Visible.allows_vertical_scroll());
        assert!(OverflowKw::Scroll.allows_vertical_scroll());
        assert!(OverflowKw::Auto.allows_vertical_scroll());
        assert!(OverflowKw::Hidden.allows_vertical_scroll());
        assert!(!OverflowKw::Clip.allows_vertical_scroll());

        assert!(!OverflowKw::Visible.creates_clip());
        assert!(OverflowKw::Scroll.creates_clip());
        assert!(OverflowKw::Auto.creates_clip());
        assert!(OverflowKw::Hidden.creates_clip());
        assert!(OverflowKw::Clip.creates_clip());
    }

    #[test]
    fn rejects_unrelated_keywords() {
        assert_eq!(OverflowKw::from_keyword(Keyword::Italic), None);
        assert_eq!(OverflowKw::from_keyword(Keyword::Nowrap), None);
    }
}
