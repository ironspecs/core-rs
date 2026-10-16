//! Shared DOM-style event details and dispatch context.
//!
//! This file represents the event vocabulary used by `ui-dom` for local
//! dispatch and input rebroadcast. Its purpose is to keep event names, phases,
//! detail payloads, and dispatch-context behavior consistent across roots,
//! nodes, and tests.
//!
//! Invariants and rules:
//!
//! - these types describe event data only; dispatch policy belongs elsewhere
//! - event detail shapes should stay strict and low-level
//! - app-specific event semantics must not be encoded here
//!
//! Advice and code philosophy:
//!
//! - prefer explicit event variants over loosely typed maps
//! - keep these shapes stable so tests and callers can depend on them
pub use crate::types::dom_event_listener::{DomEventListener, DomEventListenerOptions};
pub use crate::types::dom_event_name::DomEventName;
use crate::types::{DomAttribute, dom_node::DomNodeSlotKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomEventPhase {
    None,
    Capturing,
    AtTarget,
    Bubbling,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomSystemModifierState {
    pub meta_key: bool,
    pub shift_key: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomModifierState {
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub system: DomSystemModifierState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomPointerType {
    Mouse,
    Pen,
    Touch,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomMouseButton {
    Primary,
    Auxiliary,
    Secondary,
    Back,
    Forward,
    Other(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomWheelDeltaMode {
    Pixel,
    Line,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomUiEvent {
    pub detail: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomPointerEvent {
    pub pointer_id: i64,
    pub pointer_type: DomPointerType,
    pub is_primary: bool,
    pub button: Option<DomMouseButton>,
    pub buttons: u16,
    pub client_x: f32,
    pub client_y: f32,
    pub modifiers: DomModifierState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomMouseEvent {
    pub button: Option<DomMouseButton>,
    pub buttons: u16,
    pub client_x: f32,
    pub client_y: f32,
    pub modifiers: DomModifierState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomWheelEvent {
    pub delta_x: f32,
    pub delta_y: f32,
    pub delta_z: f32,
    pub delta_mode: DomWheelDeltaMode,
    pub client_x: f32,
    pub client_y: f32,
    pub modifiers: DomModifierState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomKeyboardEvent {
    pub key: String,
    pub code: String,
    pub repeat: bool,
    pub modifiers: DomModifierState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomEventDetailKind {
    Ui,
    Pointer,
    Mouse,
    Wheel,
    Keyboard,
    Focus,
    ValueChange,
    Transition,
}

/// Focus-family event payload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomFocusEvent {
    /// The previous or next focused element for this transition when known.
    pub related_target_key: Option<DomNodeSlotKey>,
}

/// Value-change payload for text-value DOM events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomValueChangeEvent {
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomTransitionEvent {
    pub attribute: DomAttribute,
    pub elapsed_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DomEventDetail {
    Ui(DomUiEvent),
    Pointer(DomPointerEvent),
    Mouse(DomMouseEvent),
    Wheel(DomWheelEvent),
    Keyboard(DomKeyboardEvent),
    Focus(DomFocusEvent),
    /// Payload for `input` and `change` style text-value events.
    ValueChange(DomValueChangeEvent),
    Transition(DomTransitionEvent),
}

impl DomEventDetail {
    #[must_use]
    pub(crate) const fn kind(&self) -> DomEventDetailKind {
        match self {
            Self::Ui(_) => DomEventDetailKind::Ui,
            Self::Pointer(_) => DomEventDetailKind::Pointer,
            Self::Mouse(_) => DomEventDetailKind::Mouse,
            Self::Wheel(_) => DomEventDetailKind::Wheel,
            Self::Keyboard(_) => DomEventDetailKind::Keyboard,
            Self::Focus(_) => DomEventDetailKind::Focus,
            Self::ValueChange(_) => DomEventDetailKind::ValueChange,
            Self::Transition(_) => DomEventDetailKind::Transition,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomEventPropagationState {
    pub stopped: bool,
    pub immediate_stopped: bool,
}

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

/// One concrete DOM event instance.
///
/// Invariant:
///
/// - `DomEvent` is move-only
/// - one logical event instance must have one owner at a time
/// - ownership may move through dispatch, `emitted_events`, and `update(...)`
///   return values, but the event must not be cloned into parallel copies
/// - `DomEvent` owns occurrence state only; fixed event-type behavior like
///   bubbling and cancelability stays in the immutable descriptor lookup table
///   and must not be duplicated or overridden per event instance
///
/// If a caller needs a duplicable projection for logging or snapshots, add a
/// separate snapshot type instead of reintroducing `Clone` here.
#[derive(Debug, PartialEq)]
pub struct DomEvent {
    pub name: DomEventName,
    pub default_prevented: bool,
    pub propagation: DomEventPropagationState,
    pub phase: DomEventPhase,
    pub target_key: Option<DomNodeSlotKey>,
    pub current_target_key: Option<DomNodeSlotKey>,
    pub detail: DomEventDetail,
}

impl DomEvent {
    #[must_use]
    pub(crate) fn new(name: DomEventName, detail: DomEventDetail) -> Self {
        let descriptor = name.descriptor();
        assert_eq!(
            descriptor.name, name,
            "event descriptor row should match requested event name"
        );
        assert_eq!(
            detail.kind(),
            descriptor.behavior.detail_kind,
            "event detail should match canonical descriptor kind"
        );
        Self {
            name,
            default_prevented: false,
            propagation: DomEventPropagationState::default(),
            phase: DomEventPhase::None,
            target_key: None,
            current_target_key: None,
            detail,
        }
    }

    /// Returns the fixed bubbling policy from the event type descriptor.
    #[must_use]
    pub fn bubbles(&self) -> bool {
        self.name.descriptor().behavior.bubbles
    }

    /// Returns the fixed cancelability policy from the event type descriptor.
    #[must_use]
    pub fn cancelable(&self) -> bool {
        self.name.descriptor().behavior.cancelable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_uses_canonical_policy() {
        let focus_in = DomEvent::new(
            DomEventName::FocusIn,
            DomEventDetail::Focus(DomFocusEvent::default()),
        );
        assert!(focus_in.bubbles());
        assert!(!focus_in.cancelable());

        let focus = DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        );
        assert!(!focus.bubbles());
        assert!(!focus.cancelable());

        let click = DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        );
        assert!(click.bubbles());
        assert!(click.cancelable());
    }

    #[test]
    #[should_panic(expected = "event detail should match canonical descriptor kind")]
    fn constructor_rejects_detail_kind_mismatch() {
        let _ = DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Keyboard(DomKeyboardEvent {
                key: "a".to_string(),
                code: "KeyA".to_string(),
                repeat: false,
                modifiers: DomModifierState::default(),
            }),
        );
    }

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
