# FLEX_STEP7_DESIGN.md — cross-after-main (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-20. Author: the architect. Authority: CSS
Flexbox 1 §8.3/§9.4/§9.7 (cross sizing after main resolution), CSS
Images 3 §5.3 (replaced-element ratio); the engine corpus (Blink
`flex_layout_algorithm.cc:1524-1612` post-flex cross determination,
`ComputeReplacedSize`; Gecko `CanMainSizeInfluenceCrossSize:2546`,
`SizeItemInCrossAxis:4611-4645`); the registered row
(`off_spec_flex_stretch_distorts_replaced_img`); the refold budget
(FLEX_MAPPING §3.4 + the (d1)/(d2) obligations).

This document decides everything for step 7. Anything unanswered is a
document DEFECT — stop and return to the architect.

---

## D0. The step in one paragraph

Cross sizes are determined AFTER main resolution, per line: main
resolve (§9.7 as landed) → item cross at the resolved main size
(ratio-derived for replaced elements; the measured cross for everything
else; a flagged refold ONLY when the item's resolved main differs from
its measured width AND the subtree is flagged `main_influences_cross`)
→ line cross = max item outer cross → align-content packs the lines.
The pre-flex line-cross assert dies (line cross may now differ from
measured). The stretch-on-replaced distortion dies (the ratio computes
cross from resolved main — stretch applies only to non-replaced
auto-cross items). And the double-layout's real substance dies:
placement stops re-measuring intrinsic content the up fold already
computed — most concretely, wrapped-text-height is read from the fresh
record instead of re-shaped per text node per placement.

## D1. The per-line order (exact)

Per line, in this order:
1. **Main resolve** (§9.7 as landed in step 5 — unchanged).
2. **Item cross at resolved main:**
   - Replaced-ratio item (`REPLACED_RATIO` record flag) with auto
     cross: `cross = resolved_main_content / aspect_ratio` (+ cross
     border/padding) — the stretch-img fix. `stretch` does NOT distort:
     the stretched-content override is computed from the ratio, not the
     line cross.
   - `main_influences_cross`-flagged item AND resolved main ≠ measured
     main width: **refold** — re-measure the subtree's intrinsic
     content at the resolved width (text re-shape at the new width;
     %-cross re-resolution; records updated, equality-gated), then the
     cross comes from the refolded values. Counted (see D4).
   - Everything else: the measured cross (record's
     `content_height_at_width_px` / horizontal-cross equivalent),
     unchanged.
3. **Line cross** = max over the line's items of (item cross + cross
   margins), then the existing single-line/definite-container rules.
4. **Align-content packs** the lines using those line cross sizes
   (existing machinery, unchanged semantics — its INPUTS are now
   post-resolution).
5. **Placement** per item (align-items/align-self + main/cross
   offsets), unchanged.

## D2. The flag (record bit 2, finally set)

`MeasurementFlags` bit 2 (`MAIN_INFLUENCES_CROSS`) is SET in the up
fold for: text-bearing nodes (their content wraps — the existing
white-space class in bit 0 distinguishes wrap vs nowrap), any node
with percentage cross-axis forms, and any replaced-ratio node
(bit 1 — reused, no second flag). The flag's semantics: the subtree's
cross size may change when its main size changes. It is set at record
write (compute-time, from data the assembly already has) — no new
measurement.

## D3. The record-serving placement (the double-layout's death)

Today, placement's subtree walk recomputes wrapped-text-height per text
node (`wrapped_text_content_height_px`) — the fold already computed it
at the same width. After this step:

- **Width-unchanged item** (resolved main == measured main): the
  subtree walk reads the record for intrinsic sizes and wrapped height
  (fresh by construction — the fold wrote it this pass at the same
  width); NO text re-shape at placement. The wrapped-text-height call
  site gains the freshness check: record fresh and width matches → use
  `record.content_height_at_width_px`; else measure (the refold path).
- **Width-changed flagged item**: the refold re-measures (and
  re-writes) records at the resolved width BEFORE the subtree walk,
  so the walk still reads fresh records.
- **Unflagged width-changed item**: the subtree's intrinsic sizes are
  width-independent by the flag's definition — records serve; no
  re-measure.

The subtree box-writing walk itself is unchanged (boxes are written
only at placement; that is the down fold's job). What dies is the
duplicate intrinsic measurement — and it is measured by the
text-measure counters in D4.

## D4. Counters and the (d2) gate at this step

- **New counter** `FlexNodesRefolded` (enum-indexed, four-site change):
  increments once per refold (D1.2's flagged-and-changed re-measure).
- `FlexNodesPlaced` keeps its current semantics (per-child placement
  visits — the subtree box-writing walk still runs for every child;
  document this so the counter is not misread as "double-layout lives").
- **The (d2) observation on the (d1) scenario** (the budget's gate):
  the scenario's items resolve at their measured widths (content-driven
  auto sizes; the container's wrapping is at the line level, not item
  shrink) → expected refold fraction = 0. Record the observed value in
  PLAN.md's decision log at this landing. The ≤50% refold-fraction
  ceiling is therefore not breached by construction; the within-update
  memo stays designed-not-built (per the budget: it lands only on
  breach).
- **The real perf win is the shaping elimination at placement:**
  re-pin the (d1) allocation window with the documented cause (the
  placement's wrapped-text re-shapes are gone for width-unchanged
  items), and assert the text-measure-count delta in the scenario
  (measure-fold shapes continue; placement shapes → 0 for unchanged).

## D5. The pin-flip table (exact)

| Row | New asserted behavior |
|---|---|
| `off_spec_flex_stretch_distorts_replaced_img` (2 rows) | (a) flat: `align-items: stretch` on an img item (intrinsic 160×90) in a 120px line → cross == 90 (ratio-derived from resolved main 160), NOT 120; (b) nested (through a nested flex): same ratio outcome, NOT 100. Family moves on-spec |
| NEW: stretch on NON-replaced auto-cross items | a block item with auto cross stretches to the line cross (the exemption does not leak to non-replaced) |
| NEW: line cross is post-resolution | a text item whose wrapped height grows when shrunk: the line cross reflects the RESOLVED cross (taller after wrap), not the measured one |
| NEW: refold counter semantics | a flagged item whose width changes refolds once (counter == 1); an unchanged flagged item does not (counter == 0 for it) |

**The retired assert:** `assert_wrapped_line_cross_sizes_match_measurement`
dies (line cross is post-resolution; the placed-cross readback covers
the geometry). If any of its assertions carried load not covered by
the new rows, name it and add the equivalent post-resolution pin.

**Audit rule:** align-content families and cross-axis proofs may move
under the reorder (their inputs are now post-resolution): spec-correct
→ update with documented cause and report; spec-wrong → STOP.

## D6. Explicitly untouched

The §9.7 loop, item-view assembly, the auto-min derivation, the run
fold, paint, the measure fold's record write (except the flag bit),
the within-update memo (designed-not-built unless the ceiling
breaches).

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | the D1 per-line reorder, the D2 flag consumption, the D3 record-serving placement, the retired assert, the D5 pins |
| `crates/ui-dom/src/domains/layout/measurement_flags.rs` | bit 2 `MAIN_INFLUENCES_CROSS` set/query + tests |
| `crates/ui-dom/src/domains/layout/measurement_record.rs` | flag set at record write |
| `crates/ui-dom/src/types/root_dom_metric_name.rs` + increment site | `FlexNodesRefolded` |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D5 families + new pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry move |
| `crates/ui-dom/tests/perf_metric.rs`, `tests/perf_allocation.rs` | the D4 re-pins with documented causes |

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any proof
moves spec-wrong (the audit rule, including the align-content
families); (3) the (d1) scenario's observed refold fraction is NOT
the expected 0 (return with the scenario's measurements — the budget's
ceiling analysis depends on it); (4) a perf pin moves without a
documented cause. ONE commit, full gate green.
