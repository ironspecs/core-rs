//! Shared DOM-style event detail payloads.
//!
//! This file represents the event detail vocabulary used by `ui-dom` for local
//! dispatch and input rebroadcast. Its purpose is to keep event payload shapes
//! consistent across roots, nodes, and tests.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomEventDetail` enum and its `impl` blocks
//! - event detail shapes should stay strict and low-level
//! - app-specific event semantics must not be encoded here
//!
//! Advice and code philosophy:
//!
//! - prefer explicit event variants over loosely typed maps
//! - keep these shapes stable so tests and callers can depend on them
use crate::types::dom_event::{
    DomEventDetailKind, DomFocusEvent, DomKeyboardEvent, DomMouseEvent, DomPointerEvent,
    DomTransitionEvent, DomUiEvent, DomValueChangeEvent, DomWheelEvent,
};

#[cfg(test)]
use crate::types::dom_event::{DomModifierState, DomMouseButton, DomPointerType};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_kind_reports_matching_variant() {
        let pointer = DomEventDetail::Pointer(DomPointerEvent {
            pointer_id: 1,
            pointer_type: DomPointerType::Mouse,
            is_primary: true,
            button: Some(DomMouseButton::Primary),
            buttons: 1,
            client_x: 0.0,
            client_y: 0.0,
            modifiers: DomModifierState::default(),
        });
        assert_eq!(pointer.kind(), DomEventDetailKind::Pointer);

        let keyboard = DomEventDetail::Keyboard(DomKeyboardEvent {
            key: "a".to_string(),
            code: "KeyA".to_string(),
            repeat: false,
            modifiers: DomModifierState::default(),
        });
        assert_eq!(keyboard.kind(), DomEventDetailKind::Keyboard);

        let focus = DomEventDetail::Focus(DomFocusEvent::default());
        assert_eq!(focus.kind(), DomEventDetailKind::Focus);
    }
}
