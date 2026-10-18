//! One parsed declaration for the `ui-dom` style system.
//!
//! This file owns `Declaration`, the pair of a presentation property and its
//! parsed `DeclValue`. It is plain data with no behavior; source-order position
//! is the declaration's index in its owning `DeclBlock`.
//!
//! Invariants and rules:
//!
//! - a `Declaration` carries exactly one property and one value; it owns no
//!   cascade, precedence, or ordering state

use super::decl_value::DeclValue;
use super::presentation_property::PresentationProperty;

/// One typed declaration: a property and its parsed value. Source-order
/// position is the declaration's index in its `DeclBlock`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Declaration {
    pub property: PresentationProperty,
    pub value: DeclValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_constructs_and_reads_back() {
        let declaration = Declaration {
            property: PresentationProperty::Opacity,
            value: DeclValue::Number(0.5),
        };
        assert_eq!(declaration.property, PresentationProperty::Opacity);
        assert!(declaration.value.value_equals(&DeclValue::Number(0.5)));
    }
}
