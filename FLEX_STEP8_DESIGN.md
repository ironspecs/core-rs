# FLEX_STEP8_DESIGN.md — the alignment negative 0-clamp (complete design)

Status: `COMPLETE DESIGN — one branch wide; transcription is
mechanical`. Date: 2026-10-20. Author: the architect. Authority: CSS
Align 3 §5.6 (distributed alignment fallback), the adjudicated
tie-break reading (FLEX_MAPPING §5: engines clamp negative free space
to 0 — Blink `InitialContentPositionOffset`
`(free/2).ClampNegativeToZero()` (`flex_layout_algorithm.cc:1678/1684`);
Gecko safe-center collapse to start at ≤0
(`nsFlexContainerFrame.cpp:3553-3581, 3736-3762`)); the registered row
`off_spec_space_evenly_negative_free_space_does_not_fall_back_to_center`.

## D0. The step in one paragraph

`initial_main_axis_alignment_offset_px` clamps negative remaining main
space to 0 for `Center`, `SpaceAround`, and `SpaceEvenly` (flex-start
semantics — both engines' behavior); `SpaceBetween` and `FlexEnd` are
already correct (between returns 0; flex-end anchors the end). The
cross side already clamps (`.max(0.0)` — engine-matching, verified).
The registry row moves on-spec with BOTH its axis claims corrected —
the README's "falls back to center (−10px/−5px)" was a misreading of
CSS Align's fallback against the engines' clamp — and the
`space-around` "on-spec control" from Run A (pinned at −10) is
re-pinned to 0: it was itself engine-divergent.

## D1. The branch (exact)

In `initial_main_axis_alignment_offset_px` (flex.rs):
```text
FlexMainAxisAlignment::Center =>
    remaining_main_space_px.max(0.0) / 2.0
FlexMainAxisAlignment::SpaceAround =>
    if item_count == 0 { 0.0 } else { remaining_main_space_px.max(0.0) / (2.0 * count) }
FlexMainAxisAlignment::SpaceEvenly =>
    if item_count == 0 { 0.0 } else { remaining_main_space_px.max(0.0) / (count + 1) }
```
i.e., negative remaining clamps to 0 for all three arms (the initial
offset is then 0 = flex-start). `SpaceBetween` (returns 0 when
remaining ≤ 0) and `FlexEnd` (anchors the end) unchanged.

## D2. The pin table (exact)

Family `space_distribution_negative_free_space_clamps_to_flex_start`
(relabeled from the `off_spec_` family; the row moves on-spec with
corrected claims):

| Row | New asserted behavior |
|---|---|
| justify `space-evenly`, −20px free | initial offset == 0 (0-clamp, was −6.667) |
| justify `space-around`, −20px free | initial offset == 0 (0-clamp — RE-PINNED from the −10 "on-spec control," which was engine-divergent per Blink's center clamp) |
| justify `space-between`, −20px free | 0 (unchanged — correct control) |
| justify `center`, −20px free | NEW: offset == 0 (Blink clamps center's negative to 0) |
| `align-content: space-evenly`, −10px cross space | line pack offset == 0 (behavior already engine-matching via the existing `.max(0.0)`; the row's former "spec expects −5 center" claim is corrected to the engines' 0-clamp) |
| `align-content: space-around`, −10px cross | NEW control: == 0 (already clamped) |

Registry row text (the corrected claim, replacing the misreading):
"with negative free space, `space-between` falls back to flex-start and
`center`/`space-around`/`space-evenly` clamp to 0 (Blink
`InitialContentPositionOffset` `ClampNegativeToZero`; Gecko safe-center
collapses to start at ≤0) — the align-content cross side was already
engine-matching."

## D3. Explicitly untouched

`SpaceBetween`, `FlexEnd`, positive-space distribution (all batch 6
pins unmoved), the cross-side clamp code (already correct), the solve.

## D4. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | the D1 branch |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D2 family (relabel + re-pin + 2 new rows) |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry row correction + on-spec move |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause (none expected) |

## D5. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer; (2) any POSITIVE-space assertion moves (they
must not); (3) a perf pin moves without a documented cause. ONE commit,
full gate green.
