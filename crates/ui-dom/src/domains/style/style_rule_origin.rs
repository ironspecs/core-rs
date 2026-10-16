//! Stylesheet origin identity for `ui-dom`.
//!
//! This file owns the `StyleRuleOrigin` value that pairs a stylesheet origin
//! key with a numeric precedence priority. The retained style runtime keys its
//! per-origin index on the origin key, and the priority orders the origin among
//! otherwise-equally-specific matched rules.
//!
//! Invariants and rules:
//!
//! - this file owns only the `StyleRuleOrigin` struct and its `impl` block
//! - the origin key is an opaque caller-owned string; the runtime never parses
//!   it beyond `PartialEq` comparisons
//! - priority is a plain `i32`; higher values win at equal selector specificity

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleRuleOrigin {
    pub key: String,
    pub priority: i32,
}

impl StyleRuleOrigin {
    #[must_use]
    pub fn new(key: impl Into<String>, priority: i32) -> Self {
        Self {
            key: key.into(),
            priority,
        }
    }
}
