//! Grid track-size computed value (`GridTrackSize` + `TrackBreadth` +
//! `TrackMaxBreadth`).
//!
//! This file owns the closed track-size leaf vocabulary for grid's T1 property
//! set (`grid-template-columns`/`rows`, `grid-auto-columns`/`rows`). It is
//! plain `Copy` data; no resolution policy lives here (track sizing is T1, in
//! `GRID_STEP2_DESIGN.md`).
//!
//! This is the spec's uniform minmax model (`GRID_STEP2_DESIGN.md` correction 3,
//! 2026-10-20): every track is a `{min, max}` pair. The former T0 shape
//! (`GridTrackKind` + two bare `f32` bound slots) could not distinguish a
//! percent bound from a px bound, so `minmax(10%, 200px)` and
//! `minmax(10px, 200px)` collapsed to the same value. The correction splits the
//! bounds into `TrackBreadth` (the min side; `Auto | Px | Percent | MinContent
//! | MaxContent`) and `TrackMaxBreadth` (a `TrackBreadth` plus `Fr`, valid on
//! the max side only), preserving percent bounds end-to-end.
//!
//! Invariants and rules:
//!
//! - `Fr` is a max-side breadth only; a track's `min` is never `Fr` (the
//!   grammar normalizes `1fr` to `{ Auto, Fr(1.0) }`)
//! - the plain single keywords normalize to matching bounds: `auto` →
//!   `{ Auto, Auto }`; `min-content` → `{ MinContent, MinContent }`;
//!   `max-content` → `{ MaxContent, MaxContent }`; a length/percent →
//!   `{ Px/Percent(x), Px/Percent(x) }`
//! - these three types are plain data (no `impl`) so they share this file under
//!   structural rule 1; any behavior lives in the consuming files (`decl_parse.rs`
//!   normalization, `decl_value.rs` formatting, `grid.rs` resolution)

/// A grid track breadth: the min side of a track, and every non-`fr` max side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackBreadth {
    /// `auto` — resolves content-based.
    Auto,
    /// A definite length in px.
    Px(f32),
    /// A percentage of the grid container's content size.
    Percent(f32),
    /// `min-content` — the track's min-content contribution.
    MinContent,
    /// `max-content` — the track's max-content contribution.
    MaxContent,
}

/// A grid track max breadth: a `TrackBreadth` plus the flexible `fr`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackMaxBreadth {
    /// `auto` — resolves content-based (the `Auto` kind's rules).
    Auto,
    /// A definite length in px.
    Px(f32),
    /// A percentage of the grid container's content size.
    Percent(f32),
    /// `min-content` — the track's min-content contribution.
    MinContent,
    /// `max-content` — the track's max-content contribution.
    MaxContent,
    /// A flexible `fr` factor (valid on the max side only).
    Fr(f32),
}

/// A single grid track size in the spec's uniform minmax shape: a min/max
/// breadth pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridTrackSize {
    /// The track's min breadth.
    pub min: TrackBreadth,
    /// The track's max breadth.
    pub max: TrackMaxBreadth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_breadths_are_distinct() {
        assert_ne!(TrackBreadth::Auto, TrackBreadth::MinContent);
        assert_ne!(TrackBreadth::Px(10.0), TrackBreadth::Percent(10.0));
        assert_ne!(TrackBreadth::MaxContent, TrackBreadth::MinContent);
    }

    #[test]
    fn max_breadth_extends_min_with_fr() {
        // `Fr` exists on the max side only, and the non-fr max breadths mirror
        // the min breadths' vocabulary.
        assert_eq!(TrackMaxBreadth::Px(10.0), TrackMaxBreadth::Px(10.0));
        assert_ne!(TrackMaxBreadth::Auto, TrackMaxBreadth::Fr(1.0));
        assert_ne!(TrackMaxBreadth::Percent(10.0), TrackMaxBreadth::Px(10.0));
    }

    #[test]
    fn percent_bounds_are_preserved_distinctly_from_px() {
        // The correction's core invariant: a percent bound and a px bound with
        // the same scalar are DIFFERENT values (the old T0 shape collapsed them).
        let percent_min = GridTrackSize {
            min: TrackBreadth::Percent(10.0),
            max: TrackMaxBreadth::Px(200.0),
        };
        let px_min = GridTrackSize {
            min: TrackBreadth::Px(10.0),
            max: TrackMaxBreadth::Px(200.0),
        };
        assert_ne!(percent_min, px_min);
    }
}
