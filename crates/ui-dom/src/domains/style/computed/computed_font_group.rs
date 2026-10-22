//! Inherited font computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedFontGroup` struct: the computed values of the
//! inherited `font-size`, `line-height`, `font-family`, `font-weight`, and
//! `font-style` longhands. It is plain `Copy` data plus `initial()` and one
//! tiny field helper; all resolution (A5 inheritance, A6 em resolution, and the
//! `bolder`/`lighter` parent-relative weight resolution) lives in the step-3.3
//! resolver, never here.
//!
//! Invariants and rules:
//!
//! - `size_px` is the resolved pixel font size; `initial()` uses the crate's
//!   shared `DEFAULT_TEXT_FONT_SIZE_PX` (14.0) so the computed default matches
//!   today's absent-`font-size` paint/layout default
//! - `line_height_px == None` means `normal` (no explicit line box); a resolved
//!   length is floored at 1px by the resolver, not here. The floor is
//!   deliberate: cosmic-text asserts `line_height != 0.0` (`buffer.rs:254`),
//!   so a zero line height cannot reach the shaper — an engine-dependency
//!   constraint, not a grammar choice (see the off-spec registry)
//! - `family == None` means the engine default (no authored family); `Some`
//!   carries a `TextFamilyListKey` into the owning `DeclStore`'s family-list
//!   arena (text seam S2 — the structured list replaces the former verbatim
//!   `TextKey`)
//! - `weight` is the numeric weight (400 normal, 700 bold); `bolder`/`lighter`
//!   are resolved against the parent weight at compute time and never stored
//! - `style` holds exactly `Keyword::Normal | Keyword::Italic`

use crate::domains::style::Keyword;
use crate::domains::style::TextFamilyListKey;
use crate::domains::text_layout::DEFAULT_TEXT_FONT_SIZE_PX;

/// Computed values of the inherited font longhands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedFontGroup {
    /// Resolved pixel font size; `initial()` mirrors the crate default 14.0px.
    pub size_px: f32,
    /// `None` = `normal`; otherwise the resolved line box height in px.
    pub line_height_px: Option<f32>,
    /// `None` = engine default family; otherwise an interned family-list key.
    pub family: Option<TextFamilyListKey>,
    /// Numeric font weight (400 normal, 700 bold).
    pub weight: u16,
    /// `Keyword::Normal | Keyword::Italic` (the admitted font-style set).
    pub style: Keyword,
}

impl ComputedFontGroup {
    /// The no-parent font seed: default size, `normal` line height, engine
    /// default family, weight 400, normal style.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            size_px: DEFAULT_TEXT_FONT_SIZE_PX,
            line_height_px: None,
            family: None,
            weight: 400,
            style: Keyword::Normal,
        }
    }

    /// The resolved line height, falling back to `default` when `normal`.
    #[must_use]
    pub fn line_height_or(&self, default: f32) -> f32 {
        self.line_height_px.unwrap_or(default)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_matches_crate_defaults() {
        let font = ComputedFontGroup::initial();
        assert_eq!(font.size_px, DEFAULT_TEXT_FONT_SIZE_PX);
        assert_eq!(font.line_height_px, None);
        assert_eq!(font.family, None);
        assert_eq!(font.weight, 400);
        assert_eq!(font.style, Keyword::Normal);
    }

    #[test]
    fn line_height_or_uses_resolved_value_then_fallback() {
        let mut font = ComputedFontGroup::initial();
        font.line_height_px = Some(28.0);
        assert_eq!(font.line_height_or(0.0), 28.0);

        font.line_height_px = None;
        assert_eq!(font.line_height_or(16.0), 16.0);
    }
}
