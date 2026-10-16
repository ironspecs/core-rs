//! Internal simple-selector model for `ui-dom` query-selector parsing.
//!
//! This file owns the parsed single selector step used by
//! `dom_query_selector.rs`. Its purpose is to keep the id/classes/tag triple
//! small and explicit while query traversal reads it without re-parsing the
//! selector string.
//!
//! Invariants and rules:
//!
//! - this file owns only the `SimpleSelector` struct and its `impl` blocks
//! - a selector step is at most one id, one tag, and any class list

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SimpleSelector {
    pub(crate) id: Option<String>,
    pub(crate) classes: Vec<String>,
    pub(crate) tag: Option<String>,
}

impl SimpleSelector {
    pub(crate) fn is_pure_id_selector(&self) -> bool {
        self.id.is_some() && self.tag.is_none() && self.classes.is_empty()
    }
}
