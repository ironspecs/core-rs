# FLEX_STEP1_EXECUTION.md — the structural step, decomposed

Status: `EXECUTION PLAN — the architect's chained task decomposition of
FLEX_MAPPING §4 step 1`. Date: 2026-10-19. Authority: FLEX_MAPPING.md
(amended), FLEX_DEBATE_BRIEF.md (closed), PLAN.md PR 3 proof
obligations, the layout-pass doctrine. This plan is binding on the
implementation agents: data structures and algorithms are decided here,
never delegated.

**Step 1 in one sentence:** the two-fold structure and its data
structures land, semantics-preserving — zero pin flips, full suite
green, reviewed as a performance change with the perf pins as judge —
while the recursive auto-min walk and the staged double-layout
deliberately survive (their deaths are semantic: steps 4 and 7).

**Step 1's proof obligations (PLAN.md PR 3):** zero pin flips; full
gate per commit; the new perf counters and the pinned text-heavy
scenario land with it; perf pins re-pin only with a documented cause;
the 59 solver unit tests are migrated per the test-migration policy
with the per-file split produced as an artifact.

---

## 1. Grounding facts (from the corpus; the plan rests on these)

- The current walk is already fold-shaped: children return
  `FlowSize { width, height, min_width, max_width }` (flow.rs:97-104),
  folded bottom-up via `update_block_flow`; min/max-content widths are
  ALREADY computed bottom-up for every node. The record's
  `h_min`/`h_max` come free; no new measurement work in step 1.
- All item kinds share one epilogue: `measure_one_flex_item`
  (flex.rs:436-516) writes all 20 `MeasuredFlexItem` fields uniformly;
  kinds differ only in the dispatched measure (`flex.rs:529-586`). One
  conversion site.
- The intrinsic system already has the change-detection discipline:
  equality-gated writes returning bool, no manufactured dirt (the
  phantom-dirty lesson, `domains/visitor/README.md:57-66`). The record
  adopts it verbatim.
- Paint reads retained state only, in the same update (composite:
  style → layout → paint). Update-scoped lanes are viable for step 2's
  paint-order landing.
- `MeasuredFlexItem` is file-private; 18 of 20 fields have production
  reads; `main_axis`/`cross_axis` are read only by tests — they DIE in
  the cutover (dead fields, no replacement).

---

## 2. Module map (structural rules 1/5: behavior types in their own files)

| File | Status | Owns |
|---|---|---|
| `crates/ui-dom/src/domains/layout/measurement_record.rs` | NEW | `MeasurementRecord`, `MeasurementFlags`, `compute_node_measurement` (the shared exit-time computation), `replace_measurement_if_changed` (equality-gated write → bool) |
| `crates/ui-dom/src/domains/layout/flex_order_lane.rs` | NEW | `FlexOrderLane` + entry type; the adaptive insertion discipline |
| `crates/ui-dom/src/domains/layout/flex_line_lane.rs` | NEW | `FlexLineLane` + `FlexLineEntry` (ranges into the order lane) |
| `crates/ui-dom/src/domains/layout/flex_resolution_lane.rs` | NEW | `FlexResolutionLane` + entry type (parallel to the order lane) |
| `crates/ui-dom/src/types/layout_scratch.rs` | NEW | `LayoutScratch`: the RootDom-owned owner of the three lanes' storage; `reset_for_update` |
| `crates/ui-dom/src/types/dom_node_base.rs` | EDIT | new field `measurement: MeasurementRecord` beside `intrinsic`, + initializer |
| `crates/ui-dom/src/types/root_dom.rs` | EDIT | `layout_scratch: LayoutScratch` field; reset at update start |
| `crates/ui-dom/src/domains/layout/flow.rs` | EDIT | `exit_node` writes the record for the block world |
| `crates/ui-dom/src/domains/layout/flex.rs` | EDIT | the measurement restructure (up fold writes child records) and the lane-based solve (Vecs/`HashMap` die); the auto-min walk and the staged double-layout UNTOUCHED |
| `crates/ui-dom/src/types/root_dom_metric_name.rs` | EDIT | two new counters (§8) |
| `crates/ui-dom/tests/perf_metric.rs`, `tests/perf_allocation.rs` | EDIT | the (d1) scenario pin |

**Scratch ownership amendment (decided here, refining FLEX_MAPPING
§3.1's "context-owned"):** the lanes live on `RootDom`, not
`RootDomContext`. Rationale: lanes are per-update, per-document state;
the context is shared across documents, and scratch there would couple
documents' lifetimes for zero benefit (the update loop is
single-threaded, so sharing buys nothing). Update-scoped reset:
`LayoutScratch::reset_for_update` at the start of each `RootDom`
layout phase.

---

## 3. The record (exact type)

```rust
// measurement_record.rs
pub struct MeasurementRecord {
    pub h_min_content_px: f32,        // min-content inline size (bottom-up; FlowSize.min_width_px)
    pub h_max_content_px: f32,        // max-content inline size (bottom-up; FlowSize.max_width_px)
    pub content_height_at_width_px: f32, // content height at this pass's laid-out width
    pub v_min_content_px: Option<f32>, // UNCOMPUTED in step 1 (lands with its consumer, step 4)
    pub v_max_content_px: Option<f32>, // UNCOMPUTED in step 1 (lands with its consumer, step 4)
    pub flags: MeasurementFlags,      // see below
}

pub struct MeasurementFlags { pub bits: u8 }
// bit 0: white-space mode class is preserve (pre/pre-wrap/break-spaces) vs collapse (normal/pre-line)
// bit 1: replaced-ratio present (img with intrinsic ratio)
// bit 2: main_influences_cross (RESERVED — written from step 7's analysis; false in step 1)
// bit 3: anonymous-run boundary start (RESERVED — step 3a)
// bit 4: anonymous-run boundary end   (RESERVED — step 3a)
// bits 5-7: reserved, zero
```

Rules:
- `Copy`, `PartialEq`, value-compared. Written ONCE per node per pass
  at the node's exit (block world: `exit_node`; flex subtree: the
  measure fold, §5). `replace_measurement_if_changed` is equality-gated
  and returns bool; record writes NEVER mark style/layout/paint dirty
  (the phantom-dirty lesson; the write is a pass OUTPUT).
- `content_height_at_width_px` is this-pass measured (basis-dependent
  by design — it is the answer placement needs NOW). The intrinsic
  `v_*` slots stay `None` until their consumer lands (D6 discipline:
  never compute what nothing reads; step 4's auto-min consumption is
  the named consumer, and its computation — fragment counts from the
  text intrinsic layer, bottom-up accumulation — is specified in that
  step's plan, not this one).
- Freshness: reads on a node the current pass has not exited are a
  contract violation; step 1's only readers (the flex item view, §5)
  read children the fold has already exited, so freshness is
  structural. No `NotMeasured` state is introduced; the existing
  `DomIntrinsic::NotMeasured => Err(UnmeasuredIntrinsic)` discipline
  covers the intrinsic-leaf data.

---

## 4. The lanes (exact types)

```rust
// flex_order_lane.rs — the canonical child sequence for one container
struct FlexOrderLaneEntry { child_key: DomNodeSlotKey }   // key width matches the arena
// FlexOrderLane: context… RootDom-scratch-owned storage; per-container RANGE {start: u32, len: u32};
// insertion by (order, dom sequence): adaptive — O(n) when orders are untouched
// (the dom walk is already sorted), O(n²) worst on large scrambled containers
// (a NAMED bound; the merge path is the evidence-gated fallback, never speculative).

// flex_line_lane.rs
struct FlexLineEntry {
    first_order_index: u32,
    item_count: u32,
    outer_hypothetical_main_size_px: f32,
    cross_size_px: f32,
}
// lines are RANGES into the order lane; a container claims a range per line-build.

// flex_resolution_lane.rs — step-1 shape (extends in step 5, free because transient)
struct FlexResolutionEntry {
    resolved_main_size_px: f32,
    is_frozen: bool,
    // step 5 adds: violation: ViolationClass
}
// parallel indexing with the order lane (same container range).
```

Lifecycle: update-scoped (paint-order's requirement). Each container
claims ranges during its solve; all ranges reset once per update
(`reset_for_update`: lengths zeroed, capacities kept at high-water
mark). Layout writes; paint reads in the same update (step 2). Steady
state: zero allocation after warmup — the `perf_allocation` window is
the judge.

---

## 5. The cutover (the correctness argument; verified against the corpus)

**The rule: the record replaces ONLY subtree-computed content sizes.
Every style-derived value keeps its exact current production site and
resolution timing** (`resolved_node_style(child_context)` per item at
measure time, `NodeStyle` fields, computed groups) — step 1 moves
NOTHING that step 6 owns.

| `MeasuredFlexItem` field | Step-1 source |
|---|---|
| `source` | child slot key (unchanged) |
| `order` | `computed_flex().order` (node read at solve) |
| `main_axis`, `cross_axis` | **DIE** (dead in production; container style answers) |
| `align_self` | `computed_flex().align_self` (node read) |
| `flex_grow_factor` / `flex_shrink_factor` | `computed_flex().grow/.shrink` |
| `flex_base_size_px` | derived at the current site: `preferred_flex_basis_px` (group) + main border/padding (group), else the record's measured main (border-box) |
| `hypothetical_main_size_px` | derived at the current site: `clamp_main_axis_size(style, …)` against the SAME measure-time basis as today (the down-fold move is step 6's) |
| `automatic_min_main_size_px` | **the current path, UNCHANGED** (the graceful gates survive step 1; they die at step 4) |
| `min_main_size_px` / `max_main_size_px` | `main_axis_limits(style, …)` (unchanged site) |
| `main_axis_border_padding_px` | computed-box sides (node read) |
| `outer_hypothetical_main_size_px` | derived: hypothetical + margins (as today) |
| `measured_main_size_px` | **the record**: for basis-auto/content items — content main from the record + main border/padding (group); definite-basis items unaffected |
| `measured_cross_size_input_px` | **the record**: `content_height_at_width_px` (horizontal main) / `h_max_content_px` (vertical main) + cross border/padding (group), margins stripped as today |
| `margin_main_*_px`, `margin_cross_*_px` | `NodeStyle` sides at the current measure-time resolution (the container-basis move is step 6's) |

**Per-kind record production** (the identity the T2/T4 tests assert,
from the corpus inventory): text → `h_min/h_max` from
`TextIntrinsic.min/max_content_width_px`, height from the wrapped-text
measure at width; block with children → from the block walk's
`flow.min_width_px/max_width_px` and `flow.block_top_px + line_height`;
inline → the inline flow accumulation; img/control → intrinsic width
(min == max), height per the aspect-ratio/preferred-size rules;
nested flex container → `h_min == h_max ==` the inner solve's resolved
content width, height = inner resolved content height (the inner staged
solve RUNS AS TODAY in step 1 and writes its own items' records).

**The flex measurement restructure** (the heart of step 1): today,
`build_staged_measurement_solve_from_container` →
`collect_measured_flex_items_in_dom_order` builds
`Vec<MeasuredFlexItem>` by recursively walking each child. Step 1:
`relayout_flex_node`'s measurement phase becomes an UP FOLD over the
container's subtree — each child is exited exactly once (the same
dispatched measure as today, same contexts), and each exit WRITES the
child's record and pushes the child onto the container's order lane
(insertion by (order, dom index)). The solve then reads the item view
(record + `resolved_node_style` + lane entries) — the
`Vec<MeasuredFlexItem>`, `Vec<FlexLine>`, the per-container order-sort
`Vec`, and the `HashMap` re-join (flex.rs:1894-1923) DIE. Line building
runs over the lanes (ranges); the resolution runs over the resolution
lane; the placement walk (`place_active_flex_resolved_children`, the
double-layout) is UNTOUCHED. The auto-min walk
(`supported_automatic_min_main_size_px` and below) is UNTOUCHED.

**The no-write discipline, stated:** today the staged measurement solve
writes no node layout/box state (pinned by `staged_measurement_solve_*`).
Step 1's fold WRITES records — measurement outputs, not layout results.
The pins' assertions (no node-layout/box-model writes) stay green
unchanged; their doc comments gain one line naming records as the
sanctioned measurement output (comment-only edit).

---

## 6. The 59 solver unit tests (migration inventory)

Per the test-migration policy (genuine unit assertions re-home;
cross-module assertions re-express through the public surface;
bespoke-internal tests die with the bespoke structures), executed at
T5 with the per-file split produced as an artifact:

- `measured_item_record_*` (17): pin `MeasuredFlexItem` production per
  kind. Genuine per-kind assertions (base/hypothetical/auto-min values,
  intrinsic leaf handling) re-home onto the record + item-view unit
  surface. Assertions that pin the 20-field struct's SHAPE (incl. the
  two dead fields) die.
- `resolve_active_flex_container_*` (23): pin solve internals
  (distribution, clamps, packing, gaps, offsets). Genuine semantic
  assertions re-express through the lane accessors or the public
  surface; assertions of the current loop's bespoke state
  (e.g. `FlexLineItemResolutionState` shape) die.
- Others (19: line collection, order, measurement-purity, cross-axis
  helpers, reversal mapping, measured-container): triaged by the same
  rule; the `staged_measurement_solve_*` purity pins re-home with the
  doc-comment line from §5.

The artifact: a per-test split table (test → re-home/re-express/die +
destination), committed with T5.

---

## 7. Perf counters and the (d1) scenario; the (d2) baseline

New counters (zero-allocation, enum-indexed; the four-site change in
`root_dom_metric_name.rs`): `FlexNodesMeasured` (record writes in the
flex fold), `FlexNodesPlaced` (placement relayouts). **Refinement of
the brief's (d1)(i), recorded here:** the flagged-refold counters land
with the mechanism they count (step 7's cross-after-main), not in
step 1 — in step 1 the refold concept does not exist; the current
staged double-layout of every child IS the pre-mechanism cost shape.

The (d1) scenario pin (perf_metric + perf_allocation): the 240px
wrapping row, 12 text items + 4 nested text-bearing flex items, one
text mutation per update. Asserts: `FlexNodesMeasured`/`FlexNodesPlaced`
exact counts for the scenario, and steady-state zero allocation after
warmup (lanes at high-water mark).

**The (d2) baseline at step 1's gate (recorded in PLAN.md's decision
log):** the scenario's observed counters, the allocation window, and
the documented pre-mechanism shape (the double-layout ≡ 100% refold
analog). The ≤50% refold-fraction ceiling (ratified (d1)(iv)) binds
step 7's flagged-refold mechanism; breach at THAT gate lands the
within-update memo.

---

## 8. The task chain (narrow, gated, chained; one commit per task)

| # | Scope | Files | Acceptance (all plus full gate, zero pin flips) |
|---|---|---|---|
| T1 | The record type + node integration | `measurement_record.rs`, `dom_node_base.rs` | record on node, equality-gated write returns bool; unit tests for flags + the write gate; no production reads |
| T2 | Block-world record writes | `flow.rs` (exit_node + the shared computation) | per-kind identity tests: record == current walk outputs (text/block/inline/img/control); full suite green |
| T3 | The lanes + scratch | the three lane files, `layout_scratch.rs`, `root_dom.rs` | insertion discipline pinned (sorted-input O(n), scrambled correctness, stable ties); reset-for-update keeps capacity; unit tests |
| T4 | The flex measurement restructure + lane-based solve | `flex.rs` (+ item-view), using T1–T3 | the 135 flex proofs + full suite green UNCHANGED (the strongest gate); Vecs/`HashMap` deleted; auto-min walk and double-layout untouched (diff-verifiable); `staged_measurement_solve_*` comments updated |
| T5 | The 59-test migration | `flex.rs` test module (+ re-homed files per rule 1) | the per-test split artifact committed; re-homed tests assert through the record/lane/public surface; structural lint green |
| T6 | Counters + scenario pin + baseline | `root_dom_metric_name.rs`, both perf files, PLAN.md | counters green; scenario pin asserts exact counts + steady-state zero allocation; the (d2) baseline recorded in the decision log with the double-layout note |

Each task: `cargo fmt`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace`,
`cargo xtask lint` — all green before commit. Commit messages:
`feat(ui-dom): <task> (flex step 1, T<n>)`. T4 is the performance
change of record: its review runs the scenario pin and the full suite.

---

## 9. Named non-goals for step 1 (a task that drifts here is wrong)

- NO change to any resolution timing or basis (percentage margins,
  padding, min/max resolve exactly where they do today).
- NO death for the recursive auto-min walk or the graceful gates
  (step 4's) or the staged double-layout (step 7's).
- NO `v_*` intrinsic computation (step 4's), NO refold mechanism or
  counters (step 7's), NO anonymous-run handling (step 3's), NO
  abs-pos/none exclusion change (step 3's), NO paint-order change
  (step 2's).
- NO behavior change observable to any proof: zero pin flips is the
  contract; a moved pin without a documented cause is a stop.

## 10. Risk register

- **R1 — identity mismatch per kind** (record ≠ current computed for
  some item kind): gated by T2/T4 per-kind identity tests derived from
  the corpus inventory; a kind that cannot match exactly is a STOP,
  not a fudge.
- **R2 — nested flex recursion**: the inner solve runs as today and
  writes records; identity = the inner resolved container sizes (per
  the corpus: min == max == resolved content width). Pinned in T4.
- **R3 — the no-write pins' intent**: record writes are measurement
  outputs; §5's comment-only edit names the discipline explicitly.
- **R4 — lane high-water growth on pathological fan-out**: visible via
  the scenario pin's allocation window and the high-water accessor;
  the merge-sort fallback is designed, evidence-gated, never
  speculative.
- **R5 — test-migration drag**: if T5's re-express surface proves
  insufficient for a family, the family's fate is decided by the
  policy (die as bespoke-internal) rather than growing production
  surface for a test.
