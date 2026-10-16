//! Activation behavior definitions for DOM elements.
//!
//! This file owns the activation behavior *definitions*: the per-element
//! keyboard, pointer, focus, blur, and activation descriptors plus the
//! retained pressed-pointer-click and range-drag session state. The
//! default-behavior lifecycle methods that execute those definitions live
//! on `RootDom` in `types/root_dom.rs`.

use crate::types::DomNodeSlotKey;
use crate::types::primitives::PointPx;

/// Selects which behavior gets first chance at keyboard interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnKeyboard {
    #[default]
    None,
    Button,
    InputControl,
    /// Runs the built-in select keyboard behavior for opening and committing the picker.
    Select,
    TextEdit,
    PresentedOptions,
}

/// Selects which behavior gets first chance at pointer interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnPointer {
    #[default]
    None,
    InputControl,
    TextEdit,
}

/// Selects the post-click lifecycle for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterClick {
    #[default]
    None,
    Activate,
}

/// Selects the committed activation action for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnActivation {
    #[default]
    None,
    /// Runs the built-in select activation behavior after an uncanceled click.
    Select,
    Toggle,
    PresentedOptions,
}

/// Selects the work that runs after one element becomes focused.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterFocus {
    #[default]
    None,
    PresentedOptions,
}

/// Selects which behavior runs when one element loses focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnBlur {
    #[default]
    None,
    /// Runs the built-in select blur behavior for closing the picker.
    Select,
    PresentedOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PressedPointerClickTarget {
    pub(crate) target_key: DomNodeSlotKey,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveRangeDragSession {
    pub(crate) target_key: DomNodeSlotKey,
    pub(crate) root_to_subtree_point_offset: PointPx,
    pub(crate) focus_entry_value: String,
    pub(crate) dirty: bool,
}
