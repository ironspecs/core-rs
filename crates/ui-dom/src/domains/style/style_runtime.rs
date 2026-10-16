//! Retained style rule store for `ui-dom`.
//!
//! This file owns the `StyleRuntime` value that compiles authored stylesheet
//! text into retained, indexed rules and answers candidate/matched-rule lookups
//! for the shared style pipeline. Selector parsing, specificity, precedence,
//! and the node-match walk stay in `style::mod`; the runtime keeps the rule
//! store and its id/class/tag/universal/origin/identity indexes.
//!
//! Invariants and rules:
//!
//! - this file owns only the `StyleRuntime` struct and its `impl` block
//! - rule compilation delegates to the parent module's selector and block
//!   parsers; the runtime never parses selector syntax on its own
//! - the indexes are retained per origin and per right-most selector so
//!   candidate lookup and origin unload stay O(indexed) rather than O(rules)
//! - pseudo-state counts and the revision counter are kept exact and
//!   zero-allocation; both are updated under the same retained-store lock

use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use super::StyleRuleOrigin;
use super::{
    CompiledRule, CssBlock, NodeLookup, NodeStyleIndex, PseudoStateCounts, StyleRule, StyleRuleKey,
    append_rule_keys, compute_rule_identity_hash, extract_right_most_selector, parse_css_blocks,
    parse_selector, pseudo_state_counts, selector_matches_node, specificity_for_selector_parts,
};
use crate::types::{DomError, DomNodeSlotKey, DomPseudoState, parse_style_attr_map};

#[cfg(test)]
use super::{SelectorSpecificity, precedence_tuple};
#[cfg(test)]
use crate::types::{DomAttribute, DomAttributeValue};

#[derive(Debug, Default)]
pub struct StyleRuntime {
    rules: SlotMap<StyleRuleKey, CompiledRule>,
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

impl StyleRuntime {
    pub(crate) fn load_stylesheet(
        &mut self,
        origin: StyleRuleOrigin,
        css_text: &str,
    ) -> Result<usize, DomError> {
        let StyleRuleOrigin {
            key: origin_key,
            priority: origin_priority,
        } = origin;
        let blocks = parse_css_blocks(css_text)?;
        let mut inserted = 0usize;
        for block in blocks {
            let compiled_rule = self.compile_rule(
                StyleRuleOrigin {
                    key: origin_key.clone(),
                    priority: origin_priority,
                },
                block,
            )?;
            if self.has_equivalent_active_rule(&compiled_rule) {
                continue;
            }
            let key = self.rules.insert(compiled_rule);
            self.index_rule(key)?;
            inserted += 1;
        }
        if inserted != 0 {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(inserted)
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
    ) -> Result<Option<&StyleRule>, DomError> {
        let Some(rule) = self.rules.get(key) else {
            return Ok(None);
        };
        if !rule.active {
            return Ok(None);
        }
        if !selector_matches_node(lookup, node_key, &rule.selector_parts)? {
            return Ok(None);
        }
        Ok(Some(&rule.rule))
    }

    fn compile_rule(
        &mut self,
        origin: StyleRuleOrigin,
        block: CssBlock,
    ) -> Result<CompiledRule, DomError> {
        let selector_parts = parse_selector(&block.selector)?;
        let right_most = extract_right_most_selector(&selector_parts)?;
        let specificity = specificity_for_selector_parts(&selector_parts);
        let attr_map = parse_style_attr_map(&block.declarations)?;
        let order = self.order_counter;
        self.order_counter = self
            .order_counter
            .checked_add(1)
            .expect("style rule order counter should not overflow");
        let rule = StyleRule {
            selector: block.selector,
            attr_map,
            specificity,
            origin,
            order,
        };
        let identity_hash = compute_rule_identity_hash(&rule, &selector_parts);
        Ok(CompiledRule {
            rule,
            selector_parts,
            right_most,
            identity_hash,
            active: true,
        })
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
            .entry(rule.rule.origin.key.clone())
            .or_default()
            .push(key);
        self.identity_index
            .entry(rule.identity_hash)
            .or_default()
            .push(key);
        self.register_pseudo_state_rule(pseudo_state_counts(&rule.selector_parts));
        Ok(())
    }

    fn has_equivalent_active_rule(&self, candidate: &CompiledRule) -> bool {
        let Some(keys) = self.identity_index.get(&candidate.identity_hash) else {
            return false;
        };
        keys.iter().any(|key| {
            self.rules.get(*key).is_some_and(|existing| {
                existing.active
                    && existing.selector_parts == candidate.selector_parts
                    && existing.rule.attr_map == candidate.rule.attr_map
                    && existing.rule.specificity == candidate.rule.specificity
                    && existing.rule.origin == candidate.rule.origin
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
    fn load_stylesheet_rejects_invalid_declaration() {
        let mut runtime = StyleRuntime::default();
        let error = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "div { tab-index: 0; color: #112233; }",
            )
            .expect_err("invalid declaration should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);
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
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("2em".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("1.5em".to_string()))
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn load_stylesheet_accepts_flex_surface_declarations() {
        let mut runtime = StyleRuntime::default();
        runtime
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

        let rule_key = runtime
            .class_index
            .get("flex")
            .and_then(|keys| keys.first())
            .copied()
            .expect("flex rule");
        let rule = runtime.rules.get(rule_key).expect("rule");
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexDirection),
            Some(&DomAttributeValue::String("column".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexWrap),
            Some(&DomAttributeValue::String("wrap".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexFlow),
            Some(&DomAttributeValue::String("column wrap".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexGrow),
            Some(&DomAttributeValue::String("2".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexShrink),
            Some(&DomAttributeValue::String("3".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::FlexBasis),
            Some(&DomAttributeValue::String("20px".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::Flex),
            Some(&DomAttributeValue::String("1 0 auto".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::Order),
            Some(&DomAttributeValue::String("4".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::JustifyContent),
            Some(&DomAttributeValue::String("space-between".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::AlignItems),
            Some(&DomAttributeValue::String("stretch".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::AlignSelf),
            Some(&DomAttributeValue::String("center".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::AlignContent),
            Some(&DomAttributeValue::String("space-around".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::Gap),
            Some(&DomAttributeValue::String("12px".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::RowGap),
            Some(&DomAttributeValue::String("8px".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::ColumnGap),
            Some(&DomAttributeValue::String("10px".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::PlaceContent),
            Some(&DomAttributeValue::String("center start".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::PlaceItems),
            Some(&DomAttributeValue::String("stretch center".to_string()))
        );
        assert_eq!(
            rule.rule.attr_map.get(&DomAttribute::PlaceSelf),
            Some(&DomAttributeValue::String("end auto".to_string()))
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
            rule.rule.specificity,
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
            .map(|rule| precedence_tuple(&rule.rule))
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
                .filter(|rule| rule.rule.origin.key == "theme")
                .all(|rule| !rule.active)
        );
        assert!(
            runtime
                .rules
                .values()
                .filter(|rule| rule.rule.origin.key == "base")
                .all(|rule| rule.active)
        );
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

        assert_eq!(first, 1);
        assert_eq!(second, 0);
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

        assert_eq!(first, 1);
        assert_eq!(second, 0);
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

        assert_eq!(first, 1);
        assert_eq!(second, 0);
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

        assert_eq!(first, 1);
        assert_eq!(second, 0);
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

        assert_eq!(first, 1);
        assert_eq!(second, 1);
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

        assert_eq!(first, 1);
        assert_eq!(second, 1);
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

        assert_eq!(first, 1);
        assert_eq!(unloaded, 1);
        assert_eq!(reloaded, 1);
    }

    #[test]
    fn load_stylesheet_parses_block_comments() {
        let mut runtime = StyleRuntime::default();
        let inserted = runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                "/* header */ .cta { width: 10px; } /* footer */",
            )
            .expect("commented stylesheet should parse");

        assert_eq!(inserted, 1);
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
}
