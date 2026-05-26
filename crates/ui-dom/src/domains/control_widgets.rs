//! Pure paint helpers for built-in input control chrome.
//!
//! This module deliberately receives final geometry and visual values. It does
//! not read DOM state, inspect pseudo states, or choose disabled/active colors.

use arrayvec::ArrayVec;

use crate::domains::layer_ops::{IconPaintOp, LayerPaintPlan, RectPaintOp};
use crate::types::DomRect;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ControlWidgetPaintOps {
    pub(crate) rects: ArrayVec<RectPaintOp, 8>,
    pub(crate) icons: ArrayVec<IconPaintOp, 4>,
}

impl ControlWidgetPaintOps {
    pub(crate) fn append_to_plan(self, plan: &mut LayerPaintPlan) {
        plan.rects.extend(self.rects);
        plan.icons.extend(self.icons);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RangeBarSpec {
    pub(crate) base_track_rect: DomRect,
    pub(crate) filled_track_rect: DomRect,
    pub(crate) thumb_rect: DomRect,
    pub(crate) base_track_rgba: [f32; 4],
    pub(crate) filled_track_rgba: [f32; 4],
    pub(crate) thumb_rgba: [f32; 4],
    pub(crate) track_corner_radii_px: [f32; 4],
    pub(crate) thumb_corner_radii_px: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CheckboxMarkSpec {
    pub(crate) outer_rect: DomRect,
    pub(crate) mark_rect: Option<DomRect>,
    pub(crate) background_rgba: [f32; 4],
    pub(crate) mark_rgba: [f32; 4],
    pub(crate) border_widths_px: [f32; 4],
    pub(crate) border_top_rgba: [f32; 4],
    pub(crate) border_right_rgba: [f32; 4],
    pub(crate) border_bottom_rgba: [f32; 4],
    pub(crate) border_left_rgba: [f32; 4],
    pub(crate) corner_radii_px: [f32; 4],
    pub(crate) mark_corner_radii_px: [f32; 4],
}

#[must_use]
pub(crate) fn range_bar_ops(spec: RangeBarSpec) -> ControlWidgetPaintOps {
    let mut ops = ControlWidgetPaintOps::default();
    ops.rects.push(solid_rect(
        spec.base_track_rect,
        spec.base_track_rgba,
        spec.track_corner_radii_px,
    ));
    ops.rects.push(solid_rect(
        spec.filled_track_rect,
        spec.filled_track_rgba,
        spec.track_corner_radii_px,
    ));
    ops.rects.push(solid_rect(
        spec.thumb_rect,
        spec.thumb_rgba,
        spec.thumb_corner_radii_px,
    ));
    ops
}

#[must_use]
pub(crate) fn checkbox_mark_ops(spec: CheckboxMarkSpec) -> ControlWidgetPaintOps {
    let mut ops = ControlWidgetPaintOps::default();
    ops.rects.push(RectPaintOp {
        position_px: [spec.outer_rect.left_px, spec.outer_rect.top_px],
        size_px: [
            spec.outer_rect.width_px.max(0.0),
            spec.outer_rect.height_px.max(0.0),
        ],
        fill_rgba: spec.background_rgba,
        corner_radii_px: spec.corner_radii_px,
        border_widths_px: spec.border_widths_px,
        border_top_rgba: spec.border_top_rgba,
        border_right_rgba: spec.border_right_rgba,
        border_bottom_rgba: spec.border_bottom_rgba,
        border_left_rgba: spec.border_left_rgba,
    });
    if let Some(mark_rect) = spec.mark_rect {
        ops.rects.push(solid_rect(
            mark_rect,
            spec.mark_rgba,
            spec.mark_corner_radii_px,
        ));
    }
    ops
}

fn solid_rect(rect: DomRect, rgba: [f32; 4], corner_radii_px: [f32; 4]) -> RectPaintOp {
    RectPaintOp {
        position_px: [rect.left_px, rect.top_px],
        size_px: [rect.width_px.max(0.0), rect.height_px.max(0.0)],
        fill_rgba: rgba,
        corner_radii_px,
        border_widths_px: [0.0; 4],
        border_top_rgba: [0.0; 4],
        border_right_rgba: [0.0; 4],
        border_bottom_rgba: [0.0; 4],
        border_left_rgba: [0.0; 4],
    }
}

#[cfg(test)]
mod tests {
    use crate::types::DomRect;

    use super::{CheckboxMarkSpec, RangeBarSpec, checkbox_mark_ops, range_bar_ops};

    fn rect(left_px: f32, top_px: f32, width_px: f32, height_px: f32) -> DomRect {
        DomRect {
            left_px,
            top_px,
            width_px,
            height_px,
        }
    }

    fn assert_f32_array_eq<const N: usize>(actual: [f32; N], expected: [f32; N]) {
        for index in 0..N {
            assert!(
                (actual[index] - expected[index]).abs() <= f32::EPSILON,
                "expected {expected:?}, got {actual:?}",
            );
        }
    }

    #[test]
    fn range_bar_ops_emit_base_fill_and_thumb_rects() {
        let ops = range_bar_ops(RangeBarSpec {
            base_track_rect: rect(0.0, 4.0, 100.0, 4.0),
            filled_track_rect: rect(0.0, 4.0, 40.0, 4.0),
            thumb_rect: rect(35.0, 0.0, 12.0, 12.0),
            base_track_rgba: [0.1, 0.1, 0.1, 1.0],
            filled_track_rgba: [0.2, 0.4, 0.8, 1.0],
            thumb_rgba: [0.9, 0.9, 1.0, 1.0],
            track_corner_radii_px: [2.0; 4],
            thumb_corner_radii_px: [6.0; 4],
        });

        assert_eq!(ops.rects.len(), 3);
        assert_f32_array_eq(ops.rects[1].size_px, [40.0, 4.0]);
        assert_f32_array_eq(ops.rects[2].corner_radii_px, [6.0; 4]);
    }

    #[test]
    fn checkbox_mark_ops_omit_mark_when_mark_rect_is_absent() {
        let ops = checkbox_mark_ops(CheckboxMarkSpec {
            outer_rect: rect(0.0, 0.0, 16.0, 16.0),
            mark_rect: None,
            background_rgba: [0.0; 4],
            mark_rgba: [1.0; 4],
            border_widths_px: [1.0; 4],
            border_top_rgba: [0.5, 0.5, 0.5, 1.0],
            border_right_rgba: [0.5, 0.5, 0.5, 1.0],
            border_bottom_rgba: [0.5, 0.5, 0.5, 1.0],
            border_left_rgba: [0.5, 0.5, 0.5, 1.0],
            corner_radii_px: [3.0; 4],
            mark_corner_radii_px: [2.0; 4],
        });

        assert_eq!(ops.rects.len(), 1);
        assert_f32_array_eq(ops.rects[0].border_widths_px, [1.0; 4]);
    }
}
