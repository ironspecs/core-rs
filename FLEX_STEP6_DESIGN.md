# FLEX_STEP6_DESIGN.md — down-fold context bases (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-20. Author: the architect. Authority: CSS
2.1 §10.1/§10.5/§10.7 (used sizes), CSS Values §percentages (margin/
padding resolve against the CONTAINING BLOCK's width); FLEX_MAPPING
§3.1 (the layering rule) and §4 step 6; the two registered rows this
step flips (`off_spec_flex_container_ignores_min_max_main_cross_sizes`,
`off_spec_percentage_resolution_against_flex_container`).

This document decides everything for step 6. Anything unanswered is a
document DEFECT — stop and return to the architect.

---

## D0. The step in one paragraph

Two localized corrections to the down-fold's basis plumbing, both
instances of the same rule: **a value is resolved against the basis
that owns it, read from the down-fold context.** (a) A flex container's
used main/cross sizes are min/max-CLAMPED (the vertical axis currently
reads raw `style.height_px`, bypassing the clamps the block machinery
applies everywhere else). (b) A flex ITEM's margin/padding percentages
resolve against the CONTAINER's content-box width at placement
(currently re-resolved against the item's OWN resolved content width at
placement time — the measure-time container basis is overridden).

## D1. Container used sizes are clamped (fix (a), exact)

Sites: `entered_container_main_size` / `entered_container_cross_size`
(flex.rs — the container main/cross probes). Today: horizontal main →
`entered.content_width_px` (block-clamped, correct); vertical main (and
cross) → raw `style.height_px` (raw specified, UNCLAMPED).

The fix: resolve the specified size through the same clamp the block
machinery uses — read `min_height_px`/`max_height_px` (and
`min_width_px`/`max_width_px` for uniformity) from the container's
`NodeStyle` (already resolved against the correct basis per PR 1 item
7's percentage table: min/max-height percentages → cb height, with
indefinite → none/0) and clamp the specified value:

```text
used(specified, min, max) = specified.map(|s| clamp(s, min, max))
```

applied at both probe sites for BOTH axes (the horizontal arm keeps
`entered.content_width_px` — already clamped upstream — but the
min/max-height arms of the cross probe get the same treatment for
uniformity; if the horizontal arm ever reads a specified value
directly, it clamps identically). `auto` stays `None` (content-derived,
as today). This is exactly what the block machinery computes — same
`NodeStyle`, same resolved min/max — so the values are identical by
construction, no second semantics introduced.

**Rows flipped** (`off_spec_flex_container_ignores_min_max_main_cross_sizes`,
4 rows): (a) column `min-height > height` → justify/line-break use the
clamped 200 (today raw 100: B at 40, spec 90); (b) column
`max-height < height` → clamped 40 (today raw 90); (c) column + wrap
breaks at the CLAMPED height (2 columns, not 3); (d) row
`min-height > height` → cross distribution uses the clamped height
(item 100, not 50). Family moves on-spec.

## D2. Item margin/padding percentages resolve against the container (fix (b), exact)

The CSS rule (CSS Values §percentages; CSS 2.1 §8.3/§8.4): margin and
padding percentages resolve against the CONTAINING BLOCK's WIDTH. A
flex item's containing block is the flex container — NOT the item's own
resolved content width.

The bug's shape: measure-time style resolution already uses the
container's width as basis (correct); PLACEMENT
(`active_flex_resolved_child_context`) sets the item-context
`available_width_px` to the ITEM's resolved content width, and the
item's `NodeStyle` is re-resolved against it — overriding the correct
measure-time values for `margin-%`/`padding-%`.

The fix (the two-basis separation): the placement item-context's
`available_width_px` (the basis the item's `NodeStyle` percentages
resolve against) is the CONTAINER's content-box width (carried from the
container's entered record), NOT the item's resolved main size. The
item's resolved main size continues to arrive via the content-size
OVERRIDE (`node_content_width_override_px` — unchanged, separate
channel). The item's CHILDREN still resolve against the item's content
box via the child context derived from the item's ENTERED record (that
path does not consult the context `available_width_px` field being
changed — see the verification obligation).

**Verification obligation (binding, listed because the brief may not
guess):** enumerate every reader of `available_width_px` in the
placement item-context and its descendants of the same context value:
(1) the item's `NodeStyle` resolution (the fix's target — now
container-based, correct); (2) `resolve_node_content_width` for the
item's own box (must remain driven by the override, not this field —
verify and cite); (3) the derivation of the children's context (must
come from the entered record's content size, not this field — verify
and cite). Any reader whose behavior changes beyond the margin/padding
percentages is a STOP unless it is also spec-correct per the same
containing-block rule.

**Rows flipped** (`off_spec_percentage_resolution_against_flex_container`,
2 divergent rows + 5 on-spec controls): `row_item_margin_left_percent`
→ 20px (container-basis, was 5px item-basis); `row_item_padding_left_percent`
→ 20px border-box accordingly (was 5px). The 5 on-spec rows (width %,
height %, column width %, column height %, wrapped-line break) must NOT
move. Family moves on-spec.

## D3. Explicitly untouched

The measure-time percentage resolution (already container-based),
`node_content_width_override_px` mechanics, the entered-record child
context derivation, container horizontal main sizes (already clamped),
the solve, the lanes, the auto-min derivation.

## D4. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | D1's clamp at the two probe sites; D2's container-basis at the placement item-context (+ the verification enumeration in a comment) |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D1/D2 family flips + any new pins the verification requires |
| `crates/ui-dom/tests/layout_proofs/README.md` | two registry rows move on-spec |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D5. The transcription contract

Implement THIS DOCUMENT exactly, including the D2 verification
obligation BEFORE changing the site (report the three cited readers
with file:line FIRST). STOP conditions: (1) any case the document does
not answer; (2) any proof moves spec-wrong (the audit rule); (3) any
behavior change beyond the D2 target (the verification obligation) that
is not also spec-correct; (4) a perf pin moves without a documented
cause. ONE commit, full gate green.
