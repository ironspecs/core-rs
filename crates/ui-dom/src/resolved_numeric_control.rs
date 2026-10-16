//! Resolved numeric control state for `ui-dom`.
//!
//! This file owns the resolved numeric state (`kind`, current/min/max/step
//! values) for number and range inputs, plus the value math (clamping,
//! snapping, stepping, fraction mapping) shared by both. Its purpose is to
//! keep numeric editing behavior explicit and testable without owning
//! hit-testing or paint geometry.
//!
//! Invariants and rules:
//!
//! - this file owns only the `ResolvedNumericControl` struct and its `impl`
//!   block
//! - every surfaced value string clamps into `[minimum_value_px,
//!   maximum_value_px]` and snaps to the authored step before it is returned

use crate::element_controls::{
    DEFAULT_RANGE_MAX_VALUE, DEFAULT_RANGE_MIN_VALUE, NumericControlKind, NumericStepDirection,
    numeric_value_string, parsed_positive_step_attr, parsed_string_attr,
};
use crate::types::DomAttribute;
use crate::types::dom_node::DomElementNode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedNumericControl {
    kind: NumericControlKind,
    current_value: Option<f32>,
    min_value: Option<f32>,
    max_value: Option<f32>,
    step_value: f32,
}

impl ResolvedNumericControl {
    #[must_use]
    pub(crate) fn new(element: &DomElementNode, kind: NumericControlKind) -> Self {
        Self {
            kind,
            current_value: parsed_string_attr(element, &DomAttribute::Value),
            min_value: parsed_string_attr(element, &DomAttribute::Min),
            max_value: parsed_string_attr(element, &DomAttribute::Max),
            step_value: parsed_positive_step_attr(element),
        }
    }

    #[must_use]
    pub(crate) fn display_value_px(&self) -> f32 {
        self.clamped_value_px()
    }

    #[must_use]
    pub(crate) fn stepped_value_string(&self, direction: NumericStepDirection) -> String {
        let next_value = match direction {
            NumericStepDirection::Decrement => self.clamped_value_px() - self.step_value,
            NumericStepDirection::Increment => self.clamped_value_px() + self.step_value,
        };
        numeric_value_string(self.snap_and_clamp_value_px(next_value))
    }

    #[must_use]
    pub(crate) fn keyboard_step_value_string(&self, key: &str) -> Option<String> {
        match key {
            "ArrowDown" | "ArrowLeft" => {
                Some(self.stepped_value_string(NumericStepDirection::Decrement))
            }
            "ArrowUp" | "ArrowRight" => {
                Some(self.stepped_value_string(NumericStepDirection::Increment))
            }
            "Home" if self.kind == NumericControlKind::Range => {
                Some(numeric_value_string(self.minimum_value_px()))
            }
            "End" if self.kind == NumericControlKind::Range => {
                Some(numeric_value_string(self.maximum_value_px()))
            }
            "PageDown" if self.kind == NumericControlKind::Range => Some(numeric_value_string(
                self.snap_and_clamp_value_px(self.clamped_value_px() - self.step_value * 10.0),
            )),
            "PageUp" if self.kind == NumericControlKind::Range => Some(numeric_value_string(
                self.snap_and_clamp_value_px(self.clamped_value_px() + self.step_value * 10.0),
            )),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn value_string_from_fraction(&self, fraction: f32) -> String {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let raw_value = minimum_value_px + (maximum_value_px - minimum_value_px) * fraction;
        numeric_value_string(self.snap_and_clamp_value_px(raw_value))
    }

    #[must_use]
    pub(crate) fn value_fraction(&self) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let span_px = maximum_value_px - minimum_value_px;
        if span_px <= f32::EPSILON {
            return 0.0;
        }
        ((self.clamped_value_px() - minimum_value_px) / span_px).clamp(0.0, 1.0)
    }

    #[must_use]
    fn clamped_value_px(&self) -> f32 {
        self.snap_and_clamp_value_px(
            self.current_value
                .unwrap_or_else(|| self.default_value_px()),
        )
    }

    #[must_use]
    fn default_value_px(&self) -> f32 {
        match self.kind {
            NumericControlKind::Number => self.min_value.unwrap_or(0.0),
            NumericControlKind::Range => {
                let minimum_value_px = self.minimum_value_px();
                minimum_value_px + (self.maximum_value_px() - minimum_value_px) * 0.5
            }
        }
    }

    #[must_use]
    fn minimum_value_px(&self) -> f32 {
        match self.kind {
            NumericControlKind::Number => self.min_value.unwrap_or(f32::NEG_INFINITY),
            NumericControlKind::Range => self.min_value.unwrap_or(DEFAULT_RANGE_MIN_VALUE),
        }
    }

    #[must_use]
    fn maximum_value_px(&self) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let fallback_maximum_value_px = match self.kind {
            NumericControlKind::Number => f32::INFINITY,
            NumericControlKind::Range => DEFAULT_RANGE_MAX_VALUE,
        };
        self.max_value
            .unwrap_or(fallback_maximum_value_px)
            .max(minimum_value_px)
    }

    #[must_use]
    fn snap_and_clamp_value_px(&self, value_px: f32) -> f32 {
        let minimum_value_px = self.minimum_value_px();
        let maximum_value_px = self.maximum_value_px();
        let step_base_px = self.min_value.unwrap_or(0.0);
        let snapped_value_px =
            ((value_px - step_base_px) / self.step_value).round() * self.step_value + step_base_px;
        snapped_value_px.clamp(minimum_value_px, maximum_value_px)
    }
}
