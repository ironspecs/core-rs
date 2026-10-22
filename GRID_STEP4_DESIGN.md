# GRID_STEP4_DESIGN.md — grid T3: template areas, grid-area, and named lines (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-21. Author: the architect. Authority:
`GRID_MAPPING.md` (D3, D5), CSS Grid 1 §7.3 (`grid-template-areas`),
§7.4 (`grid-area`), §7.1 (named lines, area-implied `<name>-start`/
`<name>-end` per §7.3.1); the landed T2 solver.

This document decides everything for grid's T3 — the dashboard page
shell: `grid-template-areas` (string rows → named rectangular areas),
`grid-area` (a single area name OR one-to-four `<grid-line>` values),
and named lines (explicit `[name]` in track lists plus the implied
`<area>-start`/`<area>-end` lines every named area creates). Anything
unanswered is a document DEFECT — stop and return to the architect.

> **Adjudication 2026-10-22 (PR #27 review) — `grid-area: <name>` is a
> shorthand, not an area-name slot.** D2's lone-`<custom-ident>` branch
> and D4 rule 1 are SUPERSEDED: `grid-area: <name>` expands to the FOUR
> placement longhands as `Named(name)` (the margin-shorthand model), and
> the item-side `area_name` field is REMOVED — `Named(name)` resolution
> does area-match first (the side's edge: `-start` reads `min_*`,
> `-end` reads `max_* + 1`), then named-line match, then auto fallback.
> This makes a later placement longhand win its slot
> (`grid-area: header; grid-column-start: 2`) and makes re-setting the
> shorthand between forms clear the previous form. The wire snapshot for
> `grid-area: header` serializes as the four `Named(header)` longhands.

---

## D0. T3 in one paragraph

`grid-template-areas` parses string rows into a validated, interned
area map (rectangularity and uniform-column rules enforced with
counted drops); `grid-area` resolves a name at placement time against
the container's area map and named lines (explicit `[name]` tokens and
the implied `<area>-start`/`<area>-end` lines per §7.3.1); the T3 gate
(`[` line-name tokens and `grid-area` names, currently dropping with
counted reports) inverts to admitted. The `<grid-line>` vocabulary
gains `Named`/`NamedSpan` variants for name references in longhands.

## D1. `grid-template-areas` (grammar, validation, representation)

**Grammar** (`decl_parse.rs`): `grid-template-areas: <string>+`. Each
string is one row: whitespace-separated cell tokens, each either a
dot-run (`.`, `..`, `...` — an empty cell) or a `<custom-ident>` (a
named area cell). Quotes are stripped per the existing string handling
(the quoted-family caveat's shape).

**Validation (all drops are counted, per the G rule):**
- Every row string must have the SAME column count (spec §7.3.1:
  unequal column counts invalidate the WHOLE declaration → drop +
  count).
- Every name must form a single rectangle (spec §7.3.1: same-named
  cells that do not tile a rectangle invalidate the declaration →
  drop + count).
- Capacity: > 8 rows, > 12 columns, or > 12 distinct names → drop +
  count (the capacity-gate precedent; registered).

**Representation (arena-interned, the track-list arena's shape):**
`GridAreasKey(u32)` into `DeclStore.grid_areas:
SlotMap<GridAreasKey, GridTemplateAreas>` with value-dedup interning.
`GridTemplateAreas` (`computed/grid_template_areas.rs`, `Copy`):

```rust
struct GridTemplateAreas {
    cells: ArrayVec<[ArrayVec<[Option<TextKey>; 12]>; 8]>, // [row][col], None = dot
    names: ArrayVec<[(TextKey, AreaRect); 12]>,            // deduped name → rect
    row_count: u8, col_count: u8,
}
struct AreaRect { min_row: u8, min_col: u8, max_row: u8, max_col: u8 } // inclusive
```

`DeclValue::GridAreas(GridAreasKey)` (4 bytes, the `TrackList` model).
`ComputedGridGroup.template_areas: GridAreasKey` (initial: the empty
map's interned key).

**Area-implied named lines (§7.3.1):** every named area `foo` creates
lines `foo-start` (the area's start edge) and `foo-end` (the area's
end edge) in BOTH axes, registered into the container's named-line
maps alongside explicit `[name]` lines.

## D2. `grid-area` (grammar and expansion)

**Grammar** (`decl_parse.rs`): `grid-area: <grid-line>{1,4} |
<custom-ident>`, expanded at parse (A1's single expansion point):

- **`<custom-ident>` alone** → the item's `area_name` field (a
  `TextKey`; placement-time resolution per D4).
- **1–4 `<grid-line>` values** (the standard shorthand defaulting):
  1 value → `row_start = X`, others `Auto`; 2 values →
  `row_start, column_start`; 3 → + `row_end`; 4 → all four. `span N`
  alone → `row_start = Span(N)`, others `Auto`.
- Longhands `grid-column-start` etc. gain name forms:
  `<custom-ident>` (a named line) and `span <custom-ident> [integer]`.

**`GridLine` extends** (`computed/grid_line.rs`):
`{ Auto, Line(i32), Span(u32), Named(TextKey), NamedSpan(TextKey, u32) }`.

**Group fields:** item side gains `area_name: Option<TextKey>`
(initial `None`); the four `*_start/end` fields carry the new
`Named`/`NamedSpan` variants.

## D3. Named lines in track lists

The T3 gate inverts: `[name]` or `[name1 name2]` (space-separated,
inside one bracket pair) before a track size is ADMITTED. Each name
interns (`TextKey`) and registers into the container's named-line
map per axis: `name → ArrayVec<[line_index; 4]>` (a name may label
multiple lines; capacity 4 per name, counted drop beyond). Track-list
serialization (the wire format) gains the bracket round-trip:
`[main] 1fr [aside]` re-serializes with brackets for the canonical
snapshot round-trip.

**The storage representation (added 2026-10-21 under the
transcription's STOP — the original draft admitted names without
saying where they live).** The track-list arena's value type becomes
`GridTrackList` (`computed/grid_track_list.rs`, NEW — structural rule
1: it has behavior — serialization and name registration):

```rust
struct GridTrackList {
    tracks: ArrayVec<[GridTrackSize; 12]>,
    /// (name_key, line_index) — a bracket group attaches to the line at
    /// its position (the line BEFORE the following track); a trailing
    /// bracket names the final line at index `tracks.len()`.
    line_names: ArrayVec<[(TextKey, u8); 8]>,
}
```

Rules: capacity 8 name registrations per list (drop + count beyond);
a name appearing more than 4 times in one list drops + counts at parse
(the multiple-same-name span-count rule in D4 stays valid up to 4).
Dedup interns by FULL content equality (tracks + name pairs), so two
identical named templates intern to one key. The arena type change
(from `ArrayVec<GridTrackSize>` to `GridTrackList`) is a documented
representation change with a cause: the T3 gate inversion requires
retained names; every current use (intern, resolve, formatter, dedup)
adjusts. **Placement-time name comparison (confirmed):** resolve
`TextKey` to `&str` via the container's interner and compare by
content equality — allocation-free, no hashing.

## D4. Placement-time name resolution (exact)

Resolving an item's placement per axis, in this order:

1. **`area_name` set (item has `grid-area: foo`):** if `foo` matches a
   named AREA in the container's area map → the item occupies that
   `AreaRect` (definite both axes: start at `rect.min_*`, span
   `rect.max_* − rect.min_* + 1`). If `foo` matches a named LINE
   (explicit or implied) → resolve as `Named` per rule 2 with the
   name. If `foo` matches NOTHING → treated as `Auto` (spec §7.4's
   fallback for a name with no matching area — documented, pinned, NOT
   a drop).
2. **`Named(name)` in a longhand:** resolve against the named-line map
   (explicit `[name]` lines first, then area-implied
   `<name>-start`/`<name>-end`): single matching line → that line
   index. **Multiple matching lines → the FIRST match (CORRECTED
   2026-10-22 against the spec: multiple same-name lines form a named
   set that is "exclusively indexed by filtering the placement by
   name" (CSS Grid 1 §8.1.3) — a bare `<custom-ident>` is "treated as
   if the integer 1 had been specified" = the first match, NOT a span
   across the matches. The earlier draft's "treated as Span(count)"
   rule does not exist in the spec and is deleted; both engines agree
   (Blink `FirstPosition`, Gecko `aNth = 1`).** No match → `Auto`
   (documented, pinned).
3. **`NamedSpan(name, n)`:** span `n` starting at the FIRST matching
   line — and `span <custom-ident>` with NO integer is span **1** from
   the first match (CSS Grid 1 §8.3: "If the placement contains only a
   span for a named line, replace it with a span of 1"). The
   multiple-same-name span-count rule in the earlier draft is deleted
   with it.
4. Line numbers and spans then resolve per T2's rules (negative
   translation, the wrap bounds).

## D5. The pin table (exact)

| Pin | Asserted |
|---|---|
| `grid_template_areas_page_shell` | `"header header" "sidebar main" "footer footer"` + `grid-template-columns: 100px 1fr` → header spans both columns, sidebar in col 1, main in col 2, footer spans both; items named via `grid-area` fill their rects |
| `grid_template_areas_empty_dot_cells` | `". main main"` → column 1 empty (no area), main in cols 2–3 |
| `grid_template_areas_non_rectangular_name_drops` | `"a a" "a b"` (a not rectangular) → WHOLE declaration drops + count, group initial |
| `grid_template_areas_non_uniform_columns_drops` | `"a a" "b"` → drop + count |
| `grid_template_areas_capacity_gates_drop` | 9 rows / 13 columns / 13 names → each drop + count |
| `grid_area_name_fills_its_area_rect` | `grid-area: header` → the header rect's start, span, and stretch |
| `grid_area_unknown_name_becomes_auto` | `grid-area: nosuch` → auto placement (documented, not a drop) |
| `grid_area_integer_and_span_forms` | `grid-area: 2`, `grid-area: 2 / 3`, `grid-area: 2 / 3 / 4`, `grid-area: 2 / 3 / 4 / 5`, `grid-area: span 2` → the D2 defaulting table |
| `grid_named_lines_in_track_lists` | `grid-template-columns: [main] 100px [aside] 1fr` + `grid-column: main` → the item starts at line 1; `grid-column: aside` → line 2 |
| `grid_area_implied_start_end_lines` | `grid-row-start: header-end` → the item starts after the header area |
| `grid_multiple_same_name_lines_span_count` | two `main` lines: `grid-column: main` → span 2 across both; `grid-column: span main` → span 2; `grid-column: span main 3` → span 3 from the first |
| `grid_named_line_unknown_becomes_auto` | `grid-column: nosuch` → auto (documented, pinned) |
| `grid_areas_and_explicit_template_tracks` | areas naming a subset of a larger explicit template (the rest of the tracks size and place normally) |
| `grid_areas_definite_items_abs_pos_area_rect` | an abs-pos child of an area-named item → static position = the area rect (the T2 area refinement with named areas) |
| `grid_template_shorthand_still_drops` | `grid-template:` → drop + count (T4 gate, unchanged) |
| `grid_area_wire_round_trip` | the canonical style snapshot serializes areas + named lines + `Named`/`NamedSpan` longhands and re-parses to the same group |

## D6. Admission guards and still-gated

Counted drops (all registered, all pinned): non-rectangular name,
non-uniform columns, >8 rows, >12 columns, >12 names, >4 same-name
line registrations. Still dropping unchanged: `grid-template`/`grid`
shorthands (T4), `repeat(auto-fill|auto-fit, …)` (T3.5), `subgrid`
(value), every non-grid tail property.

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/style/computed/grid_template_areas.rs` (NEW) | `GridTemplateAreas` + `AreaRect` |
| `crates/ui-dom/src/domains/style/computed/grid_track_list.rs` (NEW) | `GridTrackList` (the D3 storage representation — serialization + name registration) |
| `crates/ui-dom/src/domains/style/computed/grid_line.rs` | the `Named`/`NamedSpan` variants |
| `crates/ui-dom/src/domains/style/decl_store.rs` | the `grid_areas` arena + interning |
| `crates/ui-dom/src/domains/style/decl_value.rs` | the `GridAreas` variant + formatter arms |
| `crates/ui-dom/src/domains/style/decl_parse.rs` | the areas grammar + validation, the `grid-area` expansion, the `[name]` grammar, the name longhands |
| `crates/ui-dom/src/domains/style/computed/computed_grid_group.rs` | `template_areas` key, `area_name` |
| `crates/ui-dom/src/domains/style/computed/mod.rs`, `style/mod.rs` | re-exports (mechanical) |
| `crates/ui-dom/src/domains/style/presentation_property.rs` | the `GridTemplateAreas` + `GridArea` METADATA rows |
| `crates/ui-dom/src/domains/style/project.rs` | the resolver arms |
| `crates/ui-dom/src/domains/style/field_mapping.rs` | dirty classification |
| `crates/ui-dom/src/types/root_dom.rs` | exhaustive-match arms for the new variants (mechanical pipeline) |
| `crates/ui-dom/src/types/dom_node.rs` | `presented_px` arm (mechanical pipeline) |
| `crates/ui-dom/src/domains/layout/grid.rs` | the areas map construction, the D4 resolution, implied start/end lines, named-line spans |
| `crates/ui-dom/tests/layout_proofs/style/grid_admission.rs` | the admission guards + the T3 gate inversions |
| `crates/ui-dom/tests/layout_proofs/layout/grid_layout.rs` | the D5 family |
| `crates/ui-dom/tests/layout_proofs/README.md` | the gate rows (inversions, capacity, rectangularity) |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
pin moves except the D5-specified gate inversions (the T1/T2 families
and still-gated pins must NOT move); (3) a perf pin moves without a
documented cause; (4) any structure allocates in steady state beyond
arena interning (once per new interned value). ONE commit, full gate
green.
