//! Canonical internal DOM event descriptor builder for `ui-dom`.
//!
//! This file owns the fluent builder that constructs the fixed event
//! descriptor rows in `dom_event_definition.rs`. Each row is the one source of
//! truth for event-type behavior; concrete `DomEvent` values remain
//! occurrence-only and must look up fixed behavior through the descriptor
//! table instead of carrying editable copies.

use crate::types::dom_event::DomEventDetailKind;
use crate::types::dom_event_behavior::DomEventBehavior;
use crate::types::dom_event_definition::DomEventDescriptor;
use crate::types::dom_event_name::DomEventName;

pub(crate) struct DomEventDescriptorBuilder {
    descriptor: DomEventDescriptor,
}

impl DomEventDescriptorBuilder {
    #[must_use]
    pub(crate) const fn new(name: DomEventName) -> Self {
        Self {
            descriptor: DomEventDescriptor {
                name,
                dom_name: "",
                behavior: DomEventBehavior::new(false, false, DomEventDetailKind::Ui),
            },
        }
    }

    #[must_use]
    pub(crate) const fn dom_name(mut self, dom_name: &'static str) -> Self {
        self.descriptor.dom_name = dom_name;
        self
    }

    #[must_use]
    pub(crate) const fn behavior(mut self, behavior: DomEventBehavior) -> Self {
        self.descriptor.behavior = behavior;
        self
    }

    #[must_use]
    pub(crate) const fn build(self) -> DomEventDescriptor {
        self.descriptor
    }
}
