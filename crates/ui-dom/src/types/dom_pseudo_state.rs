//! Shared pseudo-state input and flags for `ui-dom`.
//!
//! This file keeps the public pointer-state input and element pseudo-state flag
//! types in one place without owning any root mutation policy. The compact
//! bit-set (`DomPseudoStateBits`) lives in its own file and is re-exported
//! here.

pub use crate::types::dom_pseudo_state_bits::DomPseudoStateBits;

/// Public pointer facts used to update root-owned pseudo-state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DomPointerState {
    /// Horizontal pointer position in root-local DOM pixels.
    pub x_px: f32,
    /// Vertical pointer position in root-local DOM pixels.
    pub y_px: f32,
    /// Whether the primary pointer button is currently pressed.
    pub primary_button_down: bool,
    /// Vertical wheel delta in root-local DOM pixels for this update tick.
    pub scroll_delta_y_px: f32,
}

/// One supported pseudo-state flag for element nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomPseudoState {
    /// Pointer hover state.
    Hover = 1,
    /// Current active interaction state.
    Active = 1 << 1,
    /// Current focus state.
    Focus = 1 << 2,
    /// Placeholder focus-visible state.
    FocusVisible = 1 << 3,
}

impl DomPseudoState {
    pub(crate) fn bit(self) -> u8 {
        match self {
            Self::Hover => 1,
            Self::Active => 1 << 1,
            Self::Focus => 1 << 2,
            Self::FocusVisible => 1 << 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_state_bit_maps_each_variant_to_a_distinct_bit() {
        let variants = [
            DomPseudoState::Hover,
            DomPseudoState::Active,
            DomPseudoState::Focus,
            DomPseudoState::FocusVisible,
        ];
        for (index, variant) in variants.iter().copied().enumerate() {
            assert_ne!(variant.bit(), 0);
            assert!(variant.bit().is_power_of_two());
            for other in variants.iter().copied().skip(index + 1) {
                assert_ne!(variant.bit(), other.bit());
            }
        }

        let mut bits = DomPseudoStateBits::default();
        assert!(!bits.contains(DomPseudoState::Hover));
        bits.insert(DomPseudoState::Hover);
        assert!(bits.contains(DomPseudoState::Hover));
    }
}
