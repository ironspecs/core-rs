//! Stable retained SVG placeholder identity for `ui-dom`.
//!
//! This file owns the identity key that banks retained SVG placeholder ops by
//! authored icon name. Its purpose is to keep SVG placeholder identity stable
//! and hashable without owning rasterization or layer-pool policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `SvgPaintId` struct and its `impl` block
//! - `from_name` is test-only: production identity is assigned by the layer
//!   pool, so the name-derived constructor is gated to `#[cfg(test)]`

#[cfg(test)]
use super::stable_placeholder_id;

/// Stable retained SVG placeholder identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SvgPaintId(u64);

impl SvgPaintId {
    /// Derives one stable retained SVG placeholder identity from the authored icon name.
    #[cfg(test)]
    #[must_use]
    pub fn from_name(icon_name: &str) -> Self {
        Self(stable_placeholder_id(icon_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_is_stable_and_name_sensitive() {
        assert_eq!(SvgPaintId::from_name("menu"), SvgPaintId::from_name("menu"));
        assert_ne!(
            SvgPaintId::from_name("menu"),
            SvgPaintId::from_name("settings"),
        );
        assert_ne!(SvgPaintId::from_name("a"), SvgPaintId::from_name("b"));
    }
}
