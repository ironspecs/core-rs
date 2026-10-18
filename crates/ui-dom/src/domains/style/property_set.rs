//! Fixed bitset over `PresentationProperty` for `ui-dom`.
//!
//! This file owns `PropertySet`, a 69-bit set in two words that answers O(1)
//! membership without scanning a block's declarations. It backs `DeclBlock`'s
//! `present` membership tracking.
//!
//! Invariants and rules:
//!
//! - bit index is `PresentationProperty::index` (0..=68), split across two
//!   `u64` words at word 0 (bits 0..=63) and word 1 (bits 64..=68)
//! - `set`/`clear` are idempotent; no out-of-range bit is ever touched

use super::presentation_property::PresentationProperty;

/// Fixed-capacity bitset over `PresentationProperty` (69 bits in two words):
/// O(1) membership without scanning a block's declarations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PropertySet([u64; 2]);

impl PropertySet {
    /// `new` is test-only: `DeclBlock` constructs its `present` bitset through
    /// the derived `Default` (`DeclBlock::new` calls `Self::default()`), never
    /// `PropertySet::new`.
    #[allow(dead_code)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, property: PresentationProperty) {
        let index = property.index();
        self.0[index / 64] |= 1u64 << (index % 64);
    }

    pub fn clear(&mut self, property: PresentationProperty) {
        let index = property.index();
        self.0[index / 64] &= !(1u64 << (index % 64));
    }

    /// `contains` is test-only: `DeclBlock::get` scans the declaration list
    /// directly, so production membership reads go through `get`, not
    /// `contains`.
    #[allow(dead_code)]
    #[must_use]
    pub fn contains(&self, property: PresentationProperty) -> bool {
        let index = property.index();
        (self.0[index / 64] & (1u64 << (index % 64))) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_contains_and_clear_across_both_words() {
        let mut set = PropertySet::new();
        assert!(!set.contains(PresentationProperty::FontSize));
        assert!(!set.contains(PresentationProperty::TransitionDelay));

        set.set(PresentationProperty::FontSize);
        assert!(set.contains(PresentationProperty::FontSize));
        assert!(!set.contains(PresentationProperty::TransitionDelay));

        set.set(PresentationProperty::TransitionDelay);
        assert!(set.contains(PresentationProperty::FontSize));
        assert!(set.contains(PresentationProperty::TransitionDelay));

        set.clear(PresentationProperty::FontSize);
        assert!(!set.contains(PresentationProperty::FontSize));
        assert!(set.contains(PresentationProperty::TransitionDelay));

        set.clear(PresentationProperty::TransitionDelay);
        assert!(!set.contains(PresentationProperty::TransitionDelay));
        assert!(!set.contains(PresentationProperty::FontSize));
    }
}
