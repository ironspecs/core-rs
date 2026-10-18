//! Non-inherited transform computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedTransformGroup` struct: the computed value of
//! the single-purpose `transform` longhand. It is plain `Copy` data plus
//! `initial()`; resolution lives in the step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - **Single-purpose group, per S1.1**: transform is paint-class (never
//!   layout) *and* layer-membership-relevant, so it cannot live in the box
//!   group (whose class is layout+paint) without changing today's dirty
//!   classes. Its A7 diff row is paint + layer-membership re-evaluation.
//! - **`has_transform` mirrors today's layer rule verbatim.** Today a node has
//!   an unclipped layer when its transform attribute is present and `!= "none"`
//!   (`domains/layout/style.rs::node_has_unclipped_layer`), so even
//!   `translateX(0)` creates a layer. `has_transform` is `true` for any
//!   non-`none` transform declaration, regardless of the resolved translate.
//! - `translate_px` is the resolved `[x, y]` px translate (initial `[0.0, 0.0]`).

/// Computed value of the single-purpose `transform` longhand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedTransformGroup {
    /// Resolved `[x, y]` px translate.
    pub translate_px: [f32; 2],
    /// `true` when a transform declaration other than `none` is present,
    /// mirroring `domains/layout/style.rs::node_has_unclipped_layer`'s string
    /// check (`transform != "none"`).
    pub has_transform: bool,
}

impl ComputedTransformGroup {
    /// The no-parent transform seed: no translate, no layer.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            translate_px: [0.0; 2],
            has_transform: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_is_no_translate_and_no_transform() {
        let transform = ComputedTransformGroup::initial();
        assert_eq!(transform.translate_px, [0.0; 2]);
        assert!(!transform.has_transform);
    }
}
