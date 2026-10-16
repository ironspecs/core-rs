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
