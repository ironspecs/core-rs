//! Non-inherited grid computed group (`GRID_STEP1_DESIGN.md` D2).
//!
//! This file owns the `ComputedGridGroup` struct only. Its three field
//! vocabularies live in their own files (`grid_track_size.rs`,
//! `grid_auto_flow_kw.rs`, plus the `GridTrackListKey` handle in
//! `decl_value.rs`), each owning its type + helpers (structural rule 1). It is
//! plain `Copy` data plus `initial()`; resolution lives in the step-3.3
//! resolver (`project.rs`).
//!
//! Invariants and rules:
//!
//! - **`template_columns`/`template_rows` are arena-relative keys.** The value
//!   is a `GridTrackListKey` into the owning `DeclStore`'s track-list arena;
//!   `initial()` uses `GridTrackListKey::default()` — the empty-list sentinel (a
//!   null key is never a valid arena entry, and the T1 solver reads it as "no
//!   authored tracks"). Track lists are interned value-deduped, so two equal
//!   templates share one key.
//! - **Initial values mirror the CSS initials**: `row` auto-flow, `auto`
//!   auto-track sizes, empty explicit tracks.

use super::grid_auto_flow_kw::GridAutoFlowKw;
use super::grid_track_size::{GridTrackSize, TrackBreadth, TrackMaxBreadth};
use crate::domains::style::decl_value::GridTrackListKey;

/// Computed values of the non-inherited grid longhands (the T1 set).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedGridGroup {
    /// Interned explicit column track list; `null()` = the empty list.
    pub template_columns: GridTrackListKey,
    /// Interned explicit row track list; `null()` = the empty list.
    pub template_rows: GridTrackListKey,
    /// `Row` is the CSS initial (`grid-auto-flow: row`).
    pub auto_flow: GridAutoFlowKw,
    /// `Auto` is the CSS initial (`grid-auto-columns: auto`).
    pub auto_columns: GridTrackSize,
    /// `Auto` is the CSS initial (`grid-auto-rows: auto`).
    pub auto_rows: GridTrackSize,
}

impl ComputedGridGroup {
    /// The no-parent grid seed: every field at its CSS initial. The track-list
    /// keys are `null()` — the empty-list sentinel, since `initial()` is a
    /// static seed with no owning store (the T1 solver reads `null()` as "no
    /// authored tracks").
    #[must_use]
    pub fn initial() -> Self {
        Self {
            template_columns: GridTrackListKey::default(),
            template_rows: GridTrackListKey::default(),
            auto_flow: GridAutoFlowKw::Row,
            auto_columns: GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            },
            auto_rows: GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_matches_css_initials() {
        let grid = ComputedGridGroup::initial();
        assert_eq!(grid.template_columns, GridTrackListKey::default());
        assert_eq!(grid.template_rows, GridTrackListKey::default());
        assert_eq!(grid.auto_flow, GridAutoFlowKw::Row);
        assert_eq!(grid.auto_columns.min, TrackBreadth::Auto);
        assert_eq!(grid.auto_columns.max, TrackMaxBreadth::Auto);
        assert_eq!(grid.auto_rows.min, TrackBreadth::Auto);
    }
}
