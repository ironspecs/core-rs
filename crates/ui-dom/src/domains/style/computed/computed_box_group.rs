//! Non-inherited box computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedBoxGroup` struct only. Its three field
//! vocabularies live in their own files (`display_kw.rs`, `position_kw.rs`,
//! `overflow_kw.rs`), each owning its keyword enum + helpers (structural rule
//! 1). It is plain `Copy` data plus `initial()`; resolution lives in the
//! step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - **`Option<f32>` = auto/unset.** `width`/`height`/min/max/top/left/right/
//!   bottom are `None` when unset (initial `None`); lengths resolve to px at
//!   style time, `em` against the element's own font size (A6) — the resolver's
//!   job, not this file's.
//! - **`margin`/`padding`/`border_width` are `[top, right, bottom, left]`,**
//!   initial `[0.0; 4]` each — mirroring today's absent-edge default of
//!   `unwrap_or(0.0)` in `domains/layout/style.rs::box_edge_insets_for`.

use super::display_kw::DisplayKw;
use super::overflow_kw::OverflowKw;
use super::position_kw::PositionKw;

/// Computed values of the non-inherited box longhands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedBoxGroup {
    /// `DisplayKw::Block` is today's absent default (`presented_display` falls
    /// through to `Block` for element nodes, `domains/layout/style.rs`).
    pub display: DisplayKw,
    /// `PositionKw::Static` is today's absent default (`presented_position`).
    pub position: PositionKw,
    /// `OverflowKw::Visible` is today's absent default (no clip, no scroll).
    pub overflow: OverflowKw,
    /// `auto = false`, `isolate = true`; absent defaults to false today
    /// (`domains/layout/style.rs::isolate_creates_clipped_layer`).
    pub isolate: bool,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub top: Option<f32>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub bottom: Option<f32>,
    /// `[top, right, bottom, left]`, px; initial `[0.0; 4]` (absent-edge default
    /// `unwrap_or(0.0)` in `box_edge_insets_for`).
    pub margin: [f32; 4],
    /// `[top, right, bottom, left]`, px; initial `[0.0; 4]`.
    pub padding: [f32; 4],
    /// `[top, right, bottom, left]`, px; initial `[0.0; 4]`.
    pub border_width: [f32; 4],
}

impl ComputedBoxGroup {
    /// The no-parent box seed: every field at its CSS/today default.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            display: DisplayKw::Block,
            position: PositionKw::Static,
            overflow: OverflowKw::Visible,
            isolate: false,
            width: None,
            height: None,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            top: None,
            left: None,
            right: None,
            bottom: None,
            margin: [0.0; 4],
            padding: [0.0; 4],
            border_width: [0.0; 4],
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_matches_todays_absent_defaults() {
        let box_ = ComputedBoxGroup::initial();
        assert_eq!(box_.display, DisplayKw::Block);
        assert_eq!(box_.position, PositionKw::Static);
        assert_eq!(box_.overflow, OverflowKw::Visible);
        assert!(!box_.isolate);
        assert_eq!(box_.width, None);
        assert_eq!(box_.height, None);
        assert_eq!(box_.min_width, None);
        assert_eq!(box_.min_height, None);
        assert_eq!(box_.max_width, None);
        assert_eq!(box_.max_height, None);
        assert_eq!(box_.top, None);
        assert_eq!(box_.left, None);
        assert_eq!(box_.right, None);
        assert_eq!(box_.bottom, None);
        assert_eq!(box_.margin, [0.0; 4]);
        assert_eq!(box_.padding, [0.0; 4]);
        assert_eq!(box_.border_width, [0.0; 4]);
    }
}
