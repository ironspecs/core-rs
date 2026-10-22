//! The generic font-family keyword (`FontFamilyGeneric`).
//!
//! This file owns the closed set of generic family keywords the `font-family`
//! grammar maps to `Generic(_)` (text seam S2, CSS Fonts 4 §3.1: `serif`,
//! `sans-serif`, `monospace`, `system-ui`). It is a plain `Copy` enum plus the
//! name mapping; the family-list value that carries it lives in
//! `text_family_list.rs`.

/// The closed set of generic family keywords the `font-family` grammar maps to
/// `Generic(_)` (CSS Fonts 4 §3.1: `serif`, `sans-serif`, `monospace`,
/// `system-ui`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FontFamilyGeneric {
    Serif,
    SansSerif,
    Monospace,
    SystemUi,
}

impl FontFamilyGeneric {
    /// Maps a bare (unquoted, single-token) generic keyword to its variant.
    #[must_use]
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "serif" => Some(Self::Serif),
            "sans-serif" => Some(Self::SansSerif),
            "monospace" => Some(Self::Monospace),
            "system-ui" => Some(Self::SystemUi),
            _ => None,
        }
    }

    /// The canonical (bare) CSS keyword name, used by `to_css_string`.
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::SansSerif => "sans-serif",
            Self::Monospace => "monospace",
            Self::SystemUi => "system-ui",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_keywords_map_and_round_trip() {
        assert_eq!(
            FontFamilyGeneric::from_name("serif"),
            Some(FontFamilyGeneric::Serif)
        );
        assert_eq!(
            FontFamilyGeneric::from_name("sans-serif"),
            Some(FontFamilyGeneric::SansSerif)
        );
        assert_eq!(
            FontFamilyGeneric::from_name("monospace"),
            Some(FontFamilyGeneric::Monospace)
        );
        assert_eq!(
            FontFamilyGeneric::from_name("system-ui"),
            Some(FontFamilyGeneric::SystemUi)
        );
        assert_eq!(FontFamilyGeneric::from_name("fantasy"), None);
        assert_eq!(FontFamilyGeneric::SansSerif.name(), "sans-serif");
    }
}
