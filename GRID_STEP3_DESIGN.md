# GRID_STEP3_DESIGN.md — grid T2: spans, rows, auto-flow, justify, and the occupancy generalization (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-21. Author: the architect. Authority:
`GRID_MAPPING.md` (D3–D5), CSS Grid 1 §7.1 (`<grid-line>`), §8.5
(auto-placement), §10 (alignment), §12 (track sizing); the landed T1
solver (`grid.rs`).

This document decides everything for grid's T2: the four placement
longhands and their shorthands (`span N`, integer lines, negative-line
translation), explicit `grid-template-rows` sizing, `grid-auto-flow`
(row/column/dense), `justify-items`/`justify-self` (with the
`place-items`/`place-self` expansion corrections), and the `CellMap`
occupancy structure generalizing T1's row-major fill into §8.5's full
auto-placement. Anything unanswered is a document DEFECT — stop and
return to the architect.

---

## D0. T2 in one paragraph

Grid items gain explicit placement: `<grid-line>` longhands
(`auto | <nonzero-integer> | span <positive-integer>`) with the
shorthands expanding at parse, negative lines translating per both
engines (`explicit_count − k + 1`, i.e. `1 − min(start)` offset).
Row tracks size like column tracks (same phases, against the
container's height axis). Auto-placement generalizes to §8.5: definite
items first, then locked-to-one-axis, then remaining autos per
auto-flow (row/column, sparse/dense) over a `CellMap` occupancy
bitmap. `justify-items`/`justify-self` align items within cells on
the inline axis (the flex cross-axis machinery keeps the block axis),
and `place-items`/`place-self` expand to both sides. The §12.1
re-run is deferred, named (D7).

## D1. The placement property surface (exact)

### Grammar (`decl_parse.rs`)

- `grid-column-start`, `grid-column-end`, `grid-row-start`,
  `grid-row-end`: a single `<grid-line>`:
  `auto` → `GridLine::Auto`; `<integer>` (non-zero — zero drops +
  count; zero is not a line) → `GridLine::Line(i32)` (negatives
  admitted, translated at placement, D4); `span <integer>` (must be
  positive — 0 or negative drops + count) → `GridLine::Span(u32)`;
  bare `span` (no integer) → drop + count. Named lines (`<custom-ident>`
  with or without `span`) → drop + count, the T3 gate (named, unchanged
  from T1's line-name gate).
- `grid-column`, `grid-row` shorthands: `<grid-line> [ / <grid-line> ]?`
  — expand at parse (A1's single expansion point): one value that is
  `span N` → start `Auto`, end `Span(N)`; one value otherwise → start =
  the value, end `Auto`; two values → start/end in order. The
  `GridLine` longhands write `column_start/end` (or `row_start/end`)
  on the group's ITEM side (below).

### Group fields (item side, `ComputedGridGroup`)

`column_start: GridLine`, `column_end: GridLine`, `row_start: GridLine`,
`row_end: GridLine` — initial `Auto` (all four). `GridLine`
(`computed/grid_line.rs`, `Copy`): `{ Auto, Line(i32), Span(u32) }`.
Resolver + dirty classification: placement changes → layout+paint (the
model rows in `field_mapping.rs`).

### `justify-items` (container) / `justify-self` (item)

- `justify-items`: `normal | stretch | start | end | center` →
  `JustifyItemsKw { Normal, Stretch, Start, End, Center }` on the
  group's CONTAINER side (initial `Normal`).
- `justify-self`: `auto | normal | stretch | start | end | center` →
  `JustifySelfKw` on the group's ITEM side (initial `Auto`).
- `place-items` and `place-self` shorthands: expand to BOTH sides —
  `place-items` writes `align-items` (existing `AlignItemsKw`) AND
  `justify-items` (new); `place-self` writes `align-self` AND
  `justify-self`. The current expansions write only the align side
  (research-noted gap) — corrected in this landing, with a pin that a
  grid container's `place-items: center` centers on the inline axis.

### Still-gated (unchanged counted drops)

`grid-template-areas`, `grid-area`, the `grid`/`grid-template`
shorthands, named lines, `repeat(auto-fill|auto-fit, …)`, and
`subgrid` (value). Their gate pins must not move.

## D2. `GridLine` resolution and the negative-line translation

At placement, per axis, an item's definite `Line(k)` resolves:
`k > 0` → track index `k − 1`; `k < 0` → `explicit_count + k + 1`
(so `Line(-1)` = the last explicit line's far edge... precisely, the
line INDEX: negative lines count from the end of the explicit grid,
`Line(-1) → explicit_count`, `Line(-2) → explicit_count − 1`; both
engines' `1 − min(start)` offset is the same arithmetic applied to the
most-negative definite start in the container, NOT per item — compute
the container's `explicit_origin_offset = 1 − min(all definite starts
in the container, 1)` ONCE per container and add it to every negative
`Line(k)` (both engines: Blink `column_start_offset/row_start_offset`
from most negative definite start; Gecko `mExplicitGridOffsetCol/Row =
1 − min`). **Correction (2026-10-21, the coverage probe's finding):**
the offset applies to the 1-based line numbers, and the number of
left-implicit tracks created equals the offset — e.g.
`grid-column: -5` in a 2-track grid resolves to 0-based line −2 →
offset `1 − min(−1, 1) = 2` → the item lands in 0-based track 0, the
leftmost of TWO left-implicit tracks (an earlier draft of this note
mis-computed the outcome as one implicit track and the item landing in
the second explicit track; the implementation and both engines agree —
the pin scenario is `grid-template-columns: 100px 100px;
grid-auto-columns: 100px`, definite `-5 / span 1` → left 0, first auto
→ left 100 (second left-implicit track), second auto → left 200 (first
explicit track)). For `Span(n)` with a definite `Line(k)` on the other side,
the span resolves from the resolved line. For `Span(n)` with `Auto` on
the other side, the span is placed by the auto-placement with length n.

## D3. Row track sizing

Explicit row tracks (`grid-template-rows`) size by the SAME phase
algorithm as columns (init → intrinsic resolution → maximize → fr
expand (with the per-restart cap) → stretch auto), against the
container's definite height when definite; when indefinite, rows size
to content (the T1 implicit-row rule, and `grid-auto-rows` for any
implicit rows beyond the explicit count). Item contributions to a
row's intrinsic tracks: the item's content cross (`content_height_at_width`
after any width-dependent refold).

## D4. The `CellMap` occupancy and §8.5 auto-placement

**`CellMap`** (scratch-owned, high-water-mark, update-scoped): a
row-major bitmap of claimed cells, dimensions `explicit_rows + rows_added
× explicit_columns` (rows grow as needed — appending zeroed rows to a
`Vec<bool>`-like lane is the one allowed resize, amortized into
high-water mark; a `u64`-bitset-per-row lane (`grid_cell_map.rs`) keeps
it allocation-free after warmup). `CellMap::claim(row, col, row_span,
col_span)` and `CellMap::is_free(row, col, row_span, col_span)`.

**§8.5, exact order (order lane = order-modified sequence):**
1. **Both-axes definite** (definite `Line` or `Span`+`Line` on both
   axes): resolve their cells (D2), claim.
2. **Locked-to-major-axis** (definite in exactly one axis): for each
   in order, scan the OTHER axis from the start of the definite axis's
   line for the first free span; claim.
3. **Remaining autos**: per `auto_flow`:
   - **Row sparse**: cursor `(0, 0)`; for each item in order: advance
     the cursor to the first free cell fitting its spans (scan
     row-major); claim and continue from that cell.
   - **Row dense**: after each placement, reset the cursor to the
     first free cell in the grid (row-major scan from `(0, 0)`).
   - **Column / column dense**: the column-major mirrors.
4. Items with `Span(n)` (and Auto on the other axis) participate in
   the auto-placement with their span.

The T1 row-major fill is exactly step 3-row-sparse for all-`Auto`,
span-1 items — state the reduction in the code comment.

## D5. `justify-items`/`justify-self` at placement

Per item per its cell: the inline-axis (horizontal in our world)
alignment inside the cell's track width, per `justify-self` (falling
back to the container's `justify-items`, falling back to `Normal`):

- `Normal`/`Stretch`: stretch to the track width (today's T1 default;
  definite-size and replaced-ratio exemptions unchanged).
- `Start`: the item's own width (content or definite), at the cell's
  inline-start edge.
- `End`: at the inline-end edge.
- `Center`: centered in the cell.
- Absorbed auto margins continue to take precedence (§8.1) over
  justify-* exactly as they did over stretch.

The BLOCK axis (row height) keeps the existing cross-axis machinery
(`align-items`/`align-self` — the flex group's alignment vocabulary
applies to grid items' block axis per CSS Grid §10.2 — implement via
the same resolved-alignment mapping flex uses; pin that
`align-items: end` on a grid container bottom-aligns items in their
rows).

## D6. The pin table (exact)

| Pin | Asserted |
|---|---|
| `grid_column_span_places_across_two_tracks` | `grid-column: span 2` → the item occupies two consecutive columns, sized by both tracks + the gap |
| `grid_row_span_places_across_two_rows` | `grid-row: span 2` |
| `grid_column_start_end_integer_lines` | `grid-column: 2 / 4` → item in columns 2–3 (line indices), auto items flow around it |
| `grid_negative_lines_translate_from_the_explicit_end` | `grid-column: -1 / -3` in a 3-track grid → resolves to lines 4/2 (spanning tracks 2–3), with the container-wide `1 − min(start)` offset |
| `grid_span_zero_and_span_negative_drop` | `span 0`, `span -1` → drop + count (admission gate) |
| `grid_line_zero_drops` | `grid-column-start: 0` → drop + count |
| `grid_named_lines_still_drop` | `[main]`, `span main` → drop + count (T3 gate, unchanged) |
| `grid_auto_flow_column_fills_column_major` | `grid-auto-flow: column` → items fill down column 1 before column 2 |
| `grid_auto_flow_dense_backfills_holes` | a definite item at (0,1) then autos: sparse leaves (0,0) empty; `dense` fills it with the next auto item |
| `grid_template_rows_fr_and_fixed` | `grid-template-rows: 40px 1fr 2fr` against a definite container height |
| `grid_template_rows_minmax_and_content` | row `minmax(60px, max-content)` with content |
| `grid_auto_rows_beyond_explicit_count` | items past the explicit rows → `grid-auto-rows` sizing |
| `grid_justify_items_start_end_center_in_cell` | container `justify-items: center` centers a smaller item in its track; `start`/`end` edges |
| `grid_justify_self_overrides_container` | one item `justify-self: end` against container `justify-items: center` |
| `grid_align_items_block_axis_in_rows` | container `align-items: end` bottom-aligns items in their rows |
| `grid_place_items_expands_to_justify_items` | `place-items: center` on a grid container → inline-axis centering (the expansion correction) |
| `grid_place_self_expands_to_justify_self` | item `place-self: center` → both axes centered |
| `grid_auto_margins_still_beat_justify` | `margin: auto` centers in the cell regardless of `justify-items` |
| `grid_definite_then_locked_then_auto_order` | a definite item, a one-axis-locked item, and autos resolve in §8.5's order |
| `grid_rerun_deferred_named_boundary` | the D7 deferral row |

## D7. The §12.1 re-run deferral (named)

The §12.1.3 re-resolve (re-run column sizing after row sizing when
items' resolved block sizes could change intrinsic column
contributions) is DEFERRED with a named boundary row (class
unimplemented): it first matters for content-based column tracks whose
items' intrinsic contributions depend on resolved block size
(block-size-dependent min/max content — e.g. percentage-block items or
aspect-ratio-driven cases). T2's admitted features do not require it;
the row names the deferral and its triggering conditions so the first
admission that needs it (likely T3 areas or a consumer-named case)
must implement it. Registered in `layout_proofs/README.md`.

## D8. Explicitly untouched (named)

Areas/`grid-area`/named lines (T3), auto-fill/fit (T3.5), the
shorthands (T4), subgrid (gated), baselines (deferred), the T1 solver
phases, the run fold, the abs-pos pattern (grid area from definite
lines → area edges refines the T1 padding-box model for items with
definite placement in both axes — implement it as part of D4's abs-pos
pass: area-rectangle static position per CSS Grid §8.4 when both axes
are definite, padding-box otherwise).

## D9. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/style/computed/grid_line.rs` (NEW) | `GridLine` |
| `crates/ui-dom/src/domains/style/computed/justify_items_kw.rs` (NEW) | the enum |
| `crates/ui-dom/src/domains/style/computed/justify_self_kw.rs` (NEW) | the enum |
| `crates/ui-dom/src/domains/style/computed/computed_grid_group.rs` | the item/container fields |
| `crates/ui-dom/src/domains/style/computed/mod.rs`, `style/mod.rs` | re-exports (mechanical) |
| `crates/ui-dom/src/domains/style/presentation_property.rs` | the placement + justify METADATA rows |
| `crates/ui-dom/src/domains/style/decl_parse.rs` | the `<grid-line>` grammar, the shorthands, the justify keywords, the `place-*` expansion corrections |
| `crates/ui-dom/src/domains/style/decl_value.rs` | the `DeclValue::GridLine(GridLine)` variant + the exhaustive formatter/dom-value arms (added after the transcription's STOP, 2026-10-21 — mechanical pipeline) |
| `crates/ui-dom/src/types/root_dom.rs` | arms for the six new property variants in `decl_value_for_groups` (transition-overlay read path — step-interpolation default like every non-animatable property) and for `DeclGrammar::GridLine` in `decl_value_matches_grammar` (same correction) |
| `crates/ui-dom/src/types/dom_node.rs` | the `presented_px` arm for the new `DeclValue::GridLine` variant (same correction) |
| `crates/ui-dom/src/types/layout_scratch.rs` | the `CellMap`'s home (scratch-owned, high-water-mark, update-scoped — the only allocation-free home; same correction, and the reason `GRID_STEP2_DESIGN.md` listed this file for the track lane) |
| `crates/ui-dom/src/domains/style/project.rs` | the group resolver for the new fields |
| `crates/ui-dom/src/domains/style/field_mapping.rs` | dirty classification |
| `crates/ui-dom/src/domains/layout/grid_cell_map.rs` (NEW) | the `CellMap` |
| `crates/ui-dom/src/domains/layout/grid.rs` | D2 resolution, D3 row sizing, D4 placement, D5 justify, the D8 area-refined abs-pos |
| `crates/ui-dom/src/domains/layout/grid_item_view.rs` (if it exists; else `grid.rs`) | the view's span fields |
| `crates/ui-dom/tests/layout_proofs/layout/grid_layout.rs` | the D6 family |
| `crates/ui-dom/tests/layout_proofs/style/grid_admission.rs` | the admission gates (span/line-zero guards, named-line gate) |
| `crates/ui-dom/tests/layout_proofs/README.md` | the gate rows (span/zero guards, the D7 deferral, the area gate updates) |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D10. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
pin moves except the D6-specified additions (the T1 family and the
still-gated pins must NOT move); (3) a perf pin moves without a
documented cause; (4) the `CellMap` or any structure allocates in
steady state (warmup high-water only). ONE commit, full gate green.
