//! Visitor-domain traversal algorithms for `ui-dom`.
//!
//! This module owns traversal-specific logic that should run work during tree
//! visits instead of collecting intermediate result lists for later passes.
//!
//! Invariants and rules:
//!
//! - visitors keep traversal-local state only
//! - visitors do not create alternate ownership boundaries
//! - visitors should prefer direct execution over temporary collections
pub(crate) mod event_target;
pub(crate) mod inherited_style;
pub(crate) mod inherited_style_context;
pub(crate) mod pseudo_state;
pub(crate) mod scroll;
pub(crate) mod style_recompute;
pub(crate) mod winners_fold;
