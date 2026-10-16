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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pure_id_selector_only_for_bare_id() {
        let bare_id = SimpleSelector {
            id: Some("app".to_string()),
            classes: Vec::new(),
            tag: None,
        };
        assert!(bare_id.is_pure_id_selector());

        let id_plus_class = SimpleSelector {
            id: Some("app".to_string()),
            classes: vec!["card".to_string()],
            tag: None,
        };
        assert!(!id_plus_class.is_pure_id_selector());

        let tag_only = SimpleSelector {
            id: None,
            classes: Vec::new(),
            tag: Some("div".to_string()),
        };
        assert!(!tag_only.is_pure_id_selector());

        let no_id = SimpleSelector {
            id: None,
            classes: vec!["card".to_string()],
            tag: Some("div".to_string()),
        };
        assert!(!no_id.is_pure_id_selector());
    }
}
