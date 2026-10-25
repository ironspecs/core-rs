//! The minimal node-store surface the unified fragment-import splice walks.
//!
//! What this file represents:
//!
//! - a three-method trait (`get`, `insert`, `append_child_to_parent`) that is
//!   exactly the node-store access a splice performs on its destination store
//! - the seam that lets ONE splice walk serve both destinations: the live
//!   document's metric-instrumented `RootDomNodes` proxy and the fragment
//!   store's raw `SlotMap`
//!
//! Ownership boundary:
//!
//! - this is the observability seam, not an algorithm. It declares WHAT the
//!   splice touches; the implementors decide HOW (instrumented vs plain).
//!
//! Invariants and rules:
//!
//! - the trait carries no default method bodies — it is a pure interface, so
//!   each store's behavior lives entirely in its impl
//! - `get`/`insert` are the instrumented-access methods: the live impl routes
//!   them through `RootDomNodes`'s inherent methods (which increment the
//!   `RootDomNodesMetricName` counters); the raw impl routes them to the
//!   `SlotMap` directly
//! - `append_child_to_parent` is deliberately NOT instrumented in either impl:
//!   it relinks existing nodes through the raw slot store, exactly matching
//!   the pre-splice `append_child_to_parent(&mut self.nodes, ...)` access
//!   pattern, so the node-store metric pins stay put
//!
//! Extraction note (PR 5, E1 phase 2): the E4 finding (observability is
//! load-bearing, not decoration) said a walk over `&mut SlotMap` drops
//! `Get`/`GetMut` counts via `DerefMut` coercion. The trait resolves that
//! without a journal: the instrumentation lives in the store impl. It is a
//! trait (not a free function) because the two store types share no local
//! supertype, and implementing it for the foreign `SlotMap` directly would
//! force a `slot_map.rs` file (structural rule 1) — so the raw store is
//! wrapped by the local `RawNodeStore` type instead.
use crate::types::DomError;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};

pub(crate) trait NodeStore {
    /// Returns one node by its slot key.
    fn get(&self, key: DomNodeSlotKey) -> Option<&DomNode>;

    /// Inserts one node and returns its allocated slot key.
    fn insert(&mut self, value: DomNode) -> DomNodeSlotKey;

    /// Links `child_key` under `parent_key`, returning the child key.
    fn append_child_to_parent(
        &mut self,
        parent_key: DomNodeSlotKey,
        child_key: DomNodeSlotKey,
    ) -> Result<DomNodeSlotKey, DomError>;
}
