//! Retained style rule and declaration store for `ui-dom`.
//!
//! This file owns the `StyleRuntime` value that compiles authored stylesheet
//! text into retained, indexed rules backed by an arena of typed declaration
//! blocks, and answers candidate/matched-rule lookups for the shared style
//! pipeline. Selector parsing, specificity, precedence, the node-match walk,
//! and the rule identity hash stay in `style::mod`; the runtime keeps the rule
//! store, the `DeclStore` arena, and the id/class/tag/universal/origin/identity
//! indexes.
//!
//! Invariants and rules:
//!
//! - this file owns only the `StyleRuntime` struct and its `impl` block
//! - rule compilation delegates to the parent module's selector and block
//!   parsers; the runtime never parses selector syntax on its own
//! - declaration parsing is drop-and-continue (CSS 2.1 §4.1.7): malformed
//!   declarations are counted into the load report and a monotonic
//!   `declarations_dropped` counter, never silently swallowed
//! - the indexes are retained per origin and per right-most selector so
//!   candidate lookup and origin unload stay O(indexed) rather than O(rules)
//! - dedup runs before a block is committed, so a duplicate load never leaks a
//!   declaration block into the arena
//! - `load_stylesheet` is atomic (buffer-then-commit): the rule store, the
//!   indexes, `order_counter`, and `revision` mutate only after every block
//!   has parsed, so a typed `Err` means nothing happened to the rule store;
//!   declaration drops are still counted and reported on a failing load (the
//!   G drop-report contract — drops are observations, not rule-store
//!   mutations)
//! - pseudo-state counts and the revision counter are kept exact and
//!   zero-allocation; both are updated under the same retained-store lock

use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use super::decl_block::DeclBlock;
use super::decl_drop_report::StylesheetLoadReport;
use super::decl_parse::parse_declaration_block;
use super::{
    CompiledRule, DeclBlockKey, DeclStore, Declaration, NodeLookup, NodeStyleIndex,
    PrecedenceTuple, PseudoStateCounts, RightMostSelector, SelectorSpecificity, SimpleSelector,
    StyleRuleKey, StyleRuleOrigin, TextKey, append_rule_keys, compute_rule_identity_hash,
    extract_right_most_selector, parse_css_blocks, parse_selector, pseudo_state_counts,
    selector_matches_node, specificity_for_selector_parts,
};
use crate::types::{DomError, DomNodeSlotKey, DomPseudoState};

#[cfg(test)]
use super::{DeclLength, DeclValue, Keyword, PresentationProperty};

#[derive(Debug, Default)]
pub struct StyleRuntime {
    rules: SlotMap<StyleRuleKey, CompiledRule>,
    decl_store: DeclStore,
    declarations_dropped: u64,
    id_index: HashMap<String, Vec<StyleRuleKey>>,
    class_index: HashMap<String, Vec<StyleRuleKey>>,
    tag_index: HashMap<String, Vec<StyleRuleKey>>,
    universal_index: Vec<StyleRuleKey>,
    origin_index: HashMap<String, Vec<StyleRuleKey>>,
    identity_index: HashMap<u64, Vec<StyleRuleKey>>,
    hover_rule_count: usize,
    active_rule_count: usize,
    focus_rule_count: usize,
    focus_visible_rule_count: usize,
    order_counter: u64,
    revision: u64,
}

/// One rule staged by phase 1 of `load_stylesheet` before any rule-store or
/// index mutation: `(decl_block, selector, selector_parts, right_most,
/// specificity, identity_hash)`. The `selector` string is retained for
/// diagnostics/debug only (no matching or hashing role). The origin is uniform
/// across a load, so phase 2 reconstructs it rather than staging it per rule.
type StagedRule = (
    DeclBlock,
    String,
    Vec<SimpleSelector>,
    RightMostSelector,
    SelectorSpecificity,
    u64,
);

/// The equivalence-relevant fields of one rule, extracted from either an
/// active `CompiledRule` or a staged `StagedRule`, so the active-rule dedup
/// and the same-load staged dedup share one comparison.
struct RuleEquivalence<'a> {
    selector_parts: &'a [SimpleSelector],
    declarations: &'a [Declaration],
    specificity: SelectorSpecificity,
    origin: &'a StyleRuleOrigin,
}

/// Equivalence test shared by the active-rule dedup and the same-load staged
/// dedup. Two rules are equivalent when their selector parts, declarations,
/// specificity, and origin all match — the exact field set the pre-restructure
/// mid-loop insert compared. Both dedup tiers ride this one comparison so they
/// can never drift apart.
fn rules_are_equivalent(a: &RuleEquivalence<'_>, b: &RuleEquivalence<'_>) -> bool {
    a.selector_parts == b.selector_parts
        && a.declarations == b.declarations
        && a.specificity == b.specificity
        && a.origin == b.origin
}

/// Same-load staged dedup, hash-bucketed by identity hash to mirror the active
/// tier's `identity_index` discipline: only the staged entries in the matching
/// bucket are re-checked with the shared `rules_are_equivalent`, so a load is
/// O(N) with hash collisions re-checked rather than a full linear scan of
/// every staged rule.
fn has_equivalent_staged_rule(
    staged: &[StagedRule],
    staged_identity_index: &HashMap<u64, Vec<usize>>,
    identity_hash: u64,
    selector_parts: &[SimpleSelector],
    declarations: &[Declaration],
    specificity: SelectorSpecificity,
    origin: &StyleRuleOrigin,
) -> bool {
    staged_identity_index
        .get(&identity_hash)
        .is_some_and(|positions| {
            positions.iter().any(|&pos| {
                let (staged_decl, _, staged_parts, _, staged_spec, _) = &staged[pos];
                rules_are_equivalent(
                    &RuleEquivalence {
                        selector_parts,
                        declarations,
                        specificity,
                        origin,
                    },
                    &RuleEquivalence {
                        selector_parts: staged_parts.as_slice(),
                        declarations: staged_decl.declarations(),
                        specificity: *staged_spec,
                        origin,
                    },
                )
            })
        })
}

impl StyleRuntime {
    /// Loads one stylesheet origin's rules into the retained store atomically.
    ///
    /// The load is buffer-then-commit: phase 1 parses and stages every block
    /// (selector parse, right-most extraction, specificity, declaration parse,
    /// identity hash, dedup check) without mutating the rule store, the
    /// indexes, `order_counter`, or `revision`; phase 2 commits the staged
    /// rules only after every block has parsed. A typed `Err` therefore means
    /// nothing happened to the rule store. Declaration drops are still counted
    /// and reported on a failing load (the G drop-report contract): drops are
    /// observations of malformed input, not rule-store mutations, so they
    /// accumulate even when a later block fails the whole load.
    pub(crate) fn load_stylesheet(
        &mut self,
        origin: StyleRuleOrigin,
        css_text: &str,
    ) -> Result<StylesheetLoadReport, DomError> {
        let StyleRuleOrigin {
            key: origin_key,
            priority: origin_priority,
        } = origin;
        let origin = StyleRuleOrigin {
            key: origin_key.clone(),
            priority: origin_priority,
        };

        // Phase 1 — parse and stage. Nothing here mutates `self.rules`, the
        // indexes, `order_counter`, or `revision`. Declaration text is
        // interned and drops are counted (the G contract), but no block is
        // inserted until every block has parsed.
        let blocks = parse_css_blocks(css_text)?;
        let mut report = StylesheetLoadReport::default();
        let mut staged: Vec<StagedRule> = Vec::new();
        let mut staged_identity_index: std::collections::HashMap<u64, Vec<usize>> =
            std::collections::HashMap::new();
        for block in blocks {
            let selector_parts = parse_selector(&block.selector)?;
            let right_most = extract_right_most_selector(&selector_parts)?;
            let specificity = specificity_for_selector_parts(&selector_parts);
            let (decl_block, drop_report) =
                parse_declaration_block(&block.declarations, self.decl_store.texts_mut());
            report.declarations_applied += drop_report.applied;
            report.declarations_dropped += drop_report.dropped;
            self.declarations_dropped = self.declarations_dropped.saturating_add(
                u64::try_from(drop_report.dropped)
                    .expect("declaration drop count should fit into u64"),
            );

            let identity_hash = compute_rule_identity_hash(
                &origin,
                specificity,
                &selector_parts,
                decl_block.declarations(),
            );
            // Two-tier dedup: skip a block that matches an already-committed
            // active rule (reload dedup) OR an already-staged rule in this
            // same load (same-load dedup); both tiers share
            // `rules_are_equivalent`. Same-load dedup is hash-bucketed by
            // identity hash, mirroring the active tier's `identity_index`, so a
            // load is O(N) with `rules_are_equivalent` re-checking collisions.
            let has_active_equivalent = self.has_equivalent_active_rule(
                identity_hash,
                &selector_parts,
                decl_block.declarations(),
                specificity,
                &origin,
            );
            let has_staged_equivalent = has_equivalent_staged_rule(
                &staged,
                &staged_identity_index,
                identity_hash,
                &selector_parts,
                decl_block.declarations(),
                specificity,
                &origin,
            );
            if has_active_equivalent || has_staged_equivalent {
                continue;
            }
            staged.push((
                decl_block,
                block.selector,
                selector_parts,
                right_most,
                specificity,
                identity_hash,
            ));
            staged_identity_index
                .entry(identity_hash)
                .or_default()
                .push(staged.len() - 1);
        }

        // Phase 2 — commit. Only reached when every block parsed. Order is
        // assigned from `order_counter` in buffer order, and `revision` bumps
        // exactly once when at least one rule committed.
        for (decl_block, selector, selector_parts, right_most, specificity, identity_hash) in staged
        {
            let decl = self.decl_store.insert(decl_block);
            let order = self.order_counter;
            self.order_counter = self
                .order_counter
                .checked_add(1)
                .expect("style rule order counter should not overflow");
            let origin = StyleRuleOrigin {
                key: origin_key.clone(),
                priority: origin_priority,
            };
            let key = self.rules.insert(CompiledRule {
                selector,
                selector_parts,
                right_most,
                decl,
                specificity,
                origin,
                order,
                identity_hash,
                active: true,
            });
            self.index_rule(key)?;
            report.rules_inserted += 1;
        }
        if report.rules_inserted != 0 {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(report)
    }

    pub(crate) fn unload_stylesheet(&mut self, origin_key: &str) -> usize {
        let Some(keys) = self.origin_index.remove(origin_key) else {
            return 0;
        };
        let mut deactivated = 0usize;
        for key in keys {
            let Some(rule) = self.rules.get_mut(key) else {
                continue;
            };
            if !rule.active {
                continue;
            }
            let pseudo_counts = pseudo_state_counts(&rule.selector_parts);
            rule.active = false;
            self.unregister_pseudo_state_rule(pseudo_counts);
            deactivated += 1;
        }
        if deactivated != 0 {
            self.revision = self.revision.saturating_add(1);
        }
        deactivated
    }

    #[must_use]
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Monotonic count of declarations dropped by drop-and-continue parsing
    /// across all loads on this runtime.
    #[must_use]
    pub(crate) fn declarations_dropped_count(&self) -> u64 {
        self.declarations_dropped
    }

    /// Live declaration blocks in the stylesheet arena.
    #[must_use]
    pub(crate) fn decl_block_count(&self) -> usize {
        self.decl_store.live_block_count()
    }

    #[must_use]
    pub(crate) fn decl_block(&self, key: DeclBlockKey) -> Option<&DeclBlock> {
        self.decl_store.get(key)
    }

    #[must_use]
    pub(crate) fn resolve_text(&self, key: TextKey) -> Option<&str> {
        self.decl_store.resolve(key)
    }

    #[must_use]
    pub(crate) fn uses_pseudo_state(&self, pseudo_state: DomPseudoState) -> bool {
        match pseudo_state {
            DomPseudoState::Hover => self.hover_rule_count != 0,
            DomPseudoState::Active => self.active_rule_count != 0,
            DomPseudoState::Focus => self.focus_rule_count != 0,
            DomPseudoState::FocusVisible => self.focus_visible_rule_count != 0,
        }
    }

    pub(crate) fn candidate_keys_for_node(
        &self,
        node_index: NodeStyleIndex<'_>,
    ) -> Vec<StyleRuleKey> {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        if let Some(id) = node_index.id {
            append_rule_keys(
                &mut candidates,
                &mut seen,
                self.id_index.get(id).map(Vec::as_slice),
            );
        }
        for class in node_index.classes {
            append_rule_keys(
                &mut candidates,
                &mut seen,
                self.class_index.get(class).map(Vec::as_slice),
            );
        }
        append_rule_keys(
            &mut candidates,
            &mut seen,
            self.tag_index.get(node_index.tag).map(Vec::as_slice),
        );
        append_rule_keys(
            &mut candidates,
            &mut seen,
            Some(self.universal_index.as_slice()),
        );
        candidates
    }

    pub(crate) fn active_rule_for_node(
        &self,
        key: StyleRuleKey,
        lookup: NodeLookup<'_>,
        node_key: DomNodeSlotKey,
    ) -> Result<Option<(PrecedenceTuple, DeclBlockKey)>, DomError> {
        let Some(rule) = self.rules.get(key) else {
            return Ok(None);
        };
        if !rule.active {
            return Ok(None);
        }
        if !selector_matches_node(lookup, node_key, &rule.selector_parts)? {
            return Ok(None);
        }
        Ok(Some((
            (rule.origin.priority, rule.specificity, rule.order),
            rule.decl,
        )))
    }

    fn index_rule(&mut self, key: StyleRuleKey) -> Result<(), DomError> {
        let rule = self.rules.get(key).ok_or(DomError::InvalidNodeKey)?;
        if let Some(id) = rule.right_most.id.as_ref() {
            self.id_index.entry(id.clone()).or_default().push(key);
        }
        for class in &rule.right_most.classes {
            self.class_index.entry(class.clone()).or_default().push(key);
        }
        if let Some(tag) = rule.right_most.tag.as_ref() {
            self.tag_index.entry(tag.clone()).or_default().push(key);
        }
        if rule.right_most.universal {
            self.universal_index.push(key);
        }
        self.origin_index
            .entry(rule.origin.key.clone())
            .or_default()
            .push(key);
        self.identity_index
            .entry(rule.identity_hash)
            .or_default()
            .push(key);
        self.register_pseudo_state_rule(pseudo_state_counts(&rule.selector_parts));
        Ok(())
    }

    fn has_equivalent_active_rule(
        &self,
        identity_hash: u64,
        selector_parts: &[SimpleSelector],
        declarations: &[Declaration],
        specificity: SelectorSpecificity,
        origin: &StyleRuleOrigin,
    ) -> bool {
        let Some(keys) = self.identity_index.get(&identity_hash) else {
            return false;
        };
        keys.iter().any(|key| {
            self.rules.get(*key).is_some_and(|existing| {
                existing.active
                    && self.decl_store.get(existing.decl).is_some_and(|block| {
                        rules_are_equivalent(
                            &RuleEquivalence {
                                selector_parts,
                                declarations,
                                specificity,
                                origin,
                            },
                            &RuleEquivalence {
                                selector_parts: existing.selector_parts.as_slice(),
                                declarations: block.declarations(),
                                specificity: existing.specificity,
                                origin: &existing.origin,
                            },
                        )
                    })
            })
        })
    }

    fn register_pseudo_state_rule(&mut self, pseudo_counts: PseudoStateCounts) {
        self.hover_rule_count = self
            .hover_rule_count
            .saturating_add(pseudo_counts.hover_count);
        self.active_rule_count = self
            .active_rule_count
            .saturating_add(pseudo_counts.active_count);
        self.focus_rule_count = self
            .focus_rule_count
            .saturating_add(pseudo_counts.focus_count);
        self.focus_visible_rule_count = self
            .focus_visible_rule_count
            .saturating_add(pseudo_counts.focus_visible_count);
    }

    fn unregister_pseudo_state_rule(&mut self, pseudo_counts: PseudoStateCounts) {
        self.hover_rule_count = self
            .hover_rule_count
            .checked_sub(pseudo_counts.hover_count)
            .expect("style runtime hover rule count should not underflow");
        self.active_rule_count = self
            .active_rule_count
            .checked_sub(pseudo_counts.active_count)
            .expect("style runtime active rule count should not underflow");
        self.focus_rule_count = self
            .focus_rule_count
            .checked_sub(pseudo_counts.focus_count)
            .expect("style runtime focus rule count should not underflow");
        self.focus_visible_rule_count = self
            .focus_visible_rule_count
            .checked_sub(pseudo_counts.focus_visible_count)
            .expect("style runtime focus-visible rule count should not underflow");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_stylesheet_rejects_invalid_selector() {
        let mut runtime = StyleRuntime::default();
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "div > span { width: 10px; }",
            )
            .expect_err("invalid selector should fail");
        assert_eq!(error, DomError::InvalidSelector);
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".cta..primary { width: 10px; }",
            )
            .expect_err("invalid multi-class selector should fail");
        assert_eq!(error, DomError::InvalidSelector);
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "#hero.primary { width: 10px; }",
            )
            .expect_err("unsupported id-plus-class selector should fail");
        assert_eq!(error, DomError::InvalidSelector);
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "button#hero { width: 10px; }",
            )
            .expect_err("unsupported tag-plus-id selector should fail");
        assert_eq!(error, DomError::InvalidSelector);
    }

    #[test]
    fn failed_load_is_atomic() {
        let mut runtime = StyleRuntime::default();

        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".good { width: 1px } div > span { }",
            )
            .expect_err("the bad `div > span` selector should fail the load");
        assert_eq!(error, DomError::InvalidSelector);

        // Nothing happened to the rule store, the arena, the indexes, or the
        // revision: an `Err` means "nothing applied".
        assert!(runtime.rules.is_empty());
        assert_eq!(runtime.decl_store.live_block_count(), 0);
        assert!(runtime.id_index.is_empty());
        assert!(runtime.class_index.is_empty());
        assert!(runtime.tag_index.is_empty());
        assert!(runtime.universal_index.is_empty());
        assert!(runtime.origin_index.is_empty());
        assert!(runtime.identity_index.is_empty());
        assert_eq!(runtime.revision(), 0);

        // A later valid load must not resurrect the failed load's valid prefix:
        // a `.good` candidate lookup returns nothing attributable to it.
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 0), ".other { width: 2px }")
            .expect("valid sheet should load");
        assert!(!runtime.class_index.contains_key("good"));
        assert_eq!(runtime.class_index.get("other").map(Vec::len), Some(1));
    }

    #[test]
    fn failed_load_still_counts_declaration_drops() {
        let mut runtime = StyleRuntime::default();

        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".good { width: 1px; bogus } div > span { }",
            )
            .expect_err("the bad `div > span` selector should fail the load");
        assert_eq!(error, DomError::InvalidSelector);

        // The G drop-report contract survives the atomicity fix: the malformed
        // `bogus` declaration in the valid prefix is still counted even though
        // the load failed (drops are observations, not rule-store mutations).
        assert_eq!(runtime.declarations_dropped_count(), 1);
    }

    #[test]
    fn load_stylesheet_accepts_hover_and_active_selectors() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "#hero:hover { width: 10px; } button:active { height: 12px; }",
            )
            .expect("pseudo-state stylesheet should load");
        assert_eq!(runtime.id_index.get("hero").map(Vec::len), Some(1));
        assert_eq!(runtime.tag_index.get("button").map(Vec::len), Some(1));
    }

    #[test]
    fn load_stylesheet_accepts_focus_and_focus_visible_selectors() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "button:focus { width: 10px; } input:focus-visible { height: 12px; }",
            )
            .expect("focus selectors should load");
        assert_eq!(runtime.tag_index.get("button").map(Vec::len), Some(1));
        assert_eq!(runtime.tag_index.get("input").map(Vec::len), Some(1));
    }

    #[test]
    fn load_stylesheet_accepts_multi_class_and_descendant_selectors() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".cta.primary { width: 10px; } .panel .cta.primary:hover { height: 12px; }",
            )
            .expect("multi-class stylesheet should load");

        assert_eq!(runtime.class_index.get("cta").map(Vec::len), Some(2));
        assert_eq!(runtime.class_index.get("primary").map(Vec::len), Some(2));
        assert_eq!(runtime.class_index.get("panel").map(Vec::len), None);
    }

    #[test]
    fn load_stylesheet_drops_unadmitted_declarations_with_report() {
        let mut runtime = StyleRuntime::default();
        let report = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "div { tab-index: 0; color: #112233; }",
            )
            .expect("unadmitted declaration should drop, not reject the load");
        assert_eq!(report.declarations_applied, 1);
        assert_eq!(report.declarations_dropped, 1);
        assert_eq!(report.rules_inserted, 1);
        assert_eq!(runtime.declarations_dropped_count(), 1);
    }

    #[test]
    fn load_stylesheet_accepts_em_declarations_in_step_4() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".scaled { width: 2em; font-size: 1.5em; }",
            )
            .expect("stylesheet should load");

        let rule_key = runtime
            .class_index
            .get("scaled")
            .and_then(|keys| keys.first())
            .copied()
            .expect("scaled rule");
        let rule = runtime.rules.get(rule_key).expect("rule");
        let block = runtime.decl_block(rule.decl).expect("decl block");
        assert_eq!(
            block.get(PresentationProperty::Width),
            Some(&DeclValue::Length(DeclLength::Em(2.0)))
        );
        assert_eq!(
            block.get(PresentationProperty::FontSize),
            Some(&DeclValue::Length(DeclLength::Em(1.5)))
        );
    }

    #[test]
    fn load_stylesheet_accepts_flex_surface_declarations() {
        let mut runtime = StyleRuntime::default();
        let report = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".flex {
                    flex-direction: column;
                    flex-wrap: wrap;
                    flex-flow: column wrap;
                    flex-grow: 2;
                    flex-shrink: 3;
                    flex-basis: 20px;
                    flex: 1 0 auto;
                    order: 4;
                    justify-content: space-between;
                    align-items: stretch;
                    align-self: center;
                    align-content: space-around;
                    gap: 12px;
                    row-gap: 8px;
                    column-gap: 10px;
                    place-content: center start;
                    place-items: stretch center;
                    place-self: end auto;
                }",
            )
            .expect("stylesheet should load");

        assert_eq!(report.declarations_applied, 18);
        assert_eq!(report.declarations_dropped, 0);
        assert_eq!(report.rules_inserted, 1);

        let rule_key = runtime
            .class_index
            .get("flex")
            .and_then(|keys| keys.first())
            .copied()
            .expect("flex rule");
        let rule = runtime.rules.get(rule_key).expect("rule");
        let block = runtime.decl_block(rule.decl).expect("decl block");

        let get = |property: PresentationProperty| -> DeclValue {
            *block
                .get(property)
                .unwrap_or_else(|| panic!("expected {property:?} in block"))
        };
        assert_eq!(
            get(PresentationProperty::FlexDirection),
            DeclValue::Keyword(Keyword::Column)
        );
        assert_eq!(
            get(PresentationProperty::FlexWrap),
            DeclValue::Keyword(Keyword::Wrap)
        );
        assert_eq!(get(PresentationProperty::FlexGrow), DeclValue::Number(1.0));
        assert_eq!(
            get(PresentationProperty::FlexShrink),
            DeclValue::Number(0.0)
        );
        assert_eq!(
            get(PresentationProperty::FlexBasis),
            DeclValue::Keyword(Keyword::Auto)
        );
        assert_eq!(get(PresentationProperty::Order), DeclValue::Integer(4));
        assert_eq!(
            get(PresentationProperty::JustifyContent),
            DeclValue::Keyword(Keyword::Start)
        );
        assert_eq!(
            get(PresentationProperty::AlignItems),
            DeclValue::Keyword(Keyword::Stretch)
        );
        assert_eq!(
            get(PresentationProperty::AlignSelf),
            DeclValue::Keyword(Keyword::End)
        );
        assert_eq!(
            get(PresentationProperty::AlignContent),
            DeclValue::Keyword(Keyword::Center)
        );
        assert_eq!(
            get(PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Px(8.0))
        );
        assert_eq!(
            get(PresentationProperty::ColumnGap),
            DeclValue::Length(DeclLength::Px(10.0))
        );
    }

    #[test]
    fn load_stylesheet_computes_specificity_and_right_most_indexes() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 0),
                "#hero { width: 10px; } .cta.primary { height: 10px; } div.primary.selected { top: 1px; } * { left: 1px; }",
            )
            .expect("stylesheet should load");

        assert_eq!(runtime.id_index.get("hero").map(Vec::len), Some(1));
        assert_eq!(runtime.class_index.get("cta").map(Vec::len), Some(1));
        assert_eq!(runtime.class_index.get("primary").map(Vec::len), Some(2));
        assert_eq!(runtime.class_index.get("selected").map(Vec::len), Some(1));
        assert_eq!(runtime.tag_index.get("div").map(Vec::len), Some(1));
        assert_eq!(runtime.universal_index.len(), 1);
    }

    #[test]
    fn candidate_lookup_deduplicates_rule_keys() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "div.primary.selected { width: 10px; }",
            )
            .expect("stylesheet should load");

        let candidates = runtime.candidate_keys_for_node(NodeStyleIndex {
            id: None,
            classes: &["primary".to_string(), "selected".to_string()],
            tag: "div",
        });
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn multi_class_selector_counts_each_class_in_specificity() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "button.primary.selected:hover { width: 10px; }",
            )
            .expect("multi-class pseudo stylesheet should load");

        let rule_key = runtime
            .class_index
            .get("primary")
            .and_then(|keys| keys.first())
            .copied()
            .expect("primary rule");
        let rule = runtime.rules.get(rule_key).expect("rule");
        assert_eq!(
            rule.specificity,
            SelectorSpecificity {
                id: 0,
                class: 3,
                tag: 1,
            }
        );
    }

    #[test]
    fn precedence_tuple_orders_by_origin_specificity_and_order() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(StyleRuleOrigin::new("base", 0), ".cta { width: 10px; }")
            .expect("base");
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), "div.cta { width: 12px; }")
            .expect("theme");

        let mut tuples = runtime
            .rules
            .values()
            .map(|rule| (rule.origin.priority, rule.specificity, rule.order))
            .collect::<Vec<_>>();
        tuples.sort();

        assert_eq!(
            tuples[0].1,
            SelectorSpecificity {
                id: 0,
                class: 1,
                tag: 0
            }
        );
        assert_eq!(
            tuples[1].1,
            SelectorSpecificity {
                id: 0,
                class: 1,
                tag: 1
            }
        );
        assert!(tuples[0].0 < tuples[1].0);
    }

    #[test]
    fn unload_marks_origin_rules_inactive_without_rebuild() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".cta { width: 10px; } .cta { height: 10px; }",
            )
            .expect("theme");
        runtime
            .load_stylesheet(StyleRuleOrigin::new("base", 0), ".cta { top: 1px; }")
            .expect("base");

        let unloaded = runtime.unload_stylesheet("theme");
        assert_eq!(unloaded, 2);
        assert!(!runtime.origin_index.contains_key("theme"));
        assert!(
            runtime
                .rules
                .values()
                .filter(|rule| rule.origin.key == "theme")
                .all(|rule| !rule.active)
        );
        assert!(
            runtime
                .rules
                .values()
                .filter(|rule| rule.origin.key == "base")
                .all(|rule| rule.active)
        );
    }

    #[test]
    fn same_load_duplicate_rule_dedups_to_one_commit() {
        let mut runtime = StyleRuntime::default();
        let report = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".a { width: 1px } .a { width: 1px }",
            )
            .expect("duplicate same-load rules should dedup");
        assert_eq!(report.rules_inserted, 1);
        assert_eq!(runtime.decl_block_count(), 1);
        assert_eq!(runtime.class_index.get("a").map(Vec::len), Some(1));

        // Control: same selector, different declarations are not duplicates.
        let mut control = StyleRuntime::default();
        let report = control
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".a { width: 1px } .a { width: 2px }",
            )
            .expect("different declarations should both commit");
        assert_eq!(report.rules_inserted, 2);

        // Reload dedup still holds: a second load of the same sheet inserts 0.
        let mut reload = StyleRuntime::default();
        let first = reload
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".a { width: 1px }")
            .expect("first load");
        let second = reload
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".a { width: 1px }")
            .expect("second load");
        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 0);
    }

    #[test]
    fn same_load_dedup_uses_identity_hash_buckets() {
        // The same-load dedup tier is hash-bucketed by identity hash (mirroring
        // the active tier's `identity_index`), so a load is O(N) rather than
        // O(N²). This pins the O(N) shape at volume by correctness, not
        // wall-clock timing: an exact duplicate of the first rule at the end of
        // a 200-distinct-rule sheet is still deduped, and the control commits
        // all 200.
        let mut rules = Vec::with_capacity(201);
        for i in 0..200 {
            rules.push(format!(".r{i} {{ width: 1px }}"));
        }
        rules.push(".r0 { width: 1px }".to_string());
        let sheet = rules.join(" ");
        let report = StyleRuntime::default()
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), &sheet)
            .expect("duplicate-bearing sheet should load");
        assert_eq!(report.rules_inserted, 200);

        let mut control_rules = Vec::with_capacity(200);
        for i in 0..200 {
            control_rules.push(format!(".r{i} {{ width: 1px }}"));
        }
        let control_sheet = control_rules.join(" ");
        let report = StyleRuntime::default()
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), &control_sheet)
            .expect("control sheet should load");
        assert_eq!(report.rules_inserted, 200);
    }

    #[test]
    fn load_stylesheet_skips_equivalent_active_rules() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 0);
    }

    #[test]
    fn load_stylesheet_skips_equivalent_active_rules_with_flex_surface_declarations() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".cta { justify-content: space-between; gap: 12px; }",
            )
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".cta { justify-content: space-between; gap: 12px; }",
            )
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 0);
    }

    #[test]
    fn load_stylesheet_skips_equivalent_rules_with_selector_whitespace_variation() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "div .cta { width: 10px; }",
            )
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "   div    .cta   {   width: 10px; }",
            )
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 0);
    }

    #[test]
    fn load_stylesheet_skips_equivalent_multi_class_descendant_rules_with_whitespace_variation() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".panel .cta.primary { width: 10px; }",
            )
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "  .panel   .cta.primary   { width: 10px; }",
            )
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 0);
    }

    #[test]
    fn load_stylesheet_keeps_non_equivalent_rules_for_same_selector() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 12px; }")
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 1);
    }

    #[test]
    fn load_stylesheet_keeps_equivalent_rules_when_origin_differs() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(StyleRuleOrigin::new("base", 0), ".cta { width: 10px; }")
            .expect("first load should pass");
        let second = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("second load should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(second.rules_inserted, 1);
    }

    #[test]
    fn load_stylesheet_allows_reloading_after_origin_unload() {
        let mut runtime = StyleRuntime::default();
        let first = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("first load should pass");
        let unloaded = runtime.unload_stylesheet("theme");
        let reloaded = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("reload should pass");

        assert_eq!(first.rules_inserted, 1);
        assert_eq!(unloaded, 1);
        assert_eq!(reloaded.rules_inserted, 1);
    }

    #[test]
    fn load_stylesheet_parses_block_comments() {
        let mut runtime = StyleRuntime::default();
        let report = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "/* header */ .cta { width: 10px; } /* footer */",
            )
            .expect("commented stylesheet should parse");

        assert_eq!(report.rules_inserted, 1);
    }

    #[test]
    fn load_stylesheet_rejects_unclosed_block_comment() {
        let mut runtime = StyleRuntime::default();
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "/* header .cta { width: 10px; }",
            )
            .expect_err("unclosed comment should fail");

        assert_eq!(error, DomError::InvalidSelector);
    }

    #[test]
    fn dedup_skip_does_not_leak_a_decl_block() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("first load");
        assert_eq!(runtime.decl_block_count(), 1);

        let second = runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".cta { width: 10px; }")
            .expect("duplicate load");
        assert_eq!(second.rules_inserted, 0);
        assert_eq!(runtime.decl_block_count(), 1);
    }

    #[test]
    fn drop_counter_accumulates_across_loads() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 1), ".a { width: 10px; bad; }")
            .expect("first load");
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".b { height: notalength; }",
            )
            .expect("second load");

        assert_eq!(runtime.declarations_dropped_count(), 2);
    }

    #[test]
    fn unload_tombstones_keep_blocks_in_the_arena() {
        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".cta { width: 10px; } .cta { height: 10px; }",
            )
            .expect("theme");
        assert_eq!(runtime.decl_block_count(), 2);

        let unloaded = runtime.unload_stylesheet("theme");
        assert_eq!(unloaded, 2);
        // Tombstones keep their blocks: reclamation is open question 6.
        assert_eq!(runtime.decl_block_count(), 2);
    }
}
