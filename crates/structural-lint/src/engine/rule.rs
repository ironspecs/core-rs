//! The [`Rule`] trait — the contract every lint rule implements.
//!
//! [`Rule`] is behavior-bearing (it carries default method bodies), so this
//! file is named after the trait and carries its own unit tests per rule 2.
//! Each rule provides a stable `id`, a `check_file` callback for the file
//! phase, and a `check_workspace` callback for the workspace phase.
//!
//! # Local invariants
//!
//! - Default bodies are no-ops — rules override only the phases they need.
//! - Rules never print; they only `emit` through the [`Emitter`] trait.
//! - The trait is `Send + Sync` so rule objects can be stored in a `Vec`.

use crate::engine::context::FileContext;
use crate::engine::emitter::Emitter;
use crate::engine::index::WorkspaceIndex;

/// One isolated structural-lint check with a stable rule id.
pub(crate) trait Rule: Send + Sync {
    /// Stable rule id for this check.
    ///
    /// Backed by each rule's `const ID` and validated by
    /// `rules::tests::rule_ids_match_expected_order`. The current run path stamps
    /// `Self::ID` into diagnostics directly; this method is the consumer-facing
    /// identity contract for the future rule-enable config (DESIGN-SPEC §12).
    #[allow(dead_code)] // §12 config contract; kept consistent with emit via `const ID`.
    fn id(&self) -> &'static str;

    /// File-phase check. Default: no-op.
    fn check_file(&self, _ctx: &FileContext<'_>, _emitter: &mut dyn Emitter) {}

    /// Workspace-phase check. Default: no-op.
    fn check_workspace(&self, _index: &WorkspaceIndex, _emitter: &mut dyn Emitter) {}
}

#[cfg(test)]
use crate::engine::vec_emitter::VecEmitter;

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopRule;

    impl Rule for NoopRule {
        fn id(&self) -> &'static str {
            "noop"
        }
    }

    #[test]
    fn default_bodies_are_noops() {
        let source = "";
        let file = syn::parse_file(source).unwrap();
        let rel = String::from("crates/x/src/mod.rs");
        let stem = String::from("mod");

        let ctx = FileContext {
            relative_path: &rel,
            filename_stem: &stem,
            is_mod_rs: true,
            file: &file,
        };

        let mut em = VecEmitter::default();
        let rule = NoopRule;

        rule.check_file(&ctx, &mut em);
        assert!(em.diagnostics.is_empty());

        rule.check_workspace(&WorkspaceIndex::default(), &mut em);
        assert!(em.diagnostics.is_empty());
    }
}
