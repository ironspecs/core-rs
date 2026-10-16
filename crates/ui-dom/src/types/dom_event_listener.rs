//! Shared DOM event-listener callback types.
//!
//! This file keeps listener callback identity and listener options together so
//! detached fragments and live roots can stay aligned when they both support
//! listener authoring. The listener-facing callback context
//! (`DomEventListenerContext`) lives in its own file and is re-exported here,
//! and the owner-facing listener mutation seam (`DomEventListenerOwner`) lives
//! in its own file.

use std::any::TypeId;
use std::hash::{Hash, Hasher};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub use crate::types::dom_event_listener_context::DomEventListenerContext;

use crate::types::dom_event_dispatch_context::DomEventDispatchContext;
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::DomNode;
use crate::types::root_dom::RootDom;

#[cfg(test)]
use crate::types::RootDomContext;
#[cfg(test)]
use crate::types::dom_event::DomEvent;
#[cfg(test)]
use crate::types::dom_event::{DomModifierState, DomMouseButton, DomMouseEvent};
#[cfg(test)]
use crate::types::dom_event_detail::DomEventDetail;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_listener(_context: &mut DomEventListenerContext<'_>) {}

    #[test]
    fn listener_new_assigns_unique_ids_and_invoke_calls_the_callback() {
        let listener_a = DomEventListener::new(|_| {});
        let listener_b = DomEventListener::new(|_| {});
        assert_ne!(listener_a, listener_b);
        assert_eq!(listener_a, listener_a);

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = Arc::clone(&calls);
        let listener = DomEventListener::new(move |_| {
            calls_for_listener.fetch_add(1, Ordering::SeqCst);
        });

        let mut root_dom = RootDom::new(RootDomContext::new_cpu());
        let mut dispatch = DomEventDispatchContext::new(DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        ));
        listener.invoke(&mut root_dom, &mut dispatch);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let from_fn_item = DomEventListener::from(noop_listener);
        assert!(format!("{from_fn_item:?}").contains("DomEventListener"));
    }
}
