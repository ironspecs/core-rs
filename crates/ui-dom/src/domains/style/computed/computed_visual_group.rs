//! Non-inherited visual computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedVisualGroup` struct: the computed values of the
//! non-inherited paint-facing longhands (background, border colors, radius,
//! outline, caret color, opacity). It is plain `Copy` data plus `initial()`;
//! resolution lives in the step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - every field's absent default mirrors what today's paint/composite
//!   consumers actually observe; citations are on each field below
//! - `background_rgba`/`border_rgba`/`outline_rgba` are **transparent** when
//!   absent, not "absent": paint emits a fill/border op with alpha 0 rather
//!   than skipping the op (`domains/paint/mod.rs::add_element_paint_ops`,
//!   `outline_rect_paint_op`)
//! - `caret_rgba` is `Option<[f32; 4]>` because today's caret consumer
//!   *distinguishes* absent: absent `caret-color` falls back to the element's
//!   resolved `color` (currentColor), not to transparent
//!   (`src/types/element_behaviors/paint.rs::append_text_entry_caret_rect`).
//!   `None` = `auto`/currentColor (use the color group's `color_rgba` at paint
//!   time); `Some` = an authored caret color.

/// Computed values of the non-inherited visual longhands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedVisualGroup {
    /// Transparent `[0.0; 4]` when absent (`domains/paint/mod.rs::
    /// add_element_paint_ops` reads `visual.background_rgba` into `fill_rgba =
    /// [0.0;4]`, not a skipped op).
    pub background_rgba: [f32; 4],
    /// Per side `[top, right, bottom, left]`; each transparent `[0.0; 4]` when
    /// absent (`domains/paint/mod.rs::add_element_paint_ops` reads
    /// `visual.border_rgba[i]` per side).
    pub border_rgba: [[f32; 4]; 4],
    /// `[TL, TR, BR, BL]` px; initial `[0.0; 4]` (absent radius;
    /// `domains/paint/mod.rs::border_radii_px_for_node`).
    pub radius: [f32; 4],
    /// Transparent `[0.0; 4]` when absent (`outline_rect_paint_op`).
    pub outline_rgba: [f32; 4],
    /// `0.0` when absent (`outline_rect_paint_op`'s `unwrap_or(0.0)`).
    pub outline_width: f32,
    /// `0.0` when absent (`outline_rect_paint_op`'s `unwrap_or(0.0)`).
    pub outline_offset: f32,
    /// `None` = `auto`/currentColor (fall back to the color group's
    /// `color_rgba`); `Some` = an authored caret color. Today's consumer
    /// distinguishes absent (`element_behaviors/paint.rs::append_text_entry_caret_rect`).
    pub caret_rgba: Option<[f32; 4]>,
    /// `1.0` when absent (`domains/composite/mod.rs::node_opacity`).
    pub opacity: f32,
}

impl ComputedVisualGroup {
    /// The no-parent visual seed: every field at its today-observed default.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            background_rgba: [0.0; 4],
            border_rgba: [[0.0; 4]; 4],
            radius: [0.0; 4],
            outline_rgba: [0.0; 4],
            outline_width: 0.0,
            outline_offset: 0.0,
            caret_rgba: None,
            opacity: 1.0,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_matches_todays_absent_defaults() {
        let visual = ComputedVisualGroup::initial();
        assert_eq!(visual.background_rgba, [0.0; 4]);
        assert_eq!(visual.border_rgba, [[0.0; 4]; 4]);
        assert_eq!(visual.radius, [0.0; 4]);
        assert_eq!(visual.outline_rgba, [0.0; 4]);
        assert_eq!(visual.outline_width, 0.0);
        assert_eq!(visual.outline_offset, 0.0);
        assert_eq!(visual.caret_rgba, None);
        assert_eq!(visual.opacity, 1.0);
    }
}
