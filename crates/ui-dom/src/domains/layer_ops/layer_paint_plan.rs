//! Retained layer paint plan for `ui-dom`.
//!
//! This file owns the flat retained op vectors (rects, icons, text runs,
//! strokes, SVGs, images) collected during a paint pass. Its purpose is to
//! keep the plan a plain data carrier with a single emptiness check, free of
//! layout or rasterization logic.
//!
//! Invariants and rules:
//!
//! - this file owns only the `LayerPaintPlan` struct and its `impl` block
//! - op order is retained; `is_empty` is true only when every op vector is
//!   empty

use super::{IconPaintOp, ImagePaintOp, RectPaintOp, StrokePathOp, SvgPaintOp, TextRunPaintOp};

#[cfg(test)]
use crate::domains::layer_ops::{ImagePaintId, SvgPaintId};

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

#[cfg(test)]
mod tests {
    use ui_icons::UiIconId;

    use super::*;

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
}
