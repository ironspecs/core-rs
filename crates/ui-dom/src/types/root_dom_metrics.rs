//! Root-owned metric vocabulary for `ui-dom`.
//!
//! This module defines the root-specific metric storage and history aliases
//! for retained `ui-dom` mutation profiling. The metric name vocabulary
//! (`RootDomMetricName`) lives in its own file and is re-exported here.
//! Allocation metrics are semantic counters for named mutating `RootDom` work
//! sites, while the shared storage and history mechanics live in
//! `collections::metrics`.
//!
//! Intended usage:
//!
//! - `RootDom::increment_metric(name)` must be called only after a measured
//!   mutating operation changes retained `RootDom` state.
//! - Metrics must not be incremented from selector queries, hit-test reads,
//!   style-map reads, or any other read-only traversal.
//! - Metrics must not be incremented for no-op calls that leave `RootDom` state
//!   unchanged.
//! - `RootDom::count(name)` is the aggregate-counter read API.
//! - `RootDom::history()` is the retained increment-event read API.
//! - `RootDom::reset_metrics()` starts a new profiling window.
//!
//! Invariants and requirements:
//!
//! - Metric names must be enum variants, not strings or public struct fields.
//! - Metric increments must require mutable access to `RootDom`.
//! - Counted operations must mutate retained `RootDom` state.
//! - Root-local read APIs must remain read-only; do not change them to
//!   `&mut self` to support metrics.
//! - The metrics tally must remain an attachment on `RootDom`.
//! - Metrics must not be passed down through lower-level function parameters or
//!   returned through lower-level result types.
//! - The metrics tally must not use interior mutability such as `Cell`, `Arc`,
//!   or mutexes to bypass the root mutation boundary.
//! - The root-owned tally must perform no heap allocation after `RootDom`
//!   creation.
//! - Metric counters must be fixed arrays indexed by `RootDomMetricName`.
//! - History must use `ROOT_DOM_METRIC_HISTORY_CAPACITY`; once full, new
//!   increments overwrite the oldest retained entries.
//! - `history()` must return retained entries oldest-to-newest without
//!   allocating.
//! - Allocation metrics must count intentional mutating `ui-dom` work sites, not
//!   global allocator calls, bytes, or capacity-growth details.
//!
//! Example:
//!
//! ```rust
//! # use ui_dom::{PixelDensity, RootDom, RootDomContext, RootDomMetricName};
//! let mut root = RootDom::new(RootDomContext::new_cpu());
//! root.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
//! assert_eq!(
//!     root.count(RootDomMetricName::TextIntrinsicMeasurement),
//!     1
//! );
//! assert_eq!(
//!     root.history().next().map(|entry| entry.name),
//!     Some(RootDomMetricName::TextIntrinsicMeasurement)
//! );
//! root.reset_metrics();
//! assert_eq!(root.history().next(), None);
//! ```

use collections::metrics::{MetricHistoryEntry, MetricHistoryIter, MetricSnapshot, Metrics};

pub use crate::types::root_dom_metric_name::RootDomMetricName;

/// Number of metric history entries retained by one `RootDom`.
pub const ROOT_DOM_METRIC_HISTORY_CAPACITY: usize = 128;

/// Snapshot copy of root-owned metric counter values.
pub type RootDomMetrics = MetricSnapshot<RootDomMetricName, { RootDomMetricName::COUNT }>;

/// One retained root-owned metric increment event.
pub type RootDomMetricHistoryEntry = MetricHistoryEntry<RootDomMetricName>;

/// Non-allocating iterator over retained root-owned metric increment history.
pub type RootDomMetricHistoryIter<'a> =
    MetricHistoryIter<'a, RootDomMetricName, ROOT_DOM_METRIC_HISTORY_CAPACITY>;

/// Root-owned mutable metric counter and fixed history store.
pub(crate) type RootDomMetricTally =
    Metrics<RootDomMetricName, { RootDomMetricName::COUNT }, ROOT_DOM_METRIC_HISTORY_CAPACITY>;
