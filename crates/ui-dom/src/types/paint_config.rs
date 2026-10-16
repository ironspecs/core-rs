//! Shared paint policy for `ui-dom`.
//!
//! This file represents the immutable paint policy every `RootDom` receives at
//! creation through its shared context. Its purpose is to keep backend
//! selection explicit instead of hiding it in paint globals or per-root
//! one-off state.
//!
//! Invariants and rules:
//!
//! - this file owns only the `PaintConfig` struct and its `impl` blocks
//! - the policy is immutable and copyable; it carries no runtime state
//! - backend selection must stay explicit through `PaintBackend`

use crate::types::root_dom_context::PaintBackend;

/// Immutable paint policy shared by every `RootDom` using one context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaintConfig {
    /// Preferred execution target for retained layer paint.
    pub backend: PaintBackend,
}

impl PaintConfig {
    #[must_use]
    pub const fn new(backend: PaintBackend) -> Self {
        Self { backend }
    }
}
