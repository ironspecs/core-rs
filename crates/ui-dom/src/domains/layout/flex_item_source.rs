//! Flex item source enumeration for the active flex solver.
//!
//! This file exists to satisfy structural rule 1: a type with an `impl`
//! lives in its own `snake_case` file.
//!
//! Invariants and rules:
//!
//! - `AnonymousInlineGroup` represents a contiguous run of inline-level DOM
//!   children wrapped into a single anonymous flex item per CSS Flexbox §4.

use crate::types::DomNodeSlotKey;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) enum FlexItemSource {
    BlockItem(DomNodeSlotKey),
    #[allow(dead_code)]
    AnonymousInlineGroup(DomNodeSlotKey),
}

impl FlexItemSource {
    pub(super) fn key(self) -> DomNodeSlotKey {
        match self {
            Self::BlockItem(key) | Self::AnonymousInlineGroup(key) => key,
        }
    }
}
