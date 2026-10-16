//! Bit-flag bundle describing which retained caches one attribute write
//! invalidates.
//!
//! This file owns the `AttributeInvalidation` struct and its `impl` blocks.
//! Its purpose is to pack layout, paint, style, and layer-membership
//! invalidation decisions into one copyable, mergeable value so root-owned
//! write and transition code can batch cache updates without allocating.
//!
//! Invariants and rules:
//!
//! - this file owns only the `AttributeInvalidation` struct and its `impl`
//!   blocks
//! - bit flags are fixed at compile time; do not renumber existing flags
//! - `merge` is a pure bitwise union and never clears a flag

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AttributeInvalidation {
    bits: u8,
}

impl AttributeInvalidation {
    const LAYOUT: u8 = 1 << 0;
    const PAINT: u8 = 1 << 1;
    const STYLE: u8 = 1 << 2;
    const LAYER_MEMBERSHIP: u8 = 1 << 3;

    #[must_use]
    pub(crate) const fn with_layout(mut self) -> Self {
        self.bits |= Self::LAYOUT;
        self
    }

    #[must_use]
    pub(crate) const fn with_paint(mut self) -> Self {
        self.bits |= Self::PAINT;
        self
    }

    #[must_use]
    pub(crate) const fn with_style(mut self) -> Self {
        self.bits |= Self::STYLE;
        self
    }

    #[must_use]
    pub(crate) const fn with_layer_membership(mut self) -> Self {
        self.bits |= Self::LAYER_MEMBERSHIP;
        self
    }

    #[must_use]
    pub(crate) const fn layout(self) -> bool {
        self.bits & Self::LAYOUT != 0
    }

    #[must_use]
    pub(crate) const fn paint(self) -> bool {
        self.bits & Self::PAINT != 0
    }

    #[must_use]
    pub(crate) const fn style(self) -> bool {
        self.bits & Self::STYLE != 0
    }

    #[must_use]
    pub(crate) const fn layer_membership(self) -> bool {
        self.bits & Self::LAYER_MEMBERSHIP != 0
    }

    #[must_use]
    pub(crate) const fn merge(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}
