//! Shared DOM-style event dispatch context.
//!
//! This file represents the listener-facing dispatch facade used by `ui-dom`
//! for local dispatch and input rebroadcast. Its purpose is to keep dispatch
//! phase, propagation, and default-prevention behavior consistent across
//! roots, nodes, and tests.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomEventDispatchContext` struct and its `impl` blocks
//! - the context describes event data only; dispatch policy belongs elsewhere
//! - listener calls must not mutate the underlying `DomEvent` payload directly
//!
//! Advice and code philosophy:
//!
//! - keep the dispatch facade explicit and narrow
//! - keep these shapes stable so tests and callers can depend on them
use crate::types::dom_event::{DomEvent, DomEventPhase, DomEventPropagationState};
use crate::types::dom_node::DomNodeSlotKey;

#[cfg(test)]
use crate::types::dom_event::{DomFocusEvent, DomModifierState, DomMouseButton, DomMouseEvent};
#[cfg(test)]
use crate::types::dom_event_detail::DomEventDetail;
#[cfg(test)]
use crate::types::dom_event_name::DomEventName;

/// Browser-shaped listener facade over visitor-owned DOM dispatch state.
///
/// Listeners use this to read immutable event data and to control propagation
/// without mutating the underlying [`DomEvent`] payload.
pub struct DomEventDispatchContext {
    event: DomEvent,
    target_key: Option<DomNodeSlotKey>,
    current_target_key: Option<DomNodeSlotKey>,
    phase: DomEventPhase,
    default_prevented: bool,
    propagation_stopped: bool,
    immediate_propagation_stopped: bool,
}

impl DomEventDispatchContext {
    /// Returns the immutable event payload being dispatched.
    #[must_use]
    pub fn event(&self) -> &DomEvent {
        &self.event
    }

    /// Returns the current observable DOM phase for this listener call.
    #[must_use]
    pub fn phase(&self) -> DomEventPhase {
        self.phase
    }

    /// Returns the stable target node key for the current dispatch.
    #[must_use]
    pub fn target_key(&self) -> Option<DomNodeSlotKey> {
        self.target_key
    }

    /// Returns the current node key receiving this listener invocation.
    #[must_use]
    pub fn current_target_key(&self) -> Option<DomNodeSlotKey> {
        self.current_target_key
    }

    /// Returns whether a prior listener has prevented the default action.
    #[must_use]
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// Prevents the default action when the event is cancelable.
    pub fn prevent_default(&mut self) {
        if self.event.cancelable() {
            self.default_prevented = true;
        }
    }

    /// Stops propagation to later nodes after the current listener returns.
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// Stops later listeners on this node and stops propagation to later nodes.
    pub fn stop_immediate_propagation(&mut self) {
        self.immediate_propagation_stopped = true;
        self.propagation_stopped = true;
    }

    #[must_use]
    pub(crate) fn new(event: DomEvent) -> Self {
        Self {
            event,
            target_key: None,
            current_target_key: None,
            phase: DomEventPhase::None,
            default_prevented: false,
            propagation_stopped: false,
            immediate_propagation_stopped: false,
        }
    }

    pub(crate) fn set_target_key(&mut self, target_key: DomNodeSlotKey) {
        self.target_key = Some(target_key);
    }

    pub(crate) fn enter_phase(&mut self, phase: DomEventPhase, current_target_key: DomNodeSlotKey) {
        self.phase = phase;
        self.current_target_key = Some(current_target_key);
        self.immediate_propagation_stopped = false;
    }

    #[must_use]
    pub(crate) fn propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    #[must_use]
    pub(crate) fn immediate_propagation_stopped(&self) -> bool {
        self.immediate_propagation_stopped
    }

    #[must_use]
    pub(crate) fn finish_with_event(mut self) -> DomEvent {
        self.phase = DomEventPhase::None;
        self.current_target_key = None;
        self.event.default_prevented = self.default_prevented;
        self.event.propagation = DomEventPropagationState {
            stopped: self.propagation_stopped,
            immediate_stopped: self.immediate_propagation_stopped,
        };
        self.event.phase = DomEventPhase::None;
        self.event.target_key = self.target_key;
        self.event.current_target_key = None;
        self.event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevent_default_consults_descriptor_cancelability() {
        let mut focus_dispatch = DomEventDispatchContext::new(DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        ));
        focus_dispatch.prevent_default();
        assert!(!focus_dispatch.default_prevented());

        let mut click_dispatch = DomEventDispatchContext::new(DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        ));
        click_dispatch.prevent_default();
        assert!(click_dispatch.default_prevented());
    }
}
