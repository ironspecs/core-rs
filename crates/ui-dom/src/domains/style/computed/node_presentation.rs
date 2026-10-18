//! A node's presentation state (PLAN.md S1.1).
//!
//! This file owns the `NodePresentation` struct: the per-node record that
//! carries the inline declaration block key, the seven computed groups (inline
//! by value), and the transition-longhand scheduler slot. Per S1.1, computed
//! style travels *with* the node so a visitor holding the node has everything
//! on one fetch (visit locality), beside `box_model`, `layout`, and `paint` in
//! the node record.
//!
//! **Winners-fold phase (spine 3.3g-2a).** `computed` and
//! `transition_longhands` are written by the style pass's typed winners fold
//! (`domains/visitor/winners_fold.rs`) from the cascade; `decl` is written by
//! the inline-style store (`style_mut` and the `style`-attribute route) and
//! names the node-owned block in the declaration arena. Text nodes carry
//! `Default` and are never folded.
//!
//! Invariants and rules:
//!
//! - `decl == None` means the node's inline `style` attribute was never written;
//!   `Some(DeclBlockKey)` keys the node-owned block in the declaration arena
//! - `computed` is the seven groups inline by value (no indirection, no COW)
//! - `transition_longhands` is the scheduler-facing slot (S1.1): written by the
//!   style pass from winners, read by the scheduler on value change; not a
//!   computed group and not diff-classified
//! - `previous_computed` is the seven groups as of the last *completed* style
//!   pass (`None` before the first pass — the pass's first visit establishes
//!   the baseline without diffing, so the initial style application never
//!   schedules a transition, matching CSS). The write path upserts the inline
//!   block and leaves `previous_computed` untouched, so the next
//!   pass diffs the last-pass groups against the freshly resolved groups (D2's
//!   one-timing model: the change is detected at the style update, not at the
//!   write). This is pass-side retained state, not a computed group.
//! - `Default` seeds `computed` from `ComputedGroups::initial()` (the
//!   no-parent seed), `decl` from `None`, `previous_computed` from `None`, and
//!   `transition_longhands` from its own empty `Default`

use crate::domains::style::DeclBlockKey;

use super::computed_groups::ComputedGroups;
use super::resolved_transition_longhands::ResolvedTransitionLonghands;

/// A node's presentation state: inline block + computed groups + longhands.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePresentation {
    /// Inline style block (the `style` attribute / `style_mut` store), or
    /// `None` when never written.
    pub decl: Option<DeclBlockKey>,
    /// The seven computed groups, inline by value.
    pub computed: ComputedGroups,
    /// The seven groups as of the last completed style pass, or `None` before
    /// the first pass. Written only by the pass; the write path re-projects
    /// `computed` without touching this, so the pass can diff against the
    /// last-pass value (spine 3.3g-1b, D2 one-timing).
    pub previous_computed: Option<ComputedGroups>,
    /// The cascaded transition longhands, for the scheduler (S1.1).
    pub transition_longhands: ResolvedTransitionLonghands,
}

impl Default for NodePresentation {
    fn default() -> Self {
        Self {
            decl: None,
            computed: ComputedGroups::initial(),
            previous_computed: None,
            transition_longhands: ResolvedTransitionLonghands::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seeds_no_block_initial_groups_and_empty_longhands() {
        let presentation = NodePresentation::default();
        assert_eq!(presentation.decl, None);
        assert_eq!(presentation.computed, ComputedGroups::initial());
        assert_eq!(presentation.previous_computed, None);
        assert!(presentation.transition_longhands.is_empty());
    }
}
