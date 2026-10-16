//! Shared retained layer-op vocabulary for `ui-dom`.
//!
//! This module owns the retained op payloads shared between paint planning and
//! retained paint execution. It intentionally stays narrow and free of DOM
//! planning logic.

mod icon;
mod rect;
mod text_run;
mod text_run_signature;

pub(crate) use crate::domains::text_layout::{
    TextAlign, TextFontFamilyId, TextLanguageId, TextWrapMode,
};
pub use icon::IconPaintOp;
pub use rect::RectPaintOp;
pub(crate) use text_run::TextRunId;
pub use text_run::TextRunPaintOp;
pub(crate) use text_run_signature::TextRunSignature;

/// Stable retained SVG placeholder identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SvgPaintId(u64);

impl SvgPaintId {
    /// Derives one stable retained SVG placeholder identity from the authored icon name.
    #[cfg(test)]
    #[must_use]
    pub fn from_name(icon_name: &str) -> Self {
        Self(stable_placeholder_id(icon_name))
    }
}

/// Stable retained image placeholder identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImagePaintId(u64);

impl ImagePaintId {
    /// Derives one stable retained image placeholder identity from the authored image name.
    #[must_use]
    pub fn from_name(image_name: &str) -> Self {
        Self(stable_placeholder_id(image_name))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerPaintPlan {
    /// Retained background and border rect ops.
    pub rects: Vec<RectPaintOp>,
    /// Retained flat icon ops.
    pub icons: Vec<IconPaintOp>,
    /// Retained text-run placeholder ops.
    pub text_runs: Vec<TextRunPaintOp>,
    /// Retained stroke placeholder ops.
    pub stroke_paths: Vec<StrokePathOp>,
    /// Retained SVG placeholder ops.
    pub svgs: Vec<SvgPaintOp>,
    /// Retained image placeholder ops.
    pub images: Vec<ImagePaintOp>,
}

impl LayerPaintPlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
            && self.icons.is_empty()
            && self.text_runs.is_empty()
            && self.stroke_paths.is_empty()
            && self.svgs.is_empty()
            && self.images.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePathOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Placeholder stroke color.
    pub rgba: [f32; 4],
}

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

/// Flat retained image placeholder geometry and authored image identity.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePaintOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Stable retained image placeholder identity.
    pub image_id: ImagePaintId,
}

fn stable_placeholder_id(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use ui_icons::UiIconId;

    use super::{
        IconPaintOp, ImagePaintId, ImagePaintOp, LayerPaintPlan, StrokePathOp, SvgPaintId,
        SvgPaintOp,
    };

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

    #[test]
    fn layer_paint_plan_clone_preserves_op_order() {
        let plan = LayerPaintPlan {
            icons: vec![IconPaintOp {
                position_px: [1.0, 2.0],
                size_px: [9.0, 10.0],
                icon_id: UiIconId::Scene,
                color_rgba: [0.10, 0.11, 0.12, 1.0],
            }],
            svgs: vec![
                SvgPaintOp::fixture_icon(
                    [0.0, 0.0],
                    [8.0, 8.0],
                    SvgPaintId::from_name("first"),
                    [0.1, 0.2, 0.3, 1.0],
                ),
                SvgPaintOp::fixture_icon(
                    [8.0, 0.0],
                    [8.0, 8.0],
                    SvgPaintId::from_name("second"),
                    [0.4, 0.5, 0.6, 1.0],
                ),
            ],
            images: vec![ImagePaintOp {
                position_px: [2.0, 3.0],
                size_px: [4.0, 5.0],
                image_id: ImagePaintId::from_name("avatar.png"),
            }],
            stroke_paths: vec![StrokePathOp {
                position_px: [6.0, 7.0],
                size_px: [8.0, 9.0],
                rgba: [0.9, 0.8, 0.7, 0.6],
            }],
            ..LayerPaintPlan::default()
        };

        let clone = plan.clone();

        assert_eq!(clone, plan);
    }

    #[test]
    fn placeholder_ids_are_stable_for_matching_names() {
        assert_eq!(SvgPaintId::from_name("menu"), SvgPaintId::from_name("menu"));
        assert_eq!(
            ImagePaintId::from_name("avatar.png"),
            ImagePaintId::from_name("avatar.png"),
        );
    }
}
