//! Non-inherited grid computed group (`GRID_STEP1_DESIGN.md` D2, extended by
//! `GRID_STEP3_DESIGN.md` D1 for the T2 placement/alignment fields).
//!
//! This file owns the `ComputedGridGroup` struct only. Its field vocabularies
//! live in their own files (`grid_track_size.rs`, `grid_auto_flow_kw.rs`,
//! `grid_line.rs`, `justify_items_kw.rs`, `justify_self_kw.rs`, plus the
//! `GridTrackListKey` handle in `decl_value.rs`), each owning its type + helpers
//! (structural rule 1). It is plain `Copy` data plus `initial()`; resolution
//! lives in the step-3.3 resolver (`project.rs`).
//!
//! Invariants and rules:
//!
//! - **`template_columns`/`template_rows` are arena-relative keys.** The value
//!   is a `GridTrackListKey` into the owning `DeclStore`'s track-list arena;
//!   `initial()` uses `GridTrackListKey::default()` — the empty-list sentinel (a
//!   null key is never a valid arena entry, and the T1 solver reads it as "no
//!   authored tracks"). Track lists are interned value-deduped, so two equal
//!   templates share one key.
//! - **The four placement longhands are item-side.** `column_start`/`column_end`/
//!   `row_start`/`row_end` are the resolved `<grid-line>` values (T2); `Auto` is
//!   the initial for all four. `justify_self` is item-side (`Auto` initial).
//! - **`justify_items` is container-side** (`Normal` initial), the inline-axis
//!   cell alignment vocabulary (T2, D5).
//! - **Initial values mirror the CSS initials**: `row` auto-flow, `auto`
//!   auto-track sizes, empty explicit tracks, `auto` placement lines, `normal`
//!   `justify-items`, `auto` `justify-self`.

use super::grid_auto_flow_kw::GridAutoFlowKw;
use super::grid_line::GridLine;
use super::grid_track_size::{GridTrackSize, TrackBreadth, TrackMaxBreadth};
use super::justify_items_kw::JustifyItemsKw;
use super::justify_self_kw::JustifySelfKw;
use crate::domains::style::decl_value::{GridAreasKey, GridTrackListKey};

/// Computed values of the non-inherited grid longhands (the T1 set, the T2
/// placement/alignment longhands, and the T3 areas/named-line fields).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedGridGroup {
    /// Interned explicit column track list; `null()` = the empty list.
    pub template_columns: GridTrackListKey,
    /// Interned explicit row track list; `null()` = the empty list.
    pub template_rows: GridTrackListKey,
    /// Interned template-areas map; `null()` = the empty map (no authored
    /// areas).
    pub template_areas: GridAreasKey,
    /// `Row` is the CSS initial (`grid-auto-flow: row`).
    pub auto_flow: GridAutoFlowKw,
    /// `Auto` is the CSS initial (`grid-auto-columns: auto`).
    pub auto_columns: GridTrackSize,
    /// `Auto` is the CSS initial (`grid-auto-rows: auto`).
    pub auto_rows: GridTrackSize,
    /// `Auto` is the CSS initial (`grid-column-start`).
    pub column_start: GridLine,
    /// `Auto` is the CSS initial (`grid-column-end`).
    pub column_end: GridLine,
    /// `Auto` is the CSS initial (`grid-row-start`).
    pub row_start: GridLine,
    /// `Auto` is the CSS initial (`grid-row-end`).
    pub row_end: GridLine,
    /// `Normal` is the CSS initial (`justify-items`, container side).
    pub justify_items: JustifyItemsKw,
    /// `Auto` is the CSS initial (`justify-self`, item side).
    pub justify_self: JustifySelfKw,
}

impl ComputedGridGroup {
    /// The no-parent grid seed: every field at its CSS initial. The track-list
    /// and areas-map keys are `null()` — the empty sentinels, since `initial()`
    /// is a static seed with no owning store (the solver reads `null()` as "no
    /// authored value").
    #[must_use]
    pub fn initial() -> Self {
        Self {
            template_columns: GridTrackListKey::default(),
            template_rows: GridTrackListKey::default(),
            template_areas: GridAreasKey::default(),
            auto_flow: GridAutoFlowKw::Row,
            auto_columns: GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            },
            auto_rows: GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            },
            column_start: GridLine::initial(),
            column_end: GridLine::initial(),
            row_start: GridLine::initial(),
            row_end: GridLine::initial(),
            justify_items: JustifyItemsKw::initial(),
            justify_self: JustifySelfKw::initial(),
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
        assert_eq!(grid.template_areas, GridAreasKey::default());
        assert_eq!(grid.auto_flow, GridAutoFlowKw::Row);
        assert_eq!(grid.auto_columns.min, TrackBreadth::Auto);
        assert_eq!(grid.auto_columns.max, TrackMaxBreadth::Auto);
        assert_eq!(grid.auto_rows.min, TrackBreadth::Auto);
        assert_eq!(grid.column_start, GridLine::Auto);
        assert_eq!(grid.column_end, GridLine::Auto);
        assert_eq!(grid.row_start, GridLine::Auto);
        assert_eq!(grid.row_end, GridLine::Auto);
        assert_eq!(grid.justify_items, JustifyItemsKw::Normal);
        assert_eq!(grid.justify_self, JustifySelfKw::Auto);
    }
}
