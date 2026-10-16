//! Shared DOM-style event payloads and occurrence state.
//!
//! This file represents the event vocabulary used by `ui-dom` for local
//! dispatch and input rebroadcast. Its purpose is to keep event names, phases,
//! payload shapes, and occurrence state consistent across roots, nodes, and
//! tests. The `DomEventDetail` payload vocabulary and the listener-facing
//! `DomEventDispatchContext` live in their own files and are re-exported here.
//!
//! Invariants and rules:
//!
//! - these types describe event data only; dispatch policy belongs elsewhere
//! - event payload shapes should stay strict and low-level
//! - app-specific event semantics must not be encoded here
//!
//! Advice and code philosophy:
//!
//! - prefer explicit event variants over loosely typed maps
//! - keep these shapes stable so tests and callers can depend on them
pub use crate::types::dom_event_detail::DomEventDetail;
pub use crate::types::dom_event_dispatch_context::DomEventDispatchContext;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DomEventPropagationState {
    pub stopped: bool,
    pub immediate_stopped: bool,
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
}
