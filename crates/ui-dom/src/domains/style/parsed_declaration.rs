//! One parsed declaration with its importance flag for the `ui-dom` style
//! system.
//!
//! This file owns `ParsedDeclaration`, the plain-data result of strict
//! single-declaration parsing (`parse_declaration_strict`): a property, its
//! parsed `DeclValue`, and the `!important` flag stripped from the authored
//! value (CSS Syntax 3 §5.5 — importance is part of the declaration, not the
//! value). The block parser (`parse_declaration_block`) folds the same flag
//! into the block directly; this type exists so the strict path can hand the
//! flag to the caller without widening `Declaration` itself.
//!
//! Invariants and rules:
//!
//! - a `ParsedDeclaration` carries exactly one property, one value, and one
//!   importance flag; it owns no cascade, precedence, or ordering state

use super::decl_value::DeclValue;
use super::presentation_property::PresentationProperty;

/// One typed declaration plus its `!important` flag (the flag applies to every
/// longhand a shorthand expansion produced from the same segment).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParsedDeclaration {
    pub property: PresentationProperty,
    pub value: DeclValue,
    pub important: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_declaration_constructs_and_reads_back() {
        let parsed = ParsedDeclaration {
            property: PresentationProperty::Width,
            value: DeclValue::Integer(1),
            important: true,
        };
        assert_eq!(parsed.property, PresentationProperty::Width);
        assert!(parsed.value.value_equals(&DeclValue::Integer(1)));
        assert!(parsed.important);
    }
}
