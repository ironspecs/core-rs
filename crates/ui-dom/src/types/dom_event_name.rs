//! Public DOM event identities for `ui-dom`.
//!
//! This file separates event names from payload and policy details so callers
//! can reference the vocabulary without depending on internal descriptor data.

use crate::types::dom_event_definition::{DOM_EVENT_DESCRIPTORS, DomEventDescriptor};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DomEventName {
    PointerDown,
    PointerUp,
    PointerMove,
    PointerEnter,
    PointerLeave,
    PointerCancel,
    Click,
    DoubleClick,
    ContextMenu,
    Wheel,
    KeyDown,
    KeyUp,
    Focus,
    Blur,
    FocusIn,
    FocusOut,
    Input,
    Change,
    TransitionEnd,
    TransitionCancel,
}

impl DomEventName {
    pub const COUNT: usize = 20;

    #[must_use]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::PointerDown => 0,
            Self::PointerUp => 1,
            Self::PointerMove => 2,
            Self::PointerEnter => 3,
            Self::PointerLeave => 4,
            Self::PointerCancel => 5,
            Self::Click => 6,
            Self::DoubleClick => 7,
            Self::ContextMenu => 8,
            Self::Wheel => 9,
            Self::KeyDown => 10,
            Self::KeyUp => 11,
            Self::Focus => 12,
            Self::Blur => 13,
            Self::FocusIn => 14,
            Self::FocusOut => 15,
            Self::Input => 16,
            Self::Change => 17,
            Self::TransitionEnd => 18,
            Self::TransitionCancel => 19,
        }
    }
}

impl DomEventName {
    #[must_use]
    pub(crate) fn descriptor(self) -> &'static DomEventDescriptor {
        &DOM_EVENT_DESCRIPTORS[self.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_lookup_returns_matching_rows() {
        let click = DomEventName::Click.descriptor();
        assert_eq!(click.name, DomEventName::Click);
        assert_eq!(click.dom_name, "click");
        assert!(click.behavior.bubbles);
        assert!(click.behavior.cancelable);

        let focus = DomEventName::Focus.descriptor();
        assert!(!focus.behavior.bubbles);

        for name in [
            DomEventName::PointerDown,
            DomEventName::PointerUp,
            DomEventName::PointerMove,
            DomEventName::PointerEnter,
            DomEventName::PointerLeave,
            DomEventName::PointerCancel,
            DomEventName::Click,
            DomEventName::DoubleClick,
            DomEventName::ContextMenu,
            DomEventName::Wheel,
            DomEventName::KeyDown,
            DomEventName::KeyUp,
            DomEventName::Focus,
            DomEventName::Blur,
            DomEventName::FocusIn,
            DomEventName::FocusOut,
            DomEventName::Input,
            DomEventName::Change,
            DomEventName::TransitionEnd,
            DomEventName::TransitionCancel,
        ] {
            assert_eq!(name.descriptor().name, name);
        }
    }
}
