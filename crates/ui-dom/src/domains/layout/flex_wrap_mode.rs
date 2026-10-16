//! Flex container wrap-mode vocabulary for `ui-dom`.
//!
//! This file owns the small wrap-mode enum that flex solving uses to decide
//! whether flex lines wrap and in which cross-axis direction. Its purpose is to
//! keep the wrap decision literal and cheap to compare without owning flex
//! placement policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `FlexWrapMode` enum and its `impl` block
//! - the three variants are fixed: nowrap, wrap, and reverse cross-axis wrap

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlexWrapMode {
    NoWrap,
    Wrap,
    WrapReverse,
}

impl FlexWrapMode {
    pub(crate) fn is_wrapping(self) -> bool {
        self != Self::NoWrap
    }

    pub(crate) fn is_cross_reversed(self) -> bool {
        self == Self::WrapReverse
    }
}
