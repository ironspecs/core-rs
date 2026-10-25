//! Pointer-input event construction and classification leaves.
//!
//! What this file represents:
//!
//! - the pure, stateless helpers that turn a pointer snapshot into event data
//!   or a transition classification: `event_point` (event → point),
//!   `pointer_moved`/`pointer_pressed`/`pointer_released` (previous-vs-current
//!   transition predicates), and `hover_pointer_event` (pointer → a synthetic
//!   mouse `DomPointerEvent`)
//!
//! Ownership boundary:
//!
//! - these leaves read only their parameters — no `RootDom` state — so they
//!   live here in `types/` rather than in `root_dom.rs`
//! - `hover_layer_intersects_removed_keys` deliberately STAYS in `root_dom.rs`:
//!   it is a hover-layer purge predicate (an E3-domain concern whose only
//!   caller is `clear_hover_for_removed_keys`), not a pointer-event
//!   constructor, and its parameter type `HoveredLayerEntry` is root-owned
//!
//! Invariants and rules:
//!
//! - `pointer_moved`/`pointer_pressed`/`pointer_released` classify a
//!   previous-vs-current pointer transition; a `None` previous pointer means
//!   "no prior snapshot", so press/release edges only fire on an actual edge
//! - `hover_pointer_event` always synthesizes a primary mouse pointer with no
//!   button, mirroring the hover pseudo-state's single-pointer model
//! - these helpers never allocate and never touch retained state
use crate::types::dom_event::{
    DomEvent, DomEventDetail, DomModifierState, DomPointerEvent, DomPointerType,
};
use crate::types::dom_pseudo_state::DomPointerState;
use crate::types::primitives::PointPx;

#[cfg(test)]
use crate::types::{DomEventName, DomFocusEvent};

/// Extracts the root-local point carried by a pointer- or wheel-shaped event,
/// or `None` for events without a spatial payload.
pub(crate) fn event_point(event: &DomEvent) -> Option<PointPx> {
    match event.detail {
        DomEventDetail::Pointer(detail) => Some(PointPx {
            x_px: detail.client_x,
            y_px: detail.client_y,
        }),
        DomEventDetail::Wheel(detail) => Some(PointPx {
            x_px: detail.client_x,
            y_px: detail.client_y,
        }),
        _ => None,
    }
}

/// Reports whether the pointer moved between the previous and current snapshot.
pub(crate) fn pointer_moved(
    previous_pointer: Option<DomPointerState>,
    pointer: DomPointerState,
) -> bool {
    previous_pointer.is_none_or(|previous| {
        (previous.x_px - pointer.x_px).abs() > f32::EPSILON
            || (previous.y_px - pointer.y_px).abs() > f32::EPSILON
    })
}

/// Builds the synthetic mouse pointer event that drives hover pseudo-state.
pub(crate) fn hover_pointer_event(pointer: DomPointerState) -> DomPointerEvent {
    DomPointerEvent {
        pointer_id: 1,
        pointer_type: DomPointerType::Mouse,
        is_primary: true,
        button: None,
        buttons: u16::from(pointer.primary_button_down),
        client_x: pointer.x_px,
        client_y: pointer.y_px,
        modifiers: DomModifierState::default(),
    }
}

/// Reports whether the primary button pressed between the previous and current
/// snapshot (a rising edge).
pub(crate) fn pointer_pressed(
    previous_pointer: Option<DomPointerState>,
    pointer: DomPointerState,
) -> bool {
    previous_pointer.map_or(pointer.primary_button_down, |previous| {
        !previous.primary_button_down && pointer.primary_button_down
    })
}

/// Reports whether the primary button released between the previous and
/// current snapshot (a falling edge).
pub(crate) fn pointer_released(
    previous_pointer: Option<DomPointerState>,
    pointer: DomPointerState,
) -> bool {
    previous_pointer
        .is_some_and(|previous| previous.primary_button_down && !pointer.primary_button_down)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_at(x_px: f32, y_px: f32, primary_button_down: bool) -> DomPointerState {
        DomPointerState {
            x_px,
            y_px,
            primary_button_down,
            scroll_delta_y_px: 0.0,
        }
    }

    #[test]
    fn event_point_reads_pointer_and_wheel_coordinates_and_ignores_others() {
        let pointer = DomEvent::new(
            DomEventName::PointerMove,
            DomEventDetail::Pointer(DomPointerEvent {
                pointer_id: 1,
                pointer_type: DomPointerType::Mouse,
                is_primary: true,
                button: None,
                buttons: 0,
                client_x: 12.0,
                client_y: 34.0,
                modifiers: DomModifierState::default(),
            }),
        );
        assert_eq!(
            event_point(&pointer),
            Some(PointPx {
                x_px: 12.0,
                y_px: 34.0,
            })
        );

        let focus = DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        );
        assert_eq!(event_point(&focus), None);
    }

    #[test]
    fn pointer_moved_detects_position_change_and_first_snapshot() {
        let previous = pointer_at(0.0, 0.0, false);
        let same = pointer_at(0.0, 0.0, false);
        let moved = pointer_at(1.0, 0.0, false);

        assert!(!pointer_moved(Some(previous), same));
        assert!(pointer_moved(Some(previous), moved));
        assert!(pointer_moved(None, moved));
    }

    #[test]
    fn hover_pointer_event_is_a_primary_mouse_with_no_button() {
        let pointer = pointer_at(5.0, 6.0, true);
        let event = hover_pointer_event(pointer);

        assert_eq!(event.pointer_id, 1);
        assert_eq!(event.pointer_type, DomPointerType::Mouse);
        assert!(event.is_primary);
        assert_eq!(event.button, None);
        assert_eq!(event.buttons, 1);
        assert!((event.client_x - 5.0).abs() <= f32::EPSILON);
        assert!((event.client_y - 6.0).abs() <= f32::EPSILON);
    }

    #[test]
    fn pointer_pressed_reports_only_the_rising_edge() {
        assert!(pointer_pressed(None, pointer_at(0.0, 0.0, true)));
        assert!(!pointer_pressed(None, pointer_at(0.0, 0.0, false)));
        assert!(pointer_pressed(
            Some(pointer_at(0.0, 0.0, false)),
            pointer_at(0.0, 0.0, true)
        ));
        assert!(!pointer_pressed(
            Some(pointer_at(0.0, 0.0, true)),
            pointer_at(0.0, 0.0, true)
        ));
    }

    #[test]
    fn pointer_released_reports_only_the_falling_edge() {
        assert!(!pointer_released(None, pointer_at(0.0, 0.0, false)));
        assert!(pointer_released(
            Some(pointer_at(0.0, 0.0, true)),
            pointer_at(0.0, 0.0, false)
        ));
        assert!(!pointer_released(
            Some(pointer_at(0.0, 0.0, false)),
            pointer_at(0.0, 0.0, false)
        ));
    }
}
