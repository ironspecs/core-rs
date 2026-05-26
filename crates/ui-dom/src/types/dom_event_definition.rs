//! Canonical internal DOM event definitions for `ui-dom`.
//!
//! This file owns the fixed descriptor table for event policy and expected
//! payload family. These rows are the one source of truth for event-type
//! behavior; concrete `DomEvent` values remain occurrence-only and must look up
//! fixed behavior here instead of carrying editable copies.

use crate::types::{DomEventName, dom_event::DomEventDetailKind};

pub(crate) struct DomEventDescriptor {
    pub name: DomEventName,
    pub dom_name: &'static str,
    pub behavior: DomEventBehavior,
}

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

struct DomEventDescriptorBuilder {
    descriptor: DomEventDescriptor,
}

impl DomEventDescriptorBuilder {
    #[must_use]
    const fn new(name: DomEventName) -> Self {
        Self {
            descriptor: DomEventDescriptor {
                name,
                dom_name: "",
                behavior: DomEventBehavior::new(false, false, DomEventDetailKind::Ui),
            },
        }
    }

    #[must_use]
    const fn dom_name(mut self, dom_name: &'static str) -> Self {
        self.descriptor.dom_name = dom_name;
        self
    }

    #[must_use]
    const fn behavior(mut self, behavior: DomEventBehavior) -> Self {
        self.descriptor.behavior = behavior;
        self
    }

    #[must_use]
    const fn build(self) -> DomEventDescriptor {
        self.descriptor
    }
}

// Canonical built-in event policy rows.
//
// These definitions are immutable after construction and are consulted through
// `DomEventName::descriptor()`. Concrete `DomEvent` values must not duplicate
// or override this type behavior per occurrence.
const DOM_EVENT_DESCRIPTORS: [DomEventDescriptor; DomEventName::COUNT] = [
    DomEventDescriptorBuilder::new(DomEventName::PointerDown)
        .dom_name("pointerdown")
        .behavior(DomEventBehavior::new(
            true,
            true,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::PointerUp)
        .dom_name("pointerup")
        .behavior(DomEventBehavior::new(
            true,
            true,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::PointerMove)
        .dom_name("pointermove")
        .behavior(DomEventBehavior::new(
            true,
            true,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::PointerEnter)
        .dom_name("pointerenter")
        .behavior(DomEventBehavior::new(
            false,
            false,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::PointerLeave)
        .dom_name("pointerleave")
        .behavior(DomEventBehavior::new(
            false,
            false,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::PointerCancel)
        .dom_name("pointercancel")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::Pointer,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Click)
        .dom_name("click")
        .behavior(DomEventBehavior::new(true, true, DomEventDetailKind::Mouse))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::DoubleClick)
        .dom_name("dblclick")
        .behavior(DomEventBehavior::new(true, true, DomEventDetailKind::Mouse))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::ContextMenu)
        .dom_name("contextmenu")
        .behavior(DomEventBehavior::new(true, true, DomEventDetailKind::Mouse))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Wheel)
        .dom_name("wheel")
        .behavior(DomEventBehavior::new(true, true, DomEventDetailKind::Wheel))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::KeyDown)
        .dom_name("keydown")
        .behavior(DomEventBehavior::new(
            true,
            true,
            DomEventDetailKind::Keyboard,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::KeyUp)
        .dom_name("keyup")
        .behavior(DomEventBehavior::new(
            true,
            true,
            DomEventDetailKind::Keyboard,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Focus)
        .dom_name("focus")
        .behavior(DomEventBehavior::new(
            false,
            false,
            DomEventDetailKind::Focus,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Blur)
        .dom_name("blur")
        .behavior(DomEventBehavior::new(
            false,
            false,
            DomEventDetailKind::Focus,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::FocusIn)
        .dom_name("focusin")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::Focus,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::FocusOut)
        .dom_name("focusout")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::Focus,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Input)
        .dom_name("input")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::ValueChange,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::Change)
        .dom_name("change")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::ValueChange,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::TransitionEnd)
        .dom_name("transitionend")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::Transition,
        ))
        .build(),
    DomEventDescriptorBuilder::new(DomEventName::TransitionCancel)
        .dom_name("transitioncancel")
        .behavior(DomEventBehavior::new(
            true,
            false,
            DomEventDetailKind::Transition,
        ))
        .build(),
];

impl DomEventName {
    #[must_use]
    pub(crate) fn descriptor(self) -> &'static DomEventDescriptor {
        &DOM_EVENT_DESCRIPTORS[self.index()]
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{DomEventName, dom_event::DomEventDetailKind};

    #[test]
    fn descriptor_array_covers_every_event_name_once() {
        let names = [
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
        ];
        assert_eq!(names.len(), DomEventName::COUNT);
        for name in names {
            let descriptor = name.descriptor();
            assert_eq!(descriptor.name, name);
        }
    }

    #[test]
    fn descriptors_expose_expected_policy_rows() {
        let pointer_enter = DomEventName::PointerEnter.descriptor();
        assert_eq!(pointer_enter.dom_name, "pointerenter");
        assert!(!pointer_enter.behavior.bubbles);
        assert!(!pointer_enter.behavior.cancelable);
        assert_eq!(
            pointer_enter.behavior.detail_kind,
            DomEventDetailKind::Pointer
        );

        let focus_in = DomEventName::FocusIn.descriptor();
        assert_eq!(focus_in.dom_name, "focusin");
        assert!(focus_in.behavior.bubbles);
        assert!(!focus_in.behavior.cancelable);
        assert_eq!(focus_in.behavior.detail_kind, DomEventDetailKind::Focus);

        let change = DomEventName::Change.descriptor();
        assert_eq!(change.dom_name, "change");
        assert!(change.behavior.bubbles);
        assert!(!change.behavior.cancelable);
        assert_eq!(change.behavior.detail_kind, DomEventDetailKind::ValueChange);
    }

    #[test]
    fn event_name_count_and_index_cover_the_descriptor_rows() {
        let names = [
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
        ];
        assert_eq!(names.len(), DomEventName::COUNT);
        for (index, name) in names.into_iter().enumerate() {
            assert_eq!(name.index(), index);
            assert_eq!(name.descriptor().name, name);
        }
    }
}
