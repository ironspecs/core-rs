# GRID_STEP1_DESIGN.md — grid T0: display routing + property admission (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-20. Author: the architect. Authority:
`GRID_MAPPING.md` (esp. amended D2 — arena-keyed track lists), CSS
Grid 1 §4/§5/§7, CSS Display 3 §2.7; the admission pipeline precedent
(PR 1 item 3, box-sizing).

This document decides everything for grid's T0 landing: `display:
grid`/`inline-grid` routing, and the T1 property set
(`grid-template-columns`, `grid-template-rows`, `grid-auto-flow`,
`grid-auto-columns`, `grid-auto-rows`) admitted parse-and-hold. The
T1 solver (card wall) is `GRID_STEP2_DESIGN.md` (next).

Anything unanswered is a document DEFECT — stop and return to the
architect.

---

## D0. T0 in one paragraph

Two new display values route to a new solver kind (which T1's landing
will implement; T0 routes to a fail-closed-but-not-crashing placeholder
that treats the grid container as a block container — see D5's honest
interim), the T1 property set parses into typed values held on a new
`ComputedGridGroup`, and every other grid name keeps dropping with
counted reports per its tier. This is the box-sizing admission shape
(PR 1 item 3) scaled to a display type: value first, property set
parse-and-hold, solver next.

## D1. The display admission surface (exact)

- `DISPLAY_KEYWORDS` (`presentation_property.rs:180-187`): add `"grid"`
  and `"inline-grid"`.
- `Keyword` (`keyword.rs`): add `Grid`, `InlineGrid` (display section).
- `DisplayKw` (`display_kw.rs:20-56`): add `Grid`, `InlineGrid` →
  `LayoutDisplay::Grid`, `LayoutDisplay::InlineGrid`
  (`layout/style.rs:47-55`, `380-389`).
- `LayoutSolverKind` (`flow.rs:71-76`): add `Grid`;
  `solver_kind_for_display` (`flow.rs:1948-1955`): `Grid | InlineGrid
  → Grid`. T0's interim solver arm: `relayout_grid_node` — see D5.
- Inline-outer handling (`flow.rs:1936-1941`): `InlineGrid` is
  inline-outer (like `InlineFlex`); `Grid` is block-outer.
- Blockification generalization (CSS Display 3 §2.7 — grid items
  blockify identically to flex items): `blockified_flex_item_style`
  (flex.rs) is renamed/generalized to `blockified_item_style` and used
  by both item paths; the run-membership and exclusion predicates in
  `flex_anonymous_run.rs` are already display-neutral in behavior
  (rename only if the lint demands; behavior unchanged).
- **The formatting-context flag generalization (CSS Grid 1 §4: a grid
  item establishes a new formatting context for its contents — same
  rule as flex items):** the per-parentage flag in
  `child_layout_context` (step 9's mechanism) computes
  `allows_parent_child_block_margin_collapse = false` when the parent
  is a flex OR grid container: `parent_is_flex_container(entered) ||
  parent_is_grid_container(entered)` (a new small helper beside
  `parent_is_flex_container`, `flow.rs:920-925`). Pin: a first-child
  margin inside a grid item is contained (the step-9 rule) — land with
  T0 since the flag exists before the solver.

## D2. The property admission surface (exact)

### The group

New `PresentationGroup::Grid` + `ComputedGridGroup`
(`domains/style/computed/computed_grid_group.rs`, the
`computed_flex_group.rs` shape): all `Copy`, `PartialEq`, with
`initial()`:

| Field | Type | Initial |
|---|---|---|
| `template_columns` | `GridTrackListKey` | the empty list's interned key |
| `template_rows` | `GridTrackListKey` | the empty list's interned key |
| `auto_flow` | `GridAutoFlowKw { Row, RowDense, Column, ColumnDense }` | `Row` |
| `auto_columns` | `GridTrackSize` | `Auto` |
| `auto_rows` | `GridTrackSize` | `Auto` |

`GridTrackSize` (`computed/grid_track_size.rs`, `Copy`):
`{ kind: GridTrackKind, min_px: f32, max_px: f32 }` with
`GridTrackKind { Auto, Length, Percent, MinContent, MaxContent, Minmax,
Fr(f32) }` — Length/Percent carry their value in `min_px`
(Minmax uses both bounds).

Resolver wiring: `project.rs`'s group resolver maps the group's
fields from `DeclValue` (the `TrackList` variant passes through with
the reintern step, keyword variants map to the Kw enums); dirty
classification (`field_mapping.rs`): template/auto_flow changes →
layout+paint (the flex-group classes are the model).

### The grammar (`decl_parse.rs`)

- `grid-template-columns` / `grid-template-rows`: the track-list
  grammar — space-separated `<track-size>` tokens, each
  `<length> | <percentage> | <fr> | min-content | max-content | auto |
  minmax(<length-percentage>, <length-percentage|min-content|
  max-content>)`, plus `repeat(<positive-integer>, <track-list>)`
  which EXPANDS AT PARSE (the A1 single-expansion-point rule) into N
  copies. Total expanded count > 12 → the declaration drops with a
  counted report (the capacity gate, registered). The value interns
  into the arena as `ArrayVec<[GridTrackSize; 12]>`;
  `DeclValue::TrackList(key)`. A line-name token `[` → the declaration
  drops with a counted report and a named gate row (named lines are T3;
  never silent).
- `grid-auto-flow`: `[row | column] && dense?` — one or two tokens;
  `dense` is a new `Keyword::Dense` (add it). Maps to `GridAutoFlowKw`.
- `grid-auto-columns` / `grid-auto-rows`: a single `<track-size>`
  (same leaf).
- **The arena** (`decl_store.rs`): `track_lists:
  SlotMap<GridTrackListKey, ArrayVec<[GridTrackSize; 12]>>` with
  `intern_track_list(&[GridTrackSize]) -> GridTrackListKey` doing
  value-dedup interning (linear scan on content equality — lists are
  short and few; the text interner's shape). Both DeclStore instances
  get it (stylesheet arena + inline arena); the fold's `reintern_text`
  gains a `TrackList` arm that re-interns the list into the inline
  arena.

### Still-gated (counted drops, unchanged behavior, named per tier)

`grid-template-areas`, `grid-template`, `grid`, `grid-column`,
`grid-row`, `grid-column-start`, `grid-column-end`, `grid-row-start`,
`grid-row-end`, `grid-area`, `justify-items`, `justify-self`,
`subgrid` (value of template-columns/rows), and `[` line-name tokens.
The existing gate pins for these names must keep passing UNCHANGED.

## D3. The `repeat()` expansion rules (exact)

`repeat(N, <track-list>)`: N is a positive integer (0 or negative →
drop + count; non-integer → drop + count). Expansion is literal
repetition at parse. `repeat(auto-fill|auto-fit, ...)` → drops with a
counted report and its own named gate row (the T3.5 tier, per
GRID_MAPPING D5). Nested `repeat()` inside `repeat()` → drops (CSS
forbids it; counted report).

## D4. The capacity gate (exact)

Expanded track count > 12 → drop with a counted report (`DeclDropReport`
/`StylesheetLoadReport` per the G rule), no crash, the computed field
stays at its empty-list initial. Registered as a named gate row
(`unsupported_grid_track_list_over_capacity_drops`, class
unimplemented, the >12 follow-up named).

## D5. The T0 interim solver arm (honest, bounded)

`relayout_grid_node` at T0: the grid container lays out as a BLOCK
container (children in normal block flow, grid properties inert but
held). This is the fail-closed interim that keeps T0's scope honest
(routing + admission without a solver) and is REPLACED by T1's solver
in the same arc — registered as a named interim row (class
unimplemented, "grid containers currently lay out as block containers;
the T1 solver replaces this"). NOT a divergence: a named, gated
interim with its own row and a pin (grid container with
template-columns lays out block-flow at T0).

## D6. The pin table (exact)

| Pin | Asserted |
|---|---|
| `display_grid_and_inline_grid_are_admitted` | both keywords parse, `LayoutDisplay::Grid`/`InlineGrid`, `LayoutSolverKind::Grid` routing; the old display drop pin's grid/inline-grid cases inverted (the other values keep dropping) |
| `grid_template_columns_parse_to_typed_track_lists` | `repeat(3, 1fr)`, `100px 1fr 2fr`, `minmax(100px, 200px) auto`, `min-content max-content 20%` → exact `GridTrackSize` sequences in the arena; group holds the interned key; two identical templates intern to the SAME key (dedup) |
| `grid_template_rows_parse_to_typed_track_lists` | same leaf coverage |
| `grid_auto_flow_parses_row_column_dense` | `row`, `column`, `row dense`, `column dense` → the four `GridAutoFlowKw` |
| `grid_auto_columns_and_rows_parse_single_track_size` | `auto`, `1fr`, `minmax(80px, max-content)` |
| `grid_track_list_over_capacity_drops` | 13 tracks and `repeat(13, 1fr)` → drop + count, group initial, no crash |
| `grid_line_name_tokens_drop_for_now` | `[main] 1fr [aside]` → drop + count, named gate |
| `grid_auto_fill_and_auto_fit_repeat_drops_for_now` | both → drop + count, named T3.5 gate |
| `grid_item_margins_are_contained_at_the_boundary` | a first-child `margin-top: 20px` inside a grid container's child: contained per step 9's rule (the FC flag generalization) |
| `grid_container_at_t0_lays_out_as_block_container` | the D5 interim pin (named, with its registry row) |
| `grid_shorthand_and_area_names_still_drop` | `grid-template-areas`, `grid`, `grid-column`, `grid-row`, `grid-area`, `justify-items`, `justify-self`, `subgrid` → drop + count (unchanged) |

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/style/keyword.rs` | `Grid`, `InlineGrid`, `Dense` |
| `crates/ui-dom/src/domains/style/presentation_property.rs` | the display keywords, the group variant, the T1 set's METADATA rows |
| `crates/ui-dom/src/domains/style/computed/display_kw.rs` | the two new variants |
| `crates/ui-dom/src/domains/style/computed/computed_grid_group.rs` (NEW) | the group |
| `crates/ui-dom/src/domains/style/computed/grid_track_size.rs` (NEW) | `GridTrackSize` + `GridTrackKind` |
| `crates/ui-dom/src/domains/style/computed/grid_auto_flow_kw.rs` (NEW) | the enum |
| `crates/ui-dom/src/domains/style/computed/mod.rs` | declare + re-export the three new modules/types (mechanical plumbing — added after the transcription's STOP, 2026-10-20) |
| `crates/ui-dom/src/domains/style/mod.rs` | re-export `GridTrackListKey` and the new grid types (same correction) |
| `crates/ui-dom/src/domains/style/decl_store.rs` | the track-list arena + interning |
| `crates/ui-dom/src/domains/style/decl_value.rs` | the `TrackList` variant |
| `crates/ui-dom/src/domains/style/decl_parse.rs` | the T1 grammar + repeat expansion + capacity/line-name/auto-fill-fit gates; **parse entries widen to `&mut DeclStore` (see below)** |
| `crates/ui-dom/src/domains/style/style_runtime.rs` | pass `&mut self.decl_store` to `parse_declaration_block`; expose `resolve_track_list` (added after the STOP) |
| `crates/ui-dom/src/types/root_dom.rs` | pass `&mut self.inline_decl_store` to the two parse calls; add the inline store accessor if needed (added after the STOP) |
| `crates/ui-dom/src/domains/style/style_mut.rs` | pass the inline store to both parse calls (added after the STOP) |
| `crates/ui-dom/src/domains/style/computed_groups.rs` | the group slot |
| `crates/ui-dom/src/domains/style/project.rs` | the group resolver |
| `crates/ui-dom/src/domains/style/field_mapping.rs` | dirty classification |
| `crates/ui-dom/src/domains/visitor/winners_fold.rs` | the `TrackList` reintern arm |
| `crates/ui-dom/src/domains/layout/flow.rs` | `LayoutSolverKind::Grid` + routing + the FC flag generalization + the D5 interim arm |
| `crates/ui-dom/src/domains/layout/style.rs` | `LayoutDisplay` mapping |
| `crates/ui-dom/tests/layout_proofs/style/*` | the D6 pins (home per the suite's conventions) |
| `crates/ui-dom/tests/layout_proofs/README.md` | the gate rows (display inversion, capacity, line-name, auto-fill/fit, D5 interim) |

**The parse-entry signature decision (added after the transcription's
STOP, 2026-10-20):** `parse_declaration_block(text, store:
&mut DeclStore)` and `parse_declaration_strict(property, value, store:
&mut DeclStore)` — the store is the interning owner (the text interner
is one of its fields); widening to it (rather than adding a second
arena handle) is the honest shape, and every call site is `pub(crate)`.
The text interner remains reachable via `store.texts_mut()` for all
existing text-interning paths.

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
pin moves except the D6-specified inversions (the display gate's other
values and the still-gated property pins must NOT move); (3) a perf
pin moves without a documented cause (the arena interning is
allocation-once-per-new-list, deduped — steady-state zero; if a pin
moves, STOP with evidence). ONE commit, full gate green.
