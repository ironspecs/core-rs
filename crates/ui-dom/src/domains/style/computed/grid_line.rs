//! Grid placement line computed value (`GridLine`).
//!
//! This file owns the closed `<grid-line>` value vocabulary for the four grid
//! placement longhands (`grid-column-start`/`end`, `grid-row-start`/`end`),
//! admitted in grid T2 (`GRID_STEP3_DESIGN.md` D1) and extended in grid T3
//! (`GRID_STEP4_DESIGN.md` D2) with the two name-reference forms. It is a plain
//! `Copy` enum plus `initial()`; the grammar in `decl_parse.rs` maps the token
//! forms onto it, the resolver copies it (re-interning name keys into the
//! document computed-text interner) into the item-side `ComputedGridGroup`
//! fields, and `decl_value.rs` formats it for the wire projection. No placement
//! policy lives here (line resolution — including the D4 named-line rules — is
//! in `grid.rs`).
//!
//! Invariants and rules:
//!
//! - `Line(i32)` admits negative integers (translated at placement, D2); zero is
//!   never produced — the grammar drops `0` with a counted report (zero is not a
//!   line, CSS Grid §7.1)
//! - `Span(u32)` is always positive; `span 0`/`span -1` drop at admission (D1)
//! - `Named(TextKey)` is a bare `<custom-ident>` line reference; `NamedSpan
//!   (TextKey, u32)` is `span <custom-ident> [integer]` — a span of `n` (or,
//!   when the integer is ABSENT and `n == 0`, a span across the count of
//!   matching lines, per `GRID_STEP4_DESIGN.md` D4 rule 3)
//! - the `TextKey` in the name forms is document-computed-text-relative by the
//!   time it reaches the group (the resolver re-interns it); the grammar interns
//!   the raw name into the parsing store, and `decl_value.rs`/`winners_fold.rs`
//!   re-intern it as needed
//! - `Auto` is the CSS initial for all four longhands, and is `initial()`

use crate::domains::style::decl_value::TextKey;

/// A resolved `<grid-line>`: `auto | <integer> | span <positive-integer>` plus
/// the T3 name forms `<custom-ident>` and `span <custom-ident> [integer]`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GridLine {
    /// `auto` — no definite line (the auto-placement assigns it).
    Auto,
    /// A definite integer line (negatives admitted, translated at placement).
    Line(i32),
    /// `span <positive-integer>` — a span with no definite line on that side.
    Span(u32),
    /// `<custom-ident>` — a named line (single match → that line; multiple
    /// matches → a span across them, D4 rule 2).
    Named(TextKey),
    /// `span <custom-ident> [integer]` — a span from the first matching line
    /// (D4 rule 3). `n == 0` encodes the absent-integer form: span the count of
    /// matching lines.
    NamedSpan(TextKey, u32),
}

impl GridLine {
    /// The CSS initial value: `auto` (all four placement longhands).
    #[must_use]
    pub fn initial() -> Self {
        Self::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_is_auto() {
        assert_eq!(GridLine::initial(), GridLine::Auto);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(GridLine::Auto, GridLine::Line(1));
        assert_ne!(GridLine::Line(-1), GridLine::Line(1));
        assert_ne!(GridLine::Line(1), GridLine::Span(1));
        assert_ne!(GridLine::Span(1), GridLine::Span(2));
        assert_ne!(GridLine::Named(TextKey(1)), GridLine::Named(TextKey(2)));
        assert_ne!(
            GridLine::Named(TextKey(1)),
            GridLine::NamedSpan(TextKey(1), 0)
        );
        assert_ne!(
            GridLine::NamedSpan(TextKey(1), 2),
            GridLine::NamedSpan(TextKey(1), 3)
        );
    }

    #[test]
    fn line_carries_negative_integers() {
        assert_eq!(GridLine::Line(-3), GridLine::Line(-3));
    }

    #[test]
    fn named_span_zero_is_the_absent_integer_form() {
        // The D4 rule-3 sentinel: `n == 0` means "span the count of matching
        // lines", distinct from an explicit `span name 1`.
        assert_eq!(
            GridLine::NamedSpan(TextKey(7), 0),
            GridLine::NamedSpan(TextKey(7), 0)
        );
        assert_ne!(
            GridLine::NamedSpan(TextKey(7), 0),
            GridLine::NamedSpan(TextKey(7), 1)
        );
    }
}
