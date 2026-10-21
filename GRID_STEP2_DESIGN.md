# GRID_STEP2_DESIGN.md — grid T1: the card-wall solver (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-20. Author: the architect. Authority:
`GRID_MAPPING.md` (D1–D6), CSS Grid 1 §5/§7/§8.5/§12 (track sizing),
the corroborated engine phases (Blink `grid_track_sizing_algorithm.cc:322-364`,
Gecko `CalculateSizes` `nsGridContainerFrame.cpp:6253-6286`, fr
`FindUsedFlexFraction:7512-7588`), the landed two-fold machinery.

This document decides everything for grid's T1: the card wall —
fixed tracks, `fr`, fixed `repeat()`, percent, `minmax`,
`min`/`max-content`, `gap`, default auto-placement (row-major,
all-auto), stretch-to-cell, the abs-pos model, and the container
intrinsic contribution. T1 admits NO placement properties
(`grid-column`/`grid-row` are T2); every in-flow item is auto-placed.

Anything unanswered is a document DEFECT — stop and return to the
architect.

---

## D0. T1 in one paragraph

`relayout_active_grid_node` replaces T0's block interim with the real
solver: children form grid items via the 3a mechanics (run aggregation,
exclusion, blockification); the explicit column tracks size per the
engines' phase order (init → intrinsic resolution → maximize → fr
expand → stretch auto); items fill row-major into the explicit columns
(order-modified, wrapping to implicit rows); implicit rows size to
content (the record, with the refold for width-dependent items); items
stretch to their cell by default (the grid default alignment); abs-pos
children get the T0-interim static-position model; and the container's
intrinsic sizes derive from the solved tracks. The D5 interim row moves
on-spec.

## D1. `relayout_active_grid_node` (the integration shape)

The `relayout_active_flex_node` structure, grid-flavored:
1. **Enter**: container main/cross probes (block-level `Grid`:
   containing-block width; `InlineGrid`: shrink-wrap — deferred to a
   named T2+ check, with a drop-or-block fallback? NO: InlineGrid is
   admitted at T0; its T1 sizing = shrink-to-fit per CSS 2.1 §10.3.2's
   inline-block rule — implement it, don't defer).
2. **Item formation** (measure fold): the 3a mechanics verbatim —
   run aggregation (runs are grid items), exclusion predicate
   (abs/fixed/none → the abs-pos pass), blockification. `GridItemView`
   (T1): `{source: FlexItemSource (reused — it is display-neutral in
   shape; rename later, not now), order: i32, content_main_px: f32,
   content_cross_px: f32, h_min_content_px: f32, h_max_content_px: f32,
   margins: [f32; 4] + auto flags: u8, replaced_ratio: bool,
   main_influences_cross: bool}` — content sizes read from the
   `MeasurementRecord` like the flex view (the D2a §9.9 intrinsic fold
   for nested containers applies to grid containers identically —
   verify with a nested-grid pin).
3. **Column track sizing** (D2's algorithm below).
4. **Row-major placement** (D3).
5. **Row sizing** (D4).
6. **Placement** (D5) + **abs-pos pass** (D6).
7. **Exit**: container intrinsic contribution (D7) + the record write
   (the usual `exit_node` epilogue).

## D2. Column track sizing (T1 scope)

Per explicit track from the interned list (`repeat()` already expanded
at parse). Inputs per track kind:

| Kind | Base | Limit | Growable |
|---|---|---|---|
| `Length(px)` | px | px | no |
| `Percent(p)` | p × container content width | same | no |
| `MinContent` | max over its column's items of `h_min` | same | no |
| `MaxContent` | max over its column's items of `h_max` | same | no |
| `Minmax(min, max)` | resolve(min) | resolve(max) | yes, to limit |
| `Auto` | content contribution (`h_max`) | — | yes |
| `Fr(f)` | content contribution (`h_max`) | — | fr factor f |

(`resolve()` for Percent uses container content width; minmax
percentages likewise. An item's "column" at this point is its
auto-placed cell's column — but placement needs sizes first. Resolve
the chicken-and-egg per the engines: track sizing consumes item
contributions BEFORE placement via the span model — for T1's
all-auto-row-major fill, an item's column is `index % column_count`,
computable without placement. Order lane indices map to cells by that
formula, so contributions group by `index % column_count`.)

**Phases (per the engines):**
1. **Init** per the table above.
2. **Intrinsic resolution**: for `MinContent`/`MaxContent`/`Auto`/
   `Fr` tracks, base = the contribution from the table (max over the
   track's items of the relevant record field).
3. **Maximize**: `free = container_content_width − Σ base − gap ×
   (n−1)`; if `free > 0`, distribute to growable tracks up to their
   limits in track order (each takes `min(limit − base, remaining)`;
   remaining -= taken).
4. **Fr expand** (Gecko's `FindUsedFlexFraction`): if any `Fr` track,
   `fraction = remaining_free / Σ fr_factors` (clamped ≥ 0); each fr
   track's size = `max(base, factor × fraction)`. If `Σ factors < 1`,
   the free space is scaled by the sum first (the §7.2 rule — same as
   flex's grow-sum<1 cap).
5. **Stretch auto**: `Auto` tracks absorb any remaining free space
   equally (after fr).
Zero-size edge: `Σ base + gaps > container` → tracks overflow at their
bases (no negative sizing; `Fr` gets its base, maximize/stretch skip).

## D3. Row-major auto-placement (T1)

All items are auto-placed (no placement properties in T1). Iterate the
**order lane** (the order property applies to grid auto-placement,
CSS Grid §5.4): cell for sequence index `i` = `(row = i / col_count,
col = i % col_count)`, where `col_count = explicit track count` (or 1
for the empty template). Implicit rows are created as needed. This is
exactly §8.5's auto-placement reduced to the all-auto, row-flow,
span-1 case — state that equivalence in the code comment (the T2
`CellMap` generalizes it; do NOT build the `CellMap` in T1).

## D4. Row sizing (implicit rows, T1)

Each implicit row's height = max over its items' placed cross sizes,
where a flagged `main_influences_cross` item whose resolved width
differs from its measured width refolds at its cell's track width
(the flex refold mechanism, reused: re-measure the subtree at the new
width; `FlexNodesRefolded` counts it — rename the counter
`LayoutNodesRefolded`? NO — keep `FlexNodesRefolded` for flex, add
`GridNodesRefolded` if the sites differ; decide: ONE counter
`NodesRefolded` shared, renaming the flex one in the same change with a
documented re-pin — cleaner than two counters for one mechanism).
`AutoRows` (if not `Auto`): implicit rows size per that track size's
rules (resolve `Length`/`Percent`/minmax/`Fr` against the container's
height if definite, else content).

## D5. Placement (stretch-to-cell default)

Per item, its cell rect: `left = container_content_left + Σ
track[0..col].size + gap × col`; `top = container_content_top + Σ
row[0..row].height + gap × row`; width = track size; height = row
height. **Grid default alignment (CSS Grid §6.1/§7.1): `normal`, which
behaves as `stretch` for items with auto size in the axis** — the item
stretches to the cell's track width (and row height for the cross),
UNLESS: definite size (used as-is, start-aligned), or replaced-ratio
(the flex stretch-img exemption carries: ratio-derived cross from the
track width, not stretched). Absorbed auto margins: per-item within its
cell (§8.1, the flex placement's absorbed-start mechanism applies per
axis — margins absorb the cell's free space; the cell's track size is
the constraint).

## D6. The abs-pos pass (T1)

The flex abs-pos pattern with the T1 static-position model: excluded
(abs/fixed) children get their containing block = the grid container's
padding box (auto lines fall back to padding-box edges, CSS Grid §8.4),
positioned per CSS 2.1 §10.3.7/§10.6.4 against it (the existing
abs-pos machinery, the D6-step-3b shape). The grid-AREA refinement
(definite lines → area edges) is T2's.

## D7. The container intrinsic contribution

The grid container's record (`h_min`/`h_max` for its parent's
consumption — the D2a §9.9 fold for nested containers):
- `h_min` = Σ per-track min-contribution + `gap × (n−1)`, where
  fixed→their px, `MinContent`/`Minmax(min,…)` → their min side,
  `Auto`/`Fr` → max over their items' `h_min`, `MaxContent` → `h_max`,
  `Percent` → their resolved px; for the empty template, the items'
  max `h_min`.
- `h_max` = the same with max sides (`h_max` contributions,
  `Minmax(…,max)` max side); empty template → items' max `h_max`.
Rows/cross: the container's cross = Σ row heights + gaps × (rows−1)
(computed post-placement; `content_height_at_width_px`).
Pin the nested-grid case: a grid container as a flex item and as a
block child, both contributions correct.

## D8. The pin table (exact)

| Pin | Asserted |
|---|---|
| `grid_card_wall_three_fr_tracks` | `repeat(3, 1fr)` in a 300px container, 6 items → 2 rows × 3 columns, each cell 100px wide (gaps 0), items stretched to cells; row heights by content |
| `grid_fixed_and_fr_track_mix` | `100px 1fr 2fr` in 300px → 100/66.67/133.33 (fr distribution after the fixed base) |
| `grid_minmax_track_clamps` | `minmax(100px, 200px) auto` → base 100, grows to 200 with content/free space, clamps at 200 |
| `grid_min_max_content_tracks` | `min-content max-content 20%` → track sizes from item contributions (records) and the percent track |
| `grid_fr_sum_below_one_scales_free_space` | `0.5fr 0.5fr` → the grow-sum<1 cap (the flex analog) |
| `grid_track_overflow_uses_bases` | fixed bases exceeding the container → bases hold, fr gets its base |
| `grid_gap_separates_tracks_and_rows` | `column-gap: 10px`/`row-gap: 10px` in the fr wall |
| `grid_auto_rows_size_to_content_and_specified_track_size` | implicit row heights: content-sized; and `grid-auto-rows: minmax(80px, auto)` |
| `grid_order_property_reorders_auto_placement` | `order: 1` on one card → it places last in the row-major fill |
| `grid_item_stretches_to_its_cell_by_default` | the stretch default; a definite-size item and a replaced-ratio img are NOT stretched (img derives cross from ratio) |
| `grid_item_auto_margins_absorb_cell_free_space` | `margin: auto` on a card → centered in its cell (main and cross) |
| `grid_item_margins_contained_at_the_boundary` | the T0 FC pin holds under the real solver |
| `grid_abspos_child_static_position_uses_padding_box` | the T1 abs-pos model |
| `grid_container_intrinsic_sizes_derive_from_tracks` | nested grid in a block parent and in a flex row: h_min/h_max per D7 |
| `grid_inline_grid_shrink_wraps` | `inline-grid` sizes to its tracks' total width |
| `grid_t0_block_interim_is_replaced` | the D5 interim scenario now produces the card wall (the row moves on-spec) |
| `grid_empty_template_is_one_auto_column` | no template → one auto column, cards stack |

Plus the perf pin (new scenario family in `perf_metric.rs` and
`perf_allocation.rs`): the 3-fr card wall with a text mutation per
update — assert `GridNodesMeasured`/`GridNodesPlaced` (new counters,
four-site change like the flex ones) and steady-state zero allocation
beyond arena interning (lanes/scratch at high-water mark). The counters:
`GridNodesMeasured`, `GridNodesPlaced`, and rename `FlexNodesRefolded`
→ `NodesRefolded` shared (documented re-pin).

## D9. Explicitly untouched (named)

`grid-column`/`grid-row` and all placement longhands (T2), areas (T3),
auto-fill/fit (T3.5), the `CellMap` (T2), subgrid (gated), baselines
(deferred), the §12.1 column re-resolve (T2's concern; T1 names it).
The flex solver, the run fold, the abs-pos predicate.

## D10. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/grid.rs` (NEW) | the solver (`relayout_active_grid_node`, `GridItemView`, track sizing, placement) |
| `crates/ui-dom/src/domains/layout/grid_track_lane.rs` (NEW) | the track lane (D1 of GRID_MAPPING) |
| `crates/ui-dom/src/types/layout_scratch.rs` | the fifth lane |
| `crates/ui-dom/src/domains/layout/flow.rs` | route `LayoutSolverKind::Grid` to the real solver (the D5 interim dies) |
| `crates/ui-dom/src/types/root_dom_metric_name.rs` | the grid counters + the refold rename |
| `crates/ui-dom/tests/layout_proofs/layout/grid_layout.rs` (NEW) | the D8 family |
| `crates/ui-dom/tests/layout_proofs/layout/mod.rs` | the module |
| `crates/ui-dom/tests/perf_metric.rs`, `tests/perf_allocation.rs` | the grid scenario pin |
| `crates/ui-dom/tests/layout_proofs/README.md` | the D5 interim row moves on-spec; the coverage map gains the grid rows |

## D11. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
pin moves except the D5 interim row's on-spec move and the
`FlexNodesRefolded` rename's documented re-pin; (3) a perf pin moves
without a documented cause; (4) the T1 solver allocates beyond
lanes/scratch high-water marks and once-per-new-list arena interning
(steady state must be zero). ONE commit, full gate green.
