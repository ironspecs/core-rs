//! DOM-style event propagation over ancestor links.
//!
//! What this file represents:
//!
//! - the single capture → target → bubble walk that dispatches one already-
//!   targeted event to the listeners along the target's ancestor path
//!
//! Ownership boundary:
//!
//! - this file owns only the propagation walk and its sink-eligibility
//!   wrapper; target RESOLUTION (hit testing) stays in
//!   `domains/visitor/event_target.rs`, and event CONSTRUCTION + the
//!   "record-if-bubbled" queue decision stay at the `RootDom` call sites
//! - the walk borrows `&mut RootDom` because a listener can mutate the whole
//!   document through `DomEventListenerContext::root_dom_mut()`; narrowing
//!   that listener surface is E6's `BehaviorContext`, not this extraction
//!
//! Invariants and rules (the discovered propagation contract):
//!
//! - capture phase runs root → target's parent, firing only `capture: true`
//!   listeners at each ancestor, in `DomEventPhase::Capturing`
//! - the target phase fires the target's `capture: true` listeners first, then
//!   its `capture: false` listeners, both in `DomEventPhase::AtTarget`
//! - the bubble phase runs only for bubbling events, from the target's parent
//!   → root, firing `capture: false` listeners in `DomEventPhase::Bubbling`
//! - `stop_propagation` halts all later phases; `stop_immediate_propagation`
//!   additionally halts the remaining listeners on the current node
//! - a missing node mid-walk ends the listener loop for that node silently;
//!   a missing parent in an ancestor walk is an `InvalidNodeKey` error
//! - `once` listeners are removed after their first invocation, even when the
//!   dispatch then short-circuits
//! - the walk never allocates an ancestry path: capture resolves each ancestor
//!   by a bounded upward walk and bubble walks parents iteratively
//! - the "record if bubbled" behavior is NOT part of this walk — the walk
//!   returns the finished event, and the caller decides whether to queue it
//!   (sink-eligible + bubbling) as a journal decision at the call site
use crate::types::DomError;
use crate::types::dom_event::{DomEvent, DomEventPhase};
use crate::types::dom_event_dispatch_context::DomEventDispatchContext;
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::{DomDispatchedEventListener, DomNodeSlotKey};
use crate::types::root_dom::RootDom;

#[cfg(test)]
use crate::types::{
    DomEventDetail, DomEventListener, DomEventListenerOptions, DomFocusEvent, DomModifierState,
    DomMouseButton, DomMouseEvent, DomTag, RootDomContext,
};
#[cfg(test)]
use parking_lot::Mutex;
#[cfg(test)]
use std::sync::Arc;

/// Runs the full capture → target → bubble propagation for one targeted event
/// and returns the finished event (default-prevented and propagation-stop
/// state written back) for the caller's post-dispatch journal decision.
///
/// This is the E5 primitive: the walk that every dispatch family member
/// shares. Construction, target resolution, and the record-if-bubbled queue
/// decision stay at the call sites.
pub(crate) fn propagate(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    event: DomEvent,
) -> Result<DomEvent, DomError> {
    root_dom.node(target_key).ok_or(DomError::InvalidNodeKey)?;
    let mut dispatch = DomEventDispatchContext::new(event);
    dispatch.set_target_key(target_key);
    dispatch_capture_to_target(root_dom, target_key, &mut dispatch)?;
    if !dispatch.propagation_stopped() && !dispatch.immediate_propagation_stopped() {
        dispatch_target(root_dom, target_key, &mut dispatch);
    }
    if dispatch.event().bubbles()
        && !dispatch.propagation_stopped()
        && !dispatch.immediate_propagation_stopped()
    {
        dispatch_bubble_from_target_parent(root_dom, target_key, &mut dispatch)?;
    }
    Ok(dispatch.finish_with_event())
}

/// Dispatches one targeted event and returns the sink-eligible finished event,
/// or `None` when it was default-prevented or its propagation was stopped.
#[allow(dead_code)]
pub(crate) fn dispatch_event_to_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    event: DomEvent,
) -> Result<Option<DomEvent>, DomError> {
    let dispatched_event = propagate(root_dom, target_key, event)?;
    if dispatched_event.default_prevented || dispatched_event.propagation.stopped {
        return Ok(None);
    }
    Ok(Some(dispatched_event))
}

fn dispatch_capture_to_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) -> Result<(), DomError> {
    let ancestor_count = ancestor_count_to_root(root_dom, target_key)?;
    for distance_up in (1..=ancestor_count).rev() {
        let ancestor_key = ancestor_key_at_distance(root_dom, target_key, distance_up)?;
        dispatch.enter_phase(DomEventPhase::Capturing, ancestor_key);
        dispatch_node_phase_listeners(root_dom, ancestor_key, dispatch, true);
        if dispatch.propagation_stopped() || dispatch.immediate_propagation_stopped() {
            return Ok(());
        }
    }
    Ok(())
}

fn dispatch_target(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) {
    dispatch.enter_phase(DomEventPhase::AtTarget, target_key);
    dispatch_node_phase_listeners(root_dom, target_key, dispatch, true);
    if dispatch.immediate_propagation_stopped() {
        return;
    }
    dispatch_node_phase_listeners(root_dom, target_key, dispatch, false);
}

fn dispatch_bubble_from_target_parent(
    root_dom: &mut RootDom,
    target_key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
) -> Result<(), DomError> {
    let mut current_key = node_parent_key(root_dom, target_key)?;
    while let Some(key) = current_key {
        dispatch.enter_phase(DomEventPhase::Bubbling, key);
        dispatch_node_phase_listeners(root_dom, key, dispatch, false);
        if dispatch.propagation_stopped() || dispatch.immediate_propagation_stopped() {
            return Ok(());
        }
        current_key = node_parent_key(root_dom, key)?;
    }
    Ok(())
}

fn dispatch_node_phase_listeners(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    dispatch: &mut DomEventDispatchContext,
    capture: bool,
) {
    let event_name = dispatch.event().name;
    let Some(node) = root_dom.node(key) else {
        return;
    };
    let listeners = node.dispatched_listeners_for_phase(event_name, capture);
    for dispatched_listener in listeners {
        dispatched_listener.listener.invoke(root_dom, dispatch);
        if dispatched_listener.options.once {
            remove_once_listener_from_node(root_dom, key, event_name, &dispatched_listener);
        }
        if dispatch.immediate_propagation_stopped() {
            return;
        }
    }
}

fn remove_once_listener_from_node(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    event_name: DomEventName,
    dispatched_listener: &DomDispatchedEventListener,
) {
    let Some(node) = root_dom.nodes.get_mut(key) else {
        return;
    };
    let _removed = node.remove_event_listener(
        event_name,
        dispatched_listener.listener.clone(),
        Some(dispatched_listener.options),
    );
}

fn ancestor_count_to_root(
    root_dom: &RootDom,
    target_key: DomNodeSlotKey,
) -> Result<usize, DomError> {
    let mut count = 0;
    let mut current_key = node_parent_key(root_dom, target_key)?;
    while let Some(key) = current_key {
        count += 1;
        current_key = node_parent_key(root_dom, key)?;
    }
    Ok(count)
}

fn ancestor_key_at_distance(
    root_dom: &RootDom,
    target_key: DomNodeSlotKey,
    distance_up: usize,
) -> Result<DomNodeSlotKey, DomError> {
    let mut remaining = distance_up;
    let mut current_key = target_key;
    while remaining > 0 {
        let Some(parent_key) = node_parent_key(root_dom, current_key)? else {
            return Err(DomError::InvalidNodeKey);
        };
        current_key = parent_key;
        remaining -= 1;
    }
    Ok(current_key)
}

fn node_parent_key(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
    Ok(node.parent_key())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_event() -> DomEvent {
        DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        )
    }

    fn focus_event() -> DomEvent {
        DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        )
    }

    fn tree_root_a_b() -> (RootDom, DomNodeSlotKey, DomNodeSlotKey, DomNodeSlotKey) {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document_key = root.root;
        let a = root.create_element(DomTag::Div).expect("a create");
        let b = root.create_element(DomTag::Button).expect("b create");
        root.append_child(document_key, a).expect("append a");
        root.append_child(a, b).expect("append b");
        (root, document_key, a, b)
    }

    fn recording_listener(
        visits: &Arc<Mutex<Vec<(DomEventPhase, DomNodeSlotKey)>>>,
    ) -> DomEventListener {
        let visits = Arc::clone(visits);
        DomEventListener::new(move |context| {
            visits.lock().push((
                context.phase(),
                context
                    .current_target_key()
                    .expect("listener should observe a current target"),
            ));
        })
    }

    #[test]
    fn propagate_runs_capture_then_target_then_bubble_in_ancestor_order() {
        let (mut root, document_key, a, b) = tree_root_a_b();
        let visits = Arc::new(Mutex::new(Vec::new()));
        for key in [document_key, a, b] {
            root.add_event_listener(
                key,
                DomEventName::Click,
                recording_listener(&visits),
                Some(DomEventListenerOptions {
                    capture: true,
                    once: false,
                    passive: false,
                }),
            )
            .expect("capture listener add should succeed");
            root.add_event_listener(
                key,
                DomEventName::Click,
                recording_listener(&visits),
                Some(DomEventListenerOptions {
                    capture: false,
                    once: false,
                    passive: false,
                }),
            )
            .expect("bubble listener add should succeed");
        }

        let _ = propagate(&mut root, b, click_event()).expect("propagate should succeed");

        assert_eq!(
            *visits.lock(),
            vec![
                (DomEventPhase::Capturing, document_key),
                (DomEventPhase::Capturing, a),
                (DomEventPhase::AtTarget, b),
                (DomEventPhase::AtTarget, b),
                (DomEventPhase::Bubbling, a),
                (DomEventPhase::Bubbling, document_key),
            ],
            "propagation should visit capture (root → target), target, then bubble (target → root)"
        );
    }

    #[test]
    fn target_phase_fires_capture_listener_before_bubble_listener() {
        let (mut root, _document_key, _a, b) = tree_root_a_b();
        let visits = Arc::new(Mutex::new(Vec::new()));
        root.add_event_listener(
            b,
            DomEventName::Click,
            recording_listener(&visits),
            Some(DomEventListenerOptions {
                capture: false,
                once: false,
                passive: false,
            }),
        )
        .expect("bubble listener add should succeed");
        root.add_event_listener(
            b,
            DomEventName::Click,
            recording_listener(&visits),
            Some(DomEventListenerOptions {
                capture: true,
                once: false,
                passive: false,
            }),
        )
        .expect("capture listener add should succeed");

        let _ = propagate(&mut root, b, click_event()).expect("propagate should succeed");

        assert_eq!(
            *visits.lock(),
            vec![(DomEventPhase::AtTarget, b), (DomEventPhase::AtTarget, b),],
            "the target phase should run the capture listener before the bubble listener"
        );
    }

    #[test]
    fn stop_propagation_during_capture_halts_target_and_bubble() {
        let (mut root, document_key, _a, b) = tree_root_a_b();
        let visits = Arc::new(Mutex::new(Vec::new()));
        root.add_event_listener(
            document_key,
            DomEventName::Click,
            DomEventListener::new(|context| {
                context.stop_propagation();
            }),
            Some(DomEventListenerOptions {
                capture: true,
                once: false,
                passive: false,
            }),
        )
        .expect("stopping capture listener should attach");
        root.add_event_listener(b, DomEventName::Click, recording_listener(&visits), None)
            .expect("listener add should succeed");

        let _ = propagate(&mut root, b, click_event()).expect("propagate should succeed");

        assert!(
            visits.lock().is_empty(),
            "a capture-phase stop should halt the target and bubble phases"
        );
    }

    #[test]
    fn stop_propagation_on_an_ancestor_halts_further_bubble_visits() {
        let (mut root, document_key, a, b) = tree_root_a_b();
        let visits = Arc::new(Mutex::new(Vec::new()));
        root.add_event_listener(
            a,
            DomEventName::Click,
            DomEventListener::new(|context| {
                context.stop_propagation();
            }),
            None,
        )
        .expect("stopping listener should attach");
        root.add_event_listener(
            document_key,
            DomEventName::Click,
            recording_listener(&visits),
            None,
        )
        .expect("listener add should succeed");
        root.add_event_listener(b, DomEventName::Click, recording_listener(&visits), None)
            .expect("listener add should succeed");

        let _ = propagate(&mut root, b, click_event()).expect("propagate should succeed");

        assert_eq!(
            *visits.lock(),
            vec![(DomEventPhase::AtTarget, b)],
            "the target fires, then the stopping ancestor halts the bubble before the root"
        );
    }

    #[test]
    fn non_bubbling_event_only_visits_the_target() {
        let (mut root, document_key, _a, b) = tree_root_a_b();
        let visits = Arc::new(Mutex::new(Vec::new()));
        for key in [document_key, b] {
            root.add_event_listener(key, DomEventName::Focus, recording_listener(&visits), None)
                .expect("listener add should succeed");
        }

        let _ = propagate(&mut root, b, focus_event()).expect("propagate should succeed");

        assert_eq!(
            *visits.lock(),
            vec![(DomEventPhase::AtTarget, b)],
            "a non-bubbling event should not run capture or bubble phases"
        );
    }

    #[test]
    fn sink_eligible_dispatch_returns_bubbled_event_and_drops_prevented_event() {
        let (mut root, _document_key, _a, b) = tree_root_a_b();
        root.add_event_listener(
            b,
            DomEventName::Click,
            DomEventListener::new(|context| {
                context.prevent_default();
            }),
            None,
        )
        .expect("preventing listener should attach");

        let prevented =
            dispatch_event_to_target(&mut root, b, click_event()).expect("dispatch should succeed");
        assert_eq!(
            prevented, None,
            "a default-prevented event is not sink-eligible"
        );

        let (mut clean_root, _document_key, _a, clean_b) = tree_root_a_b();
        let eligible = dispatch_event_to_target(&mut clean_root, clean_b, click_event())
            .expect("dispatch should succeed");
        assert!(
            eligible.is_some(),
            "an un-prevented bubbling event should be sink-eligible"
        );
    }
}
