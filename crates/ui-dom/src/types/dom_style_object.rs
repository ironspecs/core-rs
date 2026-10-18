//! Retained per-node style cache for pseudo-state style declarations.
//!
//! This file owns the `DomStyleObject` struct and its `impl` blocks. Its
//! purpose is to keep the authored pseudo-state (`hover`, `active`, `focus`,
//! `focus-visible`) style declaration lists and the style dirty flag in one
//! honest record without owning style recomputation or rule matching.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomStyleObject` struct and its `impl` blocks
//! - the four declaration fields are `pub(crate)` and are set wholesale from
//!   a `DomElementStateStyleDeclarations` descriptor
//! - pseudo-state declaration lookup returns borrowed slices, never owned
//!   copies
//! - behavior declarations are typed `Declaration`s (presentation properties);
//!   text-valued properties (font-family, transform, transition longhands)
//!   carry interner-relative `TextKey`s with no owning store and are not
//!   supported in behavior sheets

use crate::domains::style::Declaration;
use crate::types::dom_pseudo_state::DomPseudoState;
use crate::types::dom_tag::{DomElementStateStyleDeclarations, DomStyleDeclarationList};

#[cfg(test)]
use crate::domains::style::{DeclLength, DeclValue, PresentationProperty};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomStyleObject {
    pub dirty: bool,
    /// The unconditional base behavior sheet (spine 3.3g): typed
    /// `Declaration`s cloned from the element descriptor at creation and
    /// folded FIRST (below matched rules and inline style).
    pub(crate) base_declarations: DomStyleDeclarationList,
    pub(crate) hover_declarations: DomElementStateStyleDeclarationsField,
    pub(crate) active_declarations: DomElementStateStyleDeclarationsField,
    pub(crate) focus_declarations: DomElementStateStyleDeclarationsField,
    pub(crate) focus_visible_declarations: DomElementStateStyleDeclarationsField,
}

type DomElementStateStyleDeclarationsField = crate::types::dom_tag::DomStyleDeclarationList;

impl DomStyleObject {
    pub(crate) fn set_base_declarations(&mut self, declarations: DomStyleDeclarationList) {
        self.base_declarations = declarations;
    }

    #[must_use]
    pub(crate) fn base_declarations(&self) -> &[Declaration] {
        self.base_declarations.as_slice()
    }

    pub(crate) fn set_state_style_declarations(
        &mut self,
        declarations: DomElementStateStyleDeclarations,
    ) {
        self.hover_declarations = declarations.hover;
        self.active_declarations = declarations.active;
        self.focus_declarations = declarations.focus;
        self.focus_visible_declarations = declarations.focus_visible;
    }

    #[must_use]
    pub(crate) fn pseudo_state_declarations(&self, pseudo_state: DomPseudoState) -> &[Declaration] {
        match pseudo_state {
            DomPseudoState::Hover => self.hover_declarations.as_slice(),
            DomPseudoState::Active => self.active_declarations.as_slice(),
            DomPseudoState::Focus => self.focus_declarations.as_slice(),
            DomPseudoState::FocusVisible => self.focus_visible_declarations.as_slice(),
        }
    }

    #[must_use]
    pub(crate) fn has_pseudo_state_declarations(&self, pseudo_state: DomPseudoState) -> bool {
        !self.pseudo_state_declarations(pseudo_state).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn style_object_stores_and_reports_state_declarations() {
        let mut object = DomStyleObject::default();

        assert!(!object.has_pseudo_state_declarations(DomPseudoState::Hover));

        object.set_state_style_declarations(DomElementStateStyleDeclarations {
            hover: smallvec![Declaration {
                property: PresentationProperty::Width,
                value: DeclValue::Length(DeclLength::Px(10.0)),
            }],
            ..DomElementStateStyleDeclarations::default()
        });

        assert!(object.has_pseudo_state_declarations(DomPseudoState::Hover));
        assert_eq!(
            object.pseudo_state_declarations(DomPseudoState::Hover),
            &[Declaration {
                property: PresentationProperty::Width,
                value: DeclValue::Length(DeclLength::Px(10.0)),
            }]
        );
        assert!(
            object
                .pseudo_state_declarations(DomPseudoState::Active)
                .is_empty()
        );
    }

    #[test]
    fn style_object_stores_and_reports_base_declarations() {
        let mut object = DomStyleObject::default();
        assert!(object.base_declarations().is_empty());

        object.set_base_declarations(smallvec![Declaration {
            property: PresentationProperty::Display,
            value: DeclValue::Keyword(crate::domains::style::Keyword::Block),
        }]);

        assert_eq!(
            object.base_declarations(),
            &[Declaration {
                property: PresentationProperty::Display,
                value: DeclValue::Keyword(crate::domains::style::Keyword::Block),
            }]
        );
    }
}
