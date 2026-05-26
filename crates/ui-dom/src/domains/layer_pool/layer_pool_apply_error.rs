//! `LayerPoolApplyError` is the strict failure contract for retained layer apply requests.
//! This file exists so apply failures stay explicit and fail-fast instead of degrading into placeholder success or silent skips.
//!
//! The enum is intentionally narrow because apply failures at this boundary are ownership or install-contract violations, not a grab bag of recoverable states.
//! Keeping it separate from `LayerPool` prevents the main owner file from burying the outward error contract inside implementation detail.
//!
//! Strict invariants:
//! - apply failures must remain explicit and typed.
//! - this error type must not encode graceful fallback behavior.
//! - only real retained-boundary violations belong here.

/// Failure returned when `LayerPool` cannot complete retained GPU apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerPoolApplyError {
    /// The supplied lease no longer resolves to the current retained layer slot.
    InvalidLease(&'static str),
    /// The app does not expose the direct GPU executor wiring required for retained apply.
    MissingDirectGpuExecutor,
}
