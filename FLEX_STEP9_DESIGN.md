# FLEX_STEP9_DESIGN.md — the flow.rs trio (complete design)

Status: `COMPLETE DESIGN — rows 1–2 mechanical; row 3 has a
verification-first part; transcription is mechanical`. Date: 2026-10-20.
Author: the architect. Authority: CSS 2.1 §8.3.1 (the three-branch
collapse rule — landed for siblings in PR 1 item 2), CSS Flexbox 1 §4
(flex items establish independent formatting contexts); the three
registered rows this step flips
(`off_spec_parent_boundary_negative_margins_clamp`,
`off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative`,
`off_spec_flex_item_subtree_first_child_margin_containment`).

## D0. The step in one paragraph

The three-branch collapse rule (both-positive → `max`, both-negative →
`min`, mixed-sign → `a + b`, with overlap derived as `a + b − used` —
PR 1 item 2's landed shape) replaces the remaining non-negative-scoped
margin clamps in `flow.rs`: the parent-boundary functions and the
empty-block self-collapse (rows 1–2, mechanical). Row 3 is the
containment half: at a FLEX ITEM's formatting-context boundary, a
first-in-flow margin chain must be CONTAINED inside the item (the
first in-flow descendant positions at `content_top + collapsed_margin`)
instead of escaping or degrading to phantom height.

## D1. Row 1 — the parent-boundary clamps (mechanical)

Sites: `collapsed_parent_top_margin_px` (flow.rs:1051-1060, currently
`child_style.sides.margin.top_px.max(0.0)`),
`collapsed_parent_bottom_margin_px` (flow.rs:1106-1118, currently
`flow.previous_block_margin_bottom_px.max(0.0)`), and the
empty-intermediary propagation
(`propagated_collapsed_margin_delta_px`, flow.rs:1136-1138, currently
`child.max(own.max(0.0)) - own.max(0.0)`).

Fix: replace each clamp with the three-branch rule. **Clarification
(2026-10-20, the transcription's site-1 question, answered):**
`collapsed_parent_top_margin_px`'s return is the CHILD's collapsed
CONTRIBUTION to the parent's effective margin — return it UNCLAMPED
(negative passes; for the direct-child case it is simply the child's
margin, no chain walk anywhere in D1). The parent's effective top
margin is `three_branch(parent's OWN margin, that contribution)`,
applied AT THE CONSUMER — the site where the parent's margin is
actually computed (trace it; do not "unclamp" the child and leave the
parent's margin uncombined — that is the failure mode where the child
stops overflowing but the parent is not pulled up). The child's
position when collapsed (at the parent's content edge) is the existing
machinery and does not change. **The D1 generalization (2026-10-20, the transcription's pull-up
failure and the spillover interaction — architect's correction of the
earlier "no top-down lookahead for flag-true" claim, which was
WRONG).** Pulling the parent up requires the collapsed leading margin
chain AT THE PARENT'S POSITIONING TIME (its grandparent positions it
before its children are laid out), so the top-down chain computation
`collapsed_leading_margin_chain_px(container)` — the container's
margin.top collapsed with the first-in-flow chain through
empty/barrier-free descendants (three-branch per level, barriers stop),
from computed styles — is GENERAL, used at both boundary kinds:

- **flag-true (plain container):** the container's EFFECTIVE margin for
  positioning = `three_branch(container.margin.top, chain)` — used in
  its `flow_top` within its parent (the pull-up). The child's collapsed
  position (at the parent's content edge) is existing machinery.
  D1-(a): parent margin 0, direct child −20 → chain = −20 → effective
  margin −20 → parent top 0 → −20 (pulled up); the child sits at the
  parent's content edge (−20 absolute, at the edge — no longer
  overflowing above a stationary parent).
- **flag-false (FC/flex-item):** the chain does NOT escape (the item's
  margin to the flex container stays its authored margin); instead the
  chain offsets the first in-flow child inside (`content_top + chain`).

**The spillover re-scoping (deliberate decision stands; its scenario
adapts).** The 2026-10-18 layer-spillover pins
(`layer_bounds_clip_spillover_by_design`,
`normal_layer_paints_spillover_escape_hatch`, and the composite
`negative_margin_child_does_not_expand_clip_world`) use a FIRST-CHILD
negative margin to produce the spill — a scenario that only spills
while the clamp keeps the layer stationary. With correct first-child
collapse, the layer is pulled up WITH the child (layer top 0 → −20,
child at −20 at the content edge): the spill premise for that scenario
shape disappears. The deliberate DECISION (layers clip; spillover is a
layering decision, never a surface-expansion decision) is unchanged.
The pins are RE-SCOPED so the deliberate spill exists under correct
margins: a MIDDLE sibling with a negative margin (a first child with
zero/positive bottom, then a sibling with `margin-top: −20` — the
sibling collapse shifts it above the layer while the layer stays put).
The first-child variant becomes a separate ON-SPEC expectation (the
layer pulled up with its first child: layer top == child top == −20,
child at the content edge). The registry's deliberate row gains the
scenario note; the composite `clip_world` assertion keeps its
stationary-layer scenario via the same re-scoping. The delta function keeps its shape:
`delta = three_branch(child_margin, own_margin) − own_margin`
(positive chains stay `max` — the
`empty_intermediate_block_collapses_own_margins_with_descendant_boundaries_by_max`
control must hold at 10; negative chains pass as negative deltas).

**Rows flipped** (3 sub-cases): (a) first child `margin-top: -20px` →
the boundary passes −20 (parent pulled up 20), child at −20 (today:
clamped to 0); (b) last child `margin-bottom: -20px` → parent border
box 11 (today 1); (c) negative margin through an empty intermediary →
11 (today 1). Family moves on-spec.

## D2. Row 2 — the empty-block self-collapse (mechanical)

Site: the empty-block path in `layout_flow_size` (flow.rs, the
`max(top, bottom)` self-collapse). Fix: three-branch
(`(−10, −5) → min = −10`; mixed `(10, −5) → 5`). Row flips: following
sibling at −9 (today −4), parent height 2 (today 7); the mixed control
7 (today 12). Moves on-spec.

## D3. Row 3 — the flex-item boundary containment (verification-first)

The machinery (verified by the architect, flow.rs:1051-1138): the
first-child escape runs through `collapsed_parent_top_margin_px`, gated
by `allows_parent_child_block_margin_collapse` (LayoutContext field) —
the formatting-context gate. CSS Flexbox §4: a flex item establishes an
INDEPENDENT formatting context, so margins must NOT collapse out of it;
the first in-flow margin chain is contained (the descendant positions
at `content_top + collapsed_margin` inside the item).

**Verification obligation (BEFORE any edit — report the two answers):**
(1) What value does `allows_parent_child_block_margin_collapse` carry
in the flex placement's item context today (file:line where the item
context is built)? (2) When that flag is false at a boundary, does the
current first-in-flow positioning ALREADY place the descendant at
`content_top + collapsed_margin` (i.e., is only the flag wrong), or is
the containment geometry itself missing (the phantom-height shape from
the T2 probe: margin realized as content height without offsetting the
descendant)?

**The flag mechanism (added 2026-10-20 under the transcription's
blocker — the doc's "set the flag in the flex placement" model was
inert, and naive context threading propagates).** The flag is computed
per DIRECT parentage, not threaded:
(1) `child_layout_context` computes
`allows_parent_child_block_margin_collapse = !parent_is_flex_container`
from the PARENT's presented display (already available in the entered
record — NO extra node-store read). Non-flex parents → `true`
(current behavior for element children preserved exactly).
(2) `enter_node`'s flag becomes `matches!(Element) &&
context.allows_parent_child_block_margin_collapse` (it now RESPECTS
the context instead of recomputing blindly at `flow.rs:461`).
(3) No propagation: verify and cite that `child_layout_context` does
NOT copy this field from the parent context (`..parent` must not carry
it forward — each child context computes it fresh from its own parent,
so the root context's value never reaches grandchildren). A flex
container's direct-child context then reads `false` — flex items are
independent formatting contexts (CSS Flexbox §4) — and every other
container's reads `true`.

**The fix by case:**
- **Case (a) — only the flag is wrong:** set
  `allows_parent_child_block_margin_collapse = false` for the flex
  item's subtree context (the flex placement builds it), so the margin
  cannot escape the item; the existing contained geometry then places
  the descendant correctly. Pins prove containment.
**Case (b) — the containment geometry (option A, decided 2026-10-20
over the post-relayout alternative).** The escaped margin's value is
computed during the child's relayout — after the parent positions it —
so no post-hoc adjustment is acceptable (it would violate the
single-pass doctrine). Instead, compute the collapsed leading margin
chain TOP-DOWN at positioning time from computed styles (no layout
needed): when the parent is an FC boundary (the per-parentage flag is
false), the first in-flow child's `flow_top` includes
`collapsed_leading_margin_chain_px(first_child)`:

```text
m = first_child.margin.top
current = first_child's first in-flow child
while current is a static block with no border/padding whose margins
      may collapse through (an empty intermediary per CSS 2.1 §8.3.1):
    m = three_branch(m, current.margin.top)
    current = current's first in-flow child
m = three_branch(m, current.margin.top)   // the chain's terminal margin
```

**Reconciliation (2026-10-20, the transcription's chain-scope tension —
answered):** the chain walk is flag-FALSE-only. The three cases are
mechanically distinct: **D1-(a)** is the direct first child (no chain —
`collapsed_parent_top_margin_px` with the three-branch rule handles it);
**D1-(c)** is the ESCAPE case through an empty intermediary — the
existing bottom-up delta mechanism
(`propagated_collapsed_margin_delta_px`) already carries the chain at
the empty child's exit; it needs only the three-branch rule instead of
the non-negative `max` (D1's stated fix: `delta = three_branch(child,
own) − own`, so a negative chain passes out as a negative margin, e.g.
−20 → parent border box 11, the spec value); **D3** is the CONTAINMENT
case — the offset needs the value AT POSITIONING TIME (before the
child's exit produces it), so the top-down chain walk computes the same
chain value early, flag-false only. "Flag-true unchanged" means
unchanged MECHANISM (bottom-up delta), never unchanged rule. The
top-down walk and the bottom-up delta must agree on any input (same
chain, same rule) — pin that agreement on a two-empty-intermediary
chain at a flag-false boundary.

The same `m` then accounts the child's margin-box height at flow
accumulation (position and height share one computed value — no
phantom). The chain stops at the first non-empty or
border/padding-barriered descendant (a barrier per CSS 2.1 §8.3.1).
Chain-walk reads only computed styles — zero layout, zero allocation. The empty
intermediary's own margin-height propagation is unaffected (it is the
realization of the same margin, positioned correctly by the offset).

**Rows flipped** (`off_spec_flex_item_subtree_first_child_margin_containment`):
(b) grandchild sits 20px below the item's content-box top (contained;
today 0, phantom height). The (a) item-at-cross-start half stays
on-spec; the (c) plain-block control (collapse-out is correct there)
must NOT move — the flag change is scoped to the flex item boundary.

**Audit rule (broad):** the flag change affects every flex item's block
subtree with first-child margins (normal_flow proofs, composition
families): spec-correct → update with documented cause and report;
spec-wrong → STOP.

## D4. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flow.rs` | D1's three-branch replacements, D2's three-branch self-collapse, D3's flag (+ case-(b) geometry if required) |
| `crates/ui-dom/src/domains/layout/flex.rs` | the D3 flag in the flex item's subtree context |
| `crates/ui-dom/tests/layout_proofs/layout/normal_flow.rs` | the D1/D2 family flips |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D3 row flip (+ any case-(b) pins) |
| `crates/ui-dom/tests/layout_proofs/README.md` | three registry rows move on-spec |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause (none expected) |

## D5. The transcription contract

Execute D3's verification obligation FIRST (two answers with file:line
evidence), then implement the matching case. STOP conditions: (1) any
case the document does not answer; (2) any proof moves spec-wrong
(the audit rule, broad per D3); (3) the plain-block control moves
(it must not); (4) a perf pin moves without a documented cause. ONE
commit, full gate green.
