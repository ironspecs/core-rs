#![allow(dead_code)]

//! Shared layout-pass propagation result for `ui-dom`.
//!
//! This file owns the small result value that a layout pass returns to signal
//! whether a subtree's size change must propagate to its parent layer. Its
//! purpose is to keep the propagation decision explicit and copyable without
//! owning layout traversal policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `LayoutPassResult` struct and its `impl` block
//! - the result stays a plain `Copy` struct with two boolean flags

#[must_use]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LayoutPassResult {
    pub propagate_width_to_parent_layer: bool,
    pub propagate_height_to_parent_layer: bool,
}

impl LayoutPassResult {
    #[must_use]
    pub(crate) const fn should_propagate_to_parent_layer(self) -> bool {
        self.propagate_width_to_parent_layer || self.propagate_height_to_parent_layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_propagate_reports_only_when_an_axis_propagates() {
        let neither = LayoutPassResult {
            propagate_width_to_parent_layer: false,
            propagate_height_to_parent_layer: false,
        };
        assert!(!neither.should_propagate_to_parent_layer());

        let width = LayoutPassResult {
            propagate_width_to_parent_layer: true,
            propagate_height_to_parent_layer: false,
        };
        assert!(width.should_propagate_to_parent_layer());

        let height = LayoutPassResult {
            propagate_width_to_parent_layer: false,
            propagate_height_to_parent_layer: true,
        };
        assert!(height.should_propagate_to_parent_layer());

        let both = LayoutPassResult {
            propagate_width_to_parent_layer: true,
            propagate_height_to_parent_layer: true,
        };
        assert!(both.should_propagate_to_parent_layer());
    }
}
