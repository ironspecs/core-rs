//! The computed/used box length split (CSS Cascade §4.5, PLAN.md PR 1 item 7).
//!
//! This file owns the `BoxLength` enum: the computed form of the box-group
//! length fields. It is the one type that bridges the computed value (which
//! keeps authored `auto` and percentages symbolic) and the used value (which
//! resolves them against a containing-block basis at layout time). It is plain
//! `Copy`, value-compared data — the S1 group invariants hold; resolution lives
//! in the layout-facing readers, never here.
//!
//! Invariants and rules:
//!
//! - **`Auto` is the initial/default** and carries exactly the old `None`
//!   semantics for the `width`/`height`/min/max/top/left/right/bottom fields:
//!   the authored keyword `auto` and the CSS-wide `unset` both resolve to it.
//! - **`Px` is style-resolved**: the declaration resolver converts `px` and
//!   `em` (against the element's own font size) before a `BoxLength::Px` is
//!   ever stored in the group.
//! - **`Percent` stays symbolic into the group.** It resolves only at layout
//!   time against the containing block (`resolve`/`resolve_or`), because the
//!   containing block is not known at computed-value time.
//! - The `usize → f32` conversion rule does not apply: the basis is already
//!   `f32`, so `resolve` is pure arithmetic, allocation-free.

/// A box length in computed form (CSS Cascade §4.5): `Auto` keeps the authored
/// keyword, `Px` is style-resolved, `Percent` stays symbolic for layout-time
/// resolution against the containing block.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BoxLength {
    /// The authored `auto` keyword (or `unset`, for the optional-size fields).
    #[default]
    Auto,
    /// A style-resolved pixel length.
    Px(f32),
    /// A symbolic percentage, resolved against the containing block at layout.
    Percent(f32),
}

impl BoxLength {
    /// A resolved pixel length (the `Px` construction used by tests and the
    /// declaration resolver's style-resolved writes).
    #[must_use]
    pub fn px(pixels: f32) -> Self {
        Self::Px(pixels)
    }

    /// A symbolic percentage (the declaration form `Percent` keeps symbolic).
    #[must_use]
    pub fn percent(value: f32) -> Self {
        Self::Percent(value)
    }

    /// Resolves against the containing-block basis: `Auto → None`, `Px → Some`,
    /// `Percent → Some(basis_px × pct / 100)`.
    #[must_use]
    pub fn resolve(self, basis_px: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(px) => Some(px),
            Self::Percent(pct) => Some(basis_px * pct / 100.0),
        }
    }

    /// Resolves against the containing-block basis with a default for `Auto`.
    #[must_use]
    pub fn resolve_or(self, basis_px: f32, default: f32) -> f32 {
        self.resolve(basis_px).unwrap_or(default)
    }

    /// Resolves the `height` property against an optional containing-block
    /// height basis (CSS 2.1 §10.5): `Auto → None` (auto), `Px → Some(px)`,
    /// and `Percent → Some(basis_px × pct / 100)` only when the basis is
    /// `Some`. A percentage against an indefinite basis (`None`) resolves to
    /// `None` (auto), so the box falls back to its content height instead of
    /// collapsing to 0.
    #[must_use]
    pub fn resolve_height(self, basis_px: Option<f32>) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(px) => Some(px),
            Self::Percent(pct) => basis_px.map(|basis_px| basis_px * pct / 100.0),
        }
    }

    /// Resolves the `max-height` property against an optional containing-block
    /// height basis (CSS 2.1 §10.7): `Auto → None` (no maximum), `Px →
    /// Some(px)`, and `Percent → Some(basis_px × pct / 100)` only when the
    /// basis is `Some`. A percentage against an indefinite basis (`None`)
    /// resolves to `None`, so no maximum is applied.
    #[must_use]
    pub fn resolve_max_height(self, basis_px: Option<f32>) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(px) => Some(px),
            Self::Percent(pct) => basis_px.map(|basis_px| basis_px * pct / 100.0),
        }
    }

    /// Resolves the `min-height` property against an optional containing-block
    /// height basis (CSS 2.1 §10.7): `Auto → Some(0.0)` (CSS Sizing 3 computes
    /// `auto` to 0 for a block), `Px → Some(px)`, and `Percent →
    /// Some(basis_px × pct / 100)` when the basis is `Some`. A percentage
    /// against an indefinite basis (`None`) resolves to `Some(0.0)` — §10.7's
    /// explicit 0, distinct from `height`'s auto rule.
    #[must_use]
    pub fn resolve_min_height(self, basis_px: Option<f32>) -> Option<f32> {
        match self {
            Self::Auto => Some(0.0),
            Self::Px(px) => Some(px),
            Self::Percent(pct) => Some(basis_px.map_or(0.0, |basis_px| basis_px * pct / 100.0)),
        }
    }

    /// Resolves a length against an optional basis for the offset fields
    /// (`top`/`bottom`): `Auto → None`, `Px → Some(px)`, and `Percent →
    /// Some(basis_px × pct / 100)` only when the basis is `Some`. This is the
    /// temporary bridge for the relative-offset path: a percentage with no
    /// basis defers (resolves to `None`, i.e. no offset) rather than
    /// manufacturing a 0.0 basis. Offset-time resolution against the laid-out
    /// containing block is a follow-up (see
    /// `relative_top_percentage_defers_to_offset_time` in the layout proofs).
    #[must_use]
    pub fn resolve_optional(self, basis_px: Option<f32>) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(px) => Some(px),
            Self::Percent(pct) => basis_px.map(|basis_px| basis_px * pct / 100.0),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // box-length resolution asserts exact literals
mod tests {
    use super::*;

    #[test]
    fn resolve_maps_auto_px_and_percent() {
        assert_eq!(BoxLength::Auto.resolve(200.0), None);
        assert_eq!(BoxLength::Px(20.0).resolve(200.0), Some(20.0));
        assert_eq!(BoxLength::Percent(50.0).resolve(200.0), Some(100.0));
        assert_eq!(BoxLength::Percent(10.0).resolve(200.0), Some(20.0));
        assert_eq!(BoxLength::Percent(-10.0).resolve(200.0), Some(-20.0));
    }

    #[test]
    fn resolve_or_substitutes_default_for_auto() {
        assert_eq!(BoxLength::Auto.resolve_or(200.0, 0.0), 0.0);
        assert_eq!(BoxLength::Px(20.0).resolve_or(200.0, 0.0), 20.0);
        assert_eq!(BoxLength::Percent(50.0).resolve_or(200.0, 0.0), 100.0);
    }

    #[test]
    fn resolve_height_maps_auto_px_and_percent_against_indefinite_basis() {
        assert_eq!(BoxLength::Auto.resolve_height(None), None);
        assert_eq!(BoxLength::Auto.resolve_height(Some(200.0)), None);
        assert_eq!(BoxLength::Px(20.0).resolve_height(None), Some(20.0));
        assert_eq!(BoxLength::Px(20.0).resolve_height(Some(200.0)), Some(20.0));
        assert_eq!(
            BoxLength::Percent(50.0).resolve_height(Some(200.0)),
            Some(100.0)
        );
        assert_eq!(BoxLength::Percent(50.0).resolve_height(None), None);
    }

    #[test]
    fn resolve_max_height_maps_auto_px_and_percent_against_indefinite_basis() {
        assert_eq!(BoxLength::Auto.resolve_max_height(None), None);
        assert_eq!(BoxLength::Auto.resolve_max_height(Some(200.0)), None);
        assert_eq!(BoxLength::Px(20.0).resolve_max_height(None), Some(20.0));
        assert_eq!(
            BoxLength::Px(20.0).resolve_max_height(Some(200.0)),
            Some(20.0)
        );
        assert_eq!(
            BoxLength::Percent(50.0).resolve_max_height(Some(200.0)),
            Some(100.0)
        );
        assert_eq!(BoxLength::Percent(50.0).resolve_max_height(None), None);
    }

    #[test]
    fn resolve_min_height_maps_auto_to_zero_and_percent_to_zero_when_indefinite() {
        assert_eq!(BoxLength::Auto.resolve_min_height(None), Some(0.0));
        assert_eq!(BoxLength::Auto.resolve_min_height(Some(200.0)), Some(0.0));
        assert_eq!(BoxLength::Px(20.0).resolve_min_height(None), Some(20.0));
        assert_eq!(
            BoxLength::Px(20.0).resolve_min_height(Some(200.0)),
            Some(20.0)
        );
        assert_eq!(
            BoxLength::Percent(50.0).resolve_min_height(Some(200.0)),
            Some(100.0)
        );
        assert_eq!(BoxLength::Percent(50.0).resolve_min_height(None), Some(0.0));
    }

    #[test]
    fn resolve_optional_maps_auto_px_and_percent_against_indefinite_basis() {
        assert_eq!(BoxLength::Auto.resolve_optional(None), None);
        assert_eq!(BoxLength::Auto.resolve_optional(Some(200.0)), None);
        assert_eq!(BoxLength::Px(20.0).resolve_optional(None), Some(20.0));
        assert_eq!(
            BoxLength::Px(20.0).resolve_optional(Some(200.0)),
            Some(20.0)
        );
        assert_eq!(
            BoxLength::Percent(50.0).resolve_optional(Some(200.0)),
            Some(100.0)
        );
        assert_eq!(BoxLength::Percent(50.0).resolve_optional(None), None);
    }

    #[test]
    fn default_is_auto() {
        assert_eq!(BoxLength::default(), BoxLength::Auto);
    }

    #[test]
    fn constructors_match_variants() {
        assert_eq!(BoxLength::px(12.0), BoxLength::Px(12.0));
        assert_eq!(BoxLength::percent(25.0), BoxLength::Percent(25.0));
    }
}
