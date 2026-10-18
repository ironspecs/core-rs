# visitor

Retained tree visitors that complete their work during traversal for `ui-dom`.

## Purpose and scope

This module owns the single-pass visitors that walk retained node state and do
their work on the visit: style recompute for dirty layer-root subtrees,
inherited-style seeding shared by style and layout, scroll, pointer event
targeting, and pseudo-state tracking. It owns no solver math (that is
`domains/layout`) and no rule storage (that is `domains/style`); visitors read
and write retained node state through `RootDom`.

## Ownership boundaries

- `style_recompute.rs` owns the dirty-subtree style recompute visitor.
- `winners_fold.rs` owns the per-element typed winners fold (spine 3.3g-2a): it
  folds the behavior sheets, matched rules, and inline block into a winners
  array, then RESOLVES and WRITES the seven computed groups plus the
  transition-longhand slot directly on the node record. There is no legacy-map
  rebuild and no `apply_styles`.
- `inherited_style.rs` owns the ancestor group seed shared by the style,
  layout, and paint passes; `inherited_style_context.rs` owns the resolved
  `InheritedStyleContext` record (the three inherited groups) the seed
  produces.
- `event_target.rs`, `pseudo_state.rs`, `scroll.rs` own pointer targeting,
  pseudo-state tracking, and scroll application.

## The dirty-bit doctrine

All invalidation in `ui-dom` is per-node, per-domain boolean flags
(`style.dirty`, `layout.dirty`, paint repaint state), propagated upward to the
enclosing layer root by `RootDom` at write time. Every pass in the crate is
visitor-style: single traversal, work on the visit, nothing retained by the
pass afterward. The boolean is the only invalidation model compatible with
that architecture:

- A dirty bit is O(1) to read on the visit and O(1) to clear on the visit;
  the pass owns no state about nodes.
- Invalidation state lives on the node, one bit per domain, so independent
  passes (style, layout, paint, transitions) consume and clear their own bit
  at their own rates.
- Upward propagation with early exit at an already-dirty ancestor makes
  repeated writes under a dirty ancestor cost O(distance), not O(depth), and
  lets a layer-root flag summarize its subtree for the scheduler.

Generation/version counters are disqualified for traversal invalidation. A
version answers "did this change?" only for a consumer that retained the exact
last value it saw, and visitor passes are deliberately transient. A version
model therefore forces either a retained per-node last-seen map per pass — a
second retained structure per consumer, kept in sync with node removal,
consulted on every visit — or a node×pass matrix of remembered values. Both
invert the ownership the architecture depends on: with dirty bits, state about
what a pass owes lives on the node; with versions, it must live in the
ephemeral pass.

The `generation` fields on the `DomIntrinsic` variants were removed as a
labeled changeset against the canonical-snapshot contract: they were written
on every intrinsic recompute and read by no decision path — worse, the
preferred-size and icon paths compared them, so every style recompute
manufactured phantom layout/paint dirties for form controls and icon images.
Change detection is now plain equality over the measured fields, reported by
the returned bool. The one legitimate counter is
`StyleRuntime::revision`: boundary change detection at the shared-context
seam, with exactly one reader (`RootDom`, once per update), where no
back-pointer from the shared runtime to its roots can exist.

## The style-recompute algorithm

`style_recompute.rs` runs, per dirty layer-root subtree:

1. Seed an `InheritedStyleContext` by climbing to the nearest ancestor element
   outside the layer root and reading its three inherited groups.
2. Walk the subtree in DFS, carrying only traversal-local inherited state.
3. Match stylesheet rules for each element in the same visit.
4. Fold the element's TYPED winners — the unconditional base behavior sheet,
   the active pseudo-state behavior sheet, matched rules (ascending
   precedence), then the inline block — and resolve + write the seven computed
   groups and the transition-longhand slot directly on the node record
   (`winners_fold.rs`). This is the ONLY channel from the cascade to the
   computed groups: the legacy attr-projection channel is dead.
5. Diff the freshly resolved groups against the last completed pass's groups
   (`NodePresentation::previous_computed`, captured before the fold). The diff
   drives two things (spine 3.3g-1b, D2 one-timing): pass-driven transition
   scheduling for the moved fields (the change is detected at the style update,
   not the write) and A7's group-diff dirty classification. The first pass
   establishes the baseline without diffing, so initial style application never
   schedules a transition — but the baseline still re-evaluates layer
   membership (a freshly styled `position: fixed` node needs its layer
   materialized now that the write path no longer queues it).
6. Derive the child traversal state from the freshly resolved groups.
7. Remeasure text-node intrinsic metrics from the carried effective text
   style in the same visit.
8. Clear style dirtiness for each visited node.

The base (unconditional) behavior sheet is applied at element creation (like
the legacy `default_attrs` `display` it replaces) and is also folded as the
lowest cascade tier, so it is the winner only when no author rule or inline
declaration overrode it.

Hard rule: the live recompute path must not call `RootDom::compute_style_map`;
the one-off query API and the retained recompute path stay independent for
architecture and performance.

## The inheritance model

Since spine step 3.3d, inheritance is group-copy (A5, CSS Cascade §7): the
three inherited computed groups — font (`font-size`, `line-height`,
`font-family`, `font-weight`, `font-style`), text (`text-align`, `direction`,
`white-space`, `overflow-wrap`, `word-break`), and color (`color`) — seed from
the parent element's groups, and the four non-inherited groups (box, flex,
visual, transform) start from `initial()`. `InheritedStyleContext` is a thin,
`Copy` wrapper over those three inherited groups, carried down the DFS.

Two derivations feed it, both reading the overlay-merged computed groups (so a
mid-flight transition on the parent is the inherited value — CSS Cascade §6.1):

- `inherited_style_context_from_ancestors` seeds a layer-root subtree by
  climbing to the nearest ancestor element and reading its three groups; with
  no element ancestor (document root) the seed is `ComputedGroups::initial()`.
- `inherited_context_from_node` derives a node's own carried context from its
  freshly resolved groups; text nodes are never folded, so they inherit the
  carried context unchanged.

Inheritance is group-copy only: there is no attr materialization anymore — the
legacy `seed_materialized_map` died with the attr-projection channel. The
transmitting properties (color, font-size, line-height, white-space,
overflow-wrap, word-break, and the rest) ride the inherited groups into text
intrinsics and descendants, never through stored attrs.

Adding a transmitting property is a cross-file change: the group record, the
seed derivation, the projection resolver, the recompute visitor, and
`tests/layout_proofs/` all move together.

## Invariants and rules

- Work happens on the visit; visitors never accumulate batch state to process
  after traversal (structural rule 4).
- The inherited property set is closed; do not grow it without the cross-file
  change above and spec provenance in the proof tree.
- Visitors write through `RootDom` public or `pub(crate)` surfaces; no visitor
  reaches into node internals directly.
- Recompute decisions are recomputed from current retained state on every
  pass; no visitor caches per-node results for a later pass.
- `StylePassNodeVisited` / `StylePassNodeChanged` price the style pass:
  visited counts every element resolved (the winners fold + group resolution
  cost); changed counts resolved elements whose computed groups changed
  (consequence volume). Their ratio is the C1/C3 coarseness signal that gates
  proposal J's matched-rule cache decision.
