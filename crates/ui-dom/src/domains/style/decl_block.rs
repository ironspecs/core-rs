//! Ordered declaration block for the `ui-dom` style system.
//!
//! This file owns `DeclBlock`, the cascade's declaration-store unit: an
//! ordered, property-deduplicated list of `Declaration`s with a `PropertySet`
//! membership bitset (the Servo `PropertyDeclarationBlock` shape). Importance
//! bits arrive later with cascade tier path F.
//!
//! Invariants and rules:
//!
//! - declarations stay in source order; `upsert` updates in place and keeps the
//!   original position for an already-present property
//! - at most one declaration per property; `present` mirrors membership exactly
//! - blocks are tiny, so edits are O(n) linear scans by design
//! - `upsert`/`remove` return whether the block changed, so callers can skip
//!   dirty marking for no-op writes

use smallvec::SmallVec;

use super::decl_value::DeclValue;
use super::declaration::Declaration;
use super::presentation_property::PresentationProperty;
use super::property_set::PropertySet;

/// One declaration block in source order with a membership bitset — the
/// cascade's declaration-store unit (Servo `PropertyDeclarationBlock` shape:
/// ordered declarations, deduplicated by property, O(1) membership).
/// Importance bits arrive later with cascade tier path F.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeclBlock {
    declarations: SmallVec<[Declaration; 4]>,
    present: PropertySet,
}

impl DeclBlock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A10 minimal edit: update-in-place when the property is present
    /// (source position retained), append otherwise. Returns whether the
    /// block changed (value difference), so callers can skip dirty marking
    /// for no-op writes.
    pub fn upsert(&mut self, declaration: Declaration) -> bool {
        for existing in &mut self.declarations {
            if existing.property == declaration.property {
                if existing.value.value_equals(&declaration.value) {
                    return false;
                }
                existing.value = declaration.value;
                return true;
            }
        }
        self.present.set(declaration.property);
        self.declarations.push(declaration);
        true
    }

    /// Removes one property's declaration. Returns whether anything changed.
    pub fn remove(&mut self, property: PresentationProperty) -> bool {
        let before = self.declarations.len();
        self.declarations
            .retain(|declaration| declaration.property != property);
        if self.declarations.len() == before {
            return false;
        }
        self.present.clear(property);
        true
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
        assert!(block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1))));
        assert_eq!(block.len(), 1);
        assert!(block.present.contains(PresentationProperty::Width));
        assert_eq!(
            block.declarations()[0].property,
            PresentationProperty::Width
        );
    }

    #[test]
    fn upsert_replaces_in_place_keeping_position_and_returns_changed() {
        let mut block = DeclBlock::new();
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1)));
        block.upsert(decl(PresentationProperty::Height, DeclValue::Integer(2)));

        assert!(block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(9))));
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
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1)));
        assert!(!block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1))));
        assert_eq!(block.len(), 1);
    }

    #[test]
    fn remove_present_returns_true_and_clears_membership() {
        let mut block = DeclBlock::new();
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1)));
        assert!(block.remove(PresentationProperty::Width));
        assert!(block.is_empty());
        assert!(!block.present.contains(PresentationProperty::Width));
        assert!(block.get(PresentationProperty::Width).is_none());
    }

    #[test]
    fn remove_absent_returns_false() {
        let mut block = DeclBlock::new();
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1)));
        assert!(!block.remove(PresentationProperty::Height));
        assert_eq!(block.len(), 1);
        assert!(block.present.contains(PresentationProperty::Width));
    }

    #[test]
    fn ordering_preserved_across_mixed_upserts() {
        let mut block = DeclBlock::new();
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(1)));
        block.upsert(decl(PresentationProperty::Height, DeclValue::Integer(2)));
        block.upsert(decl(PresentationProperty::Width, DeclValue::Integer(3)));
        block.upsert(decl(PresentationProperty::Top, DeclValue::Integer(4)));

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
}
