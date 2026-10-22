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

use super::computed::{GridTemplateAreas, GridTrackList, TextFamilyList};
use super::decl_block::DeclBlock;
use super::decl_value::{GridAreasKey, GridTrackListKey, TextFamilyListKey, TextKey};
use super::text_interner::TextInterner;

slotmap::new_key_type! { pub struct DeclBlockKey; }

/// Arena of all declaration blocks for one owner (a stylesheet runtime or a
/// document's inline-style store), plus the text interner the blocks' Text
/// values resolve against, the value-deduped grid track-list arena (grid T3
/// `GridTrackList`, which retains `[name]` line names), the value-deduped
/// `font-family` list arena (text seam S2 `TextFamilyList`), and the
/// value-deduped grid template-areas arena. Generational keys make stale-block
/// access a typed None, never a use-after-free. Blocks are never freed while
/// their owner lives (stylesheet tombstones keep theirs, matching the rule
/// store's tombstone semantics today); reclamation policy is open question 6.
#[derive(Debug, Default)]
pub struct DeclStore {
    blocks: SlotMap<DeclBlockKey, DeclBlock>,
    texts: TextInterner,
    track_lists: SlotMap<GridTrackListKey, GridTrackList>,
    family_lists: SlotMap<TextFamilyListKey, TextFamilyList>,
    grid_areas: SlotMap<GridAreasKey, GridTemplateAreas>,
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
    /// lists returns the existing key for equal content (tracks + line names),
    /// and only a new list is pushed (allocation-once-per-new-list, steady-state
    /// zero — the D8 contract). Lists are short and few, so the scan is the
    /// honest shape (the text interner's).
    pub fn intern_track_list(&mut self, list: &GridTrackList) -> GridTrackListKey {
        for (key, existing) in &self.track_lists {
            if existing == list {
                return key;
            }
        }
        self.track_lists.insert(list.clone())
    }

    /// Resolves a track-list key to its retained `GridTrackList`. A stale or
    /// out-of-range key is a typed `None` (generational safety).
    #[must_use]
    pub fn resolve_track_list(&self, key: GridTrackListKey) -> Option<&GridTrackList> {
        self.track_lists.get(key)
    }

    /// Interns a grid template-areas map value-deduped (the track-list arena's
    /// shape): equal maps share one key, only a new map is pushed. Maps are
    /// short and few, so the scan is the honest shape.
    pub fn intern_grid_areas(&mut self, areas: &GridTemplateAreas) -> GridAreasKey {
        for (key, existing) in &self.grid_areas {
            if existing == areas {
                return key;
            }
        }
        self.grid_areas.insert(areas.clone())
    }

    /// Resolves an areas-map key to its retained `GridTemplateAreas`. A stale or
    /// out-of-range key is a typed `None` (generational safety).
    #[must_use]
    pub fn resolve_grid_areas(&self, key: GridAreasKey) -> Option<&GridTemplateAreas> {
        self.grid_areas.get(key)
    }

    /// Interns a `font-family` list value-deduped (the track-list arena's
    /// shape): equal lists share one key, only a new list is pushed. Lists are
    /// short and few, so the scan is the honest shape (text seam S2).
    pub fn intern_family_list(&mut self, list: &TextFamilyList) -> TextFamilyListKey {
        for (key, existing) in &self.family_lists {
            if existing == list {
                return key;
            }
        }
        self.family_lists.insert(list.clone())
    }

    /// Resolves a family-list key to its retained `TextFamilyList`. A stale or
    /// out-of-range key is a typed `None` (generational safety).
    #[must_use]
    pub fn resolve_family_list(&self, key: TextFamilyListKey) -> Option<&TextFamilyList> {
        self.family_lists.get(key)
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

    /// Level accessor over retained state: distinct interned areas maps.
    #[must_use]
    pub fn interned_grid_areas_count(&self) -> usize {
        self.grid_areas.len()
    }

    /// Level accessor over retained state: distinct interned family lists.
    #[must_use]
    pub fn interned_family_list_count(&self) -> usize {
        self.family_lists.len()
    }
}

#[cfg(test)]
use crate::domains::style::computed::{GridTrackSize, TrackBreadth, TrackMaxBreadth};

#[cfg(test)]
mod tests {
    use super::*;

    fn track_size(min: TrackBreadth, max: TrackMaxBreadth) -> GridTrackSize {
        GridTrackSize { min, max }
    }

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
        let mut list = GridTrackList::default();
        list.tracks.push(track_size(
            TrackBreadth::Px(100.0),
            TrackMaxBreadth::Px(100.0),
        ));
        list.tracks
            .push(track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(1.0)));
        let mut other = GridTrackList::default();
        other.tracks.push(track_size(
            TrackBreadth::Px(200.0),
            TrackMaxBreadth::Px(200.0),
        ));

        let mut store = DeclStore::new();
        let first = store.intern_track_list(&list);
        let second = store.intern_track_list(&list);
        let third = store.intern_track_list(&other);

        assert_eq!(first, second, "equal lists must intern to the SAME key");
        assert_ne!(first, third, "distinct lists must intern to distinct keys");
        assert_eq!(store.interned_track_list_count(), 2, "two distinct lists");

        let resolved = store.resolve_track_list(first).expect("key resolves");
        assert_eq!(resolved.tracks.as_slice(), list.tracks.as_slice());
    }

    #[test]
    fn intern_grid_areas_dedups_by_content() {
        let areas = GridTemplateAreas {
            cells: arrayvec::ArrayVec::new(),
            names: arrayvec::ArrayVec::new(),
            row_count: 0,
            col_count: 0,
        };
        let mut other = areas.clone();
        other.row_count = 1;

        let mut store = DeclStore::new();
        let first = store.intern_grid_areas(&areas);
        let second = store.intern_grid_areas(&areas);
        let third = store.intern_grid_areas(&other);

        assert_eq!(first, second, "equal maps must intern to the SAME key");
        assert_ne!(first, third, "distinct maps must intern to distinct keys");
        assert_eq!(store.interned_grid_areas_count(), 2, "two distinct maps");
    }

    #[test]
    fn resolve_track_list_out_of_range_is_none() {
        let store = DeclStore::new();
        assert_eq!(store.resolve_track_list(GridTrackListKey::default()), None);
    }

    #[test]
    fn resolve_grid_areas_out_of_range_is_none() {
        let store = DeclStore::new();
        assert_eq!(store.resolve_grid_areas(GridAreasKey::default()), None);
    }
}
