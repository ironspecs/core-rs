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

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn builder_chains_to_a_descriptor_with_exact_fields() {
        let behavior = DomElementBehavior {
            focusable: true,
            active_pseudo: true,
            ..DomElementBehavior::default()
        };

        let hover = smallvec![(
            DomAttribute::Custom("background-color".to_string()),
            DomAttributeValue::Color([0.1, 0.2, 0.3, 1.0]),
        )];
        let active = smallvec![(
            DomAttribute::Custom("background-color".to_string()),
            DomAttributeValue::Color([0.4, 0.5, 0.6, 1.0]),
        )];
        let focus = smallvec![(
            DomAttribute::Custom("outline-color".to_string()),
            DomAttributeValue::Color([0.7, 0.8, 0.9, 1.0]),
        )];
        let focus_visible = smallvec![(
            DomAttribute::Custom("outline-color".to_string()),
            DomAttributeValue::Color([1.0, 1.0, 1.0, 1.0]),
        )];

        let descriptor = DomElementDescriptorBuilder::new()
            .display("block")
            .behavior(behavior.clone())
            .attr(
                DomAttribute::Type,
                DomAttributeValue::String("button".to_string()),
            )
            .hover_style(hover.clone())
            .active_style(active.clone())
            .focus_style(focus.clone())
            .focus_visible_style(focus_visible.clone())
            .build();

        assert_eq!(
            descriptor
                .default_attrs
                .attributes
                .get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("block".to_string()))
        );
        assert_eq!(
            descriptor.default_attrs.attributes.get(&DomAttribute::Type),
            Some(&DomAttributeValue::String("button".to_string()))
        );
        assert_eq!(descriptor.behavior, behavior);
        assert_eq!(descriptor.state_style_declarations.hover, hover);
        assert_eq!(descriptor.state_style_declarations.active, active);
        assert_eq!(descriptor.state_style_declarations.focus, focus);
        assert_eq!(
            descriptor.state_style_declarations.focus_visible,
            focus_visible
        );
    }
}
