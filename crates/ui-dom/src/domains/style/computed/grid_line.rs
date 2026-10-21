//! Grid placement line computed value (`GridLine`).
//!
//! This file owns the closed `<grid-line>` value vocabulary for the four grid
//! placement longhands (`grid-column-start`/`end`, `grid-row-start`/`end`),
//! admitted in grid T2 (`GRID_STEP3_DESIGN.md` D1). It is a plain `Copy` enum
//! plus `initial()`; the grammar in `decl_parse.rs` maps the token forms onto
//! it, the resolver copies it into the item-side `ComputedGridGroup` fields, and
//! `decl_value.rs` formats it for the wire projection. No placement policy lives
//! here (line resolution against the explicit grid is D2, in `grid.rs`).
//!
//! Invariants and rules:
//!
//! - `Line(i32)` admits negative integers (translated at placement, D2); zero is
//!   never produced — the grammar drops `0` with a counted report (zero is not a
//!   line, CSS Grid §7.1)
//! - `Span(u32)` is always positive; `span 0`/`span -1` drop at admission (D1)
//! - `Auto` is the CSS initial for all four longhands, and is `initial()`

/// A resolved `<grid-line>`: `auto | <integer> | span <positive-integer>`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GridLine {
    /// `auto` — no definite line (the auto-placement assigns it).
    Auto,
    /// A definite integer line (negatives admitted, translated at placement).
    Line(i32),
    /// `span <positive-integer>` — a span with no definite line on that side.
    Span(u32),
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
    }

    #[test]
    fn line_carries_negative_integers() {
        assert_eq!(GridLine::Line(-3), GridLine::Line(-3));
    }
}
