//! Shared DOM event-listener callback types and listener-owner contract.
//!
//! This file keeps listener callback identity, listener options, and the
//! common owner-facing listener mutation seam together so detached fragments
//! and live roots can stay aligned when they both support listener authoring.

use std::any::TypeId;
use std::hash::{Hash, Hasher};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::types::DomError;
use crate::types::dom_event::{DomEvent, DomEventDispatchContext, DomEventPhase};
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::DomNode;
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::root_dom::RootDom;

#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use crate::types::root_dom_context::RootDomContext;
#[cfg(test)]
use crate::types::root_dom_fragment::RootDomFragment;
#[cfg(test)]
use crate::types::root_dom_metrics::RootDomMetricName;

pub struct DomEventListenerContext<'a> {
    root_dom: &'a mut RootDom,
    dispatch: &'a mut DomEventDispatchContext,
}

impl<'a> DomEventListenerContext<'a> {
    #[must_use]
    pub(crate) fn new(
        root_dom: &'a mut RootDom,
        dispatch: &'a mut DomEventDispatchContext,
    ) -> Self {
        Self { root_dom, dispatch }
    }

    #[must_use]
    pub fn event(&self) -> &DomEvent {
        self.dispatch.event()
    }

    #[must_use]
    pub fn phase(&self) -> DomEventPhase {
        self.dispatch.phase()
    }

    #[must_use]
    pub fn target_key(&self) -> Option<DomNodeSlotKey> {
        self.dispatch.target_key()
    }

    #[must_use]
    pub fn current_target_key(&self) -> Option<DomNodeSlotKey> {
        self.dispatch.current_target_key()
    }

    #[must_use]
    pub fn default_prevented(&self) -> bool {
        self.dispatch.default_prevented()
    }

    pub fn prevent_default(&mut self) {
        self.dispatch.prevent_default();
    }

    pub fn stop_propagation(&mut self) {
        self.dispatch.stop_propagation();
    }

    pub fn stop_immediate_propagation(&mut self) {
        self.dispatch.stop_immediate_propagation();
    }

    #[must_use]
    pub fn root_dom(&self) -> &RootDom {
        self.root_dom
    }

    pub fn root_dom_mut(&mut self) -> &mut RootDom {
        self.root_dom
    }
}

/// One DOM listener callback operating on visitor-owned dispatch state.
#[derive(Clone)]
pub struct DomEventListener {
    id: usize,
    callback: Arc<dyn for<'a> Fn(&mut DomEventListenerContext<'a>) + Send + Sync + 'static>,
}

impl DomEventListener {
    /// Creates one listener from a capturing closure or function item.
    #[must_use]
    pub fn new(
        callback: impl for<'a> Fn(&mut DomEventListenerContext<'a>) + Send + Sync + 'static,
    ) -> Self {
        static NEXT_DOM_EVENT_LISTENER_ID: AtomicUsize = AtomicUsize::new(1);
        Self {
            id: NEXT_DOM_EVENT_LISTENER_ID.fetch_add(1, Ordering::Relaxed),
            callback: Arc::new(callback),
        }
    }

    pub(crate) fn invoke(&self, root: &mut RootDom, dispatch: &mut DomEventDispatchContext) {
        let mut context = DomEventListenerContext::new(root, dispatch);
        (self.callback)(&mut context);
    }
}

impl std::fmt::Debug for DomEventListener {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DomEventListener")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl PartialEq for DomEventListener {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for DomEventListener {}

impl<F> From<F> for DomEventListener
where
    F: for<'a> Fn(&mut DomEventListenerContext<'a>) + Send + Sync + Copy + 'static,
{
    fn from(listener: F) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        TypeId::of::<F>().hash(&mut hasher);
        Self {
            id: usize::try_from(hasher.finish())
                .expect("event listener hash should fit into usize"),
            callback: Arc::new(listener),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomEventListenerOptions {
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

pub(crate) fn add_event_listener_to_node(
    node: &mut DomNode,
    event_name: DomEventName,
    listener: impl Into<DomEventListener>,
    options: Option<DomEventListenerOptions>,
) -> bool {
    let listener = listener.into();
    let already_registered = node.has_event_listener(event_name, listener.clone(), options);
    node.add_event_listener(event_name, listener, options);
    !already_registered
}

pub(crate) fn remove_event_listener_from_node(
    node: &mut DomNode,
    event_name: DomEventName,
    listener: impl Into<DomEventListener>,
    options: Option<DomEventListenerOptions>,
) -> bool {
    node.remove_event_listener(event_name, listener, options)
}

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
