//! Shared DOM event-listener owner contract.
//!
//! This file owns the `DomEventListenerOwner` trait: the owner-facing listener
//! mutation seam implemented by attached roots and detached fragments so the
//! two owners stay aligned when they both support listener authoring. The
//! listener callback type itself lives in `dom_event_listener.rs`, and the
//! listener-facing callback context lives in its own file.

use crate::types::DomError;
use crate::types::dom_event_listener::{DomEventListener, DomEventListenerOptions};
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::DomNodeSlotKey;

#[cfg(test)]
use crate::types::dom_event_listener_context::DomEventListenerContext;
#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use crate::types::root_dom::RootDom;
#[cfg(test)]
use crate::types::root_dom_context::RootDomContext;
#[cfg(test)]
use crate::types::root_dom_fragment::RootDomFragment;
#[cfg(test)]
use crate::types::root_dom_metrics::RootDomMetricName;

/// Common listener authoring seam shared by attached roots and detached fragments.
pub(crate) trait DomEventListenerOwner {
    /// Attaches one event listener to the targeted node owned by this listener author.
    fn add_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<(), DomError>;

    /// Removes one matching listener registration from the targeted node.
    fn remove_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<bool, DomError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_listener(_context: &mut DomEventListenerContext<'_>) {}

    fn assert_listener_api_deduplicates_and_removes(
        owner: &mut impl DomEventListenerOwner,
        target: DomNodeSlotKey,
    ) {
        owner
            .add_event_listener(target, DomEventName::Click, noop_listener, None)
            .expect("first listener add should succeed");
        owner
            .add_event_listener(target, DomEventName::Click, noop_listener, None)
            .expect("duplicate listener add should stay a no-op");

        assert!(
            owner
                .remove_event_listener(target, DomEventName::Click, noop_listener, None)
                .expect("first listener removal should succeed")
        );
        assert!(
            !owner
                .remove_event_listener(target, DomEventName::Click, noop_listener, None)
                .expect("second listener removal should report no match")
        );
    }

    #[test]
    fn root_dom_implements_listener_owner_contract() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let target = root.create_element(DomTag::Button).expect("target create");
        root.append_child(root.document_node().0, target)
            .expect("target attach");

        assert_listener_api_deduplicates_and_removes(&mut root, target);
    }

    #[test]
    fn root_dom_add_event_listener_counts_only_new_registrations() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let target = root.create_element(DomTag::Button).expect("target create");
        root.append_child(root.document_node().0, target)
            .expect("target attach");

        root.add_event_listener(target, DomEventName::Click, noop_listener, None)
            .expect("first listener add should succeed");
        root.add_event_listener(target, DomEventName::Click, noop_listener, None)
            .expect("duplicate listener add should stay a no-op");

        assert_eq!(
            root.count(RootDomMetricName::EventListenerRegistrationAllocation),
            1
        );
    }

    #[test]
    fn root_dom_fragment_implements_listener_owner_contract() {
        let mut fragment = RootDomFragment::new();
        let target = fragment
            .create_element(DomTag::Button)
            .expect("target create");

        assert_listener_api_deduplicates_and_removes(&mut fragment, target);
    }
}
