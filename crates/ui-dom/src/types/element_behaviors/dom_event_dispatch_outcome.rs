//! DOM event dispatch outcome for completed default dispatches.

use crate::types::dom_event::DomEvent;

pub(crate) enum DomEventDispatchOutcome {
    NotDispatched,
    Dispatched(DomEvent),
}

impl DomEventDispatchOutcome {
    #[must_use]
    pub(crate) fn default_prevented(&self) -> bool {
        match self {
            Self::NotDispatched => false,
            Self::Dispatched(event) => event.default_prevented,
        }
    }

    #[must_use]
    pub(crate) fn into_sink_eligible_event(self) -> Option<DomEvent> {
        match self {
            Self::Dispatched(event) if !event.default_prevented && !event.propagation.stopped => {
                Some(event)
            }
            Self::NotDispatched | Self::Dispatched(_) => None,
        }
    }

    #[must_use]
    pub(crate) fn into_sink_eligible_bubbled_event(self) -> Option<DomEvent> {
        self.into_sink_eligible_event()
            .filter(crate::types::DomEvent::bubbles)
    }
}
