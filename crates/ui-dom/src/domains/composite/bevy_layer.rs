//! Bevy layer marker component for `ui-dom`.
//!
//! This file owns the `BevyLayer` component that tags every spawned layer
//! entity with its originating DOM node slot key. It is the shared identity
//! anchor that composite reconciliation and hit-testing use to map a Bevy
//! entity back to a retained DOM node.
//!
//! Invariants and rules:
//!
//! - this file owns only the `BevyLayer` struct and its `impl` block
//! - the slot key stays synchronized with `RootDom` layer registration

use bevy::prelude::Component;

use crate::types::DomNodeSlotKey;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BevyLayer {
    pub(super) dom_node_slot_key: DomNodeSlotKey,
}

impl BevyLayer {
    pub(crate) fn dom_node_slot_key(self) -> DomNodeSlotKey {
        self.dom_node_slot_key
    }
}
