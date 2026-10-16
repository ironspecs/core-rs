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
