//! Typed winners fold for the retained style-recompute pass (spine 3.3g-2a).
//!
//! This file owns the per-element cascade fold that replaced the legacy
//! stringly map fold. It folds TYPED winners — the unconditional base behavior
//! sheet, the active pseudo-state behavior sheet, matched stylesheet rules
//! (ascending precedence), then the inline block — into a stack-local
//! `[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT]` winners
//! array, then RESOLVES and WRITES the seven computed groups plus the
//! `ResolvedTransitionLonghands` slot directly on the node record (seed from
//! the parent's merged groups). There is no legacy-map rebuild and no
//! `apply_styles` call: the winners are the only channel from the cascade to
//! the computed groups.
//!
//! Invariants and rules:
//!
//! - the fold order is exactly: base behavior declarations → active
//!   pseudo-state behavior declarations → matched rules (ascending
//!   `Precedence`) → inline block (max specificity)
//! - behavior entries use the `CascadeTier::NormalUa` tier with `origin_priority:
//!   i32::MIN` and a monotonic `order` (base before state); behavior sheets
//!   never carry `Text` values (their interner-relative `TextKey` has no owning
//!   store), so those are skipped exactly like the legacy
//!   `apply_pseudo_state_style_declarations`
//! - rule and inline declarations take their tier from the declaration's
//!   importance bit (`ImportantAuthor` vs `NormalAuthor`); behavior
//!   declarations are always normal today (the `ImportantUa` tier is stage 2)
//! - a declaration takes a winners slot when its `Precedence` is `>=` the
//!   current occupant's (later-or-higher wins), matching the legacy
//!   later-insert-wins `BTreeMap` fold
//! - text-valued winners are re-interned into a per-element scratch interner
//!   during the fold so the resolution and `compute_style_map` share one
//!   interner
//! - one runtime lock per element, held only during the collect phase

use std::sync::Arc;

use crate::domains::style::computed::project::{
    ProjectionContext, parent_inherited_groups, resolve_declaration_into_groups,
    transition_longhands_from_strings,
};
use crate::domains::style::computed::{ComputedGroups, ResolvedTransitionLonghands};
use crate::domains::style::{
    CascadeTier, DeclBlockKey, DeclLength, DeclValue, Declaration, NodeLookup, NodeStyleIndex,
    Precedence, PresentationGroup, PresentationProperty, SelectorSpecificity, TextInterner,
    TextKey, style_index_for_node,
};
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomPseudoState, DomTextDirection, RootDom,
};

#[cfg(test)]
use crate::domains::style::{BoxLength, Keyword, StyleRuleOrigin, TransitionLonghandProperty};
#[cfg(test)]
use crate::types::{DomTag, RootDomContext, RootDomContextConfig};

/// The collected per-element winners plus the state the resolution needs: the
/// parent's overlay-merged seed and the scratch interner the winning `Text`
/// values were re-interned into.
pub(crate) struct CollectedWinners {
    pub(crate) seed: ComputedGroups,
    pub(crate) winners: [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    pub(crate) scratch: TextInterner,
}

/// The per-element typed winners fold: collect + fold the applicable
/// declarations (the active pseudo-state behavior sheet → matched rules in
/// ascending precedence → the inline block), resolve the seven groups and the
/// transition-longhand slot, and write them directly on the node record.
///
/// The base (unconditional) behavior sheet is folded FIRST (lowest cascade
/// tier) and is also applied at element creation (see `dom_element_node.rs`)
/// exactly like the legacy `default_attrs` `display` it replaces.
pub(crate) fn fold_element_winners(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let CollectedWinners {
        seed,
        winners,
        scratch,
    } = collect_winners(root_dom, node_key)?;
    let (groups, longhands) = resolve_winners(root_dom, node_key, &seed, &winners, &scratch);
    let node = root_dom
        .nodes
        .get_mut(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    node.set_computed_groups(groups, longhands);
    Ok(())
}

/// Collects the element's applicable declarations and folds them into a
/// stack-local winners array, re-interning text into a scratch interner.
/// Shared by the pass (`fold_element_winners`) and the debug-priced
/// `compute_style_map` successor. One runtime lock, held only during the
/// matched-rules fold (`fold_matched_rules`).
pub(crate) fn collect_winners(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<CollectedWinners, DomError> {
    let mut winners: [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT] =
        [None; PresentationProperty::COUNT];
    let mut scratch = TextInterner::default();

    let node = root_dom
        .nodes
        .get(node_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let element = node.as_element().ok_or(DomError::InvalidAttributeTarget)?;
    let seed = parent_inherited_groups(&root_dom.nodes, node);

    // The behavior sheet: the unconditional base declarations first, then the
    // active pseudo-state declarations in Hover/Active/Focus/FocusVisible
    // order, all at the UA-ish tier.
    let mut behavior_order = 0u64;
    for declaration in element.style().base_declarations() {
        fold_behavior_slot(
            &mut winners,
            *declaration,
            behavior_precedence(behavior_order),
        );
        behavior_order = behavior_order.saturating_add(1);
    }
    for pseudo_state in [
        DomPseudoState::Hover,
        DomPseudoState::Active,
        DomPseudoState::Focus,
        DomPseudoState::FocusVisible,
    ] {
        if !element.has_pseudo_state(pseudo_state) {
            continue;
        }
        for declaration in element.style().pseudo_state_declarations(pseudo_state) {
            fold_behavior_slot(
                &mut winners,
                *declaration,
                behavior_precedence(behavior_order),
            );
            behavior_order = behavior_order.saturating_add(1);
        }
    }

    let node_index = style_index_for_node(node).ok_or(DomError::InvalidAttributeTarget)?;
    fold_matched_rules(&mut winners, &mut scratch, root_dom, node_key, node_index)?;

    // Inline block at top specificity (css-style-attr §4); its tier is the
    // declaration's importance bit (`ImportantAuthor` vs `NormalAuthor`).
    let inline_precedence = Precedence {
        tier: CascadeTier::NormalAuthor,
        origin_priority: i32::MAX,
        inline: true,
        specificity: SelectorSpecificity {
            id: u16::MAX,
            class: u16::MAX,
            tag: u16::MAX,
        },
        order: 0,
    };
    if let Some(block) = root_dom.inline_decl_block(node_key) {
        for (declaration, important) in block.declarations().iter().zip(block.importance()) {
            let precedence = Precedence {
                tier: if *important {
                    CascadeTier::ImportantAuthor
                } else {
                    CascadeTier::NormalAuthor
                },
                ..inline_precedence
            };
            let value = reintern_text(declaration.value, &mut scratch, |key| {
                root_dom.resolve_inline_text(key)
            });
            fold_slot(&mut winners, value, declaration.property, precedence);
        }
    }

    Ok(CollectedWinners {
        seed,
        winners,
        scratch,
    })
}

/// Folds the matched stylesheet rules (ascending precedence) under the ONE
/// runtime lock, re-interns text into the scratch interner, and applies each
/// declaration's importance bit to its tier.
///
/// Per-element allocation note: `candidate_keys_for_node` allocates a
/// `Vec<StyleRuleKey>` + dedup `HashSet` and this `matched` Vec (+ sort) is a
/// second per-element allocation. This is the preserved right-most-index
/// lookup shape, consumed in the SAME visit — not batch accumulation with
/// extra accesses (rule 4). Matched-rule caching is PLAN.md proposal J, an
/// evidence-gated follow-up; `StylePassNodeVisited` / `StylePassNodeChanged`
/// are its gate counters.
fn fold_matched_rules(
    winners: &mut [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    scratch: &mut TextInterner,
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    node_index: NodeStyleIndex<'_>,
) -> Result<(), DomError> {
    let context = Arc::clone(root_dom.context());
    let runtime = context
        .style_runtime
        .lock()
        .expect("style runtime mutex should not poison");
    let lookup = NodeLookup {
        nodes: &root_dom.nodes,
    };
    let mut matched: Vec<(i32, SelectorSpecificity, u64, DeclBlockKey)> = Vec::new();
    for key in runtime.candidate_keys_for_node(node_index) {
        let Some((precedence_tuple, decl)) = runtime.active_rule_for_node(key, lookup, node_key)?
        else {
            continue;
        };
        matched.push((
            precedence_tuple.0,
            precedence_tuple.1,
            precedence_tuple.2,
            decl,
        ));
    }
    // Rules sort by (origin priority, specificity, order); the importance tier
    // is a per-DECLARATION flag, not a per-rule flag, so it is applied inside
    // the fold rather than the rule sort.
    matched.sort_by_key(|&(origin, specificity, order, _)| (origin, specificity, order));
    for (origin_priority, specificity, order, decl) in matched {
        let Some(block) = runtime.decl_block(decl) else {
            continue;
        };
        for (declaration, important) in block.declarations().iter().zip(block.importance()) {
            let tier = if *important {
                CascadeTier::ImportantAuthor
            } else {
                CascadeTier::NormalAuthor
            };
            let precedence = Precedence {
                tier,
                origin_priority,
                inline: false,
                specificity,
                order,
            };
            let value = reintern_text(declaration.value, scratch, |key| runtime.resolve_text(key));
            fold_slot(winners, value, declaration.property, precedence);
        }
    }
    Ok(())
}

/// Resolves the seven computed groups and the transition-longhand slot from
/// the winners + parent seed. Mirrors the shared `resolve_declaration_into_groups`
/// resolver the (now-deleted) attr projection used, but reads typed winners
/// directly — no stored attrs, no shorthand fallbacks (shorthands expanded at
/// parse time).
fn resolve_winners(
    root_dom: &mut RootDom,
    node_key: DomNodeSlotKey,
    seed: &ComputedGroups,
    winners: &[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    scratch: &TextInterner,
) -> (ComputedGroups, ResolvedTransitionLonghands) {
    let parent_font_size_px = seed.font.size_px;
    let parent_weight = seed.font.weight;
    let own_font_size_px = resolve_own_font_size_px(winners, parent_font_size_px);

    let mut groups = seed.seed_for_child();
    groups.font.size_px = own_font_size_px;

    for property in PresentationProperty::ALL {
        if property == PresentationProperty::FontSize
            || property.group() == PresentationGroup::TransitionLonghands
        {
            continue;
        }
        let Some((value, _precedence)) = winners[property.index()] else {
            continue;
        };
        let mut context = ProjectionContext {
            own_font_size_px,
            parent_weight,
            text_source: scratch,
            computed_texts: &mut root_dom.computed_texts,
        };
        resolve_declaration_into_groups(&mut groups, property, value, &mut context);
    }

    // The `dir` content attribute feeds `direction` when the `Direction`
    // longhand is absent (the legacy context's Direction-preferred-over-Dir
    // rule). Only the node's own Dir-vs-Direction merge needs this;
    // descendants already see the merged value via the group-copy seed.
    apply_dir_content_attribute(root_dom, node_key, winners, &mut groups);

    let longhands = transition_longhands_from_winners(winners, scratch);

    (groups, longhands)
}

/// The UA-ish behavior-sheet precedence: the `NormalUa` tier (a named tier,
/// not the old `i32::MIN`-origin hack), below every author rule and inline
/// declaration, with a monotonic `order` so the active pseudo-state
/// declarations fold in Hover/Active/Focus/FocusVisible order.
fn behavior_precedence(order: u64) -> Precedence {
    Precedence {
        tier: CascadeTier::NormalUa,
        origin_priority: i32::MIN,
        inline: false,
        specificity: SelectorSpecificity::default(),
        order,
    }
}

/// Folds one behavior declaration. Text-valued properties are unsupported in
/// behavior sheets (interning-relative `TextKey` with no owning store), so they
/// are skipped — the legacy `apply_pseudo_state_style_declarations` contract.
fn fold_behavior_slot(
    winners: &mut [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    declaration: Declaration,
    precedence: Precedence,
) {
    if matches!(declaration.value, DeclValue::Text(_)) {
        return;
    }
    fold_slot(winners, declaration.value, declaration.property, precedence);
}

/// Re-interns a store-relative `Text` value into the per-element `scratch`
/// interner so the resolve phase and `compute_style_map` share one interner.
fn reintern_text<'a>(
    value: DeclValue,
    scratch: &mut TextInterner,
    resolve: impl Fn(TextKey) -> Option<&'a str>,
) -> DeclValue {
    match value {
        DeclValue::Text(key) => {
            let text = resolve(key).expect("interned text must resolve in its owning store");
            DeclValue::Text(scratch.intern(text))
        }
        other => other,
    }
}

/// The slot update rule: a declaration takes the slot when its precedence is
/// `>=` the current occupant's (later-or-higher wins — the legacy `BTreeMap`
/// later-insert-wins fold).
fn fold_slot(
    winners: &mut [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    value: DeclValue,
    property: PresentationProperty,
    precedence: Precedence,
) {
    let index = property.index();
    let replace = match winners[index] {
        None => true,
        Some((_, existing)) => precedence >= existing,
    };
    if replace {
        winners[index] = Some((value, precedence));
    }
}

/// Resolves the node's own font size from the `FontSize` winner against the
/// parent's font size (`em` and percentages are unit-resolved here; A6).
/// Falls back to the inherited size when no author source set it.
fn resolve_own_font_size_px(
    winners: &[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    parent_font_size_px: f32,
) -> f32 {
    match winners[PresentationProperty::FontSize.index()] {
        Some((DeclValue::Length(DeclLength::Px(px)), _)) => px,
        Some((DeclValue::Length(DeclLength::Em(em)), _)) => parent_font_size_px * em,
        Some((DeclValue::Length(DeclLength::Percent(pct)), _)) => parent_font_size_px * pct / 100.0,
        _ => parent_font_size_px,
    }
}

/// Applies the `dir` content attribute ("ltr"/"rtl") to the text group's
/// `direction` when the `Direction` longhand is absent (the legacy context's
/// Direction-preferred-over-Dir rule). The parent seed already carried any
/// inherited direction; only the node's own Dir-vs-Direction merge needs this.
fn apply_dir_content_attribute(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
    winners: &[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    groups: &mut ComputedGroups,
) {
    if winners[PresentationProperty::Direction.index()].is_some() {
        return;
    }
    let Some(node) = root_dom.nodes.get(node_key) else {
        return;
    };
    let Some(DomAttributeValue::String(direction)) = node.attr(&DomAttribute::Dir) else {
        return;
    };
    let direction = match direction.as_str() {
        "ltr" => DomTextDirection::Ltr,
        "rtl" => DomTextDirection::Rtl,
        _ => return,
    };
    groups.text.direction = direction;
}

/// Builds the transition-longhand slot from the four longhand winners (their
/// `Text` values resolve through the scratch interner to strings).
fn transition_longhands_from_winners(
    winners: &[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    scratch: &TextInterner,
) -> ResolvedTransitionLonghands {
    transition_longhands_from_strings(
        winner_text(winners, scratch, PresentationProperty::TransitionProperty),
        winner_text(winners, scratch, PresentationProperty::TransitionDuration),
        winner_text(
            winners,
            scratch,
            PresentationProperty::TransitionTimingFunction,
        ),
        winner_text(winners, scratch, PresentationProperty::TransitionDelay),
    )
}

fn winner_text<'a>(
    winners: &[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT],
    scratch: &'a TextInterner,
    property: PresentationProperty,
) -> Option<&'a str> {
    let (value, _) = winners[property.index()]?;
    let DeclValue::Text(key) = value else {
        return None;
    };
    scratch.resolve(key)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact literal comparisons in the font-size resolution tests
mod tests {
    use super::*;

    fn winners_array() -> [Option<(DeclValue, Precedence)>; PresentationProperty::COUNT] {
        [None; PresentationProperty::COUNT]
    }

    fn rule_precedence(origin: i32, specificity: SelectorSpecificity, order: u64) -> Precedence {
        Precedence {
            tier: CascadeTier::NormalAuthor,
            origin_priority: origin,
            inline: false,
            specificity,
            order,
        }
    }

    #[test]
    fn fold_slot_prefers_later_or_higher_precedence() {
        let mut winners = winners_array();
        let property = PresentationProperty::Width;

        fold_slot(
            &mut winners,
            DeclValue::Length(DeclLength::Px(10.0)),
            property,
            rule_precedence(0, SelectorSpecificity::default(), 0),
        );
        // An equal-precedence declaration (later order) wins the slot.
        fold_slot(
            &mut winners,
            DeclValue::Length(DeclLength::Px(20.0)),
            property,
            rule_precedence(0, SelectorSpecificity::default(), 1),
        );
        assert_eq!(
            winners[property.index()],
            Some((
                DeclValue::Length(DeclLength::Px(20.0)),
                rule_precedence(0, SelectorSpecificity::default(), 1)
            ))
        );

        // A lower-precedence declaration does NOT displace the occupant.
        fold_slot(
            &mut winners,
            DeclValue::Length(DeclLength::Px(5.0)),
            property,
            rule_precedence(0, SelectorSpecificity::default(), 0),
        );
        assert_eq!(
            winners[property.index()],
            Some((
                DeclValue::Length(DeclLength::Px(20.0)),
                rule_precedence(0, SelectorSpecificity::default(), 1)
            ))
        );

        // A higher-origin declaration displaces regardless of order.
        fold_slot(
            &mut winners,
            DeclValue::Length(DeclLength::Px(30.0)),
            property,
            rule_precedence(1, SelectorSpecificity::default(), 0),
        );
        assert_eq!(
            winners[property.index()],
            Some((
                DeclValue::Length(DeclLength::Px(30.0)),
                rule_precedence(1, SelectorSpecificity::default(), 0)
            ))
        );
    }

    #[test]
    fn inline_precedence_beats_author_rules_and_behavior() {
        let mut winners = winners_array();
        let property = PresentationProperty::Display;

        fold_behavior_slot(
            &mut winners,
            Declaration {
                property,
                value: DeclValue::Keyword(Keyword::Block),
            },
            behavior_precedence(0),
        );
        fold_slot(
            &mut winners,
            DeclValue::Keyword(Keyword::Flex),
            property,
            rule_precedence(
                5,
                SelectorSpecificity {
                    id: 1,
                    class: 1,
                    tag: 1,
                },
                9,
            ),
        );
        let inline_precedence = Precedence {
            tier: CascadeTier::NormalAuthor,
            origin_priority: i32::MAX,
            inline: true,
            specificity: SelectorSpecificity {
                id: u16::MAX,
                class: u16::MAX,
                tag: u16::MAX,
            },
            order: 0,
        };
        fold_slot(
            &mut winners,
            DeclValue::Keyword(Keyword::None),
            property,
            inline_precedence,
        );
        assert_eq!(
            winners[property.index()],
            Some((DeclValue::Keyword(Keyword::None), inline_precedence))
        );
    }

    #[test]
    fn behavior_sheet_skips_text_valued_declarations() {
        let mut winners = winners_array();
        let property = PresentationProperty::Transform;
        fold_behavior_slot(
            &mut winners,
            Declaration {
                property,
                value: DeclValue::Text(TextKey(7)),
            },
            behavior_precedence(0),
        );
        assert_eq!(winners[property.index()], None);
    }

    #[test]
    fn reintern_text_moves_store_text_into_scratch() {
        let mut scratch = TextInterner::default();
        let store_text = "translate(10px, 20px)";
        let value = reintern_text(DeclValue::Text(TextKey(0)), &mut scratch, |_key| {
            Some(store_text)
        });
        let DeclValue::Text(key) = value else {
            panic!("text value should re-intern to a scratch TextKey");
        };
        assert_eq!(scratch.resolve(key), Some(store_text));

        // Non-text values pass through untouched.
        let length = DeclValue::Length(DeclLength::Px(4.0));
        assert_eq!(
            reintern_text(length, &mut scratch, |_key| None),
            DeclValue::Length(DeclLength::Px(4.0))
        );
    }

    #[test]
    fn resolve_own_font_size_px_resolves_em_against_parent_and_falls_back() {
        let mut winners = winners_array();
        winners[PresentationProperty::FontSize.index()] = Some((
            DeclValue::Length(DeclLength::Em(1.5)),
            rule_precedence(0, SelectorSpecificity::default(), 0),
        ));
        assert_eq!(resolve_own_font_size_px(&winners, 10.0), 15.0);

        let mut winners = winners_array();
        winners[PresentationProperty::FontSize.index()] = Some((
            DeclValue::Length(DeclLength::Px(20.0)),
            rule_precedence(0, SelectorSpecificity::default(), 0),
        ));
        assert_eq!(resolve_own_font_size_px(&winners, 10.0), 20.0);

        let winners = winners_array();
        assert_eq!(resolve_own_font_size_px(&winners, 10.0), 10.0);
    }

    #[test]
    fn fold_element_winners_populates_transition_slot_from_the_winners() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let div = root.create_element(DomTag::Div).expect("div");
        root.append_child(root.root, div).expect("append");
        root.set_attr_str(div, "class", "drawer").expect("class");
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".drawer { transition-property: left; transition-duration: 180ms; }",
            )
            .expect("stylesheet");

        fold_element_winners(&mut root, div).expect("fold");

        let view = root.computed_style(div).expect("div");
        let longhands = view.transition_longhands();
        assert_eq!(longhands.durations_s.as_slice(), &[0.18]);
        assert_eq!(
            longhands.properties.as_slice(),
            &[TransitionLonghandProperty::Property(
                PresentationProperty::Left
            )]
        );
    }

    #[test]
    fn fold_element_winners_resolves_groups_from_winners() {
        let context = Arc::new(RootDomContext::new(RootDomContextConfig::cpu()));
        let mut root = RootDom::new(context.clone());
        let div = root.create_element(DomTag::Div).expect("div");
        root.append_child(root.root, div).expect("append");
        root.style_mut(div).set("width", "120px").expect("width");

        fold_element_winners(&mut root, div).expect("fold");

        let view = root.computed_style(div).expect("div");
        assert_eq!(view.as_box().width, BoxLength::Px(120.0));
    }
}
