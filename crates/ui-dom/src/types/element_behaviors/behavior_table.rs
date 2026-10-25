//! The static `ControlKind → BehaviorRow` lookup table (PR 5, E6-a).
//!
//! This file owns the enum-indexed table that turns a resolved control kind
//! into the entry-point functions the generic activation lifecycle invokes at
//! its seams. It is a lookup map, not a `HashMap`: the index is a closed
//! `ControlKind` enum, the rows are a fixed `const` array, and lookup is a
//! `match` with no allocation.
//!
//! Invariants and rules:
//!
//! - `ControlKind` is the single behavior KEY: `resolve_kind` maps an element
//!   to its kind, and the facade seams on `RootDom` resolve the kind, look up
//!   the row, and invoke the row's hook.
//! - the table is complete by construction: every `ControlKind` arm maps to a
//!   row (the census test pins this), even a row with every hook `None`
//!   (that is "today's no-op" for a kind with no default behavior).
//! - row hooks take `&mut RootDom` exactly as today's methods did (stage-a
//!   context); the narrowed `BehaviorContext` is E6-c's evidence work.
//! - the row field set matches the seams the inventory found: keyboard,
//!   pointer-down, click-commit, focus, and blur. There is no `on_text_input`
//!   or `on_transition_end` row today — text input runs inside the keyboard
//!   handler, and transition-end does not dispatch per kind.

use bevy::input::keyboard::KeyboardInput;

use crate::domains::visitor::event_target::DomPointEventTarget;
use crate::element_controls::{ResolvedElementControl, resolved_element_control};
use crate::types::primitives::PointPx;
use crate::types::{DomElementNode, DomError, DomNodeSlotKey, DomTag, RootDom};

use super::button::button_on_keyboard;
use super::checkbox::checkbox_click_commit;
use super::controls::{
    input_control_on_keyboard, input_control_pointer_down, presented_options_blur,
    presented_options_focus,
};
use super::select::{select_click_commit, select_on_blur, select_on_keyboard};
use super::text_entry::{text_edit_on_keyboard, text_edit_pointer_down};

#[cfg(test)]
use crate::types::dom_attr_node::DomAttributeNode;
#[cfg(test)]
use crate::types::{DomAttribute, DomAttributeValue, DomNode, RootDomContext};

/// The closed set of control kinds a DOM element can resolve to.
///
/// This is the behavior-table key. It is deliberately finer than the legacy
/// per-seam enums (`DomElementOnKeyboard::InputControl` covers four kinds
/// here) so that each kind's row is a single dispatch shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlKind {
    Button,
    Checkbox,
    Number,
    Range,
    TextInput,
    Textarea,
    Select,
}

/// The keyboard seam hook signature (the whole focused-element keyboard path).
pub(crate) type KeyboardHook =
    fn(&mut RootDom, DomNodeSlotKey, &KeyboardInput, bool, bool) -> Result<bool, DomError>;
/// The pointer-down seam hook signature.
pub(crate) type PointerDownHook =
    fn(&mut RootDom, DomPointEventTarget, PointPx) -> Result<(), DomError>;
/// The click-commit seam hook signature.
pub(crate) type ClickCommitHook = fn(&mut RootDom, DomNodeSlotKey) -> Result<bool, DomError>;
/// The focus/blur seam hook signature.
pub(crate) type FocusBlurHook = fn(&mut RootDom, DomNodeSlotKey) -> Result<(), DomError>;

/// One control kind's entry-point functions, one field per discovered seam.
///
/// A `None` field is the honest "no default behavior at this seam" — the
/// lifecycle treats it exactly as today's no-op path.
///
/// `#[allow(clippy::struct_field_names)]`: the `on_*` prefix is the design
/// vocabulary for the per-kind hook set (`on_keyboard`, `on_pointer_down`,
/// `on_click_commit`, `on_focus`, `on_blur`), not a removable stutter.
#[allow(clippy::struct_field_names)]
pub(crate) struct BehaviorRow {
    /// Keyboard key down/up handler (the whole focused-element keyboard seam).
    pub(crate) on_keyboard: Option<KeyboardHook>,
    /// Pointer-down handler.
    pub(crate) on_pointer_down: Option<PointerDownHook>,
    /// The commit step of the generic activation lifecycle (after an
    /// uncanceled click dispatch).
    pub(crate) on_click_commit: Option<ClickCommitHook>,
    /// After-focus handler.
    pub(crate) on_focus: Option<FocusBlurHook>,
    /// Blur handler.
    pub(crate) on_blur: Option<FocusBlurHook>,
}

/// The enum-indexed behavior table. The array order matches `ControlKind`'s
/// declaration order; `behavior_row` indexes it by match so the two cannot
/// drift without a compile error.
const BEHAVIOR_TABLE: [BehaviorRow; 7] = [
    // Button
    BehaviorRow {
        on_keyboard: Some(button_on_keyboard),
        on_pointer_down: None,
        on_click_commit: None,
        on_focus: None,
        on_blur: None,
    },
    // Checkbox
    BehaviorRow {
        on_keyboard: Some(input_control_on_keyboard),
        on_pointer_down: Some(input_control_pointer_down),
        on_click_commit: Some(checkbox_click_commit),
        on_focus: Some(presented_options_focus),
        on_blur: Some(presented_options_blur),
    },
    // Number
    BehaviorRow {
        on_keyboard: Some(input_control_on_keyboard),
        on_pointer_down: Some(input_control_pointer_down),
        on_click_commit: None,
        on_focus: Some(presented_options_focus),
        on_blur: Some(presented_options_blur),
    },
    // Range
    BehaviorRow {
        on_keyboard: Some(input_control_on_keyboard),
        on_pointer_down: Some(input_control_pointer_down),
        on_click_commit: None,
        on_focus: Some(presented_options_focus),
        on_blur: Some(presented_options_blur),
    },
    // TextInput
    BehaviorRow {
        on_keyboard: Some(input_control_on_keyboard),
        on_pointer_down: Some(input_control_pointer_down),
        on_click_commit: None,
        on_focus: Some(presented_options_focus),
        on_blur: Some(presented_options_blur),
    },
    // Textarea
    BehaviorRow {
        on_keyboard: Some(text_edit_on_keyboard),
        on_pointer_down: Some(text_edit_pointer_down),
        on_click_commit: None,
        on_focus: None,
        on_blur: None,
    },
    // Select
    BehaviorRow {
        on_keyboard: Some(select_on_keyboard),
        on_pointer_down: None,
        on_click_commit: Some(select_click_commit),
        on_focus: None,
        on_blur: Some(select_on_blur),
    },
];

/// Returns the row for one control kind. This is the enum-indexed lookup.
#[must_use]
pub(crate) const fn behavior_row(kind: ControlKind) -> &'static BehaviorRow {
    match kind {
        ControlKind::Button => &BEHAVIOR_TABLE[0],
        ControlKind::Checkbox => &BEHAVIOR_TABLE[1],
        ControlKind::Number => &BEHAVIOR_TABLE[2],
        ControlKind::Range => &BEHAVIOR_TABLE[3],
        ControlKind::TextInput => &BEHAVIOR_TABLE[4],
        ControlKind::Textarea => &BEHAVIOR_TABLE[5],
        ControlKind::Select => &BEHAVIOR_TABLE[6],
    }
}

/// Resolves one element's control kind, or `None` for elements with no
/// default behavior (div, span, option, datalist, custom, …).
#[must_use]
pub(crate) fn resolve_kind_for_element(element: &DomElementNode) -> Option<ControlKind> {
    match element.tag {
        DomTag::Button => Some(ControlKind::Button),
        DomTag::Select => Some(ControlKind::Select),
        DomTag::Textarea => Some(ControlKind::Textarea),
        DomTag::Input => Some(match resolved_element_control(element) {
            Some(ResolvedElementControl::Checkbox(_)) => ControlKind::Checkbox,
            Some(ResolvedElementControl::Number(_)) => ControlKind::Number,
            Some(ResolvedElementControl::Range(_)) => ControlKind::Range,
            Some(ResolvedElementControl::TextInput | ResolvedElementControl::Textarea) => {
                ControlKind::TextInput
            }
            None => ControlKind::TextInput,
        }),
        _ => None,
    }
}

/// Resolves a live element's control kind by node key.
#[must_use]
pub(crate) fn resolve_kind(root: &RootDom, key: DomNodeSlotKey) -> Option<ControlKind> {
    let element = root.node(key)?.as_element()?;
    resolve_kind_for_element(element)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every control kind resolves to a row — the census. Even a kind whose
    /// row is all `None` hooks is present (that is "today's no-op").
    #[test]
    fn every_control_kind_has_a_behavior_row() {
        for kind in [
            ControlKind::Button,
            ControlKind::Checkbox,
            ControlKind::Number,
            ControlKind::Range,
            ControlKind::TextInput,
            ControlKind::Textarea,
            ControlKind::Select,
        ] {
            let _row = behavior_row(kind);
        }
    }

    /// The row field set matches the discovered seams: the kinds that own
    /// commit behavior surface it, and the kinds that do not leave the hook
    /// `None` (the lifecycle's no-op path).
    #[test]
    fn click_commit_hooks_match_the_activation_shapes() {
        assert!(
            behavior_row(ControlKind::Checkbox)
                .on_click_commit
                .is_some()
        );
        assert!(behavior_row(ControlKind::Select).on_click_commit.is_some());
        assert!(behavior_row(ControlKind::Button).on_click_commit.is_none());
        assert!(behavior_row(ControlKind::Number).on_click_commit.is_none());
        assert!(behavior_row(ControlKind::Range).on_click_commit.is_none());
        assert!(
            behavior_row(ControlKind::TextInput)
                .on_click_commit
                .is_none()
        );
        assert!(
            behavior_row(ControlKind::Textarea)
                .on_click_commit
                .is_none()
        );
    }

    fn input_with_type(input_type: &str) -> DomElementNode {
        let DomNode::Element(mut node) = DomNode::element(DomNodeSlotKey::default(), DomTag::Input)
        else {
            panic!("input should create an element node");
        };
        node.set_attr(
            DomAttribute::Type,
            Some(DomAttributeValue::String(input_type.to_string())),
        )
        .expect("type");
        node
    }

    #[test]
    fn resolve_kind_maps_each_control_shape() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let button = root.create_element(DomTag::Button).expect("button");
        let select = root.create_element(DomTag::Select).expect("select");
        let textarea = root.create_element(DomTag::Textarea).expect("textarea");
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");
        let range = root.create_element(DomTag::Input).expect("range");
        root.set_attr_str(range, "type", "range").expect("type");
        let number = root.create_element(DomTag::Input).expect("number");
        root.set_attr_str(number, "type", "number").expect("type");
        let text = root.create_element(DomTag::Input).expect("text");
        let div = root.create_element(DomTag::Div).expect("div");

        assert_eq!(resolve_kind(&root, button), Some(ControlKind::Button));
        assert_eq!(resolve_kind(&root, select), Some(ControlKind::Select));
        assert_eq!(resolve_kind(&root, textarea), Some(ControlKind::Textarea));
        assert_eq!(resolve_kind(&root, checkbox), Some(ControlKind::Checkbox));
        assert_eq!(resolve_kind(&root, range), Some(ControlKind::Range));
        assert_eq!(resolve_kind(&root, number), Some(ControlKind::Number));
        assert_eq!(resolve_kind(&root, text), Some(ControlKind::TextInput));
        assert_eq!(resolve_kind(&root, div), None);
    }

    #[test]
    fn resolve_kind_for_element_maps_the_type_attribute() {
        assert_eq!(
            resolve_kind_for_element(&input_with_type("checkbox")),
            Some(ControlKind::Checkbox)
        );
        assert_eq!(
            resolve_kind_for_element(&input_with_type("range")),
            Some(ControlKind::Range)
        );
        assert_eq!(
            resolve_kind_for_element(&input_with_type("number")),
            Some(ControlKind::Number)
        );
        assert_eq!(
            resolve_kind_for_element(&input_with_type("text")),
            Some(ControlKind::TextInput)
        );
        assert_eq!(
            resolve_kind_for_element(&input_with_type("email")),
            Some(ControlKind::TextInput)
        );
    }
}
