//! Canonical internal DOM event policy for `ui-dom`.
//!
//! This file owns the fixed policy row shared by the event descriptor table:
//! bubbling, cancelability, and the expected detail payload family. These rows
//! are the one source of truth for event-type behavior; concrete `DomEvent`
//! values remain occurrence-only and must look up fixed behavior here instead
//! of carrying editable copies.

use crate::types::dom_event::DomEventDetailKind;

pub(crate) struct DomEventBehavior {
    pub bubbles: bool,
    pub cancelable: bool,
    pub detail_kind: DomEventDetailKind,
}

impl DomEventBehavior {
    #[must_use]
    pub(crate) const fn new(
        bubbles: bool,
        cancelable: bool,
        detail_kind: DomEventDetailKind,
    ) -> Self {
        Self {
            bubbles,
            cancelable,
            detail_kind,
        }
    }
}
