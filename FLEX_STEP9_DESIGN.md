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

Fix: replace each clamp with the three-branch rule against the ADJOINING
margin (the parent's own margin edge where the machinery already pairs
them): both-positive → `max(a, b)`; both-negative → `min(a, b)`; mixed
→ `a + b`. The empty-intermediary delta keeps its delta shape but
computes the collapsed value with the three-branch rule instead of the
non-negative `max`. Comments cite CSS 2.1 §8.3.1 and PR 1 item 2's
sibling rule.

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

**The fix by case:**
- **Case (a) — only the flag is wrong:** set
  `allows_parent_child_block_margin_collapse = false` for the flex
  item's subtree context (the flex placement builds it), so the margin
  cannot escape the item; the existing contained geometry then places
  the descendant correctly. Pins prove containment.
- **Case (b) — the containment geometry is missing:** additionally
  apply the collapsed first-in-flow margin as an offset to the first
  in-flow descendant's position inside the boundary container (the
  container's first block child chain positions at
  `content_top + collapsed_margin`), contained — never propagated out.
  Specify the exact site from the probe's evidence (the first-child
  positioning path in `next_block_child_context`/`update_block_flow`).

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
