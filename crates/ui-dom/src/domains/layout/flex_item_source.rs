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
pub(crate) enum FlexItemSource {
    BlockItem(DomNodeSlotKey),
    #[allow(dead_code)]
    AnonymousInlineGroup(DomNodeSlotKey),
}

impl FlexItemSource {
    pub(crate) fn key(self) -> DomNodeSlotKey {
        match self {
            Self::BlockItem(key) | Self::AnonymousInlineGroup(key) => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_slot_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    #[test]
    fn anonymous_inline_group_wraps_one_contiguous_run() {
        let group_key = new_slot_key();
        let item_key = new_slot_key();

        let anonymous_group = FlexItemSource::AnonymousInlineGroup(group_key);
        let block_item = FlexItemSource::BlockItem(item_key);

        // Both variants report their slot key as the single flex item identity.
        assert_eq!(anonymous_group.key(), group_key);
        assert_eq!(block_item.key(), item_key);

        // The anonymous-group item is a distinct item kind from a block item.
        assert_ne!(anonymous_group, block_item);
    }
}
