//! Interned `font-family` list computed value (`TextFamilyList`).
//!
//! This file owns the structured family-list value the `font-family` grammar
//! parses into (text seam S2, `TEXT_SEAM_DESIGN.md` D3). It is the arena value
//! for the family-list interner (`DeclStore::family_lists`), mirroring the
//! grid track-list arena (`GridTrackList`): a `TextFamilyListKey` in a computed
//! group or declaration resolves to one of these. Unlike `GridTrackList`, whose
//! `[name]` line names are store-relative `TextKey`s, a family list's names are
//! self-owned (`Arc<str>`) because the resolved list crosses the text seam into
//! the render world (via `TextRunSignature` → the extracted records), which has
//! no access to a document's interner.
//!
//! Invariants and rules:
//!
//! - `items` holds at most `TEXT_FAMILY_LIST_CAPACITY` items; a list over
//!   capacity drops at parse, never a silent cap
//! - a quoted family name's quote pair is stripped at parse and re-added by
//!   `to_css_string` only when the name needs it (whitespace/punctuation); an
//!   unquoted generic keyword (`serif`/`sans-serif`/`monospace`/`system-ui`)
//!   parses to `Generic`, and is emitted bare
//! - value-dedup interning compares full content, so two identical lists
//!   (same items, same names) intern to one key
//! - `to_css_string` is the canonical CSS serialization (quote names with
//!   whitespace/punctuation, emit generics bare), and round-trips through the
//!   grammar for every parseable list

use std::sync::Arc;

use arrayvec::ArrayVec;

use super::font_family_generic::FontFamilyGeneric;

/// The maximum family-name items one `font-family` list may hold (the
/// design-system standard; larger lists are a named D6-gated follow-up).
pub(crate) const TEXT_FAMILY_LIST_CAPACITY: usize = 8;

/// One item in a family list: a named family (quoted or unquoted) or a generic
/// keyword. A quoted generic (`"serif"`) is a NAME, not the generic keyword —
/// the parser routes only bare keywords to `Generic`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextFamilyItem {
    /// A family name. Owned (`Arc<str>`) so the resolved list crosses the seam.
    FamilyName(Arc<str>),
    /// A generic family keyword (`serif`/`sans-serif`/`monospace`/`system-ui`).
    Generic(FontFamilyGeneric),
}

/// An interned `font-family` list: the ordered family-name/generic items.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct TextFamilyList {
    /// The family items, in authored order. Empty is the initial value (no
    /// authored family → the request's default).
    pub(crate) items: ArrayVec<TextFamilyItem, TEXT_FAMILY_LIST_CAPACITY>,
}

impl TextFamilyList {
    /// The first named family (skipping generic keywords), the seed for S2's
    /// "requested first-family-if-installed" resolution (D3; S4 generalizes).
    #[must_use]
    pub(crate) fn first_family_name(&self) -> Option<&str> {
        self.items.iter().find_map(|item| match item {
            TextFamilyItem::FamilyName(name) => Some(name.as_ref()),
            TextFamilyItem::Generic(_) => None,
        })
    }

    /// The canonical CSS serialization: family names containing whitespace or
    /// ASCII punctuation are re-quoted, generics are emitted bare, and items
    /// are comma-space separated. Round-trips through the grammar.
    #[must_use]
    pub(crate) fn to_css_string(&self) -> String {
        let mut out = String::new();
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            match item {
                TextFamilyItem::FamilyName(name) => {
                    if family_name_needs_quoting(name) {
                        out.push('"');
                        out.push_str(name);
                        out.push('"');
                    } else {
                        out.push_str(name);
                    }
                }
                TextFamilyItem::Generic(generic) => out.push_str(generic.name()),
            }
        }
        out
    }
}

/// True when a family name must be quoted in the wire form: it contains ASCII
/// whitespace or punctuation (a non-`<custom-ident>` character). Non-ASCII
/// characters (CJK, accented names) are bare by the CSS family-name grammar.
fn family_name_needs_quoting(name: &str) -> bool {
    name.chars()
        .any(|ch| ch.is_ascii() && !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> TextFamilyItem {
        TextFamilyItem::FamilyName(Arc::<str>::from(text))
    }

    #[test]
    fn first_family_name_skips_generics() {
        let list = TextFamilyList {
            items: [
                TextFamilyItem::Generic(FontFamilyGeneric::SansSerif),
                name("Source Han Sans"),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(list.first_family_name(), Some("Source Han Sans"));

        let empty = TextFamilyList::default();
        assert_eq!(empty.first_family_name(), None);
    }

    #[test]
    fn serialization_quotes_names_with_whitespace_and_keeps_generics_bare() {
        let list = TextFamilyList {
            items: [
                name("My Font"),
                TextFamilyItem::Generic(FontFamilyGeneric::SansSerif),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(list.to_css_string(), "\"My Font\", sans-serif");
    }

    #[test]
    fn serialization_keeps_unquoted_non_ascii_and_hyphenated_names_bare() {
        let list = TextFamilyList {
            items: [name("ui-sans"), name("ヒラギノ角ゴ")]
                .into_iter()
                .collect(),
        };
        assert_eq!(list.to_css_string(), "ui-sans, ヒラギノ角ゴ");
    }
}
