# `collections`

`collections` owns reusable retained collection mechanics shared across runtime
crates in this repository.

## Purpose

This crate exists so generic collection/storage structures do not stay trapped
inside one product-domain crate once multiple owners need them.

It owns:
- generic bounded least-recently-used cache mechanics
- generic fixed-size metric tally and retained-history mechanics
- generic retained refresh-tracked store mechanics with explicit recency order

It does not own:
- app policy
- runtime domains
- Bevy resources
- worker-specific invalidation rules
- domain-specific metric vocabularies

## Boundary Rules

- APIs in this crate must stay generic and reusable.
- Types here must not depend on app or domain crates.
- Domain-specific policy stays in the owning crate that wraps these
  collections.
- This crate must not become a dumping ground for unrelated helpers.

## Current Modules

- `lru_cache.rs` owns strict bounded recency ordering and eviction.
- `metrics.rs` owns fixed-size enum-indexed counters and retained history.
- `refresh_tracked_store.rs` owns retained values with monotonic refresh
  metadata and most-recent-to-least-recent iteration.
