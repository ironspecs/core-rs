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

use arrayvec::ArrayVec;
use slotmap::SlotMap;

use super::computed::GridTrackSize;
use super::decl_block::DeclBlock;
use super::decl_value::{GridTrackListKey, TextKey};
use super::text_interner::TextInterner;

slotmap::new_key_type! { pub struct DeclBlockKey; }

/// The maximum expanded track count a grid track list may hold. Track lists
/// are short and few by design (the design-system standard); larger lists are
/// a named, D6-gated follow-up (an arena-page indirection, sketched, not
/// built). The parse-time capacity gate drops lists over this bound with a
/// counted report (`GRID_STEP1_DESIGN.md` D4).
pub(crate) const GRID_TRACK_LIST_CAPACITY: usize = 12;

/// Arena of all declaration blocks for one owner (a stylesheet runtime or a
/// document's inline-style store), plus the text interner the blocks' Text
/// values resolve against, and the value-deduped grid track-list arena.
/// Generational keys make stale-block access a typed
/// None, never a use-after-free. Blocks are never freed while their owner
/// lives (stylesheet tombstones keep theirs, matching the rule store's
/// tombstone semantics today); reclamation policy is open question 6.
#[derive(Debug, Default)]
pub struct DeclStore {
    blocks: SlotMap<DeclBlockKey, DeclBlock>,
    texts: TextInterner,
    track_lists: SlotMap<GridTrackListKey, ArrayVec<GridTrackSize, GRID_TRACK_LIST_CAPACITY>>,
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

    /// Interns a grid track list value-deduped: a linear scan over retained
    /// lists returns the existing key for equal content, and only a new list
    /// is pushed (allocation-once-per-new-list, steady-state zero — the D8
    /// contract). Lists are short and few, so the scan is the honest shape
    /// (the text interner's).
    pub fn intern_track_list(&mut self, list: &[GridTrackSize]) -> GridTrackListKey {
        for (key, existing) in &self.track_lists {
            if existing.as_slice() == list {
                return key;
            }
        }
        let mut retained = ArrayVec::<GridTrackSize, GRID_TRACK_LIST_CAPACITY>::new();
        retained.extend(list.iter().copied());
        self.track_lists.insert(retained)
    }

    /// Resolves a track-list key to its retained `GridTrackSize` slice. A stale
    /// or out-of-range key is a typed `None` (generational safety).
    #[must_use]
    pub fn resolve_track_list(
        &self,
        key: GridTrackListKey,
    ) -> Option<&ArrayVec<GridTrackSize, GRID_TRACK_LIST_CAPACITY>> {
        self.track_lists.get(key)
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

    /// Level accessor over retained state: distinct interned track lists.
    #[must_use]
    pub fn interned_track_list_count(&self) -> usize {
        self.track_lists.len()
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

    #[test]
    fn intern_track_list_dedups_by_content() {
        use crate::domains::style::computed::{GridTrackSize, TrackBreadth, TrackMaxBreadth};

        let list = [
            GridTrackSize {
                min: TrackBreadth::Px(100.0),
                max: TrackMaxBreadth::Px(100.0),
            },
            GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Fr(1.0),
            },
        ];
        let other = [GridTrackSize {
            min: TrackBreadth::Px(200.0),
            max: TrackMaxBreadth::Px(200.0),
        }];

        let mut store = DeclStore::new();
        let first = store.intern_track_list(&list);
        let second = store.intern_track_list(&list);
        let third = store.intern_track_list(&other);

        assert_eq!(first, second, "equal lists must intern to the SAME key");
        assert_ne!(first, third, "distinct lists must intern to distinct keys");
        assert_eq!(store.interned_track_list_count(), 2, "two distinct lists");

        let resolved = store.resolve_track_list(first).expect("key resolves");
        assert_eq!(resolved.as_slice(), &list[..]);
    }

    #[test]
    fn resolve_track_list_out_of_range_is_none() {
        let store = DeclStore::new();
        assert_eq!(store.resolve_track_list(GridTrackListKey::default()), None);
    }
}
