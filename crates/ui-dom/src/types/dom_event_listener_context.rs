//! Shared DOM event-listener callback context.
//!
//! This file keeps the listener-facing callback context that borrows the
//! listener's root and dispatch state so detached fragments and live roots can
//! stay aligned when they both support listener authoring. The listener itself
//! and the listener-owner contract live in `dom_event_listener.rs`.

use crate::types::dom_event::{DomEvent, DomEventPhase};
use crate::types::dom_event_dispatch_context::DomEventDispatchContext;
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::root_dom::RootDom;

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
