//! Ordered declaration block for the `ui-dom` style system.
//!
//! This file owns `DeclBlock`, the cascade's declaration-store unit: an
//! ordered, property-deduplicated list of `Declaration`s with a `PropertySet`
//! membership bitset and a parallel importance bit vector (the Servo
//! `PropertyDeclarationBlock` shape, with its `declarations_importance:
//! SmallBitVec` mechanics adapted to a parallel `SmallVec<[bool; 4]>` — this
//! workspace has no `smallbitvec` dependency, and a parallel bool `SmallVec`
//! is inline-capacity like the declarations, spilling with the block).
//!
//! Invariants and rules:
//!
//! - declarations stay in source order; `upsert` updates in place and keeps the
//!   original position for an already-present property
//! - at most one declaration per property; `present` mirrors membership exactly
//! - `importance` is index-aligned with `declarations` and shifts with it on
//!   `remove`
//! - blocks are tiny, so edits are O(n) linear scans by design
//! - `upsert`/`remove` return whether the block changed, so callers can skip
//!   dirty marking for no-op writes

use smallvec::SmallVec;

use super::decl_value::DeclValue;
use super::declaration::Declaration;
use super::presentation_property::PresentationProperty;
use super::property_set::PropertySet;

/// One declaration block in source order with a membership bitset and a
/// parallel importance bit vector — the cascade's declaration-store unit
/// (Servo `PropertyDeclarationBlock` shape: ordered declarations, deduplicated
/// by property, O(1) membership, importance on a parallel vector).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeclBlock {
    declarations: SmallVec<[Declaration; 4]>,
    importance: SmallVec<[bool; 4]>,
    present: PropertySet,
}

impl DeclBlock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A10 minimal edit with the four CSSOM-faithful importance rules:
    /// - existing important + new normal → no change (the important
    ///   declaration stands; the weaker normal write is a no-op)
    /// - existing important + new important → update the value in place
    ///   (position kept); changed only by value difference
    /// - existing normal + new important → update the value in place and set
    ///   the bit; always changed (the bit moved)
    /// - existing normal + new normal → today's behavior (update iff the value
    ///   differs)
    ///
    /// Returns whether the block changed, so callers can skip dirty marking for
    /// no-op writes.
    pub fn upsert(&mut self, declaration: Declaration, important: bool) -> bool {
        for (index, existing) in self.declarations.iter_mut().enumerate() {
            if existing.property != declaration.property {
                continue;
            }
            let existing_important = self.importance[index];
            match (existing_important, important) {
                (true, false) => return false,
                // Same-importance writes (important→important, normal→normal):
                // update in place, changed only by value difference.
                (true, true) | (false, false) => {
                    if existing.value.value_equals(&declaration.value) {
                        return false;
                    }
                    existing.value = declaration.value;
                    return true;
                }
                (false, true) => {
                    existing.value = declaration.value;
                    self.importance[index] = true;
                    return true;
                }
            }
        }
        self.present.set(declaration.property);
        self.declarations.push(declaration);
        self.importance.push(important);
        true
    }

    /// Removes one property's declaration, shifting its importance bit with it
    /// (index alignment maintained). Returns whether anything changed.
    pub fn remove(&mut self, property: PresentationProperty) -> bool {
        let mut index = 0;
        let mut removed = false;
        while index < self.declarations.len() {
            if self.declarations[index].property == property {
                self.declarations.remove(index);
                self.importance.remove(index);
                removed = true;
            } else {
                index += 1;
            }
        }
        if removed {
            self.present.clear(property);
        }
        removed
    }

    #[must_use]
    pub fn get(&self, property: PresentationProperty) -> Option<&DeclValue> {
        self.declarations
            .iter()
            .find(|declaration| declaration.property == property)
            .map(|declaration| &declaration.value)
    }

    #[must_use]
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }

    /// The importance flags, index-aligned with `declarations()`.
    #[must_use]
    pub fn importance(&self) -> &[bool] {
        &self.importance
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(property: PresentationProperty, value: DeclValue) -> Declaration {
        Declaration { property, value }
    }

    #[test]
    fn upsert_appends_new_property_and_sets_membership() {
        let mut block = DeclBlock::new();
        assert!(block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false
        ));
        assert_eq!(block.len(), 1);
        assert!(block.present.contains(PresentationProperty::Width));
        assert_eq!(
            block.declarations()[0].property,
            PresentationProperty::Width
        );
        assert!(!block.importance()[0]);
    }

    #[test]
    fn upsert_replaces_in_place_keeping_position_and_returns_changed() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        block.upsert(
            decl(PresentationProperty::Height, DeclValue::Integer(2)),
            false,
        );

        assert!(block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(9)),
            false
        ));
        assert_eq!(block.len(), 2);
        assert_eq!(
            block.declarations()[0].property,
            PresentationProperty::Width
        );
        assert!(
            block
                .get(PresentationProperty::Width)
                .is_some_and(|value| value.value_equals(&DeclValue::Integer(9)))
        );
    }

    #[test]
    fn upsert_same_value_returns_false() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        assert!(!block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false
        ));
        assert_eq!(block.len(), 1);
    }

    #[test]
    fn remove_present_returns_true_and_clears_membership() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        assert!(block.remove(PresentationProperty::Width));
        assert!(block.is_empty());
        assert!(!block.present.contains(PresentationProperty::Width));
        assert!(block.get(PresentationProperty::Width).is_none());
    }

    #[test]
    fn remove_absent_returns_false() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        assert!(!block.remove(PresentationProperty::Height));
        assert_eq!(block.len(), 1);
        assert!(block.present.contains(PresentationProperty::Width));
    }

    #[test]
    fn ordering_preserved_across_mixed_upserts() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        block.upsert(
            decl(PresentationProperty::Height, DeclValue::Integer(2)),
            false,
        );
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(3)),
            false,
        );
        block.upsert(
            decl(PresentationProperty::Top, DeclValue::Integer(4)),
            false,
        );

        let properties: Vec<PresentationProperty> = block
            .declarations()
            .iter()
            .map(|declaration| declaration.property)
            .collect();
        assert_eq!(
            properties,
            vec![
                PresentationProperty::Width,
                PresentationProperty::Height,
                PresentationProperty::Top
            ]
        );
    }

    // — The four CSSOM-faithful importance rules — //

    #[test]
    fn important_stands_over_a_normal_write() {
        let mut block = DeclBlock::new();
        assert!(block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            true
        ));
        // existing important + new normal → no change, returns false.
        assert!(!block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(9)),
            false
        ));
        assert!(
            block
                .get(PresentationProperty::Width)
                .is_some_and(|value| value.value_equals(&DeclValue::Integer(1)))
        );
        assert!(block.importance()[0]);
    }

    #[test]
    fn important_over_important_updates_by_value_only() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            true,
        );
        // Same value: unchanged.
        assert!(!block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            true
        ));
        // Different value: updated in place, bit stays set.
        assert!(block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(9)),
            true
        ));
        assert_eq!(block.len(), 1);
        assert!(block.importance()[0]);
        assert!(
            block
                .get(PresentationProperty::Width)
                .is_some_and(|value| value.value_equals(&DeclValue::Integer(9)))
        );
    }

    #[test]
    fn normal_promoted_to_important_sets_the_bit() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            false,
        );
        assert!(!block.importance()[0]);
        // existing normal + new important → update in place, set the bit, true.
        assert!(block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(9)),
            true
        ));
        assert_eq!(block.len(), 1);
        assert!(block.importance()[0]);
        assert!(
            block
                .get(PresentationProperty::Width)
                .is_some_and(|value| value.value_equals(&DeclValue::Integer(9)))
        );
    }

    #[test]
    fn remove_shifts_the_importance_bit_with_the_declaration() {
        let mut block = DeclBlock::new();
        block.upsert(
            decl(PresentationProperty::Width, DeclValue::Integer(1)),
            true,
        );
        block.upsert(
            decl(PresentationProperty::Height, DeclValue::Integer(2)),
            false,
        );
        assert_eq!(block.importance(), &[true, false]);

        assert!(block.remove(PresentationProperty::Width));
        assert_eq!(
            block.declarations()[0].property,
            PresentationProperty::Height
        );
        assert_eq!(block.importance(), &[false]);
        assert!(!block.present.contains(PresentationProperty::Width));
    }
}
