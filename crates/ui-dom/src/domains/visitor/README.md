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
- `inherited_style.rs` owns the ancestor seed climb shared by the style and
  layout passes; `inherited_style_context.rs` owns the resolved
  `InheritedStyleContext` record the climb produces.
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

The `generation` fields on the `DomIntrinsic` variants are the known
wrong-model artifact: written on every intrinsic recompute, read by no
decision path, and scheduled for removal as a labeled changeset against the
canonical-snapshot contract. The one legitimate counter is
`StyleRuntime::revision`: boundary change detection at the shared-context
seam, with exactly one reader (`RootDom`, once per update), where no
back-pointer from the shared runtime to its roots can exist.

## The style-recompute algorithm

`style_recompute.rs` runs, per dirty layer-root subtree:

1. Seed an `InheritedStyleContext` by climbing ancestors outside the layer
   root, capturing only the transmitting inherited properties.
2. Walk the subtree in DFS, carrying only traversal-local inherited state.
3. Match stylesheet rules for each element in the same visit.
4. Build the element's effective stylesheet-owned map from the carried
   inherited state plus the matched declarations.
5. Apply that map onto retained node attrs via `RootDom::apply_styles`, so
   style writes ride the same dirty-flag invalidation as authored writes.
6. Derive the child traversal state from the element's effective retained
   result, including transition-presented values.
7. Remeasure text-node intrinsic metrics from the carried effective text
   style in the same visit.
8. Clear style dirtiness for each visited node.

Hard rule: the live recompute path must not call `RootDom::compute_style_map`;
the one-off query API and the retained recompute path stay independent for
architecture and performance.

## The inheritance model

`InheritedStyleContext` is a closed eight-property record: `color`,
`font-size`, `line-height`, `text-align`, `direction`, `white-space`,
`overflow-wrap`, `word-break`. The ancestor climb is first-set-wins per
property with early exit once every property is known; `em` chains accumulate
into a pending scale that resolves against the nearest absolute ancestor font
size, and unitless `line-height` resolves after the font-size seed. Inherited
values are materialized into descendant node attrs during recompute, keeping
retained node attrs the single source of truth for layout and paint.

Adding a transmitting property is a cross-file change: the context record, the
climber, the recompute visitor, and `tests/layout_proofs/` all move together.

## Invariants and rules

- Work happens on the visit; visitors never accumulate batch state to process
  after traversal (structural rule 4).
- The inherited property set is closed; do not grow it without the cross-file
  change above and spec provenance in the proof tree.
- Visitors write through `RootDom` public or `pub(crate)` surfaces; no visitor
  reaches into node internals directly.
- Recompute decisions are recomputed from current retained state on every
  pass; no visitor caches per-node results for a later pass.
