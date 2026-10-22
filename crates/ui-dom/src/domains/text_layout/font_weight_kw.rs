//! The text-seam font-weight keyword (`FontWeightKw`).
//!
//! This file owns the request-carrier font weight (text seam S2): the resolved
//! numeric weight (100–900 in steps of 100) the `TextLayoutRequest` carries
//! into the shaper, and its mapping to `cosmic_text::Weight`. It is a plain
//! `Copy` newtype with no layout logic.
//!
//! Invariants and rules:
//!
//! - the carried value is always a resolved 100–900 step-100 weight (the
//!   `FontWeight` grammar's numeric arm); `bolder`/`lighter` are resolved
//!   against the parent weight at compute time, never carried here

use cosmic_text::Weight as CosmicWeight;

/// The resolved numeric font weight a text-layout request carries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct FontWeightKw(pub(crate) u16);

impl FontWeightKw {
    /// The CSS initial weight (normal).
    pub(crate) const NORMAL: Self = Self(400);

    /// Wraps a resolved computed weight (already 100–900 step 100).
    #[must_use]
    pub(crate) fn from_weight(weight: u16) -> Self {
        Self(weight)
    }

    /// Maps to the `cosmic_text::Weight` the shaper consumes.
    #[must_use]
    pub(crate) fn cosmic(self) -> CosmicWeight {
        CosmicWeight(self.0)
    }
}

impl Default for FontWeightKw {
    fn default() -> Self {
        Self::NORMAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_numeric_weights_to_cosmic_weights() {
        assert_eq!(FontWeightKw::from_weight(400).cosmic(), CosmicWeight(400));
        assert_eq!(FontWeightKw::from_weight(700).cosmic(), CosmicWeight(700));
        assert_ne!(
            FontWeightKw::from_weight(400),
            FontWeightKw::from_weight(700)
        );
        assert_eq!(FontWeightKw::NORMAL, FontWeightKw::from_weight(400));
    }
}
