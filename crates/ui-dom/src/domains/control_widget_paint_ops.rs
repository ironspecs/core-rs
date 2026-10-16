//! Built-in control-chrome paint op accumulator for `ui-dom`.
//!
//! This file owns the paint-op accumulator that built-in control helpers fill
//! and append into a shared retained paint plan. Its purpose is to keep the
//! rect/icon op buckets small and explicit without owning control geometry or
//! DOM state decisions.
//!
//! Invariants and rules:
//!
//! - this file owns only the `ControlWidgetPaintOps` struct and its `impl` block
//! - the accumulator stays fixed-capacity (`ArrayVec`) and allocation-free
//! - this file does not read DOM state or choose disabled/active colors

use arrayvec::ArrayVec;

use crate::domains::layer_ops::{IconPaintOp, LayerPaintPlan, RectPaintOp};

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

#[cfg(test)]
mod tests {
    use ui_icons::UiIconId;

    use super::*;

    #[test]
    fn append_to_plan_extends_rects_and_icons_in_order() {
        let rect_op = RectPaintOp {
            position_px: [1.0, 2.0],
            size_px: [10.0, 4.0],
            fill_rgba: [0.1, 0.2, 0.3, 1.0],
            corner_radii_px: [2.0; 4],
            border_widths_px: [0.0; 4],
            border_top_rgba: [0.0; 4],
            border_right_rgba: [0.0; 4],
            border_bottom_rgba: [0.0; 4],
            border_left_rgba: [0.0; 4],
        };
        let icon_op = IconPaintOp {
            position_px: [3.0, 4.0],
            size_px: [8.0, 8.0],
            icon_id: UiIconId::Scene,
            color_rgba: [0.9, 0.9, 1.0, 1.0],
        };

        let mut ops = ControlWidgetPaintOps::default();
        ops.rects.push(rect_op);
        ops.icons.push(icon_op.clone());

        let mut plan = LayerPaintPlan::default();
        ops.append_to_plan(&mut plan);

        assert_eq!(plan.rects, vec![rect_op]);
        assert_eq!(plan.icons, vec![icon_op]);
    }
}
