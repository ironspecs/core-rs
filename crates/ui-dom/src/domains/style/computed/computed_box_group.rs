//! Non-inherited box computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedBoxGroup` struct only. Its field vocabularies
//! live in their own files (`display_kw.rs`, `position_kw.rs`, `overflow_kw.rs`,
//! `box_sizing_kw.rs`), each owning its keyword enum + helpers (structural rule
//! 1). It is plain `Copy` data plus `initial()`; resolution lives in the
//! step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - **`BoxLength` = the computed/used split (CSS Cascade §4.5).**
//!   `width`/`height`/min/max/top/left/right/bottom are `BoxLength::Auto` when
//!   unset/`auto` (initial `Auto` — exactly the old `None` semantics); `Px` is
//!   style-resolved (em against the element's own font size, A6); `Percent`
//!   stays symbolic for layout-time resolution against the containing block.
//! - **`margin`/`padding`/`border_width` are `[top, right, bottom, left]`,**
//!   initial `[BoxLength::Px(0.0); 4]` each — mirroring today's absent-edge
//!   default of `unwrap_or(0.0)` in `domains/layout/style.rs::box_edge_insets_for`.
//!   Authored `margin: auto` is `BoxLength::Auto`, distinct from absent
//!   (`Px(0.0)`); `padding: auto` never arrives (the grammar rejects it).
//!   `border_width` stays `[f32; 4]` — no percentages, no `auto` in CSS
//!   border-width.

use super::box_length::BoxLength;
use super::box_sizing_kw::BoxSizingKw;
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
    /// `ContentBox` is today's single sizing model (the CSS UI 4 §3.1 initial):
    /// the specified `width`/`height` is the content box, padding and border
    /// outside it.
    pub box_sizing: BoxSizingKw,
    pub width: BoxLength,
    pub height: BoxLength,
    pub min_width: BoxLength,
    pub min_height: BoxLength,
    pub max_width: BoxLength,
    pub max_height: BoxLength,
    pub top: BoxLength,
    pub left: BoxLength,
    pub right: BoxLength,
    pub bottom: BoxLength,
    /// `[top, right, bottom, left]`; initial `[BoxLength::Px(0.0); 4]`
    /// (absent-edge default `unwrap_or(0.0)` in `box_edge_insets_for`). Authored
    /// `auto` is `BoxLength::Auto`, distinct from absent.
    pub margin: [BoxLength; 4],
    /// `[top, right, bottom, left]`; initial `[BoxLength::Px(0.0); 4]`
    /// (`auto` never arrives — the grammar rejects it).
    pub padding: [BoxLength; 4],
    /// `[top, right, bottom, left]`, px; initial `[0.0; 4]` (no percentages, no
    /// `auto` in CSS border-width).
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
            box_sizing: BoxSizingKw::ContentBox,
            width: BoxLength::Auto,
            height: BoxLength::Auto,
            min_width: BoxLength::Auto,
            min_height: BoxLength::Auto,
            max_width: BoxLength::Auto,
            max_height: BoxLength::Auto,
            top: BoxLength::Auto,
            left: BoxLength::Auto,
            right: BoxLength::Auto,
            bottom: BoxLength::Auto,
            margin: [BoxLength::Px(0.0); 4],
            padding: [BoxLength::Px(0.0); 4],
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
        assert_eq!(box_.box_sizing, BoxSizingKw::ContentBox);
        assert_eq!(box_.width, BoxLength::Auto);
        assert_eq!(box_.height, BoxLength::Auto);
        assert_eq!(box_.min_width, BoxLength::Auto);
        assert_eq!(box_.min_height, BoxLength::Auto);
        assert_eq!(box_.max_width, BoxLength::Auto);
        assert_eq!(box_.max_height, BoxLength::Auto);
        assert_eq!(box_.top, BoxLength::Auto);
        assert_eq!(box_.left, BoxLength::Auto);
        assert_eq!(box_.right, BoxLength::Auto);
        assert_eq!(box_.bottom, BoxLength::Auto);
        assert_eq!(box_.margin, [BoxLength::Px(0.0); 4]);
        assert_eq!(box_.padding, [BoxLength::Px(0.0); 4]);
        assert_eq!(box_.border_width, [0.0; 4]);
    }
}
