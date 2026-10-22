//! The resolved text-style handoff the style system passes the text seam.
//!
//! This file owns `ResolvedTextStyle`, the bundle of resolved font/text values
//! (family list, weight, style, direction) the main-world construction sites
//! carry from the computed groups into a `TextLayoutRequest` (text seam S2). It
//! is a plain `Copy`-adjacent data carrier (`Clone`; the family list is
//! self-owned) with no layout logic, kept as a context object so the threading
//! functions stay under the argument-count bar (AGENTS.md: context objects over
//! long parameter lists).

use crate::domains::style::computed::TextFamilyList;
use crate::types::DomTextDirection;

use super::{FontStyleKw, FontWeightKw};

/// The resolved font/text style the style system hands the text seam.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedTextStyle {
    /// The resolved family list (`Default` = empty → engine default).
    pub(crate) family: TextFamilyList,
    /// The resolved font weight (initial 400).
    pub(crate) weight: FontWeightKw,
    /// The resolved font style (initial `Normal`).
    pub(crate) style: FontStyleKw,
    /// The resolved text direction (initial `Ltr`; cosmic auto-detects bidi).
    pub(crate) direction: DomTextDirection,
}

impl ResolvedTextStyle {
    /// Builds the handoff from a resolved family list plus the inherited
    /// weight/style/direction.
    #[must_use]
    pub(crate) fn new(
        family: TextFamilyList,
        weight: FontWeightKw,
        style: FontStyleKw,
        direction: DomTextDirection,
    ) -> Self {
        Self {
            family,
            weight,
            style,
            direction,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mirrors_the_unstyled_initial() {
        let default = ResolvedTextStyle::default();
        assert_eq!(default.family, TextFamilyList::default());
        assert_eq!(default.weight, FontWeightKw::NORMAL);
        assert_eq!(default.style, FontStyleKw::Normal);
        assert_eq!(default.direction, DomTextDirection::Ltr);
    }
}
