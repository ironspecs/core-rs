# GRID_STEP6_DESIGN.md — grid T4: the `grid-template` and `grid` shorthands (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-22 (v2, same day). Author: the architect.
Authority: `GRID_MAPPING.md` (D5: "T4 — sugar: `grid-template` and
`grid` shorthands (expansion only)"), CSS Grid 1 §7.4
(`grid-template`), §7.7 (`grid`), CSS Cascade 5 §3 (shorthand
expansion); the crate's closed shorthand machinery (`Shorthand`,
`longhands_for_shorthand`, `expand_shorthand` in `decl_parse.rs` —
`grid-area` is the direct precedent).

**v2 changelog (STOP adjudication, 2026-10-22).** The first
transcription STOPped on two document defects; both adjudicated:

- **Defect A — `none` was unrepresentable for track lists.** The
  design's `none` forms require emitting a none track-list value, but
  the longhand grammar had no `none` arm and the null-key sentinel
  panicked the wire serializer. Adjudicated symmetrically with T3's
  `grid-template-areas: none` precedent: (1) the rows/columns longhand
  grammars ADMIT `none` → the null-key sentinel (it is valid CSS —
  the initial value — and was dropping unregistered: a real T0-era
  gap this tier exposes); (2) the wire gains the null-key → `"none"`
  arm for `DeclValue::TrackList` (mirroring the existing `GridAreas`
  arm), so the round-trip holds; (3) the shorthands emit the null key
  for none/missing axes. D6's "no `decl_value.rs` change" was wrong
  and is corrected.
- **Defect B — a second gated-behavior pin.** The design named only
  `grid_template_shorthand_still_drops`; but
  `unsupported_grid_property_names_drop`
  (`unsupported_properties.rs`) also asserts `grid-template`/`grid`
  drop at name lookup. It is part of the same inversion: rewritten
  (the names leave the unsupported list; the remaining cases stay).
  The D6 file map gains both files.

This document decides everything for T4. The shorthands are PURE
parse-time sugar: they expand to the already-landed longhands
(`grid-template-rows/columns/areas`, `grid-auto-flow`,
`grid-auto-rows/columns`) and never reach the computed group or the
solver. Anything unanswered is a document DEFECT — stop and return to
the architect. **No pushes** (user directive 2026-10-22: pushes
trigger the automated reviewer; local commits only until told).

## D0. T4 in one paragraph

`grid-template` and `grid` join the closed `Shorthand` table. At
parse, each expands to an ordered `Vec<Declaration>` of longhands —
`grid-template` ALWAYS sets all three template longhands,
`grid` ALWAYS sets all six grid-shaping longhands (CSS shorthand
reset semantics: omitted sub-values are their initial values, not
leftovers). Every existing grammar piece is reused verbatim (the
row/column track-list parsers, the T3 areas validation, the auto-flow
and auto-track parsers); every existing capacity gate applies
unchanged, with the shorthand dropping ATOMICALLY (one drop + count)
when any part fails. The wire emits longhands only — the shorthand is
gone post-expansion, exactly like `margin`.

## D1. `grid-template` (CSS Grid §7.4)

Grammar: `none | <'grid-template-rows'> / <'grid-template-columns'> |
[ <line-names>? <string> <track-size>? <line-names>? ]+
[ / <explicit-track-list> ]?`

Expansion order: `grid-template-rows`, `grid-template-columns`,
`grid-template-areas` (no overlaps; order is the property's formal
constituent order).

Forms:

1. **`none`** → rows `none`, columns `none`, areas `none` (the
   empty-map sentinel, per T3's `grid-template-areas: none`).
2. **`<rows> / <columns>`** → rows via `parse_row_track_list` (the
   row-axis grammar INCLUDING the T3.5 auto-repeat content gate —
   `grid-template: repeat(auto-fit, min-content) / 100px` drops),
   columns via `parse_track_list`, areas `none`.
3. **Areas form** — one or more `[names]? <string> <track-size>?
   [names]?` groups, optional `/ <explicit-track-list>`:
   - **areas**: the extracted strings, validated by the T3 areas
     machinery (rectangular, consistent column counts, name caps,
     `.` holes). Any validation failure → whole shorthand drops.
   - **rows**: the interleaved `<track-size>` per group; MISSING size
     → `auto`. Row sizes are SINGLE track-sizes — `repeat()` here
     drops + count (the `<track-size>` grammar excludes it). Line
     names attach to ROW lines (a name group before the first string
     and between/after groups; the per-group name arrays and the 8/12
     caps apply). The rows `GridTrackList` is built from parts and
     interned through the same arena path; `auto_repeat_span` and
     `owned_line_name_range` are `None`.
   - **columns**: the `/` part per `<explicit-track-list>` —
     **auto-fill/auto-fit FORBIDDEN here** (§7.4's explicit-track-list
     admits fixed `repeat(N,…)` only); `repeat(auto-fill|auto-fit,…)`
     in the areas-form columns drops + count. Missing `/` part →
     columns `none` (the areas map then widens the column count per
     the T3 rule).

## D2. `grid` (CSS Grid §7.7)

Grammar: `<'grid-template'> | <'grid-template-rows'> / [ auto-flow &&
dense? ] <'grid-auto-columns'>? | [ auto-flow && dense? ]
<'grid-auto-rows'>? / <'grid-template-columns'>`

Expansion order: rows, columns, areas, auto-flow, auto-rows,
auto-columns.

Disambiguation (parse in order, first match wins):

1. **Contains the `auto-flow` token** → form 2 or 3: `auto-flow` AFTER
   the slash → form 2 (rows + column flow); BEFORE the slash → form 3
   (row flow + columns). `[ auto-flow && dense? ]` admits
   `auto-flow dense` and `dense auto-flow`; `dense` without
   `auto-flow` drops.
2. **Otherwise** → form 1: parse as `grid-template` (all three forms,
   including `none` and the areas form).

Forms:

1. **`<'grid-template'>`** → the D1 expansion + auto-flow `row`,
   auto-rows `auto`, auto-columns `auto` (the initial values).
2. **`<rows> / auto-flow [dense]? <auto-columns-size>?`** → rows via
   the row grammar; columns `none`; areas `none`; auto-flow `column`
   or `column dense`; auto-columns = the single `<track-size>` leaf
   (existing `GridAutoColumns` grammar) or `auto` when omitted;
   auto-rows `auto`.
3. **`auto-flow [dense]? <auto-rows-size>? / <columns>`** → rows
   `none`; columns via the column grammar; areas `none`; auto-flow
   `row` or `row dense`; auto-rows = the leaf or `auto`; auto-columns
   `auto`.

**Reset semantics are the point**: `grid:` always emits all six
longhands, so a prior `grid-auto-rows: 99px` or `grid-template-rows:
5px` is RESET by any `grid:` declaration (the expanded longhands
overwrite in cascade order).

## D3. Shared rules (both shorthands)

- **Atomicity**: any failure (grammar, areas validation, capacity
  gate) drops the WHOLE shorthand: zero longhands emitted, one
  declaration drop counted.
- **Capacity gates**: unchanged — ≤12 tracks per list, ≤8 areas rows,
  ≤12 area columns per string row, ≤12 area names, the line-name caps,
  the T3.5 auto-repeat gates (nested repeat, bare-fr span, row-axis
  content span, zero-track span).
- **Cascade order**: expanded longhands append in expansion order;
  a later declaration in the same style source overrides an earlier
  expanded one (`grid-template: 40px / 100px; grid-template-rows: 1fr`
  → rows `1fr`).
- **Wire**: longhands only. `canonical_style_snapshot` /
  `compute_style_map` emit the longhand set; no shorthand
  recomposition (the crate's existing model, `margin` precedent).
  **`none` representation (v2)**: `none` for rows/columns is the
  null-key sentinel `GridTrackListKey::default()` — the longhand
  grammars admit `none` → null key (the T3 `grid-template-areas:
  none` precedent; ASCII case-insensitive per the registered
  exception), the wire serializes null-key `TrackList` → `"none"`
  (a new arm mirroring the existing `GridAreas` null arm), and the
  shorthands emit the null key for none/missing axes. Round-trip:
  `grid-template-columns: none` on the wire re-parses to the same
  sentinel.
- **`is_custom_ident` guards**: `auto-flow`, `dense`, and `none` are
  keywords, never area/line names, in the shorthand grammars too.

## D4. The pin table (exact)

| Pin | Asserted |
|---|---|
| `grid_template_admits_and_expands` | `grid-template: 40px 1fr / 100px 1fr` → rows/columns/areas emitted as longhands on the wire; layout boxes identical to a longhand-authored control in the SAME test |
| `grid_template_none_resets_all_three` | prior `grid-template-columns: 100px` then `grid-template: none` → items stack in implicit auto columns (columns none wins) |
| `grid_template_areas_form_sets_all_three` | `grid-template: [top] "a b" 40px [middle] "c d" 1fr [bottom] / 100px 1fr` → areas, rows (40/1fr), columns (100/1fr), row line names; `grid-area: b` and `grid-row: middle` resolve; layout matches the longhand control |
| `grid_template_areas_form_without_columns` | `grid-template: "a b" 40px` → columns none; the areas map widens to 2 implicit columns; `grid-area: b` lands in column 2 |
| `grid_template_areas_form_default_row_size_auto` | `grid-template: "a" 40px "b" / 100px` → second row sizes to content |
| `grid_template_areas_wider_than_columns_widens` | `grid-template: "a b c" 40px / 100px 100px` → the third areas column creates an implicit column; `grid-area: c` lands there |
| `grid_template_areas_form_auto_repeat_columns_drops` | `grid-template: "a" 40px / repeat(auto-fit, 100px)` → drop + count (explicit-track-list forbids auto-repeat) |
| `grid_template_areas_form_repeat_row_size_drops` | `grid-template: "a" repeat(2, 40px) / 100px` → drop + count (row sizes are single track-sizes) |
| `grid_shorthand_template_form` | `grid: 40px / 100px` → identical to the grid-template expansion + auto-flow `row`, auto-tracks `auto` (wire asserts all six) |
| `grid_shorthand_auto_flow_column_form` | `grid: 40px 1fr / auto-flow dense 80px` → rows 40/1fr; auto-flow `column dense`; auto-columns `80px`; columns/areas none; an overflow item sizes its implicit column to 80px |
| `grid_shorthand_auto_flow_row_form` | `grid: auto-flow 60px / 100px 1fr` → auto-flow `row`; auto-rows `60px`; columns 100/1fr; rows/areas none |
| `grid_shorthand_resets_implicit_props` | prior `grid-auto-rows: 99px; grid-template-rows: 5px` then `grid: auto-flow 60px / 100px` → auto-rows 60, rows none (both priors erased) |
| `grid_shorthand_dense_either_order` | `grid: dense auto-flow 60px / 100px` ≡ `grid: auto-flow dense 60px / 100px` (both `row dense`) |
| `grid_shorthand_longhand_override_order` | `grid-template: 40px / 100px; grid-template-rows: 1fr` in one style → rows `1fr` |
| `grid_shorthand_capacity_gate_atomic` | `grid-template:` with a 13-column areas string → the WHOLE shorthand drops as one counted declaration (rows/columns/areas all unset) |
| `grid_template_and_grid_gates_invert` | the T3-era `grid_template_shorthand_still_drops` pin INVERTS: both shorthands admit; garbage (`grid: nonsense`) still drops + count |
| `grid_track_list_none_longhand_admits` (v2) | `grid-template-columns: none` / `grid-template-rows: none` admit → null-key sentinel; the wire emits `none`; the wire string re-parses to the same sentinel (round-trip); `NONE` also admits (ASCII case-insensitive, the registered exception) |
| `unsupported_grid_property_names_pin_rewritten` (v2) | `unsupported_grid_property_names_drop` (`unsupported_properties.rs`) is rewritten: `grid-template`/`grid` leave the unsupported list (they now parse); the pin's remaining unsupported cases and its control stay green |

## D5. Explicitly untouched (named)

The solver, the computed group, every T0–T3.5 grammar and gate,
subgrid, baselines, the §12.1 re-run, shorthand recomposition on the
wire (the crate emits longhands — a named, deliberate model), the
intrinsic-path gap phrasing (the automated reviewer's non-blocking
note — by design, the intrinsic fold sums pre-collapse definitions).

## D6. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/style/decl_parse.rs` | `Shorthand::GridTemplate` + `Shorthand::Grid`; name table; `longhands_for_shorthand`; `expand_grid_template` + `expand_grid` (the areas-form group parser reusing the T3 validation and the track-list parsers); the `none` arm in both track-list longhand grammars (v2) |
| `crates/ui-dom/src/domains/style/decl_value.rs` | the null-key `TrackList` → `"none"` wire arm (v2 — mirrors the existing `GridAreas` null arm) |
| `crates/ui-dom/tests/layout_proofs/style/grid_admission.rs` | the gate inversions + the drop pins |
| `crates/ui-dom/tests/layout_proofs/style/unsupported_properties.rs` | the rewritten name-lookup pin (v2 — Defect B) |
| `crates/ui-dom/tests/layout_proofs/layout/grid_layout.rs` | the layout-equivalence and reset pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry row (T4 inversions + the longhand `none` admission) |
| `crates/ui-dom/src/domains/style/README.md` | the shorthand-table row (both names, expansion sets) |
| `crates/ui-dom/tests/perf_metric.rs`, `tests/perf_allocation.rs` | CONDITIONAL re-pin only, documented cause |

No new `PresentationProperty` variants; no solver change.

## D7. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (name it); (2) any existing pin moves except
the D4-specified inversion (`grid_template_shorthand_still_drops` —
it must be rewritten as the admit pin, not deleted); (3) a perf pin
moves without a documented cause; (4) ANY push (the user's directive:
local commits only). ONE commit, full gate green.
