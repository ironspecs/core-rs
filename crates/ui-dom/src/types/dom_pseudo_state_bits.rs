//! Shared element pseudo-state bit-set for `ui-dom`.
//!
//! This file owns the compact bit-set that tracks one element's active
//! pseudo-state flags. Its purpose is to keep hover, active, focus, and
//! focus-visible state in one copyable value without owning any root mutation
//! policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomPseudoStateBits` struct and its `impl` blocks
//! - the bit layout is fixed and matches `DomPseudoState`'s discriminants

use crate::types::dom_pseudo_state::DomPseudoState;

/// Bit-set of the current element pseudo-state flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomPseudoStateBits(u8);

impl DomPseudoStateBits {
    /// Returns whether this bit-set includes the provided pseudo-state.
    #[must_use]
    pub fn contains(self, pseudo_state: DomPseudoState) -> bool {
        self.0 & pseudo_state.bit() != 0
    }

    pub(crate) fn insert(&mut self, pseudo_state: DomPseudoState) {
        self.0 |= pseudo_state.bit();
    }

    pub(crate) fn remove(&mut self, pseudo_state: DomPseudoState) {
        self.0 &= !pseudo_state.bit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_insert_contains_remove_round_trip() {
        let mut bits = DomPseudoStateBits::default();
        assert!(!bits.contains(DomPseudoState::Hover));
        assert!(!bits.contains(DomPseudoState::Active));

        bits.insert(DomPseudoState::Hover);
        assert!(bits.contains(DomPseudoState::Hover));
        assert!(!bits.contains(DomPseudoState::Active));

        bits.insert(DomPseudoState::Active);
        assert!(bits.contains(DomPseudoState::Hover));
        assert!(bits.contains(DomPseudoState::Active));

        bits.remove(DomPseudoState::Hover);
        assert!(!bits.contains(DomPseudoState::Hover));
        assert!(bits.contains(DomPseudoState::Active));
    }
}
