//! Canonical internal DOM element descriptor builder for `ui-dom`.
//!
//! This file owns the fluent builder that constructs `DomElementDescriptor`
//! values for built-in tags in `dom_tag.rs`. Each descriptor seeds default
//! attrs, built-in behavior, and state style declarations without re-deriving
//! those defaults in each app.

use crate::types::dom_attribute::DomAttribute;
use crate::types::dom_tag::{
    DomElementBehavior, DomElementDescriptor, DomElementStateStyleDeclarations,
    DomStyleDeclarationList,
};
use crate::types::{DomAttributeValue, DomAttrs};

#[derive(Clone, Debug)]
pub(crate) struct DomElementDescriptorBuilder {
    descriptor: DomElementDescriptor,
}

impl DomElementDescriptorBuilder {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            descriptor: DomElementDescriptor {
                default_attrs: DomAttrs::default(),
                behavior: DomElementBehavior::default(),
                state_style_declarations: DomElementStateStyleDeclarations::default(),
            },
        }
    }

    #[must_use]
    pub(crate) fn display(self, value: &str) -> Self {
        self.attr(
            DomAttribute::Display,
            DomAttributeValue::String(value.to_string()),
        )
    }

    #[must_use]
    pub(crate) fn behavior(mut self, behavior: DomElementBehavior) -> Self {
        self.descriptor.behavior = behavior;
        self
    }

    #[must_use]
    pub(crate) fn attr(mut self, attribute: DomAttribute, value: DomAttributeValue) -> Self {
        self.descriptor
            .default_attrs
            .attributes
            .insert(attribute, value);
        self
    }

    #[must_use]
    pub(crate) fn hover_style(mut self, declarations: DomStyleDeclarationList) -> Self {
        self.descriptor.state_style_declarations.hover = declarations;
        self
    }

    #[must_use]
    pub(crate) fn active_style(mut self, declarations: DomStyleDeclarationList) -> Self {
        self.descriptor.state_style_declarations.active = declarations;
        self
    }

    #[must_use]
    pub(crate) fn focus_style(mut self, declarations: DomStyleDeclarationList) -> Self {
        self.descriptor.state_style_declarations.focus = declarations;
        self
    }

    #[must_use]
    pub(crate) fn focus_visible_style(mut self, declarations: DomStyleDeclarationList) -> Self {
        self.descriptor.state_style_declarations.focus_visible = declarations;
        self
    }

    #[must_use]
    pub(crate) fn build(self) -> DomElementDescriptor {
        self.descriptor
    }
}
