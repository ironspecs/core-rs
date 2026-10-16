//! Public crate surface for shared retained collection mechanics.
//!
//! This crate owns generic, reusable collection and storage structures used by
//! multiple runtime crates in this repository. It keeps these mechanics out of
//! app-domain crates while avoiding a vague helper bucket.
//!
//! Invariants and rules:
//!
//! - this crate owns only generic collection/storage mechanics
//! - public APIs here must stay free of app-domain vocabulary
//! - domain-specific policy belongs in the caller that wraps these types
//! - this crate must not become a miscellaneous helper crate
#![forbid(unsafe_code)]
#![forbid(clippy::as_conversions)]
#![forbid(clippy::cast_possible_truncation)]
#![forbid(clippy::cast_possible_wrap)]
#![forbid(clippy::cast_precision_loss)]
#![forbid(clippy::cast_sign_loss)]
#![forbid(clippy::checked_conversions)]

pub mod lru_cache;
pub mod metric_history_iter;
pub mod metric_history_store;
pub mod metric_snapshot;
pub mod metrics;
pub mod refresh_tracked_store;
pub mod refresh_tracked_store_recency_item;
pub mod refresh_tracked_store_recency_iter;
