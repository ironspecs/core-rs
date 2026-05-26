//! Shared pseudo-state input and flags for `ui-dom`.
//!
//! This file keeps the public pointer-state input and element pseudo-state flag
//! types in one place without owning any root mutation policy.

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

/// Bit-set of the current element pseudo-state flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomPseudoStateBits(u8);

impl DomPseudoState {
    fn bit(self) -> u8 {
        match self {
            Self::Hover => 1,
            Self::Active => 1 << 1,
            Self::Focus => 1 << 2,
            Self::FocusVisible => 1 << 3,
        }
    }
}

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
