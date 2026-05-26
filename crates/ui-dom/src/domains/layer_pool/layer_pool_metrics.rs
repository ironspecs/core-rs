//! `LayerPool` metric aliases and store shape.
//! This file exists so the metric container choice stays attached to the layer-pool boundary and does not get casually inlined or redefined elsewhere.
//!
//! The public snapshot type, public history aliases, and private store type are split here because callers need read-only metric inspection while the owner needs the mutable ring-buffered store.
//! Keeping the aliases together makes the fixed-count metric layout obvious at the module boundary.
//!
//! Strict invariants:
//! - the public metric shape is a snapshot, not live mutable storage.
//! - the public history shape matches the shared metrics pattern used by `RootDom`.
//! - the internal metric store stays private to the layer-pool boundary.
//! - the metric count must stay tied to `LayerPoolMetricName::COUNT`.

#[allow(clippy::wildcard_imports)]
use super::*;
use collections::metrics::{MetricHistoryEntry, MetricHistoryIter};

/// Snapshot copy of `LayerPool` metric counters.
pub type LayerPoolMetrics = MetricSnapshot<LayerPoolMetricName, { LayerPoolMetricName::COUNT }>;

/// One retained `LayerPool` metric increment event.
pub type LayerPoolMetricHistoryEntry = MetricHistoryEntry<LayerPoolMetricName>;

/// Non-allocating iterator over retained `LayerPool` metric increment history.
pub type LayerPoolMetricHistoryIter<'a> =
    MetricHistoryIter<'a, LayerPoolMetricName, LAYER_POOL_HISTORY_CAPACITY>;

pub(super) type LayerPoolMetricStore =
    Metrics<LayerPoolMetricName, { LayerPoolMetricName::COUNT }, LAYER_POOL_HISTORY_CAPACITY>;
