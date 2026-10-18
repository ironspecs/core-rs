//! Non-inherited flex computed group (PLAN.md S1.1).
//!
//! This file owns the `ComputedFlexGroup` struct only. Its six field
//! vocabularies live in their own files (`flex_direction_kw.rs`,
//! `flex_wrap_kw.rs`, `flex_basis_kw.rs`, `justify_content_kw.rs`,
//! `align_items_kw.rs`, `align_content_kw.rs`), each owning its keyword enum +
//! helpers (structural rule 1). It is plain `Copy` data plus `initial()`;
//! resolution lives in the step-3.3 resolver.
//!
//! Invariants and rules:
//!
//! - **`align_self: None` = `auto`** (inherit the container's `align-items`),
//!   exactly the flex solver's own `Option<FlexCrossAxisAlignment>` (`auto` →
//!   `None` in `flex.rs::item_align_self`).
//! - **Initial values mirror the solver's absent defaults**: `row`, `nowrap`,
//!   grow 0.0, shrink 1.0, basis auto, order 0, `flex-start`, `stretch`,
//!   align-self auto, `stretch`, gap `[0,0]` — each cited on its field below.

use super::align_content_kw::AlignContentKw;
use super::align_items_kw::AlignItemsKw;
use super::flex_basis_kw::FlexBasisKw;
use super::flex_direction_kw::FlexDirectionKw;
use super::flex_wrap_kw::FlexWrapKw;
use super::justify_content_kw::JustifyContentKw;

/// Computed values of the non-inherited flex longhands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedFlexGroup {
    /// `FlexDirectionKw::Row` is today's absent default (`flex.rs` staged/
    /// supported direction falls through to `(Horizontal, false)`).
    pub direction: FlexDirectionKw,
    /// `FlexWrapKw::NoWrap` is today's absent default (`flex.rs` falls through
    /// to `FlexWrapMode::NoWrap`).
    pub wrap: FlexWrapKw,
    /// `0.0` is today's absent default
    /// (`flex.rs::item_flex_grow_factor_from_node`).
    pub grow: f32,
    /// `1.0` is today's absent default
    /// (`flex.rs::item_flex_shrink_factor_from_node`).
    pub shrink: f32,
    /// `FlexBasisKw::Auto` is today's absent default
    /// (`flex_basis_source_from_node` returns `None`, which the solver treats
    /// as auto/content).
    pub basis: FlexBasisKw,
    /// `0` is today's absent default (`flex.rs::item_order_from_node`); `i64`
    /// declarations clamp to `i32` at resolve.
    pub order: i32,
    /// `FlexStart` is today's absent default (`flex.rs::staged_justify_content`).
    pub justify_content: JustifyContentKw,
    /// `Stretch` is today's absent default (`flex.rs::staged_align_items`).
    pub align_items: AlignItemsKw,
    /// `None` = `auto` (inherit container `align-items`), mirroring
    /// `flex.rs::item_align_self`.
    pub align_self: Option<AlignItemsKw>,
    /// `Stretch` is today's absent default (`flex.rs::staged_align_content`).
    pub align_content: AlignContentKw,
    /// `[row, column]` px; initial `[0.0, 0.0]` (absent gap).
    pub gap: [f32; 2],
}

impl ComputedFlexGroup {
    /// The no-parent flex seed: every field at its CSS/today default.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            direction: FlexDirectionKw::Row,
            wrap: FlexWrapKw::NoWrap,
            grow: 0.0,
            shrink: 1.0,
            basis: FlexBasisKw::Auto,
            order: 0,
            justify_content: JustifyContentKw::FlexStart,
            align_items: AlignItemsKw::Stretch,
            align_self: None,
            align_content: AlignContentKw::Stretch,
            gap: [0.0; 2],
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn initial_matches_todays_absent_defaults() {
        let flex = ComputedFlexGroup::initial();
        assert_eq!(flex.direction, FlexDirectionKw::Row);
        assert_eq!(flex.wrap, FlexWrapKw::NoWrap);
        assert_eq!(flex.grow, 0.0);
        assert_eq!(flex.shrink, 1.0);
        assert_eq!(flex.basis, FlexBasisKw::Auto);
        assert_eq!(flex.order, 0);
        assert_eq!(flex.justify_content, JustifyContentKw::FlexStart);
        assert_eq!(flex.align_items, AlignItemsKw::Stretch);
        assert_eq!(flex.align_self, None);
        assert_eq!(flex.align_content, AlignContentKw::Stretch);
        assert_eq!(flex.gap, [0.0; 2]);
    }
}
