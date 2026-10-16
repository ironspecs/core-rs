//! DOM event dispatch outcome for completed default dispatches.

use crate::types::dom_event::DomEvent;

#[cfg(test)]
use crate::types::{DomEventDetail, DomEventName, DomFocusEvent};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn focus_event() -> DomEvent {
        DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        )
    }

    #[test]
    fn dispatch_outcome_into_sink_eligible_event_follows_dispatch_state() {
        assert_eq!(
            DomEventDispatchOutcome::NotDispatched.into_sink_eligible_event(),
            None
        );

        let eligible =
            DomEventDispatchOutcome::Dispatched(focus_event()).into_sink_eligible_event();
        assert_eq!(eligible, Some(focus_event()));

        let mut prevented = focus_event();
        prevented.default_prevented = true;
        assert_eq!(
            DomEventDispatchOutcome::Dispatched(prevented).into_sink_eligible_event(),
            None
        );

        let mut stopped = focus_event();
        stopped.propagation.stopped = true;
        assert_eq!(
            DomEventDispatchOutcome::Dispatched(stopped).into_sink_eligible_event(),
            None
        );
    }
}
