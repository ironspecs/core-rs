//! The text-seam font-style keyword (`FontStyleKw`).
//!
//! This file owns the request-carrier font style (text seam S2): the closed
//! `Normal | Italic` set the `TextLayoutRequest` carries into the shaper, and
//! its mapping to `cosmic_text::Style`. Oblique stays dropped at parse (the
//! closed grammar; the registry's `unsupported_font_style_oblique_drops` pin
//! stands). It is a plain `Copy` enum with no layout logic.

use cosmic_text::Style as CosmicStyle;

use crate::domains::style::Keyword;

/// The resolved font style a text-layout request carries.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum FontStyleKw {
    #[default]
    Normal,
    Italic,
}

impl FontStyleKw {
    /// The CSS initial style (normal).
    pub(crate) const NORMAL: Self = Self::Normal;

    /// Maps the computed `font-style` keyword (`Normal | Italic`) to the
    /// request-carrier form.
    #[must_use]
    pub(crate) fn from_keyword(keyword: Keyword) -> Self {
        if keyword == Keyword::Italic {
            // Oblique (and every other keyword) never reaches the computed
            // group; the parser's closed set guarantees Normal|Italic only.
            Self::Italic
        } else {
            Self::Normal
        }
    }

    /// Maps to the `cosmic_text::Style` the shaper consumes.
    #[must_use]
    pub(crate) fn cosmic(self) -> CosmicStyle {
        match self {
            Self::Normal => CosmicStyle::Normal,
            Self::Italic => CosmicStyle::Italic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_computed_keywords_to_cosmic_styles() {
        assert_eq!(
            FontStyleKw::from_keyword(Keyword::Normal).cosmic(),
            CosmicStyle::Normal
        );
        assert_eq!(
            FontStyleKw::from_keyword(Keyword::Italic).cosmic(),
            CosmicStyle::Italic
        );
        assert_ne!(FontStyleKw::Normal, FontStyleKw::Italic);
    }
}
