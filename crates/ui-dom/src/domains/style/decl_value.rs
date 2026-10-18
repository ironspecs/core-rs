//! Typed declaration values for the `ui-dom` style system.
//!
//! This file owns the typed value model for parsed declarations: `DeclLength`,
//! `DeclValue`, `TextKey`, and the cascade `Precedence` record. Text values are
//! interned and store-relative: a `TextKey` is an opaque index that is
//! meaningless without the owning store's interner (`TextInterner`), so a text
//! value never carries its own `String` copy on the declaration.
//!
//! Invariants and rules:
//!
//! - `Precedence` field order is the cascade sort order (the cascade tier
//!   first, then origin priority, then the inline flag above any selector
//!   specificity, then specificity, then source order); stages 2–4 of cascade
//!   tier path F extend the `CascadeTier` enum additively without reshaping the
//!   key
//! - `value_equals` is the single documented equality intent for cascade and
//!   upsert change detection; it is plain `PartialEq`, named so callers state
//!   their intent

use super::cascade_tier::CascadeTier;
use super::keyword::Keyword;
use crate::types::DomAttributeValue;

/// A parsed length: the closed `unset | 0 | <px> | <em> | <percentage>`
/// grammar. `Percent` is the declaration form for ALL percentages; resolution
/// is per property (font contexts resolve at style time; box lengths stay
/// symbolic into the group).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclLength {
    Unset,
    Px(f32),
    Em(f32),
    Percent(f32),
}

/// Opaque handle to interned text owned by a `DeclStore`; store-relative.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextKey(pub u32);

/// Cascade precedence record for one declaration (field order IS the sort
/// order: the cascade tier first, then origin priority, then the inline-style
/// flag above any selector specificity, then specificity, then source order).
/// The `tier` field projects CSS Cascade 5 §6.1's origin-and-importance ladder
/// (see `cascade_tier.rs`); stages 2–4 of cascade tier path F extend the
/// `CascadeTier` ENUM additively (`ImportantUa` above, `Animation` between
/// `NormalAuthor` and `ImportantAuthor`, layer indices within a tier) rather
/// than reshaping these semantics.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Precedence {
    pub tier: CascadeTier,
    pub origin_priority: i32,
    pub inline: bool,
    pub specificity: super::SelectorSpecificity,
    pub order: u64,
}

/// A parsed declaration value: length, color, keyword, number, integer,
/// interned text, or the typed translate pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclValue {
    Length(DeclLength),
    Color([f32; 4]),
    Keyword(Keyword),
    Number(f32),
    Integer(i64),
    Text(TextKey),
    /// The scheduler's interpolation form for `transform`. Declarations stay
    /// text-interned (`Keyword(None)` / `Text`) and resolve into the transform
    /// group; this variant is how the group hands the scheduler a typed
    /// `[x, y]` translate pair. It is produced only at group-read time
    /// (`decl_value_for_groups`), never by declaration parse.
    Translate([f32; 2]),
}

impl DeclValue {
    /// True when two values are equal for cascade/upsert change detection.
    /// (Plain `PartialEq`; named helper so callers document intent.)
    #[must_use]
    pub fn value_equals(&self, other: &DeclValue) -> bool {
        self == other
    }
}

/// Projects one typed `DeclValue` to its public wire `DomAttributeValue`.
/// `resolve_text` resolves store-owned interned text; it must resolve (the
/// interner and the block live in the same store, so a `Text` key here is a
/// self-owned structural invariant — the `expect` is never on boundary data).
pub(crate) fn decl_value_to_dom_value<'a>(
    value: &DeclValue,
    resolve_text: impl Fn(TextKey) -> Option<&'a str>,
) -> DomAttributeValue {
    match value {
        DeclValue::Length(DeclLength::Unset) => DomAttributeValue::String("unset".to_string()),
        DeclValue::Length(DeclLength::Px(value)) => DomAttributeValue::String(format!("{value}px")),
        DeclValue::Length(DeclLength::Em(value)) => DomAttributeValue::String(format!("{value}em")),
        DeclValue::Length(DeclLength::Percent(value)) => {
            DomAttributeValue::String(format!("{value}%"))
        }
        DeclValue::Color(rgba) => DomAttributeValue::Color(*rgba),
        DeclValue::Keyword(keyword) => DomAttributeValue::String(keyword.name().to_string()),
        DeclValue::Number(value) => DomAttributeValue::String(format!("{value}")),
        DeclValue::Integer(value) => DomAttributeValue::String(value.to_string()),
        DeclValue::Text(key) => DomAttributeValue::String(
            resolve_text(*key)
                .expect("interned text must resolve in its owning store")
                .to_string(),
        ),
        DeclValue::Translate([x, y]) => {
            DomAttributeValue::String(format!("translate({x}px, {y}px)"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_equals_matches_across_variants() {
        assert!(
            DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Em(10.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Px(10.0))
                .value_equals(&DeclValue::Length(DeclLength::Unset))
        );
        assert!(
            DeclValue::Length(DeclLength::Percent(50.0))
                .value_equals(&DeclValue::Length(DeclLength::Percent(50.0)))
        );
        assert!(
            !DeclValue::Length(DeclLength::Percent(50.0))
                .value_equals(&DeclValue::Length(DeclLength::Px(50.0)))
        );
        assert!(DeclValue::Keyword(Keyword::Auto).value_equals(&DeclValue::Keyword(Keyword::Auto)));
        assert!(
            !DeclValue::Keyword(Keyword::Auto).value_equals(&DeclValue::Keyword(Keyword::Hidden))
        );
        assert!(DeclValue::Number(1.5).value_equals(&DeclValue::Number(1.5)));
        assert!(DeclValue::Integer(-3).value_equals(&DeclValue::Integer(-3)));
        assert!(!DeclValue::Integer(3).value_equals(&DeclValue::Number(3.0)));
        assert!(DeclValue::Text(TextKey(7)).value_equals(&DeclValue::Text(TextKey(7))));
        assert!(!DeclValue::Text(TextKey(7)).value_equals(&DeclValue::Text(TextKey(8))));
        assert!(
            DeclValue::Color([1.0, 0.0, 0.0, 1.0])
                .value_equals(&DeclValue::Color([1.0, 0.0, 0.0, 1.0]))
        );
        assert!(DeclValue::Translate([3.0, 4.0]).value_equals(&DeclValue::Translate([3.0, 4.0])));
        assert!(!DeclValue::Translate([3.0, 4.0]).value_equals(&DeclValue::Translate([4.0, 3.0])));
    }

    #[test]
    fn decl_value_to_dom_value_formats_every_decl_value_kind() {
        let resolve_none = |_key: TextKey| -> Option<&str> { None };
        let resolve_text = |_key: TextKey| -> Option<&str> { Some("ui-sans, sans-serif") };

        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Unset), resolve_none),
            DomAttributeValue::String("unset".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Px(20.0)), resolve_none),
            DomAttributeValue::String("20px".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Px(0.0)), resolve_none),
            DomAttributeValue::String("0px".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Px(-10.0)), resolve_none),
            DomAttributeValue::String("-10px".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Px(0.5)), resolve_none),
            DomAttributeValue::String("0.5px".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Em(1.5)), resolve_none),
            DomAttributeValue::String("1.5em".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Percent(50.0)), resolve_none),
            DomAttributeValue::String("50%".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Length(DeclLength::Percent(-10.0)), resolve_none),
            DomAttributeValue::String("-10%".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Color([1.0, 0.0, 0.0, 0.5]), resolve_none),
            DomAttributeValue::Color([1.0, 0.0, 0.0, 0.5])
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Keyword(Keyword::Justified), resolve_none),
            DomAttributeValue::String("justify".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Keyword(Keyword::RowReverse), resolve_none),
            DomAttributeValue::String("row-reverse".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Number(2.0), resolve_none),
            DomAttributeValue::String("2".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Number(0.5), resolve_none),
            DomAttributeValue::String("0.5".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Integer(-3), resolve_none),
            DomAttributeValue::String("-3".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Text(TextKey(7)), resolve_text),
            DomAttributeValue::String("ui-sans, sans-serif".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Translate([5.0, 0.0]), resolve_none),
            DomAttributeValue::String("translate(5px, 0px)".to_string())
        );
        assert_eq!(
            decl_value_to_dom_value(&DeclValue::Translate([0.0, -3.5]), resolve_none),
            DomAttributeValue::String("translate(0px, -3.5px)".to_string())
        );
    }
}
