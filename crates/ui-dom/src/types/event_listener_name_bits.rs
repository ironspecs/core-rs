//! Exact event-name listener presence cache for non-bubbling dispatch.
//!
//! This type owns the compact bitset that lets root-owned dispatch reject
//! non-bubbling dispatch quickly when a target has no listeners for an exact
//! event name. The cache must stay synchronized with node-local listener
//! buckets; if the two drift apart, non-bubbling dispatch correctness and
//! performance both degrade.

use std::collections::BTreeMap;

use crate::types::DomEventName;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct EventListenerNameBits {
    bits: u32,
}

impl EventListenerNameBits {
    pub(crate) const CAPACITY: usize = 32;

    #[must_use]
    pub(crate) fn has(self, event_name: DomEventName) -> bool {
        self.bits & Self::bit(event_name) != 0
    }

    pub(crate) fn set(&mut self, event_name: DomEventName) {
        self.bits |= Self::bit(event_name);
    }

    pub(crate) fn clear(&mut self, event_name: DomEventName) {
        self.bits &= !Self::bit(event_name);
    }

    /// Synchronizes one exact-name cache bit against the targeted listener
    /// bucket and drops that bucket when it becomes empty.
    pub(crate) fn sync_event_name_to_event_listener<T>(
        &mut self,
        event_name: DomEventName,
        event_listeners: &mut BTreeMap<DomEventName, Vec<T>>,
    ) {
        if event_listeners
            .get(&event_name)
            .is_some_and(|listeners| !listeners.is_empty())
        {
            self.set(event_name);
            return;
        }
        event_listeners.remove(&event_name);
        self.clear(event_name);
    }

    fn bit(event_name: DomEventName) -> u32 {
        1u32 << event_name.index()
    }
}

const _: () = assert!(DomEventName::COUNT <= EventListenerNameBits::CAPACITY);

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::EventListenerNameBits;
    use crate::types::DomEventName;

    #[test]
    fn sync_event_name_to_event_listener_sets_bit_when_bucket_has_entries() {
        let mut bits = EventListenerNameBits::default();
        let mut event_listeners = BTreeMap::new();
        event_listeners.insert(DomEventName::Click, vec![1u8]);

        bits.sync_event_name_to_event_listener(DomEventName::Click, &mut event_listeners);

        assert!(bits.has(DomEventName::Click));
        assert!(event_listeners.contains_key(&DomEventName::Click));
    }

    #[test]
    fn sync_event_name_to_event_listener_clears_bit_and_removes_empty_bucket() {
        let mut bits = EventListenerNameBits::default();
        let mut event_listeners = BTreeMap::new();
        event_listeners.insert(DomEventName::Click, Vec::<u8>::new());
        bits.set(DomEventName::Click);

        bits.sync_event_name_to_event_listener(DomEventName::Click, &mut event_listeners);

        assert!(!bits.has(DomEventName::Click));
        assert!(!event_listeners.contains_key(&DomEventName::Click));
    }

    #[test]
    fn sync_event_name_to_event_listener_only_touches_targeted_event_name() {
        let mut bits = EventListenerNameBits::default();
        let mut event_listeners = BTreeMap::new();
        event_listeners.insert(DomEventName::Click, Vec::<u8>::new());
        event_listeners.insert(DomEventName::Focus, vec![1u8]);
        bits.set(DomEventName::Click);
        bits.set(DomEventName::Focus);

        bits.sync_event_name_to_event_listener(DomEventName::Click, &mut event_listeners);

        assert!(!bits.has(DomEventName::Click));
        assert!(bits.has(DomEventName::Focus));
        assert!(!event_listeners.contains_key(&DomEventName::Click));
        assert!(event_listeners.contains_key(&DomEventName::Focus));
    }
}
