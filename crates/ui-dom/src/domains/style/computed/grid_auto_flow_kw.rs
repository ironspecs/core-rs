//! Grid auto-flow computed keyword (`GridAutoFlowKw`).
//!
//! This file owns the closed `grid-auto-flow` keyword vocabulary: the four
//! flow/density combinations CSS Grid 1 §7 admits. It is a plain `Copy` enum
//! plus `initial()`; the grammar in `decl_parse.rs` maps the one-or-two-token
//! value form (`[row | column] && dense?`) onto it, and the resolver copies it
//! into `ComputedGridGroup.auto_flow`. No placement policy lives here.
//!
//! Invariants and rules:
//!
//! - the variant set is the closed `grid-auto-flow` set the presentation
//!   registry admits (`presentation_property.rs`'s T1 METADATA rows)
//! - `Row` is the CSS initial value (`row`), and is `initial()`

/// The four `grid-auto-flow` outcomes (`row` is the CSS initial).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GridAutoFlowKw {
    Row,
    RowDense,
    Column,
    ColumnDense,
}

impl GridAutoFlowKw {
    /// The CSS initial value: `row` (no dense packing).
    #[must_use]
    pub fn initial() -> Self {
        Self::Row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_is_row() {
        assert_eq!(GridAutoFlowKw::initial(), GridAutoFlowKw::Row);
    }

    #[test]
    fn variants_are_distinct() {
        let variants = [
            GridAutoFlowKw::Row,
            GridAutoFlowKw::RowDense,
            GridAutoFlowKw::Column,
            GridAutoFlowKw::ColumnDense,
        ];
        for (index, left) in variants.iter().enumerate() {
            for (other, right) in variants.iter().enumerate() {
                assert_eq!(left == right, index == other, "{left:?} vs {right:?}");
            }
        }
    }
}
