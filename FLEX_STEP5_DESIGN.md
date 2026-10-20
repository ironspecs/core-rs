# FLEX_STEP5_DESIGN.md — §9.7 on the resolution lane (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-19. Author: the architect. Authority: CSS
Flexbox 1 §9.7 (resolve flexible lengths); the engine corpus
(`.tmp/engine-corpus/`: Blink `line_flexer.cc` entire — ctor freeze
`:26-43`, `FreezeItems` `:45-88`, `ResolveFlexibleLengths` `:90-182`,
all read firsthand by the architect; Gecko `FreezeItemsEarly:3050-3105`,
`FreezeOrRestoreEachFlexibleSize:3109-3176` — structurally identical, no
tie-break needed); the two registered rows this step flips
(`off_spec_inflexible_items_skip_hypothetical_clamps` incl. step 4's
deferred row (b); `off_spec_mixed_violation_freeze_ordering`).

This document decides everything for step 5. Anything unanswered is a
document DEFECT — stop and return to the architect.

---

## D0. The step in one paragraph

The §9.7 loop adopts the engines' exact semantics on the resolution
lane: items SEED at the (now auto-min-floored) hypothetical main size;
the constructor freeze (§9.7 step 3) marks factor-0 and
violation-at-entry items frozen AT HYPOTHETICAL; the loop resets
unfrozen items to hypothetical each iteration, distributes from base +
share, clamps per item, records per-item violation classes, and freezes
ONE class per iteration by the signed total violation — replacing the
seed-at-base, freeze-at-base, freeze-both-classes-at-once current
behavior. The hypothetical FIELD gains its floor in the same change
(step 4's deferred half), and step 4's outer-floor max() simplifies to
field + margins.

## D1. The hypothetical field floors (step 4's deferred half)

`hypothetical_main_size_px = clamp(base, resolved_min, resolved_max)
.max(automatic_min_main_size_px + main_axis_border_padding_px)` —
the field is border-box; the floor is the `901b2ee` border-box
application. Equivalence to the engines' `min' = max(min, auto_min)`
then clamp holds (floor wins exactly as min-wins-over-max). Step 4's
outer-hypothetical site (`outer = clamp(...).max(auto_min+bp) +
margins`) becomes `outer = field + margins` — the max() is a no-op
after the field floors; simplify it in this change with a comment.

## D2. The seed and the constructor freeze (exact)

`initial_flex_line_item_resolution_states` (or its lane equivalent):

1. **Seed** every item's `resolved_main_size_px = hypothetical_main_size_px`
   (the D1 floored field). NOT base.
2. **Constructor freeze** (§9.7 step 3, Blink `line_flexer.cc:26-43`):
   mark `is_frozen = true` for: (a) the item's mode-relevant flex factor
   == 0 (grow mode: grow == 0; shrink mode: shrink == 0); (b) grow mode
   AND `base > hypothetical` (a max violation at entry); (c) shrink mode
   AND `base < hypothetical` (a min violation at entry). Frozen items
   KEEP the seeded hypothetical — that is the entire fix for
   `off_spec_inflexible_items_skip_hypothetical_clamps` (they currently
   freeze at base, skipping the clamps the hypothetical carries).
3. **The all-frozen early return dies as a clamp-skipper:** when every
   item froze in the ctor pass, the line resolves to the seeded
   hypotheticals — the violation clamping IS the constructor freeze, so
   there is nothing skipped (pin it).

## D3. The resolution loop (exact engine shape)

Per line, after D2:

```text
loop (bounded by items-per-line):
    // FreezeItems (Blink :45-88):
    for each item: if !frozen { resolved = hypothetical }         // reset to hypothetical
    recompute: total_flex_factor (unfrozen, mode factor);
               free_space = line_main_inner_size
                 − Σ frozen (CURRENT resolved value + margins + gap)
                 − Σ unfrozen (base + margins + gap)
    // FROZEN CONTRIBUTION CORRECTION (2026-10-19, caught by the
    // transcription's hand-trace, adopted): frozen items contribute
    // their CURRENT value — hypothetical for ctor-frozen items, the
    // CLAMPED-AT-FREEZE value for loop-frozen items (Blink
    // `FlexedMarginBoxSize()`, line_flexer.cc:64) — not hypothetical
    // for both, as an earlier draft of this line said. Frozen items
    // keep their current value across iterations (the distribution
    // skips them); the D5 mixed-violation values only reproduce under
    // this reading.
    fractional factor-sum clamp (0 < total_factor < 1 → scale free space)   // unchanged
    early exit: grow && free_space ≤ 0 → break; shrink && free_space ≥ 0 → break   // unchanged shape
    // ResolveFlexibleLengths (Blink :90-182):
    distribute free space to unfrozen items from BASE + share
        (share = factor/total_factor for grow;
               = (shrink_factor × base)/total_weighted_shrink for shrink)   // unchanged
    for each unfrozen item: clamped = clamp_main_size_after_flex_constraints(candidate)
        // the post-901b2ee clamp: auto_min + main_bp floor, authored min/max,
        // content-box-zero guard — unchanged
        violation_class[item] = None | Min (clamped > candidate) | Max (clamped < candidate)
        resolved[item] = clamped
    total_violation = Σ signed (clamped − candidate)
    if total_violation == 0 → break
    freeze ONE class: total_violation ≥ 0 → freeze Min class;
                      total_violation < 0  → freeze Max class
    // frozen items keep their clamped value; loop
```

Termination is guaranteed (each iteration freezes ≥ 1 item or breaks);
the bound is items-per-line, matching both engines. The per-item clamp
is identical to today's; what changes is the freeze SELECTION (one
class by sign, not both classes at once) and the reset/recompute
structure (which makes the next iteration's free space correct).

## D4. The resolution lane entry

`FlexResolutionEntry` gains `violation: ViolationClass` (the T3 comment
reserved exactly this): `enum ViolationClass { None, Min, Max }`,
`Copy`. Written per item in the clamp phase; read at the freeze
selection. Transient scratch — shape change is free (T3's note).

## D5. The pin-flip table (exact)

| Row | New asserted behavior |
|---|---|
| `off_spec_inflexible_items_skip_hypothetical_clamps` (5 rows) | (a) `shrink:0` + `min-width > base` → freezes at MIN (hypothetical); (b) visible item, base < content-min → freezes at the derived auto-min (step 4's deferred row — the two-part completion); (c) `grow:0` + `max-width < base` → freezes at MAX; (d) column min-height variant → freezes at min-height; (e) no-clamp control → unchanged (base == hypothetical). Family moves on-spec |
| `off_spec_mixed_violation_freeze_ordering` (3 rows) | grow_mixed: A max-clamped 110, B == 190 (one-class freeze; today 160); shrink_mixed: A min-clamped 70, B == 70 (today 80); chained_refreeze: A 60, B 140, C 100 (one class per iteration). Family moves on-spec |
| NEW: all-frozen line | a line of entirely inflexible items resolves to their hypotheticals (no distribution, no clamp-skip) |
| NEW: the D4 outer-floor no-op | after the field floors, the outer site is `field + margins` (pin an existing floor scenario — it must not move) |

**Audit rule:** any other proof moving under the new loop (the
`resolve_active_flex_container_*` family — 23 unit tests pinning the
current loop's values, the shrink/grow distribution proofs, the
max/min freeze redistribution proofs) follows the standing rule. NOTE
for the auditor: several `resolve_active_flex_container_*` unit tests
pin values that depend on the CURRENT freeze-both-classes or
seed-at-base behavior while being nominally about other phases — those
moves are spec-correct if and only if the new value follows the D3
loop; verify each against the loop by hand, do not blanket-update.

## D6. Explicitly untouched

The grow/shrink mode choice, the fractional factor-sum clamp, the
scaled shrink factor, the per-item clamp
(`clamped_main_size_after_flex_constraints` post-`901b2ee`), the
no-free-space early exits, the lanes' other entries, placement, paint.
The hypothetical field's line-breaking consumers (the D4 floor) keep
working (now via the floored field).

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | D1 field floor + outer simplification, D2 seed + ctor freeze, D3 loop rewrite, D5 pins |
| `crates/ui-dom/src/domains/layout/flex_resolution_lane.rs` | `ViolationClass` + entry field + tests |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D5 families + new pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | two registry rows move on-spec |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any proof
moves spec-wrong (D5's audit rule, INCLUDING the hand-check obligation
for the `resolve_active_flex_container_*` family); (3) a perf pin
moves without a documented cause. ONE commit, full gate green.
