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
- `metrics.rs` owns the retained history entry shape and the `Metrics` tally
  owner; it re-exports the metric-name contract, counter snapshot, history
  store, and history iterator from their own files.
- `metric_name.rs` owns the metric-name contract (`MetricName`).
- `metric_snapshot.rs` owns the copyable enum-to-count tally snapshot.
- `metric_history_store.rs` owns the fixed-size retained history ring.
- `metric_history_iter.rs` owns the allocation-free history iterator.
- `refresh_tracked_store.rs` owns retained values with monotonic refresh
  metadata and most-recent-to-least-recent iteration; it re-exports the
  recency item and recency iterator from their own files.
- `refresh_tracked_store_recency_item.rs` owns the borrowed recency item view.
- `refresh_tracked_store_recency_iter.rs` owns the recency-ordered iterator.
