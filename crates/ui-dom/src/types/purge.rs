//! Purge-on-remove leaves for the DOM dependents stores.
//!
//! What this file represents:
//!
//! - the leaf primitives that drop a removed node key from a retained
//!   dependent store: `remove_each_in` (map-shaped), `retain_not_in_vec`
//!   (vec-shaped), and `clear_if_in` / `clear_if_in_by` (option-shaped)
//!
//! Ownership boundary:
//!
//! - these are the smallest honest leaves under `remove_node`'s purge table.
//!   They are DOM-shaped (they key on `DomNodeSlotKey` and a removed-key
//!   slice), so they live in `types/` rather than `collections`, which owns
//!   genuinely generic collection behavior without a DOM key vocabulary
//!
//! Invariants and rules:
//!
//! - a `retain_not_in_*` keeps every member whose key is NOT in `removed`;
//!   the surviving members keep their relative order
//! - a `clear_if_in` clears the option only when its key IS in `removed`, and
//!   never manufactures a value otherwise
//!
//! The two map-shaped access patterns (PR 5, E3 follow-up):
//!
//! - `remove_each_in` iterates the REMOVED set and removes each key from the
//!   map: O(removed) with `HashMap::remove`. Use it for the removal case —
//!   the store is large and only a handful of keys are being dropped. This is
//!   the shape the pre-E3 scroll-offset purge had, and it is the only map-
//!   shaped purge currently in use.
//! - a "filter the store" leaf (iterating the STORE and dropping every entry
//!   whose key is in `removed`: O(store × removed) with a linear
//!   `removed.contains`) is deliberately ABSENT. That shape applies when
//!   filtering the store is the semantic intent, but no current map-shaped
//!   dependent is that case — the vec-shaped dependents use `retain_not_in_vec`
//!   for it. E3's `retain_not_in_map` was therefore removed in the follow-up
//!   rather than left as dead code.
//!
//! Choosing the wrong side is a silent access-pattern regression, not a
//! correctness bug (both leaves produce the same final map) — which is why
//! each call site's pre-extraction shape is the authority (see `root_dom.rs`'s
//! `PURGE_DEPENDENTS` audit note).
//!
//! Extraction note (PR 5, E3): these are free functions, not a trait
//! implemented for `HashMap`/`Vec`/`BTreeSet`. Structural rule 1 names the
//! impl file after the self type (`hash_map.rs`, `vec.rs`, ...), which would
//! scatter the one-line bodies across std-type files; the free functions keep
//! one honest file for the purge leaves.
use std::collections::HashMap;

use crate::types::dom_node::DomNodeSlotKey;

/// Removes every key in `removed` from the map.
///
/// Iterates the REMOVED set (O(removed)); use it for the removal case — a
/// large store from which a handful of keys are being dropped.
pub(crate) fn remove_each_in<V>(map: &mut HashMap<DomNodeSlotKey, V>, removed: &[DomNodeSlotKey]) {
    for key in removed {
        map.remove(key);
    }
}

/// Keeps every vec member whose node key is NOT in `removed`, preserving order.
pub(crate) fn retain_not_in_vec(vec: &mut Vec<DomNodeSlotKey>, removed: &[DomNodeSlotKey]) {
    vec.retain(|key| !removed.contains(key));
}

/// Clears an option-shaped dependent whose retained node key is in `removed`.
pub(crate) fn clear_if_in(option: &mut Option<DomNodeSlotKey>, removed: &[DomNodeSlotKey]) {
    if option.is_some_and(|key| removed.contains(&key)) {
        *option = None;
    }
}

/// Clears an option-shaped dependent whose retained node key (derived by
/// `key_of`) is in `removed`.
pub(crate) fn clear_if_in_by<T>(
    option: &mut Option<T>,
    removed: &[DomNodeSlotKey],
    key_of: impl Fn(&T) -> DomNodeSlotKey,
) {
    if option
        .as_ref()
        .is_some_and(|value| removed.contains(&key_of(value)))
    {
        *option = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_each_in_removes_every_removed_key_and_keeps_the_rest() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());
        let c = slots.insert(());
        let d = slots.insert(());
        // A large store with a small removed set is exactly the removal case:
        // the leaf must walk the removed slice, not the store.
        let mut map: HashMap<DomNodeSlotKey, f32> =
            HashMap::from([(a, 1.0), (b, 2.0), (c, 3.0), (d, 4.0)]);

        remove_each_in(&mut map, &[b, c]);

        assert!(map.contains_key(&a));
        assert!(!map.contains_key(&b));
        assert!(!map.contains_key(&c));
        assert!(map.contains_key(&d));
    }

    #[test]
    fn remove_each_in_is_a_noop_when_nothing_is_removed() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());
        let mut map: HashMap<DomNodeSlotKey, f32> = HashMap::from([(a, 1.0), (b, 2.0)]);

        remove_each_in(&mut map, &[]);

        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&a));
        assert!(map.contains_key(&b));
    }

    #[test]
    fn retain_not_in_vec_keeps_order_for_survivors() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());
        let c = slots.insert(());
        let mut vec = vec![a, b, c];

        retain_not_in_vec(&mut vec, &[b]);

        assert_eq!(vec, vec![a, c]);
    }

    #[test]
    fn clear_if_in_clears_only_when_key_is_removed() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());

        let mut removed = Some(a);
        clear_if_in(&mut removed, &[a]);
        assert_eq!(removed, None);

        let mut kept = Some(a);
        clear_if_in(&mut kept, &[b]);
        assert_eq!(kept, Some(a));

        let mut empty = None;
        clear_if_in(&mut empty, &[a]);
        assert_eq!(empty, None);
    }

    #[test]
    fn clear_if_in_by_clears_by_the_derived_key() {
        #[derive(Clone, Copy, Debug, PartialEq)]
        struct Target {
            target_key: DomNodeSlotKey,
        }

        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());

        let mut removed = Some(Target { target_key: a });
        clear_if_in_by(&mut removed, &[a], |target| target.target_key);
        assert_eq!(removed, None);

        let mut kept = Some(Target { target_key: a });
        clear_if_in_by(&mut kept, &[b], |target| target.target_key);
        assert_eq!(kept, Some(Target { target_key: a }));

        let mut empty: Option<Target> = None;
        clear_if_in_by(&mut empty, &[a], |target| target.target_key);
        assert_eq!(empty, None);
    }

    #[test]
    fn retain_leaves_noop_when_nothing_is_removed() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());
        let mut vec = vec![a, b];

        retain_not_in_vec(&mut vec, &[]);

        assert_eq!(vec, vec![a, b]);
    }
}
