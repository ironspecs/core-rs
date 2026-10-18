//! Declaration arena for the `ui-dom` style system.
//!
//! This file owns `DeclStore`, the arena of all declaration blocks for one
//! owner (a stylesheet runtime or a document's inline-style store), plus the
//! `TextInterner` the blocks' `Text` values resolve against, and the
//! `DeclBlockKey` handle. Generational keys make stale-block access a typed
//! `None`, never a use-after-free. Blocks are never freed while their owner
//! lives; reclamation policy is open question 6.
//!
//! Invariants and rules:
//!
//! - a `TextKey` is meaningful only against this store's interner
//! - level accessors (`live_block_count`, `interned_text_count`) are read
//!   accessors over retained state, not emitted gauges

use slotmap::SlotMap;

use super::decl_block::DeclBlock;
use super::decl_value::TextKey;
use super::text_interner::TextInterner;

slotmap::new_key_type! { pub struct DeclBlockKey; }

/// Arena of all declaration blocks for one owner (a stylesheet runtime or a
/// document's inline-style store), plus the text interner the blocks' Text
/// values resolve against. Generational keys make stale-block access a typed
/// None, never a use-after-free. Blocks are never freed while their owner
/// lives (stylesheet tombstones keep theirs, matching the rule store's
/// tombstone semantics today); reclamation policy is open question 6.
#[derive(Debug, Default)]
pub struct DeclStore {
    blocks: SlotMap<DeclBlockKey, DeclBlock>,
    texts: TextInterner,
}

impl DeclStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, block: DeclBlock) -> DeclBlockKey {
        self.blocks.insert(block)
    }

    #[must_use]
    pub fn get(&self, key: DeclBlockKey) -> Option<&DeclBlock> {
        self.blocks.get(key)
    }

    /// Mutable access to one live block for in-place upsert/remove edits
    /// (the inline-block store's A10 minimal-edit path).
    pub(crate) fn get_mut(&mut self, key: DeclBlockKey) -> Option<&mut DeclBlock> {
        self.blocks.get_mut(key)
    }

    pub fn remove(&mut self, key: DeclBlockKey) -> Option<DeclBlock> {
        self.blocks.remove(key)
    }

    pub fn intern(&mut self, text: &str) -> TextKey {
        self.texts.intern(text)
    }

    #[must_use]
    pub fn resolve(&self, key: TextKey) -> Option<&str> {
        self.texts.resolve(key)
    }

    /// Mutable access to the store's interner, so declaration parsing interns
    /// text through the owning store (a `TextKey` is store-relative).
    pub(crate) fn texts_mut(&mut self) -> &mut TextInterner {
        &mut self.texts
    }

    /// Level accessor over retained state (observability doctrine: levels are
    /// read accessors, not emitted gauges): live declaration blocks.
    #[must_use]
    pub fn live_block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Level accessor over retained state: distinct interned text strings.
    #[must_use]
    pub fn interned_text_count(&self) -> usize {
        self.texts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut store = DeclStore::new();
        let key = store.insert(DeclBlock::new());
        assert!(store.get(key).is_some());
        assert_eq!(store.live_block_count(), 1);
    }

    #[test]
    fn remove_makes_key_stale() {
        let mut store = DeclStore::new();
        let key = store.insert(DeclBlock::new());
        assert!(store.remove(key).is_some());
        assert!(store.get(key).is_none());
        assert_eq!(store.live_block_count(), 0);
    }

    #[test]
    fn generational_stale_access() {
        let mut store = DeclStore::new();
        let first = store.insert(DeclBlock::new());
        assert!(store.get(first).is_some());
        store.remove(first);
        let second = store.insert(DeclBlock::new());
        assert!(store.get(first).is_none());
        assert!(store.get(second).is_some());
        assert_eq!(store.live_block_count(), 1);
    }

    #[test]
    fn intern_and_resolve_through_store() {
        let mut store = DeclStore::new();
        let key = store.intern("translate(10px)");
        assert_eq!(store.resolve(key), Some("translate(10px)"));
        assert_eq!(store.interned_text_count(), 1);
        assert_eq!(store.resolve(TextKey(u32::MAX)), None);
    }
}
