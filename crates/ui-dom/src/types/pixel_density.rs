//! Shared pixel density policy for `ui-dom`.
//!
//! This file represents the pixel density selection for owned layer textures.
//! Its purpose is to keep density policy explicit instead of hiding it in
//! paint globals or per-root one-off state.
//!
//! Invariants and rules:
//!
//! - this file owns only the `PixelDensity` enum and its `impl` blocks
//! - density is expressed as a multiplier of authored pixels

/// Selects the pixel density for owned layer textures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelDensity {
    /// One texture pixel per authored pixel.
    X1 = 1,
    /// Two texture pixels per authored pixel.
    X2 = 2,
    /// Four texture pixels per authored pixel.
    X4 = 4,
}

impl PixelDensity {
    #[must_use]
    pub fn multiplier(self) -> usize {
        match self {
            Self::X1 => 1,
            Self::X2 => 2,
            Self::X4 => 4,
        }
    }
}
