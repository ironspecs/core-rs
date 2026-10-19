//! Flex item source enumeration for the active flex solver.
//!
//! This file exists to satisfy structural rule 1: a type with an `impl`
//! lives in its own `snake_case` file.
//!
//! Invariants and rules:
//!
//! - `AnonymousInlineGroup` represents a maximal run of contiguous inline-level
//!   in-flow DOM children (text nodes and `display: inline` elements) wrapped
//!   into a single anonymous flex item per CSS Flexbox §4. Its identity anchor
//!   is `first_child_key`; `child_count` is the number of direct children in
//!   the run (never zero — a run starts on the first inline child). The DOM
//!   identity stays stable (we never manufacture a DOM object; the run is a
//!   virtual item, engine semantics on a crate mechanism).

use crate::types::DomNodeSlotKey;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum FlexItemSource {
    BlockItem(DomNodeSlotKey),
    AnonymousInlineGroup {
        first_child_key: DomNodeSlotKey,
        child_count: u32,
    },
}

impl FlexItemSource {
    /// The single flex-item identity anchor: the child's own key for a block
    /// item, the run's first-child key for an anonymous inline group.
    pub(crate) fn key(self) -> DomNodeSlotKey {
        match self {
            Self::BlockItem(key) => key,
            Self::AnonymousInlineGroup {
                first_child_key, ..
            } => first_child_key,
        }
    }

    /// The run's first child key (placement walks `child_count` siblings from
    /// here). Valid only for `AnonymousInlineGroup`.
    pub(crate) fn first_child_key(self) -> DomNodeSlotKey {
        match self {
            Self::BlockItem(key) => key,
            Self::AnonymousInlineGroup {
                first_child_key, ..
            } => first_child_key,
        }
    }

    /// The run's direct-child count (placement's sibling-walk length). Valid
    /// only for `AnonymousInlineGroup`.
    pub(crate) fn child_count(self) -> u32 {
        match self {
            Self::BlockItem(_) => 0,
            Self::AnonymousInlineGroup { child_count, .. } => child_count,
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
        let first_child_key = new_slot_key();
        let item_key = new_slot_key();

        let anonymous_group = FlexItemSource::AnonymousInlineGroup {
            first_child_key,
            child_count: 3,
        };
        let block_item = FlexItemSource::BlockItem(item_key);

        // Both variants report their slot key as the single flex item identity.
        assert_eq!(anonymous_group.key(), first_child_key);
        assert_eq!(block_item.key(), item_key);

        // The run carries its first-child anchor and its direct-child count.
        assert_eq!(anonymous_group.first_child_key(), first_child_key);
        assert_eq!(anonymous_group.child_count(), 3);
        assert_eq!(block_item.child_count(), 0);

        // The anonymous-group item is a distinct item kind from a block item.
        assert_ne!(anonymous_group, block_item);
    }
}
