//! Grid track-size computed value (`GridTrackSize` + `GridTrackKind`).
//!
//! This file owns the closed track-size leaf vocabulary for grid's T1 property
//! set (`grid-template-columns`/`rows`, `grid-auto-columns`/`rows`). It is
//! plain `Copy` data plus the two sentinel constants for the `minmax` keyword
//! bounds; no resolution policy lives here (track sizing is T1, in
//! `GRID_STEP2_DESIGN.md`).
//!
//! Invariants and rules:
//!
//! - `Length`/`Percent` carry their value in `min_px` (the kind tracks the
//!   unit); `Fr(f32)` carries the fraction in its payload; `Minmax` uses both
//!   `min_px` and `max_px` as the two numeric bounds.
//! - A `minmax` bound that is a keyword is encoded as a sentinel in the
//!   numeric slot: `MAX_CONTENT_BOUND` (`f32::INFINITY`, an unbounded growth
//!   limit) and `MIN_CONTENT_BOUND` (`f32::NEG_INFINITY`). The T1 solver
//!   interprets the sentinels; T0 only holds them. Both are self-owned
//!   constants, never boundary data.
//! - The `minmax` numeric bounds do not track px-vs-percent (the `Minmax`
//!   kind is unit); that resolution is a T1 concern, matching the design's
//!   "Minmax uses both bounds" holding shape.

/// The closed track-size kind vocabulary (`GRID_STEP1_DESIGN.md` D2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrackKind {
    Auto,
    Length,
    Percent,
    MinContent,
    MaxContent,
    Minmax,
    Fr(f32),
}

/// A single grid track size: a kind plus the two bound slots the kind uses.
/// `Length`/`Percent` carry their value in `min_px`; `Minmax` carries both
/// bounds (`min_px`, `max_px`); every other kind leaves both at `0.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridTrackSize {
    pub kind: GridTrackKind,
    pub min_px: f32,
    pub max_px: f32,
}

impl GridTrackSize {
    /// The `max-content` `minmax` upper-bound sentinel (an unbounded growth
    /// limit). Produced by the grammar for `minmax(<len>, max-content)`.
    pub const MAX_CONTENT_BOUND: f32 = f32::INFINITY;

    /// The `min-content` `minmax` upper-bound sentinel. Produced by the grammar
    /// for `minmax(<len>, min-content)`; the T1 solver resolves it to the
    /// track's intrinsic min-content size.
    pub const MIN_CONTENT_BOUND: f32 = f32::NEG_INFINITY;
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // sentinel and initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn sentinels_are_distinct_and_non_finite() {
        assert!(GridTrackSize::MAX_CONTENT_BOUND.is_infinite());
        assert!(GridTrackSize::MIN_CONTENT_BOUND.is_infinite());
        assert_ne!(
            GridTrackSize::MAX_CONTENT_BOUND,
            GridTrackSize::MIN_CONTENT_BOUND
        );
    }

    #[test]
    fn kinds_carry_their_payloads() {
        assert_eq!(GridTrackKind::Fr(1.5), GridTrackKind::Fr(1.5));
        assert_ne!(GridTrackKind::Length, GridTrackKind::Percent);
        assert_ne!(GridTrackKind::Auto, GridTrackKind::MinContent);
    }
}
