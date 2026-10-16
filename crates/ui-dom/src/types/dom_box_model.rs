//! Shared box-model geometry for `ui-dom`.
//!
//! This file owns the geometry-only box snapshot that moves layout results into
//! paint, hit testing, and presentation. Its purpose is to keep the border box,
//! margin, border, and padding facts small, explicit, and reusable without
//! embedding layout policy into the shape itself.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomBoxModel` struct and its `impl` blocks
//! - values here are pure geometry, not business logic
//! - the type must stay cheap to copy and easy to inspect in tests

use serde::{Deserialize, Serialize};

use crate::types::box_model::BoxEdgeInsets;
use crate::types::primitives::DomRect;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DomBoxModel {
    pub border_box: DomRect,
    pub margin: BoxEdgeInsets,
    pub border: BoxEdgeInsets,
    pub padding: BoxEdgeInsets,
}

impl DomBoxModel {
    /// Creates one geometry-only box model that paint can interpret later.
    ///
    /// This constructor exists so callers can build an explicit box snapshot
    /// without depending on layout internals. It is intended for shared geometry
    /// setup, not for hiding layout policy inside the type.
    #[must_use]
    pub fn new(
        border_box: DomRect,
        margin: BoxEdgeInsets,
        border: BoxEdgeInsets,
        padding: BoxEdgeInsets,
    ) -> Self {
        Self {
            border_box,
            margin,
            border,
            padding,
        }
    }

    /// Provides the zero/default geometry state for nodes without effective layout.
    ///
    /// This is the canonical empty box used before layout has assigned real
    /// geometry or when a node has no effective size. Callers should prefer it
    /// over inventing ad hoc zero rectangles.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_model_zero_and_new_report_expected_edges() {
        let zero = DomBoxModel::zero();
        assert_eq!(zero.border_box, DomRect::default());
        assert_eq!(zero.margin, BoxEdgeInsets::default());
        assert_eq!(zero.border, BoxEdgeInsets::default());
        assert_eq!(zero.padding, BoxEdgeInsets::default());

        let border_box = DomRect {
            left_px: 1.0,
            top_px: 2.0,
            width_px: 100.0,
            height_px: 40.0,
        };
        let margin = BoxEdgeInsets {
            left_px: 4.0,
            top_px: 5.0,
            right_px: 6.0,
            bottom_px: 7.0,
        };
        let border = BoxEdgeInsets {
            left_px: 1.0,
            top_px: 1.0,
            right_px: 1.0,
            bottom_px: 1.0,
        };
        let padding = BoxEdgeInsets {
            left_px: 8.0,
            top_px: 9.0,
            right_px: 10.0,
            bottom_px: 11.0,
        };

        let authored = DomBoxModel::new(border_box, margin, border, padding);
        assert_eq!(authored.border_box, border_box);
        assert_eq!(authored.margin, margin);
        assert_eq!(authored.border, border);
        assert_eq!(authored.padding, padding);
    }
}
