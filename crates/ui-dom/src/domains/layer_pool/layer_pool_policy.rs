//! `LayerPoolPolicy` selects how one retained layer participates in automatic retained work.
//! This file exists so upload and invalidation policy stays explicit data on the layer record instead of becoming scattered branch logic across callers.
//!
//! The enum is small because these are stable policy modes, not transient executor state.
//! Keeping it separate from `LayerPool` makes it easier to review whether a new mode is a real retained-layer policy or just a local workaround.
//!
//! Strict invariants:
//! - policy is chosen once per layer record and consumed by the owner and queue boundaries.
//! - policy must not hide extra behavior beyond upload pressure and invalidation participation.
//! - new modes should only be added for real retained-layer lifecycle differences.

/// Selects how one retained layer participates in automatic DOM-driven work.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LayerPoolPolicy {
    /// Queue retained updates immediately and keep current invalidation rules.
    Normal,
    /// Defer retained uploads when the queue is already under upload pressure.
    BestEffort,
    /// Suppress automatic DOM-driven retained uploads for this layer.
    Offscreen,
    /// Suppress DOM-driven retained uploads while keeping the layer composited.
    External,
}
