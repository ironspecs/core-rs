# FLEX_MAPPING.md — the PR 3 mapping document (debate subject)

Status: `DEBATE INPUT — no code until resolved with third-party agreement against the obligations in FLEX_DEBATE_BRIEF.md (the work order)`
Date: 2026-10-19; amended 2026-10-19 under the adversarial review
(PLAN.md decision log, same date). Author: the architect. Corpus: the
two-axis enumeration (Runs A/B/C, 424 layout proofs, 23 divergent proofs
in 20 registry rows), the engine sources at `.tmp/engine-corpus/`
(GITIGNORED, manifest-pinned — Blink `third_party/blink/renderer/core/layout/flex/`
at Chromium `61ef1bef`; Gecko `layout/generic/nsFlexContainerFrame.cpp` +
`nsCSSFrameConstructor.cpp` at mozilla-central `0fdb388b`;
`flex_layout_utils.cc` does not exist upstream — see the manifest),
and the layout-pass doctrine (PLAN.md decision log, 2026-10-19).

This document does four things: (1) maps our solver to Blink and Gecko,
field-by-field and phase-by-phase, every delta marked; (2) assigns every
registered complaint to the structure that must change, with a per-row
restructure/in-place classification (§1); (3) states the target
architecture — the two-fold, zero-allocation solver — as decided by the
architect; (4) answers PR 3's six debate questions with evidence (§7),
including the kill-criterion reassessment against its restated,
falsifiable form (PLAN.md PR 3). **Amendments folded from the review:**
the §1 classification column and criticality honesty, the §3.1 record
respec and lane lifecycle, the §3.2 run-aggregation / whitespace /
static-position models, the §3.4 refold budget, the §4 step-1 policy
split, §6's verification-burden restatement, and §8's triaged attack
list and third-party obligations.

Engine tie-break rule (settled by PLAN.md): when Blink and Gecko disagree,
WPT decides first and Blink second. Two adjudications under this rule are
already recorded in §5.

---

## 1. The complaint list (the debate's input, now executable)

Twenty divergent rows plus companions, each a running proof. Ordered by
product criticality against the named client signal ("elements are not
moving where they should"; absolute-positioning workarounds). The
criticality ordering is the architect's INFERENCE from the signal — no
client has confirmed the ranking; the debate may re-order on client
evidence, and the classification column makes any row pullable
regardless of rank.

**The classification column (added per review).** Each row is classified
by what its honest fix requires: **YES** = the fix is a structural delta
and cannot land honestly on the current solver; **NO** = an in-place
pin-flip landing is honest today; **PARTIAL** = a named half lands in
place, the rest needs the new structures. The column's product is
SEQUENCING: the restructure itself stands on the layout-pass doctrine
(the recursive walk and per-container allocations die regardless of this
document's conclusion), so the classification answers the only live
question — which rows, if any, are client-critical enough to pull
forward as in-place pin-flip landings on the current solver, accepting
double work (the row flips in place, then re-pins on the new
structures). Each pull is recorded in §4. Rows left for the sequence are
cheap on the new structures — each is a one-phase branch — which is why
landing them with the sequence is the default.

### Tier 1 — hits ordinary designs constantly

| Row | One line | Fix site (§4 phase) | Needs the restructure? |
|---|---|---|---|
| `off_spec_anonymous_inline_runs_are_not_merged` | contiguous inline runs must form ONE item (gap/justify phantom items today) | item formation | **YES** — item formation IS the delta; engines form items below the algorithm; the run-aggregation fold (§3.2) has no honest current-solver site |
| `off_spec_inflexible_items_skip_hypothetical_clamps` | inflexible items freeze at base, not hypothetical; SIBLINGS mis-position | §9.7 seed | **PARTIAL** — clamping the seed by definite min/max is in-place; the auto-min floor and percentage min/max need the record + down fold |
| `off_spec_hypothetical_main_size_not_floored_by_auto_min` | hypothetical skips the auto-min floor → wrong wrap points and overflow | down-fold seed | **YES** — the floor is the fold-derived automatic minimum; the walk's approximation is the divergence |
| `off_spec_flex_auto_min_unsupported_shapes_use_measured_size` | graceful gate: nested non-row containers auto-min at full measured size | up fold (subsumed) | **YES** — subsumed by the fold; the walk dies by doctrine |
| `off_spec_flex_auto_min_growing_children_use_measured_size` | graceful gate: growing child in the walk forces the measured fallback | up fold (subsumed) | **YES** — subsumed by the fold; the walk dies by doctrine |
| `off_spec_flex_container_ignores_min_max_main_cross_sizes` | vertical container sizes read raw `style.height_px`, unclamped | down-fold context | **NO** — a clamp at the sizing-projection read; the down fold makes it principled, not possible |
| `off_spec_percentage_resolution_against_flex_container` | item margin/padding % resolve against the ITEM, not the container | down-fold context | **NO** — pass the container-width basis at the resolve site |
| `off_spec_flex_stretch_distorts_replaced_img` | `align-items: stretch` distorts images instead of exempting the ratio | cross sizing | **NO** — a cross-sizing branch (replaced-ratio exemption) |
| `off_spec_flex_auto_min_nowrap_text_floors_at_widest_word` | nowrap auto-min floors at widest word, not the full run (§4.5) | text intrinsic | **PARTIAL** — mode-aware min-content is text-intrinsic work independent of the folds; correct CONSUMPTION of it needs the fold-derived auto-min |
| `off_spec_flex_item_inline_level_display_blockification` | `display: inline` items don't stretch (cross stays line-height) | item formation | **PARTIAL** — the blockify decision can move to today's item collection; the stretch consequence lands with item-view formation |
| `off_spec_abspos_and_display_none_children_occupy_flex_slots` | abs-pos/none children occupy item slots and gap adjacencies | item formation | **PARTIAL** — exclusion at collection is in-place; the static-position model (§3.2) is down-fold placement work |

### Tier 2 — correctness edges and paint

| Row | One line | Fix site | Needs the restructure? |
|---|---|---|---|
| `off_spec_mixed_violation_freeze_ordering` | both violation classes frozen at once; spec freezes one class per iteration by sign | §9.7 freeze loop | **NO** — freeze one class per iteration by signed total; a loop change |
| `off_spec_space_evenly_negative_free_space_does_not_fall_back_to_center` | justify-side negative fallback; target is the engine 0-clamp (see §5) | alignment | **NO** — the adjudicated 0-clamp (§5) |
| `off_spec_flex_paint_ignores_order_modified_sequence` | paint walks DOM order, not order-modified | paint walker | **NO** — paint consumes the order-modified sequence the solver already computes (§2.2 MATCHES); the lane makes it canonical, not possible |
| `off_spec_flex_item_subtree_first_child_margin_containment` | first-child margin swallowed at the item's formatting-context boundary (item-at-0 half on-spec) | item block subtree | **NO** — flow.rs BFC containment, non-flex structure |
| `off_spec_parent_boundary_negative_margins_clamp` (normal flow, companion) | parent-boundary negative margins clamp to 0 | flow.rs (non-flex) | **NO** — flow.rs, item 2's shape |
| `off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative` (normal flow, companion) | empty-block self-collapse uses plain max | flow.rs (non-flex) | **NO** — flow.rs, item 2's shape |

**Tally:** four Tier-1 rows strictly require the restructure (anonymous
runs, the auto-min floor, both graceful gates). The restructure does not
need the row count — it stands on the doctrine; the tally exists so the
debate can see exactly what the sequence buys beyond the doctrine, and
so a "fix the NO rows in place and stop" counterproposal is priced
honestly (it leaves the walk, the per-container allocations, and the
double-layout in production — the allocation and recursion the doctrine
forbids — and strands the four YES rows).

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
  (S1).** Written once per dirty node by the up fold. **The layering
  rule (added per review): the up fold computes BASIS-INDEPENDENT
  quantities only. Anything whose value depends on the containing block —
  percentage margins/padding/min/max, a percentage basis, the
  hypothetical main size — resolves in the down fold.** The original
  spec stored `margin_sums: [f32; 4]` and a write-time
  `hypothetical_main`: final `f32`s for quantities that admit
  percentages whose basis exists only in the down fold — the same
  resolution-time layering error as
  `off_spec_percentage_resolution_against_flex_container`, baked into
  the structure. Rigorously applied, the rule also SHRINKS the record:
  authored style (margins, padding, min/max, basis as `BoxLength`)
  already lives inline on the node in the S1 computed groups, and the
  down fold reads it there by neighbor access — the record carries only
  what the up fold COMPUTES:
  - `{h_min_content, h_max_content, v_min_content, v_max_content,
    flags: MeasurementFlags}` — four intrinsic sizes per physical axis
    (the parent maps its main/cross to physical per its own direction;
    writing modes are unadmitted, so the mapping is the row/column
    table), plus flags. ~20 bytes per node; value comparison gives
    change detection and dirty gating for free.
  - **Automatic minimum: derived, not stored.** At the parent's §9.7
    seed: `auto_min = min(min_content_main, definite specified main
    size)` for the admitted non-scroll subset (§4.5's simplified
    boundary, named; scroll-container and transferred-size suggestions
    stay gated with `aspect-ratio`). This is what "the record's content
    fields subsume the walk" means; the nowrap divergence is killed
    because the TEXT intrinsic layer returns mode-aware min-content (the
    flags carry the mode class).
  - **Hypothetical main: computed at seed, not stored.** Resolved base
    (authored basis resolved against the down-fold context; `content`
    basis → max-content — the on-spec row) clamped by resolved min/max,
    floored at the automatic minimum BEFORE clamping (engine semantics,
    §2.1 — Blink/Gecko compute it inside the algorithm, which for us IS
    the down fold).
  - Flags: white-space mode class (min-content must be mode-aware — the
    nowrap divergence), replaced-ratio present (the ratio itself is read
    on demand from the asset record at cross time),
    `main_influences_cross`, anonymous-run boundary markers.
- **Three context-owned scratch lanes, UPDATE-scoped, high-water-mark
  reused, steady-state zero allocation.** The original spec said
  "per-container use resets length" — that lifecycle cannot serve the
  paint-order fix (paint runs a full pass AFTER layout and must walk the
  same canonical sequence), so the lanes are update-scoped: each lane is
  a context-owned arena of entries; a container's use claims a RANGE
  (start + len) in the current update's lane; all ranges reset once per
  update (lengths zeroed, capacities kept at high-water mark). Layout
  writes the ranges; paint reads them; no per-container allocation in
  steady state. (a) **order lane**: `u32` child indices — the original
  `u16` had no overflow story (65,535 children or lines per container is
  boundary-shaped data; a silent cap is forbidden by the panic policy,
  and a typed rejection would be the crate's first layout-time capacity
  error — `u32` matches the arena's key width and kills the question
  for four bytes an entry). The permutation by `(order, dom sequence)`
  is built by insertion: adaptive, O(n) on the common case (order
  untouched → the dom sequence is already sorted), O(n²) worst on large
  scrambled containers — a NAMED bound, not a hidden one: the §3.4
  counters make a regression visible, and a merge path is the designed,
  evidence-gated fallback, never speculative. No `Vec` per container, no
  `HashMap`. (b) **line lane**: `{first_order_index: u32,
  item_count: u32, outer_hypothetical_main: f32, cross_size: f32}` per
  line — lines are RANGES into the order lane, never `Vec<item>`.
  (c) **resolution lane**: parallel to the order lane,
  `{resolved_main: f32, frozen: bool, violation: ViolationClass}` per
  item. The `perf_allocation` window is the judge: steady-state zero,
  warmup-bounded growth only.
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
  content-based minimum derives from its own record's min-content
  fields at the parent's seed time — shape-general and grow-blind.
  Both graceful gates and the review-flagged coarse over-estimate
  disappear; their registry rows move on-spec when the fold lands.
- Anonymous inline runs collapse by the run-aggregation fold specified
  below; abs-pos/`display:none` children are excluded from the item
  view at the same site (engine item-formation semantics), with the
  static-position model pinned below.

**Run aggregation (the anonymous-item mechanism — specified per review;
Tier-1 row #1 rests on it).** A maximal run of contiguous inline-level
in-flow children contributes ONE virtual item: a scratch entry
`{first_child_key, child_count, record}`, never a DOM object (engines
mutate their tree; our DOM identity stays stable — the semantic is
theirs, the mechanism ours). The run's record is a horizontal fold over
the run's children carrying three cursors: (i) the accumulated
min-content maximum; (ii) the trailing unbreakable-fragment width WITH
its font metrics, joined to the next child's leading fragment when no
soft wrap opportunity intervenes across the boundary — adjacent spans
without intervening space form ONE unbreakable segment (CSS Text 3 §4),
and the widest segment may span node boundaries; (iii) the accumulated
max-content sum (no breaks — a plain sum). Heterogeneous fonts across
the run are carried per fragment. Cross sizes derive from the strut:
max over the run's children of used line-height (baseline alignment
stays a gated admission; the strut rule is today's semantic, named so
its replacement is visible).

**Whitespace-only runs** are dropped when — and only when — the run's
characters are COLLAPSIBLE under the container's white-space mode class
(`normal`, `pre-line`): Gecko's frame-construction semantics, reading
Flexbox §4's parenthetical ("characters that can be affected by a
white-space property") against CSS Text 3 processing. Under preserved
modes (`pre`, `pre-wrap` — both admitted) the run forms an item. The
pin cites the WPT `flexbox-whitespace-handling-*` family per the
per-claim provenance rule; the third party verifies the engine reading
(obligation b, §8).

**abs-pos static position (model pinned per review).** The child is
excluded from item slots, then positioned as if it were a 0×0 flex item
inserted at its ORDER-MODIFIED position — participating in line
placement and gap adjacency — resolved through the container's
alignment properties (Blink's `AxisEdge` mapping is the reference;
Flexbox §4.1 with CSS Align 3). Its containing block is the container's
padding box. WPT adjudicates conflicts per the tie-break rule. The
original one-liner ("from the container's alignment properties")
omitted the 0×0 order-modified model and would have produced wrong
static positions for abs-pos children between items.

**Down fold (resolution + placement, top-down).** The visitor carries
`LayoutContext { containing_block_width: f32, height_basis: Option<f32>,
accumulated_offset }`. At a flex container: container used main/cross
come from the context (min/max-clamped upstream by the block machinery
— kills the raw-height divergence); authored margin/padding/min/max
forms are read from the node's computed groups and resolved against the
context basis — item margin/padding percentages against the
CONTAINER's content-box width (kills the item-basis divergence, per the
§3.1 layering rule); hypothetical main sizes are computed at seed per
§3.1; line formation and §9.7 resolution run over the scratch lanes;
each child's resolved rect is written to its node record before
descent. Flagged `main_influences_cross` subtrees re-fold ONCE per
flagged nesting level during descent — nested flags refold nested
subtrees inside the parent's refold, bounded by depth and counted per
level (Gecko's flag semantics); the default path never re-measures.
Replaced items derive cross from ratio given resolved main (kills the
stretch-distorts-img divergence). Paint consumes the order lane's
canonical sequence (kills the paint-order divergence — the update-scoped
lane lifecycle in §3.1 is what makes this possible).

### 3.3 The §9.7 loop on the scratch lanes (engine-identical semantics)

Seed every item at its down-fold-computed hypothetical main size (§3.1:
resolved base, floored at the derived automatic minimum, clamped by
resolved min/max) — inflexible items freeze there (kills the
inflexible-clamp divergence). Iterate, bounded by items per
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
  per-flagged-subtree, once per nesting level, and counted by metric.
- **The refold budget (added per review; debate-ratified).** Every
  text-bearing item in a shrink context flags `main_influences_cross` —
  text wraps, and text is the most common content in UI — so "the
  default path never re-measures" is false exactly where measurement is
  most expensive (cosmic shaping). The engines' answer is within-pass
  caching (Gecko `CachedFlexItemData`), which the no-speculative-caches
  doctrine rejects. The honest bound: pin a text-heavy flex scenario in
  `perf_metric`/`perf_allocation` with the structural step; the
  construction guarantees total intrinsic measurements per dirty update
  ≤ 2× the up-fold count (each subtree refolds at most once per level);
  the refold fraction is exposed by level accessor. The budget number —
  the flagged-refold fraction ceiling in the pinned text-heavy scenario
  (starting position: ≤ 50% of measured flex nodes) — is ratified by
  the debate. **The fallback is designed NOW, not improvised later:** a
  WITHIN-UPDATE memo (context-owned, keyed by `(node, resolved width)`,
  reset per update) — not a cross-update cache, so the doctrine is
  untouched. Exceeding the budget triggers the memo's own landing with
  its perf pins.
- New perf counters (zero-allocation, enum-indexed): flex nodes
  measured, flex nodes placed, flagged refolds, scratch high-water
  marks (level accessor, not a counter). History pins stay ordering-only.

---

## 4. Fix sequencing (each its own commit; behavior change travels with its pin flips)

Ordered by complaint criticality; each step flips its rows and runs the
full gate — EXCEPT step 1, the structural step: zero pin flips by
contract, the full suite green, reviewed as a performance change with
the perf pins as judge. The pre-review draft bundled the up fold, three
behavior flips, and the test re-homing into one commit; the
structural-migration policy's bundling ban cuts both ways ("never
bundle algorithm changes into layout migrations" — and never bundle
behavior flips into algorithm changes), so they are split. Steps 2+ are
semantic landings on the new structures. Any §1 NO/PARTIAL row the
debate pulls forward lands as its own in-place pin-flip commit on the
current solver BEFORE step 1, recorded here; none are pulled by
default.

1. **The two folds + `MeasurementRecord` + scratch lanes,
   semantics-preserving.** The up fold writes records for all dirty
   nodes; the flex measurement sites cut over to record reads where the
   values are identical by construction; the per-container item `Vec`s,
   the `HashMap` re-join, and the per-container order-sort `Vec` die
   (their functions move to the lanes). The recursive auto-min walk and
   the staged double-layout SURVIVE this step unchanged — their deaths
   are semantic and land at steps 4 and 7 with their pin flips. ZERO
   pin flips; full suite green; the algorithm change reviewed as a
   performance change, perf pins as judge. The 59 solver unit tests
   re-home/re-express/die here per the test-migration policy (verified
   counts: 17 `measured_item_record_*`, 23
   `resolve_active_flex_container_*`, 19 others — the per-file split is
   a debate artifact). The new perf counters and the pinned text-heavy
   refold scenario (§3.4) land in this step.
2. **Order-modified canonical sequence + paint-order fix.** Paint
   consumes the order lane. Flips: paint-order row.
3. **Item-view formation, three landings:** (a) anonymous runs via the
   §3.2 run-aggregation fold (flips the anonymous-runs row; retires the
   dead `AnonymousInlineGroup` variant's dormancy); (b) abs-pos/none
   exclusion + the §3.2 static-position model (flips the slots row);
   (c) blockification completion — `inline` → `block` (flips the
   blockification row's cross-axis divergence).
4. **Auto-min by fold + mode-aware min-content in the text intrinsic
   layer.** The recursive walk DIES here. Flips: hypothetical-floor,
   nowrap-auto-min rows, both graceful-gate rows (the gates and the
   over-estimate die).
5. **§9.7 on the resolution lane** (down-fold-computed hypothetical
   seed, one-class freeze by signed total). Flips: inflexible-clamp,
   mixed-violation rows.
6. **Down-fold context bases** (container used sizes; container-basis
   percentages per the §3.1 layering rule). Flips: container min/max,
   item-% rows.
7. **Cross-after-main** (post-resolution cross sizing, replaced-ratio
   exemption, flagged refold). The staged double-layout of every child
   DIES here. Flips: stretch-img row. Retires: the pre-flex cross
   assert.
8. **Alignment negative fallback** (0-clamp per §5). Flips the
   space-evenly row (with its corrected expectation).
9. **The flow.rs trio** (parent-boundary negative clamp, empty-block
   self-collapse — the three-branch rule, item 2's shape; subtree
   margin containment — BFC containment inside flex items). Flips three
   rows. Non-flex, small, unblocks nothing — scheduled here because the
   containment fix shares the flow.rs sites.
10. **Admission candidates** (auto margins §8.1 first, then flex-basis
    %, intrinsic keywords — each D6-gated, its own landing, on the new
    structures; the debate scopes which are in, and whether any §1 row
    pulled forward per the classification).

---

## 5. Adjudications under the tie-break rule (decided)

1. **Negative-free-space alignment fallback:** both engines clamp to 0
   (Blink `ClampNegativeToZero`; Gecko safe-keyword collapse). Our
   registry pinned unsafe center as spec-correct — a misreading.
   DECIDED: target is the engine 0-clamp on the justify side; the
   align-content side already yields 0 (engine-matching). The row's
   expectation corrects when step 8 lands.
2. **Subtree first-child margin:** the row's original "item offsets by
   20px" claim was a misreading (Flexbox §4 independent formatting
   context; both engines read only the item's own margins — item-at-0
   is ours and is on-spec). The margin being SWALLOWED (phantom
   empty-block height) IS divergent from CSS 2.1 §8.3.1 containment.
   DECIDED and landed (b3e9aec): the row carries the corrected split.

---

## 6. The verification burden (restated per review)

The style-system redesign's correctness argument was STRUCTURAL
ISOMORPHISM: `DeclBlock` mirrors Servo's `PropertyDeclarationBlock`
field-for-field, so correctness could be audited at the field level
against the source. This design makes a different bet: engine SEMANTICS
on doctrine-shaped data ("the semantic is theirs, the mechanism ours" —
true of the whole architecture, not just anonymous runs: scratch lanes
are not Blink's `FlexLine` vector, and the record is not Gecko's item).
That is a strictly weaker verification position — there is no
field-level isomorphism to audit.

The substitute is behavioral equivalence, and it is owed per phase, not
per row:

- Every phase of §§3.2–3.3 gets an engine-behavior proof family keyed
  by spec section with WPT provenance per the per-claim provenance
  rule — INCLUDING phases that move no registered row (line-formation
  invariants, greedy break-never-before-first-item, gap-once-per-
  adjacency, the single-line cross short-circuit). The per-row pin
  flips are necessary, not sufficient: the rows pin the KNOWN
  divergences; the phase families pin the skeleton the review can no
  longer audit structurally.
- The phase families are the substitute review surface the third party
  audits (obligation a, §8).

This is the honest cost of the doctrine's shape; naming it keeps the
plan from citing the style redesign as precedent for a different bet.

---

## 7. PR 3's debate questions, re-answered post-review

1. **What is the actual complaint list?** Answered by construction:
   §1's two-axis table (20 divergent rows + companions + the gate
   candidates), produced by the spec-surface and composition
   enumerations against the named client signal. The debate's remaining
   product is SEQUENCING via the §1 classification: which NO/PARTIAL
   rows, if any, pull forward as in-place pin-flip landings on client
   criticality (accepting double work — the row flips in place, then
   re-pins on the new structures), each pull recorded in §4.
2. **Map-first vs admit-first?** Re-scoped per review. The pre-review
   headline — "every Tier-1 fix site IS a structural delta; the
   question DISSOLVES" — was overstated: four Tier-1 rows strictly
   require the restructure (§1 tally); the rest are in-place or
   partial. And the restructure no longer needs the rows' justification
   at all: the layout-pass doctrine mandates it independently (the
   recursive walk and per-container allocations die regardless of this
   document's conclusion). The live question is the per-row
   classification, adjudicated row by row — the debate may move any
   row's classification on evidence. The recommendation is unchanged in
   substance, honest in shape: adopt engine SEMANTICS on the two-fold
   architecture, fix-by-fix in §4's order, each landing gated by the
   full proof suite; the in-place alternative is priced per row in §1,
   not dismissed in bulk.
3. **What does "done" mean?** Unchanged: §4 steps 1–9 land every
   registered flex divergence on-spec or reclassified, plus the
   step-10 admissions the debate scopes (auto margins §8.1 is the
   clients' top ask). Full structural alignment (baseline alignment,
   collapse struts, fragmentation, balanced wrap) is explicitly NOT
   done — those stay gated, named.
4. **What does it cost the solver's unit tests?** Verified counts (the
   pre-review figures were stale): 59 `#[test]` in `flex.rs` — 17
   `measured_item_record_*`, 23 `resolve_active_flex_container_*`, 19
   others. Genuine unit assertions re-home onto the record/lane unit
   surface; cross-module assertions re-express through the public
   surface; bespoke-internal tests die with the bespoke structures (the
   test-migration policy). The per-file split is a debate artifact
   produced at step 1.
5. **Kill criterion assessment.** The pre-review assessment ("does not
   fire") answered a criterion that could not fire by construction —
   "closer than feared" is unmeasurable, and the doctrine settled the
   same day already mandates the restructure. Assessment against the
   RESTATED criterion (PLAN.md PR 3 — dies on measured reversal of the
   doctrine, or on third-party refutation of the §2.1 skeleton claims):
   no such measurement exists (the doctrine's evidence — recursion and
   per-element/per-container allocation in the hot path — is the corpus
   itself), and the §2.1 skeleton claims are source-cited, so the
   criterion does not fire TODAY. The difference is that it now CAN:
   it is falsifiable in both directions, and obligation (e) makes the
   third party its adjudicator, not this document's author.
6. **Grid's arc position (added per review; REQUIRED OUTPUT).** See §8 —
   the debate must output a dated decision; the recommendation is
   recorded there.

---

## 8. What the debate must still decide, and the third party's obligations

**Open for the debate (the pre-review attack list, triaged):**

- **The refold budget's NUMBER (§3.4).** The pre-review "central bet"
  attack survives as a ratification: the bound (≤ 2× measurements by
  construction) and the memo fallback are now designed; the debate sets
  the fraction ceiling for the pinned text-heavy scenario.
- **§4 ordering against client criticality, now executable.** The §1
  classification makes any NO/PARTIAL row pullable as an in-place
  pin-flip landing before step 1; the debate decides which pulls, if
  any, justify the double work. (Is any Tier-2 row a client blocker
  that should jump the queue?)
- **Grid's arc position — REQUIRED OUTPUT (PLAN.md PR 3 question 6).**
  Recommendation: grid admission enters the arc immediately after the
  fix sequence, as its own entry between PR 3 and PR 4 — the intrinsic
  fold grid needs will exist, the gate enumeration (14 properties +
  display) is ready, and the named signal ranks grid with flex;
  deferring past PR 5 starves half the signal for the arc's remainder.
  "Later, unspecified" does not close the question.
- **Resolved by the amendments (no longer debate fodder):** the
  scratch-lane capacity/lifecycle attack (update-scoped ranges, `u32`);
  the step-1 bundling (split per policy); the record's resolution-time
  layering (basis-independent up fold); the whitespace-drop condition
  (collapsibility-conditioned); the abs-pos static-position model
  (0×0 order-modified).

**The third party's obligations (agreement must address each BY NAME; a
general assent is not agreement):**

(a) **Source spot-check.** From the fetched Blink/Gecko sources
    (session corpus), verify the five §2.1 skeleton claims: item
    formation below the algorithm; content-box base with the automatic
    minimum folded into the hypothetical clamp; §9.7 hypothetical seed
    plus one-class-per-iteration freeze by signed total violation;
    cross-after-main with `CanMainSizeInfluenceCrossSize`;
    negative-free-space clamp. Each confirmed or refuted FROM THE
    SOURCES, not from this document's paraphrase.
(b) **Counterexample hunt.** Attack the run-aggregation cursors
    (heterogeneous fonts, fragment joining across node boundaries,
    preserved-whitespace runs) and the refold-once-per-level bound
    (nested flags). Produce a failing scenario from the enumerated
    corpus, or confirm none found.
(c) **Classification audit.** Attack every §1 NO/PARTIAL row for a
    hidden structural dependency that would make an in-place landing
    dishonest.
(d) **Budget sign-off.** Confirm the refold budget is measurable on
    the pinned scenario and the memo fallback is designed rather than
    improvised.
(e) **Kill-criterion adjudication.** Confirm the restated criterion
    (PLAN.md PR 3) is falsifiable in both directions, and adjudicate it
    against the doctrine evidence.
