# FLEX_MAPPING.md — the PR 3 mapping document (debate subject)

Status: `DEBATE INPUT — no code until resolved with third-party agreement`
Date: 2026-10-19. Author: the architect. Corpus: the two-axis enumeration
(Runs A/B/C, 424 layout proofs, 23 divergent proofs in 20 registry rows),
the fetched engine sources (Blink `third_party/blink/renderer/core/layout/flex/`,
Gecko `layout/generic/nsFlexContainerFrame.cpp` + `nsCSSFrameConstructor.cpp`),
and the layout-pass doctrine (PLAN.md decision log, 2026-10-19).

This document does four things: (1) maps our solver to Blink and Gecko,
field-by-field and phase-by-phase, every delta marked; (2) assigns every
registered complaint to the structure that must change; (3) states the
target architecture — the two-fold, zero-allocation solver — as decided
by the architect; (4) answers PR 3's five debate questions with evidence,
including why the kill criterion does NOT fire.

Engine tie-break rule (settled by PLAN.md): when Blink and Gecko disagree,
WPT decides first and Blink second. Two adjudications under this rule are
already recorded in §5.

---

## 1. The complaint list (the debate's input, now executable)

Twenty divergent rows plus companions, each a running proof. Ordered by
product criticality against the named client signal ("elements are not
moving where they should"; absolute-positioning workarounds).

### Tier 1 — hits ordinary designs constantly

| Row | One line | Fix site (§4 phase) |
|---|---|---|
| `off_spec_anonymous_inline_runs_are_not_merged` | contiguous inline runs must form ONE item (gap/justify phantom items today) | item formation |
| `off_spec_inflexible_items_skip_hypothetical_clamps` | inflexible items freeze at base, not hypothetical; SIBLINGS mis-position | §9.7 seed |
| `off_spec_hypothetical_main_size_not_floored_by_auto_min` | hypothetical skips the auto-min floor → wrong wrap points and overflow | item record clamp |
| `off_spec_flex_auto_min_unsupported_shapes_use_measured_size` | graceful gate: nested non-row containers auto-min at full measured size | up fold (subsumed) |
| `off_spec_flex_auto_min_growing_children_use_measured_size` | graceful gate: growing child in the walk forces the measured fallback | up fold (subsumed) |
| `off_spec_flex_container_ignores_min_max_main_cross_sizes` | vertical container sizes read raw `style.height_px`, unclamped | down-fold context |
| `off_spec_percentage_resolution_against_flex_container` | item margin/padding % resolve against the ITEM, not the container | down-fold context |
| `off_spec_flex_stretch_distorts_replaced_img` | `align-items: stretch` distorts images instead of exempting the ratio | cross sizing |
| `off_spec_flex_auto_min_nowrap_text_floors_at_widest_word` | nowrap auto-min floors at widest word, not the full run (§4.5) | text intrinsic |
| `off_spec_flex_item_inline_level_display_blockification` | `display: inline` items don't stretch (cross stays line-height) | item formation |
| `off_spec_abspos_and_display_none_children_occupy_flex_slots` | abs-pos/none children occupy item slots and gap adjacencies | item formation |

### Tier 2 — correctness edges and paint

| Row | One line | Fix site |
|---|---|---|
| `off_spec_mixed_violation_freeze_ordering` | both violation classes frozen at once; spec freezes one class per iteration by sign | §9.7 freeze loop |
| `off_spec_space_evenly_negative_free_space_does_not_fall_back_to_center` | justify-side negative fallback; target is the engine 0-clamp (see §5) | alignment |
| `off_spec_flex_paint_ignores_order_modified_sequence` | paint walks DOM order, not order-modified | paint walker |
| `off_spec_flex_item_subtree_first_child_margin_containment` | first-child margin swallowed at the item's formatting-context boundary (item-at-0 half on-spec) | item block subtree |
| `off_spec_parent_boundary_negative_margins_clamp` (normal flow, companion) | parent-boundary negative margins clamp to 0 | flow.rs (non-flex) |
| `off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative` (normal flow, companion) | empty-block self-collapse uses plain max | flow.rs (non-flex) |

### Unimplemented gates that are admission candidates (D6-named need: the clients)

`flex-basis` percentages; main-axis auto margins (§8.1 — the single most
common consumer pattern; cross-axis accidental path registered); intrinsic
size keywords (`min-content`/`max-content`/`fit-content`); `aspect-ratio`;
baseline alignment; `visibility: collapse`; grid (14 property gates + the
display gate — a separate algorithm, sequenced after flex per the arc).

---

## 2. The structural mapping (condensed; full inventories in session corpus)

### 2.1 Where both engines agree and we differ — the fix-to-theirs core

Both engines share one skeleton, and our deltas from it are exactly the
complaint list. Item formation happens BELOW the algorithm (Blink wraps
inline runs in anonymous blocks at tree build; Gecko's frame constructor
does the same and drops whitespace-only runs; both exclude abs-pos/none
children from item slots). The item record pairs a CONTENT-BOX base size
with a hypothetical that folds the automatic minimum INTO the min/max
clamp before clamping. §9.7 seeds at hypothetical, freezes inflexible
items there, and iterates: distribute by factor, record per-item violation
class, sum the signed total violation, freeze ONE class per iteration by
sign. Cross sizes are determined AFTER main resolution (items re-laid-out
or measured at the flexed size where main size can influence cross size —
Gecko flags these `CanMainSizeInfluenceCrossSize`). Negative free-space
alignment falls back by clamping to zero (Blink) / safe-keyword collapse
(Gecko) — never unsafe center. Paint and display-list building consume
the order-modified sequence (Gecko's `CSSOrderAwareFrameIterator`).

### 2.2 Where we match or are justified — no action

| Concept | Status |
|---|---|
| grow/shrink factors, order-modified layout sequence, greedy line breaking (never break before first item), line cross = largest outer item cross, single-line cross short-circuit, gap-once-per-adjacency, container auto main = largest line | MATCHES both engines |
| Border-box base vs engines' content-box base | JUSTIFIED-OURS (representation; equivalent math via `main_axis_border_padding_px`) |
| Key-based item identity (slot keys) vs engines' object pointers | JUSTIFIED-OURS (the crate's arena DOM model) |
| No solver-state cache (Gecko `CachedFlexItemData`, Blink `LayoutResultCacheSlot`) | JUSTIFIED-OURS (crate doctrine: no speculative caches; dirty-bit gating is the cache) |
| Separate transient resolution state vs engines' in-place item mutation | JUSTIFIED-OURS (no-persisted-solver-state invariant) |
| f32 vs Blink `LayoutUnit` (1/64) / Gecko nscoord (1/60) | JUSTIFIED-OURS (existing crate numeric policy; quantization deltas are out of scope) |
| Horizontal physical writing-mode + `direction` | JUSTIFIED-OURS (writing modes unadmitted, registered gate) |
| Reversal mechanics (reverse lines + offset math vs Blink `ApplyReversals` / Gecko axis tracker) | MATCHES semantically |

### 2.3 Where Blink and Gecko disagree (tie-break fodder)

Balanced line breaking (Blink-only Knuth-Plass; greedy is the shared
default and ours stays greedy — `flex-wrap: balance` unadmitted);
violation bookkeeping state shapes (semantically identical, structurally
different); auto-min gating details (scrollability predicates, aspect-
ratio transferred-size suggestion — Gecko richer); stretch mechanism
(Blink constraint-space behavior vs Gecko pre-resolved clamped cross);
abs-pos static-position mapping; `visibility: collapse` struts (Gecko
machinery found, Blink not in fetched sources); continuation/
fragmentation; caching; quantization. None of these block the arc; each
is named so it cannot become an unplanned debate.

---

## 3. The target architecture (architect's decision, binding)

The doctrine (PLAN.md 2026-10-19): a reflow MUST NOT allocate; layout is
ONE bottom-up intrinsic fold and ONE top-down resolution/placement fold
per update, gated by dirty bits; neighbor access only through arena links
and the visitor's carried context; no free recursion outside the folds.

### 3.1 Data structures

- **`MeasurementRecord` — node-inline, fixed-size, `Copy`, value-compared
  (S1).** Written once per node per dirty pass by the up fold:
  `{min_content_main, max_content_main, min_content_cross,
  max_content_cross, flex_base_main, hypothetical_main, auto_min_main,
  main_border_padding, cross_border_padding, margin_sums: [f32; 4],
  flags: MeasurementFlags}`. Axis-relative so a flex container reads
  children in either axis. `hypothetical_main` folds `auto_min_main`
  into the clamp AT WRITE TIME (engine semantics, §2.1). Flags carry:
  white-space mode class (min-content must be mode-aware — the nowrap
  divergence), replaced-ratio present, `main_influences_cross`,
  anonymous-run boundary markers. Value comparison gives change
  detection and dirty gating for free.
- **Three context-owned scratch lanes, high-water-mark reused, steady-
  state zero allocation.** (a) **order lane**: child-index permutation
  by `(order, dom sequence)` — built by insertion, no `Vec` per
  container, no `HashMap`; (b) **line lane**: `{first_order_index: u16,
  item_count: u16, outer_hypothetical_main: f32, cross_size: f32}` per
  line — lines are RANGES into the order lane, never `Vec<item>`;
  (c) **resolution lane**: parallel to the order lane
  `{resolved_main: f32, frozen: bool, violation: ViolationClass}` per
  item. Per-container use resets length, keeps capacity. The
  `perf_allocation` window is the judge: steady-state zero, warmup-bounded
  growth only.
- **What dies:** `Vec<MeasuredFlexItem>` per container, `Vec<FlexLine>`
  with per-line `Vec<FlexItemSource>`, the `HashMap` re-join
  (`flex.rs:1894-1923`), the per-container order-sort `Vec`, the
  recursive auto-min walk, and the staged double-layout of every child.

### 3.2 The two folds

**Up fold (intrinsic measurement, bottom-up).** One visit per dirty
node. Children complete before parents; each node computes its record
from its children's records via `first_child`/`next_sibling` arena
links. Consequences by construction:

- The recursive auto-min walk is SUBSUMED: a nested flex container's
  auto-min is its own record's content fields — shape-general and
  grow-blind. Both graceful gates and the review-flagged coarse
  over-estimate disappear; their registry rows move on-spec when the
  fold lands.
- Anonymous inline runs collapse: contiguous inline-level children
  contribute ONE virtual item to the parent's item view — a scratch
  entry `{first_child_key, child_count, record}`, never a DOM object
  (engines mutate their tree; our DOM identity stays stable — the
  semantic is theirs, the mechanism ours). Whitespace-only runs are
  dropped here (Gecko semantics).
- abs-pos/`display:none` children are excluded from the item view at
  the same site (engine item-formation semantics); abs-pos static
  position resolves at placement from the container's alignment
  properties (Blink's `AxisEdge` mapping is the reference).

**Down fold (resolution + placement, top-down).** The visitor carries
`LayoutContext { containing_block_width: f32, height_basis: Option<f32>,
accumulated_offset }`. At a flex container: container used main/cross
come from the context (min/max-clamped upstream by the block machinery
— kills the raw-height divergence); line formation and §9.7 resolution
run over the scratch lanes; each child's resolved rect is written to
its node record before descent; item margin/padding percentages resolve
against the context's containing-block width (kills the item-basis
divergence). Flagged `main_influences_cross` subtrees re-fold ONCE at
the resolved width during descent (Gecko's flag semantics); the default
path never re-measures. Replaced items derive cross from ratio given
resolved main (kills the stretch-distorts-img divergence). Paint
consumes the order lane's canonical sequence (kills the paint-order
divergence).

### 3.3 The §9.7 loop on the scratch lanes (engine-identical semantics)

Seed every item at `hypothetical_main` (inflexible items freeze there —
kills the inflexible-clamp divergence). Iterate, bounded by items per
line: compute free space; distribute by factor shares; record each
item's violation class in the resolution lane; sum the signed total;
freeze ONE class by sign; loop. Line cross sizes resolve AFTER main
resolution, from child records plus any flagged refolds. This is
Blink's `LineFlexer`/Gecko's `ResolveFlexibleLengths` with our data
structures; no tie-break needed (the engines are structurally identical
here).

### 3.4 Performance contract

- Zero allocation in steady state (the `perf_allocation` window extends
  to flex scenarios: a dirty-flex update allocates nothing after
  warmup).
- One up visit + one down visit per dirty node; the flagged refold is
  per-flagged-subtree, once, and counted by metric.
- New perf counters (zero-allocation, enum-indexed): flex nodes
  measured, flex nodes placed, flagged refolds, scratch high-water
  marks (level accessor, not a counter). History pins stay ordering-only.

---

## 4. Fix sequencing (each its own commit; behavior change travels with its pin flips)

Ordered by complaint criticality; each step flips its rows and runs the
full gate. Steps 1–3 are the foundation (they change where data lives);
4+ are semantic landings on the new structures.

1. **The `MeasurementRecord` + up-fold measurement of flex containers
   and item views** (anonymous runs, abs-pos/none exclusion, blockify
   `inline` → `block`). Flips: anonymous-runs, abs-pos/none slots,
   inline-blockification rows. Retires: per-container item `Vec`s, the
   dead `AnonymousInlineGroup` variant's dormancy.
2. **The scratch lanes + order-modified iteration + paint-order fix.**
   Flips: paint-order row. Retires: the `HashMap` re-join.
3. **Auto-min by fold** (hypothetical floor folded into the record;
   nowrap mode-aware min-content in the text intrinsic layer). Flips:
   hypothetical-floor, nowrap-auto-min rows, both graceful-gate rows
   (the gates and the over-estimate die). Retires: the recursive walk.
4. **§9.7 on the resolution lane** (hypothetical seed, one-class freeze
   by signed total). Flips: inflexible-clamp, mixed-violation rows.
5. **Down-fold context bases** (container used sizes; container-basis
   percentages). Flips: container min/max, item-% rows.
6. **Cross-after-main** (post-resolution cross sizing, replaced-ratio
   exemption, flagged refold). Flips: stretch-img row. Retires: the
   pre-flex cross assert.
7. **Alignment negative fallback** (0-clamp per §5). Flips the
   space-evenly row (with its corrected expectation).
8. **The two normal-flow companions** (parent-boundary negative clamp,
   empty-block self-collapse: the three-branch rule, item 2's shape).
   Flips those rows. Non-flex, small, unblocks nothing — scheduled here
   because the subtree-margin containment fix shares the flow.rs sites.
9. **Subtree margin containment** (BFC containment inside flex items).
   Flips the containment row.
10. **Admission candidates** (auto margins §8.1 first, then flex-basis
    %, intrinsic keywords — each D6-gated, its own landing, on the new
    structures; the debate scopes which are in).

---

## 5. Adjudications under the tie-break rule (decided)

1. **Negative-free-space alignment fallback:** both engines clamp to 0
   (Blink `ClampNegativeToZero`; Gecko safe-keyword collapse). Our
   registry pinned unsafe center as spec-correct — a misreading.
   DECIDED: target is the engine 0-clamp on the justify side; the
   align-content side already yields 0 (engine-matching). The row's
   expectation corrects when step 7 lands.
2. **Subtree first-child margin:** the row's original "item offsets by
   20px" claim was a misreading (Flexbox §4 independent formatting
   context; both engines read only the item's own margins — item-at-0
   is ours and is on-spec). The margin being SWALLOWED (phantom
   empty-block height) IS divergent from CSS 2.1 §8.3.1 containment.
   DECIDED and landed (b3e9aec): the row carries the corrected split.

---

## 6. PR 3's debate questions, answered with the evidence

1. **What is the actual complaint list?** Answered by construction:
   §1's two-axis table (20 divergent rows + companions + the gate
   candidates), produced by the spec-surface and composition
   enumerations against the named client signal.
2. **Map-first vs admit-first?** The evidence dissolves the framing.
   Every Tier-1 fix site IS a structural delta from the engines (§2.1):
   there is no honest "admit into the current solver" for anonymous
   runs, hypothetical floors, or post-flex cross sizing — those ARE
   the engines' structures. And a byte-identical migration would move
   zero coverage by construction (PLAN.md's own scope honesty). The
   recommendation this document carries into the debate: adopt engine
   SEMANTICS on the two-fold architecture, fix-by-fix in §4's order —
   each landing is simultaneously the structure work and the behavior
   fix, each gated by the full proof suite. The alternative (fix in
   place, no restructure) is costed honestly: it leaves the recursive
   walk, the re-join `HashMap`, the per-container `Vec`s, and the
   double-layout in production — the allocation and recursion the
   doctrine now forbids — and makes every subsequent admission harder.
3. **What does "done" mean?** The named subset that resolves the
   complaint list: §4 steps 1–9 land every registered flex divergence
   on-spec or reclassifies it, plus the step-10 admissions the debate
   scopes (auto margins §8.1 is the clients' top ask). Full structural
   alignment (baseline alignment, collapse struts, fragmentation,
   balanced wrap) is explicitly NOT done — those stay gated, named.
4. **What does it cost the solver's ~52 unit tests?** The
   `measured_item_record_*` family (11) re-homes onto the record's
   unit surface (genuine unit assertions survive); the
   `resolve_active_flex_container_*` family (23) re-expresses through
   the scratch-lane/record accessors where they pin genuine semantics,
   and dies where they pin bespoke internals (per the test-migration
   policy: genuine assertions re-home, cross-module assertions
   re-express through the public surface, bespoke-internal tests die
   with the bespoke structures). Estimated split for the debate:
   ~half re-home, a third re-express, the rest die — final count is a
   debate artifact, per-file.
5. **Kill criterion assessment (does the mapping conclude the solver
   is already closer to the engines than feared?).** NO — the
   criterion does not fire. The §9.2/§9.7 SKELETON is close (phase
   list maps ~1:1), but the accumulated deltas are each real,
   spec-pinned, and engine-confirmed (§2.1), and the cross-axis phase
   order touches architecture, not just math. The mapping supports a
   targeted sequenced fix list (§4: ~8 solver steps + 2 non-flex),
   not a stop — and not a wholesale rewrite either: the item/line/
   container field sets and the greedy skeleton carry over.

---

## 7. What the debate must attack

- The two-fold architecture's central bet: that `MeasurementRecord` +
  flagged-refold covers the engines' measure-twice behavior without a
  second general layout. The stress case is text-heavy flex items in
  shrink contexts (the refold flag's hit rate) — the perf counters
  from step 1 are the evidence either way.
- The scratch-lane capacities (unbounded children/lines per container
  vs fixed lanes: high-water-mark growth is the answer; attack the
  failure mode, not the average case).
- The step-1 scope: item-view formation + record in one step is the
  biggest single commit of the sequence; argue whether it splits.
- The §4 ordering against the client criticality ranking (steps 1–5
  are the complaint list's Tier 1; is any Tier-2 row a client blocker
  that should jump the queue?).
- Grid sequencing: the gate enumeration (14 properties + display) is
  ready; does grid's admission enter the arc after step 6 (cross
  sizing exists for both), or after step 10?
