//! Purge-on-remove leaves for the DOM dependents stores.
//!
//! What this file represents:
//!
//! - the leaf primitives that drop a removed node key from a retained
//!   dependent store: `retain_not_in_map` (map-shaped), `retain_not_in_vec`
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
//! Extraction note (PR 5, E3): these are free functions, not a trait
//! implemented for `HashMap`/`Vec`/`BTreeSet`. Structural rule 1 names the
//! impl file after the self type (`hash_map.rs`, `vec.rs`, ...), which would
//! scatter three one-line bodies across three std-type files; the free
//! functions keep one honest file for the purge leaves.
use std::collections::HashMap;

use crate::types::dom_node::DomNodeSlotKey;

/// Keeps every map entry whose node key is NOT in `removed`.
pub(crate) fn retain_not_in_map<V>(
    map: &mut HashMap<DomNodeSlotKey, V>,
    removed: &[DomNodeSlotKey],
) {
    map.retain(|key, _| !removed.contains(key));
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
    fn retain_not_in_map_keeps_only_non_removed_keys() {
        let mut slots = slotmap::SlotMap::with_key();
        let a = slots.insert(());
        let b = slots.insert(());
        let c = slots.insert(());
        let mut map: HashMap<DomNodeSlotKey, f32> = HashMap::from([(a, 1.0), (b, 2.0), (c, 3.0)]);

        retain_not_in_map(&mut map, &[b]);

        assert!(map.contains_key(&a));
        assert!(!map.contains_key(&b));
        assert!(map.contains_key(&c));
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
        let mut map: HashMap<DomNodeSlotKey, f32> = HashMap::from([(a, 1.0), (b, 2.0)]);
        let mut vec = vec![a, b];

        retain_not_in_map(&mut map, &[]);
        retain_not_in_vec(&mut vec, &[]);

        assert_eq!(map.len(), 2);
        assert_eq!(vec, vec![a, b]);
    }
}
