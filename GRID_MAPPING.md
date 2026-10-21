# GRID_MAPPING.md — the grid admission map and design authority

Status: `DESIGN AUTHORITY — no new debate required`. Date: 2026-10-20.
Author: the architect. Corpus: the grid engine corpus
(`.tmp/engine-corpus/`, Blink `layout/grid/` at `61ef1bef`, Gecko
`nsGridContainerFrame.cpp` at `0fdb388b`, manifest-pinned), the
field/phase research corpus (session, four mapping tables, fetched
citations throughout), and the landed two-fold architecture.

**Why no new debate.** Grid's admission was decided by the debate's
required output (PLAN.md decision log, 2026-10-19: grid ADOPTED,
enters immediately after the flex fix sequence, before PR 4). The
process questions the flex debate settled apply unchanged: two folds,
zero-allocation steady state, no new primitives without measurement,
proof per landing, admission per D6 (named consumer need — the same
clients who could not use flexbox named grid). This document carries
the design, the slicing, and the one genuinely new decision (the
track-list representation, D2 below). The tie-break rule carries from
flex: WPT decides first, Blink second.

---

## 1. The complaint list (what we are admitting for)

The clients' sentence, grid half: dashboards and card walls that need
CSS Grid — the layouts they currently build with absolute positioning
(plain-Bevy behavior, defeating the library). The gate rows that define
the admitted surface (all currently drop-and-count, registered
unimplemented):

- `display: grid`/`inline-grid` (value gate),
- `grid-template-columns`, `grid-template-rows`, `grid-template-areas`,
  `grid-template`, `grid-auto-flow`, `grid-auto-columns`,
  `grid-auto-rows`, `grid-column`, `grid-row`, `grid-column-start`
  (+ all four longhands), `grid-area`, `grid` (shorthand),
  `justify-items`, `justify-self` (14 property gates).

Criticality per the client signal (and the research's tiering,
adopted): **card walls first** — `grid-template-columns` with `fr`,
fixed `repeat()`, `gap` (already admitted), default auto-placement;
**featured content second** — `grid-row`/`grid-column` `span N`;
**dashboard page shells third** — areas, `justify-items`/`justify-self`;
**auto-fill/auto-fit** as its own tier (the `repeat(auto-fit,
minmax(240px, 1fr))` idiom — popular but couples placement to
container size, so it lands deliberately later, not first).

## 2. The engine baseline (corpus-verified, mutually corroborating)

Both engines share one skeleton, and their two implementations of the
hard part (track sizing) agree: track sizing is init →
**resolve intrinsic track sizes** (span-grouped contribution phases) →
**maximize** (distribute free space to growth limits) → **expand
flexible tracks** (fr, `FindFrSize`/`FindUsedFlexFraction` — the SAME
algorithm in Blink `grid_track_sizing_algorithm.cc:1127-1316` and
Gecko `nsGridContainerFrame.cpp:7512-7588`) → **stretch auto**.
Placement is order-aware iteration → line resolution (spans, negative
lines, named lines) → auto-placement per §8.5 (definite →
locked-to-major → remaining) → per-item constraint space → align →
layout. Items are blockified and inline runs wrapped into anonymous
items at box/frame construction (both engines); abs-pos children are
excluded with static position resolved against the **grid area**
(§8.4), not the container.

Disagreements (tie-break fodder, none blocking): track storage
(run-length-encoded sets, Blink, vs fully expanded + `kMaxLine=10000`
clamp, Gecko — we take Gecko's expanded shape, §D1); auto-placement
occupancy structures (Blink's sorted-item-list+cursor+heap vs Gecko's
`CellMap` bitmap — we take Gecko's as the honest simple choice, §D4);
re-run discipline (Blink re-runs conditionally, Gecko always re-resolves
inline after block — we take Gecko's unconditional §12.1.1–12.1.3 for
the first admission, cheaper to verify); rounding (`LayoutUnit` vs
`NSToCoordRound` + remainder-to-leading — we take Gecko's remainder
rule, which matches our existing gap distribution's shape). Baselines,
subgrid, fragmentation, and gap-decoration are **deferred, named** —
consistent with flex's baseline gap.

## 3. What is reused verbatim (the outer third — already production)

- **`MeasurementRecord` bottom-up fold**: produces exactly the
  min/max-content and block-size-at-width contributions the track
  sizing algorithm consumes — replacing both engines' ad-hoc intrinsic
  caches (Blink's per-call measure spaces; Gecko's
  `CachedIntrinsicSizes`/`CachedBAxisMeasurement`).
- **The lane discipline** (`LayoutScratch`, per-container ranges,
  high-water-mark, update-scoped): grid adds a **track lane** (§D1)
  and an **occupancy structure** (§D4); the item and order lanes carry
  verbatim.
- **Anonymous inline-run formation** (`flex_anonymous_run.rs`): grid
  forms anonymous items identically (CSS Grid §4 + Display §2.7); the
  run fold, member metrics, whitespace drop, and exclusion predicate
  are display-family-agnostic. Only the item-view assembly is
  flex-flavored (grow/shrink/basis) — a new `GridItemView` reuses the
  mechanics.
- **Abs-pos exclusion + post-placement pass**: the predicate
  (`node_is_excluded_flex_child` — to be renamed display-neutral) and
  the placement pass carry; the static-position MODEL swaps from
  flex's 0×0 order-slot point to grid's area rectangle (§8.4: auto
  lines fall back to padding-box edges; `justify-self`/`align-self`
  align within it — both engines agree).
- **The admission pipeline**: registry + `DeclGrammar` +
  `property_shape` + shorthand expansion at parse + computed group
  file pattern + resolver wiring + dirty classification +
  `field_mapping`. Proven by box-sizing, and it is the reason the
  property-side of grid is mostly mechanical.
- **Auto-margin absorption concept**: per-item within its grid area at
  placement (same §8.1 semantics, different site from flex's line
  distribution).

## 4. What grid genuinely adds (cost order, with the designs)

1. **The track sizing algorithm (§12.3–12.7 + fr)** — the largest
   single new algorithm. Both engines' implementations are in-corpus
   and mutually corroborating (init → intrinsic resolution by span
   group → maximize → fr expand → stretch auto). Port Gecko's phase
   order (its `CalculateSizes` is one function, easier to verify
   phase-by-phase against Blink's).
2. **Auto-placement with 2-D occupancy (§8.5)** — no flex analog.
   Gecko's `CellMap` occupancy bitmap + per-row sparse cursors is the
   honest simple structure; dense mode pins cursors at 0,0.
3. **Line resolution** — spans (`span N`), explicit integer lines,
   negative-line translation (`1 − min(start)` — both engines agree),
   named lines and areas (T3 tier).
4. **The §12.1 axis re-run gate** — columns → rows → re-resolve
   columns after rows resolve (Gecko's unconditional 12.1.3 for the
   first admission; Blink's conditional re-run is the later
   optimization).
5. **The variable-length computed value** — decided at D2 below.
6. **Named deferrals**: baselines (consistent with flex's named gap),
   subgrid (tree-wide extension in both engines — out of first
   admission), auto-fill/auto-fit collapsed tracks (own tier, T3.5),
   fragmentation.

---

## D1. The track lane

A fifth lane in `LayoutScratch`, per-axis entries, Gecko's expanded
shape (Blink's RLE rejected — a memory optimization for
`repeat(10000,…)`, which our D2 capacity gate already excludes):

```rust
struct GridTrackEntry {
    base_px: f32,       // resolved base size after the current phase
    limit_px: f32,      // growth limit (minmax max, fit-content limit)
    state_bits: u8,     // per Gecko TrackSize::mState (auto-min, max-content,
                        // fr, frozen, fit-content-clamp, infinitely-growable)
    flex_factor: f32,   // fr value (0.0 = non-flexible)
    is_flexible: bool,
}
```

Tracks build from `ComputedGridGroup.template_columns/rows` at
container measure; per-container ranges claimed during the solve,
update-scoped, high-water-mark — identical lifecycle to the flex lanes.

## D2. The track-list representation (THE FORK — decided, then amended)

Track lists are the first variable-length computed values; the seven
groups are fixed-`Copy` (S1: inline, value-compared, zero-allocation).
The interned-text + lazy-parse model (transforms) fails: track lists
are read at measure, solve, AND place (repeated parsing or a per-pass
parse cache — both wrong).

**First decision (2026-10-20):** fixed-capacity `ArrayVec` inline in
`ComputedGridGroup`. **Amended the same day under the carriage
analysis:** inline is wrong twice over. (1) `DeclValue` is a small
`Copy` type closed over `{Length, Color, Keyword, Number, Integer,
Text}` — a ~144-byte `TrackList(ArrayVec)` variant would inflate every
`DeclBlock` in the system (declaration blocks are `SmallVec`-inline in
node records). (2) Two inline 12-capacity lists per group is a
per-node memory hit for EVERY node, grid or not. The S1-consistent
shape is the existing `DeclBlockKey` precedent (arena-keyed, `Copy`,
value-compared, no refcounting):

```rust
// In DeclStore (both arenas — context-shared stylesheet and RootDom inline):
track_lists: SlotMap<GridTrackListKey, ArrayVec<[GridTrackSize; 12]>>
// with value-dedup interning (shared templates intern to one entry),
// GridTrackListKey: Copy, u32, store-relative (like TextKey).
// DeclValue gains: TrackList(GridTrackListKey)  // 4 bytes, Copy
// ComputedGridGroup.template_columns/rows: GridTrackListKey
```

Capacity-12 stands unchanged: more than 12 expanded tracks drops at
admission with a counted report and a registered gate (never a silent
cap; the G rule). The fold's `reintern_text` path gains the
track-list reintern (stylesheet-arena keys re-intern to the inline
arena when folding into the group — the existing mechanism, extended
mechanically). Twelve covers the design-system standard; larger is a
named, D6-gated follow-up (an arena-page indirection, sketched, not
built).

```rust
struct GridTrackSize {            // Copy
    kind: GridTrackKind,          // Auto | Length(f32) | Percent(f32) | Fr(f32)
                                  // | MinContent | MaxContent | Minmax{min,max}
    min_px: f32, max_px: f32,     // minmax bounds (0.0 otherwise)
}
```

`grid-template-areas` strings stay interned text (`TextKey`, T3,
consumed once at placement, the font-family/transform model). Line
names inside track lists are deferred to the areas tier (T3).

`ComputedGridGroup` (non-inherited, new `PresentationGroup::Grid`):
`template_columns/rows: ArrayVec<[GridTrackSize; 12]>`,
`template_areas: Option<TextKey>`, `auto_flow: GridAutoFlowKw`,
`auto_columns/rows: GridTrackSize`, `justify_items: JustifyItemsKw`,
and item-side `justify_self: Option<…>`, `column_start/end`,
`row_start/end: GridLine` where `GridLine { Auto, Line(i32), Span(u32) }`
— all fixed-size, `Copy`, value-compared. The shorthands
(`grid-template`, `grid`) never reach the group: they expand at parse
per A1's single-expansion-point rule.

## D3. The solver (integration model: `relayout_active_grid_node`)

The flex integration shape, with grid's phases:

1. **Enter** (container main/cross probes, as flex).
2. **Item formation in the measure fold**: run aggregation (reused),
   exclusion predicate (reused), per-item `GridItemView` =
   {source, order, span classes per axis (auto/span/definite),
   justify_self, margins + auto flags, record-derived content sizes}.
   The view assembly is new; the mechanics are 3a's.
3. **Line resolution**: per item per axis, from the group →
   `LineRange` (definite start from explicit lines, or auto).
4. **Auto-placement (§8.5)**: order lane drives the order-modified
   sequence; the occupancy `CellMap` claims cells (definite items
   first, then locked-to-major, then remaining per auto-flow);
   `CellMap` is a context-scratch bitmap sized to
   `ceil(explicit + max-auto-tracks)` — high-water-mark, update-scoped.
5. **Track sizing** (D1's lane): init from the group → intrinsic
   resolution by span group (item records supply contributions) →
   maximize → fr expand (Gecko's `FindUsedFlexFraction`) → stretch
   auto; then rows, then re-resolve columns (§12.1.3 unconditional).
6. **Placement**: per-item area → child context carrying the resolved
   area rect (the flex placement pattern), alignment inside the area;
   the abs-pos pass with the area-rectangle static-position model.
7. **Exit**: container intrinsic contribution — a SIMPLIFIED track
   sizing under min/max-content constraints at the measure-fold exit
   (the `flex_container_intrinsic_inline_sizes` pattern; both engines
   do a full sizing per constraint — the simplified form is the named
   first approximation, pinned honestly).

## D4. Auto-placement occupancy

Gecko's `CellMap`: a bitmap of claimed cells, dimensions
`(explicit_tracks + max_auto_items) × explicit_tracks` clamped to a
named bound (the D2 capacity on explicit tracks; the auto dimension is
bounded by item count). Owned by the scratch (high-water-mark,
update-scoped). Cursors: one per row for sparse flow; dense pins the
cursor at the row's first free cell. The algorithm per §8.5 literally:
definite items → items locked to the major axis → remaining items in
order-modified sequence.

## D5. The admission slicing (each tier its own landing, full gate)

- **T0 — `display: grid`/`inline-grid` + the T1 property set
  parse-and-hold.** Value admission (keywords + `LayoutDisplay::Grid` +
  `LayoutSolverKind::Grid` + routing); `grid-template-columns`,
  `grid-template-rows`, `grid-auto-flow`, `grid-auto-columns`,
  `grid-auto-rows` admitted into the grammar and the group with
  resolvers and dirty classification. The display gate inverts to
  on-spec; the property gates for these names flip to admitted-and-held.
- **T1 — the card wall**: the track lane, track sizing (fixed tracks,
  `fr`, fixed `repeat()`, percent, `minmax`, `min`/`max-content`),
  auto-placement (row flow, sparse), placement + abs-pos model, the
  container intrinsic contribution. `gap` works (already admitted).
  Pin family: browser-derived card-wall scenarios (3-col fr wall,
  repeat fixed count, minmax, mixed track kinds, gaps, container
  intrinsic widths).
- **T2 — featured content**: `grid-column`/`grid-row` + the four
  longhands (`span N`, integer lines, negative-line translation),
  `grid-template-rows` track sizing, `grid-auto-flow` (column, dense),
  `justify-items`/`justify-self` (with the `place-items`/`place-self`
  expansion writes corrected to feed the grid group).
- **T3 — page shells**: `grid-template-areas` (interned strings),
  `grid-area`, named lines inside track lists. **T3.5 — auto-fill/
  auto-fit** as its own landing (repetition count couples to container
  size; collapsed tracks for auto-fit; placement re-runs on resize).
- **T4 — sugar**: `grid-template` and `grid` shorthands (expansion only).

**Per-tier proof obligations (the flex discipline carried):** every
tier flips its gate rows in the same change; pin families are
browser-oracle-derived with WPT/Gecko/Blink citations per the
per-claim provenance rule; perf pins re-pin only with a documented
cause; the >12-track capacity gate is pinned at T0 (drop + count + no
crash); subgrid drops with a counted report and a named gate row at
the tier it would otherwise parse (T2).

## D6. Kill-equivalents and honest risks

Nothing in the research says "stop" — the machinery is proven, the
engines agree on the hard part, and the intrinsic cache both engines
spend code on is already free (the record). The honest risks, named:
(1) **track sizing correctness** is the largest single algorithm; the
mitigation is that both engines corroborate in-corpus and the pin
families are browser-oracle, not self-derived. (2) **auto-placement
occupancy** is genuinely new structure — the risk is overbuilding;
Gecko's `CellMap` is deliberately the simple choice. (3) **the
variable-length value** is the S1 exception; the capacity gate must be
loud (counted, registered, and named in MIGRATION.md) or it reads as a
silent cap. (4) **areas/auto-fit** add placement-order complexity late;
their tiers are isolated so a deferral there never strands T1/T2.
