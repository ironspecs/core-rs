//! Shared metric-name contract for enum-addressed metrics storage.
//!
//! This file owns the `MetricName` trait: the mechanical contract every metric
//! vocabulary implements so counter snapshots, history stores, and tallies can
//! address fixed counter indexes without tying metric meaning to a specific
//! runtime domain.
//!
//! Strict invariants:
//! - metric names must resolve to stable counter indexes
//! - implementors stay `Copy` so reads remain allocation-free

/// Mechanical contract for enum-addressed metrics storage.
pub trait MetricName: Copy {
    /// Returns the fixed counter index for this metric.
    fn index(self) -> usize;
}
