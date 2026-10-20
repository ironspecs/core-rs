# FLEX_STEP10_DESIGN.md — auto margins §8.1 (complete design)

Status: `COMPLETE DESIGN — the final step of the fix sequence;
transcription is mechanical`. Date: 2026-10-20. Author: the architect.
Authority: CSS Flexbox 1 §8.1 (auto margins absorb free space in their
axis, prior to alignment); the engine corpus (Blink
`flex_item.h:123` `main_axis_auto_margin_count` + absorption at
positioning `flex_layout_algorithm.cc:1867-1878,1966-1991`; Gecko
`MainAxisPositionTracker::ResolveAutoMarginsInMainAxis:3629`,
`SingleLineCrossAxisPositionTracker::ResolveAutoMarginsInCrossAxis:4014`);
the registered gate `flex_item_auto_margin_computes_to_zero_for_now`
and the registered accidental slice
`column_flex_block_item_cross_auto_margins_absorb_free_space`; the
debate's step-10 default (auto margins §8.1 in-arc first, PLAN.md
decision log).

## D0. The step in one paragraph

A flex item's `margin: auto` absorbs free space in its axis — the
single most common consumer flex pattern (centering, pushing items
apart) and the clients' top ask. Main axis: after §9.7, positive free
space distributes EQUALLY among the line's auto main margins, and
justify-content has no effect (auto margins take all of it). Cross
axis: per item, the line's remaining cross space around the item
distributes equally among that item's auto cross margins, and
align-items/align-self has no effect on that item. Negative free space
→ auto margins compute to 0. The zero-placeholder gate dies; the
step-6 `auto_margin_reference_width_px` field becomes the
double-absorption GUARD (the block solver never absorbs inside a flex
item — the flex solver owns §8.1).

## D1. Auto detection (exact)

A margin side is auto iff the computed box group's margin field for
that side is `BoxLength::Auto` (read from the node's computed groups at
item-view assembly — no new resolution). The item view gains two bits
per axis: `margin_main_start_auto`, `margin_main_end_auto`,
`margin_cross_start_auto`, `margin_cross_end_auto`. Auto margins are
NOT resolved at measure/style time (they need free space, which exists
only post-§9.7); they resolve at the positioning phases below and are
written into the item's final box model (border box position and
margin fields for paint/hit-test).

## D2. Main-axis absorption (exact, per line)

After §9.7 (unchanged), per line:
1. `free_space = line_main_inner_size − Σ(item.outer_resolved_main +
   margins(resolved, auto=0 so far) + gap)` — the same free space the
   §9.7 loop computed (reuse its value; no recompute).
2. `count = Σ over items of (main_start_auto + main_end_auto)` (0–2 per
   item).
3. If `free_space > 0 && count > 0`: each auto main margin
   `+= free_space / count`; the line's justify offset AND inter-item
   spacing are 0 (justify-content has NO effect when auto margins
   absorb — Blink absorbs at positioning before justify,
   `flex_layout_algorithm.cc:1867-1878`; Gecko's tracker likewise).
4. Else: auto margins = 0; justify as today (unchanged for
   count == 0 lines).
5. Item offsets accumulate as today: `offset + resolved_main +
   margin_start(resolved or absorbed) + gap` — the absorbed value feeds
   both the next item's offset and the item's final `box_model.margin`.

## D3. Cross-axis absorption (exact, per line per item)

After the post-resolution line cross (step 7's order), per item:
1. `free_cross = line_cross − item.outer_cross` (outer includes
   cross margins, currently auto=0).
2. `count = cross_start_auto + cross_end_auto` (0–2).
3. If `free_cross > 0 && count > 0`: each auto cross margin
   `+= free_cross / count`; the item's align-items/align-self offset is
   0 (no effect on that item).
4. Else: auto cross margins = 0; align as today.
5. The absorbed values feed the item's cross position and its
   `box_model.margin`.

## D4. The guard (step-6 field, re-roled)

`auto_margin_reference_width_px` STAYS, with its role re-documented:
it is the double-absorption GUARD — the block solver's
`resolve_block_auto_margins` inside a flex item sees `free ≤ 0`, so
main-axis auto margins resolve ONLY through the flex solver's §8.1
distribution (never twice). The comment at the site is updated from
"the deferred admission's mechanism" to "the guard that keeps the flex
and block auto-margin solvers disjoint (§8.1 landed in the flex
solver)."

## D5. The pin table (exact)

| Row | New asserted behavior |
|---|---|
| `flex_item_auto_margin_computes_to_zero_for_now` → `flex_item_main_auto_margins_absorb_free_space` | 200px row, 50px item, `margin-left/right: auto` → 75/75 (was 0/0); the item centers. Gate row moves on-spec |
| NEW: `margin-left: auto` pushes to main-end | 200px row, 50px item, `margin-left: auto` → left == 150 |
| NEW: auto margins override justify | `justify-content: space-between` + an auto main margin → justify ignored; free space to the margins |
| NEW: negative free space → 0 | container 30px, item 50px, `margin: auto` → margins 0, item overflows per justify (no absorption) |
| NEW: cross-axis centering in a row container | row container height 100, item 20 cross, `margin-top/bottom: auto` → cross offset 40 (centered); `align-items: flex-end` ignored for that item |
| NEW: two auto margins vs one, and multi-item equal distribution | per-side share arithmetic pinned |
| `column_flex_block_item_cross_auto_margins_absorb_free_space` (accidental slice) | SAME geometry (centered, left 20 in the 60px container) — now via the flex §8.1 path, NOT the block accident; the accidental block-solver path inside flex items is removed and the pin's header records the unification |

**Audit rule:** any proof with auto margins in flex contexts (the
centering families from PR 1 item 7's block `margin: 0 auto` — those
are BLOCK contexts and must NOT move; flex composition families) follows
the standing rule.

## D6. Explicitly untouched

The §9.7 loop, the block solver's `resolve_block_auto_margins` (block
contexts — PR 1 item 7's `margin: 0 auto` centering in normal flow is
unaffected), the guard field's fallback semantics for non-flex
contexts, the lanes, placement's DOM order, paint.

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | D1's auto bits on the view, D2's main-axis distribution, D3's cross distribution, the guard re-documentation, D5 pins |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D5 families |
| `crates/ui-dom/tests/layout_proofs/style/unsupported_values.rs` | the gate row's relabel (moved to `flex_layout.rs` or relabeled in place per the family's home) |
| `crates/ui-dom/tests/layout_proofs/README.md` | the gate row moves on-spec; the accidental-slice row's unification note |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause (none expected) |

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer; (2) any proof moves spec-wrong (the audit
rule, including the block-context centering pins which must NOT move);
(3) any double absorption (an auto margin absorbing in BOTH solvers —
the guard must prevent it; if a scenario shows it, STOP with evidence);
(4) a perf pin moves without a documented cause. ONE commit, full gate
green.
