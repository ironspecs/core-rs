//! Shared DOM event-listener callback context.
//!
//! This file keeps the listener-facing callback context that borrows the
//! listener's root and dispatch state so detached fragments and live roots can
//! stay aligned when they both support listener authoring. The listener itself
//! lives in `dom_event_listener.rs`, and the listener-owner contract lives in
//! `dom_event_listener_owner.rs`.

use crate::types::dom_event::{DomEvent, DomEventPhase};
use crate::types::dom_event_dispatch_context::DomEventDispatchContext;
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::root_dom::RootDom;

#[cfg(test)]
use crate::types::RootDomContext;
#[cfg(test)]
use crate::types::dom_event::{DomModifierState, DomMouseButton, DomMouseEvent};
#[cfg(test)]
use crate::types::dom_event_detail::DomEventDetail;
#[cfg(test)]
use crate::types::dom_event_name::DomEventName;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_accessors_delegate_to_dispatch() {
        let mut root_dom = RootDom::new(RootDomContext::new_cpu());
        let (document_key, _) = root_dom.document_node();

        let event = DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        );
        let mut dispatch = DomEventDispatchContext::new(event);
        dispatch.set_target_key(document_key);
        dispatch.enter_phase(DomEventPhase::AtTarget, document_key);

        let expected_phase = dispatch.phase();
        let expected_target = dispatch.target_key();
        let expected_current_target = dispatch.current_target_key();
        let event_ptr: *const DomEvent = dispatch.event();

        let context = DomEventListenerContext::new(&mut root_dom, &mut dispatch);

        assert!(std::ptr::eq(context.event(), event_ptr));
        assert_eq!(context.phase(), expected_phase);
        assert_eq!(context.target_key(), expected_target);
        assert_eq!(context.current_target_key(), expected_current_target);
        assert_eq!(context.root_dom().document_node().0, document_key);
    }
}
