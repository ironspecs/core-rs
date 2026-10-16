//! `UiIconDensity` owns the retained atlas density variants for the flat UI icon path.
//!
//! The density ordering is the stable contract shared by atlas generation and
//! asset-path lookup. The square design-box base size it scales lives in `lib.rs`
//! as `UI_ICON_DESIGN_BOX_PX`.
//!
//! Strict invariants:
//! - this file owns only the `UiIconDensity` enum and its `impl` blocks.

use crate::UI_ICON_DESIGN_BOX_PX;

/// Retained atlas density variants supported by the flat UI icon path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiIconDensity {
    X1,
    X2,
    X4,
}

impl UiIconDensity {
    /// Stable retained-density order used by atlas generation.
    pub const ALL: [Self; 3] = [Self::X1, Self::X2, Self::X4];

    /// Returns the retained density multiplier for this atlas variant.
    #[must_use]
    pub const fn multiplier(self) -> u32 {
        match self {
            Self::X1 => 1,
            Self::X2 => 2,
            Self::X4 => 4,
        }
    }

    /// Returns the stable generated-asset suffix for this density.
    #[must_use]
    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::X1 => "x1",
            Self::X2 => "x2",
            Self::X4 => "x4",
        }
    }

    /// Returns the square design-box size, in retained pixels, for this density.
    #[must_use]
    pub const fn design_box_size_px(self) -> u32 {
        UI_ICON_DESIGN_BOX_PX * self.multiplier()
    }
}
