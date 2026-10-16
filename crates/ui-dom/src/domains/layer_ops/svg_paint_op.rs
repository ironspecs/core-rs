//! Flat retained SVG paint op for `ui-dom`.
//!
//! This file owns the retained SVG placeholder geometry and authored icon
//! identity lowered into the paint plan. Its purpose is to keep the SVG op
//! payload flat and shader-oriented without owning rasterization or
//! layer-pool policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `SvgPaintOp` struct and its `impl` block
//! - `fixture_icon` is test-only and lives behind `#[cfg(test)]`

use super::SvgPaintId;

/// Flat retained SVG placeholder geometry and authored icon identity.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgPaintOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Stable retained SVG placeholder identity.
    pub svg_id: SvgPaintId,
    /// Placeholder fill color for the future SVG draw.
    pub color_rgba: [f32; 4],
}

impl SvgPaintOp {
    #[cfg(test)]
    #[must_use]
    pub fn fixture_icon(
        position_px: [f32; 2],
        size_px: [f32; 2],
        svg_id: SvgPaintId,
        color_rgba: [f32; 4],
    ) -> Self {
        Self {
            position_px,
            size_px,
            svg_id,
            color_rgba,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_vec2_eq(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            (actual[0] - expected[0]).abs() < f32::EPSILON
                && (actual[1] - expected[1]).abs() < f32::EPSILON,
            "expected {expected:?}, got {actual:?}"
        );
    }

    fn assert_vec4_eq(actual: [f32; 4], expected: [f32; 4]) {
        assert!(
            (actual[0] - expected[0]).abs() < f32::EPSILON
                && (actual[1] - expected[1]).abs() < f32::EPSILON
                && (actual[2] - expected[2]).abs() < f32::EPSILON
                && (actual[3] - expected[3]).abs() < f32::EPSILON,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn fixture_icon_preserves_requested_fields() {
        let op = SvgPaintOp::fixture_icon(
            [3.0, 4.0],
            [20.0, 10.0],
            SvgPaintId::from_name("menu"),
            [0.1, 0.2, 0.3, 0.4],
        );

        assert_eq!(op.svg_id, SvgPaintId::from_name("menu"));
        assert_vec4_eq(op.color_rgba, [0.1, 0.2, 0.3, 0.4]);
        assert_vec2_eq(op.position_px, [3.0, 4.0]);
        assert_vec2_eq(op.size_px, [20.0, 10.0]);
    }
}
