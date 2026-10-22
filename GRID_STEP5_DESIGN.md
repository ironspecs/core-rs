# GRID_STEP5_DESIGN.md — grid T3.5: auto-fill and auto-fit repetition (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-22 (v3, same day). Author: the architect.
Authority: `GRID_MAPPING.md` (D5's T3.5 tier), CSS Grid 1 §7.2.3.2
(repeat-to-fill); Blink `CalculateAutomaticRepetitions`
(`grid_layout_utils.cc`), Gecko `InitRepeatTracks`
(`nsGridContainerFrame.cpp`); the landed T3 solver.

This document decides everything for grid's T3.5 — the card-wall
idiom `repeat(auto-fit, minmax(240px, 1fr))`: repetition counts
computed at layout time from the container's definite size, the
auto-fit collapse of empty tracks, and placement re-runs on container
resize. Anything unanswered is a document DEFECT — stop and return to
the architect.

**v3 changelog (third-party review remediation, 2026-10-22).** The
review verdict was REMEDIATE with four blocking findings; all
adjudicated:

- **B1 — zero-track span**: a names-only inner list
  (`repeat(auto-fill, [x])`) stored `count == 0` and divided by zero
  at layout. §7.2.3's `<auto-repeat>` grammar requires ≥1
  `<track-size>`: a span with zero track sizes DROPS AT PARSE +
  count. Pinned.
- **B2 — conversion before clamp**: the count converted f32→u32
  BEFORE the capacity clamp, panicking on astronomical/infinite
  definite sizes. The clamp now compares in f32 first (any quotient
  above the lane capacity clamps to it, metric fires); conversion
  happens only on the clamped value. Pinned (huge width → no panic,
  N = 12, metric = 1).
- **B3 — boundary line-name ownership**: a name immediately after the
  close paren (`repeat(auto-fill, 100px) [after] 200px`) was stored at
  the same line index as a trailing name INSIDE the inner list —
  mis-resolved and double-emitted by the serializer. The parse now
  records the span's OWNED line-name range (span-inner names vs
  suffix names); expansion shifts only suffix-owned names and the
  serializer partitions by ownership. Pinned (resolution at the
  expanded position; round-trip stable).
- **B4 — row-axis collapse**: the auto-fit collapse was column-only.
  §7.2.3.2 is per-axis: a symmetric `collapsed_rows` bitmap feeds
  `size_row_tracks` and the fr distribution re-runs over surviving
  rows. Pinned (600px height, 1 item → 600).
- **Non-blocking 1 — row-axis content basis**: row counts derived
  from inline-axis (width) contributions — a wrong-axis count is
  worse than a gate. Row-axis content-based span tracks
  (`min-content` / `max-content` / `auto` in `grid-template-rows:
  repeat(auto-fill|auto-fit, …)`) DROP AT PARSE + count; the
  block-axis intrinsic basis is a named deferred admission.
  Column-axis content bases keep `h_min`/`h_max` (correct axis). The
  code's content basis is one global max over the measured views
  (exact for single-track spans; per-position multi-track refinement
  is named and deferred); this text is amended to match.
- **Non-blocking 2 — minmax basis**: D2's "minmax → its min bound"
  contradicts §7.2.3.2 ("treated as its max track sizing function if
  that is definite"). Amended: minmax → its max bound IF DEFINITE,
  else its min bound. Pinned (`minmax(100px, 200px)` at 600 → N=3).
- **Non-blocking 3/4 — registry notes**: the zero-basis span
  (`repeat(auto-fill, 0px)`) clamps to the lane capacity with the
  metric every pass (graceful, same divergence class as bare-fr —
  noted); the inline-grid intrinsic-constraint fill (§7.2.3.1
  max-content repetition count) is unimplemented — noted as deferred.
- **Non-blocking 5 — stale docs**: the `grid_admission.rs` file
  header (still listing the inverted gates) and the README typo are
  fixed in the remediation.

**v2 changelog (STOP adjudication, 2026-10-22).** The first
transcription STOPped on four pin-table defects; all adjudicated:

- **auto-fill sizing**: no collapse means empty flexible tracks still
  flex — the auto-fill pin is 300/300 (fr-expand verbatim), not
  360/240. The 360/240 figure implied an occupancy-aware fr
  distribution that §7.2.3.2 does not have for auto-fill.
- **the 800px resize row**: N=3 → 800/3 = 266.67 each; "200 each" was
  an arithmetic error.
- **multi-track span pin**: uses a fixed `100px 200px` list (N=2) —
  the fr-bearing list's basis was circular under the bare-fr rule.
- **bare `fr` in an auto-fill/fit span**: DROPS AT PARSE with a
  counted report (the zero-basis case makes the count formula
  diverge; the content-based repetition basis §7.2.3.2 requires for
  it is a named deferred admission). The D2 basis table therefore
  never sees a bare fr.
- **added**: the repetition count ALSO clamps to the track-lane
  capacity (prefix + N×span + suffix ≤ 12) with a layout-time metric;
  and `grid_line_context.rs` joins the file map (expanded line names
  feed name resolution).

---

## D0. T3.5 in one paragraph

`repeat(auto-fill, <track-list>)` and `repeat(auto-fit, <track-list>)`
are admitted: the parse stores the list as ONE copy plus an
`AutoRepeat { kind, start, count }` span marker; at track-list
resolution (layout time, when the container's main size is definite)
the repetition count `N = floor((size + gap) / (Σ bases + gap))` (min 1)
expands the span to N copies; tracks size per the T1/T2 phases; items
place; and for auto-fit, tracks whose cells hold no item COLLAPSE to
zero and their free space re-distributes to the non-collapsed tracks.
A container-size change re-marks the grid dirty and the whole cycle
re-runs (the dirty-bit doctrine — no special invalidation).

## D1. Grammar and representation (exact)

**Grammar** (`decl_parse.rs`): `repeat(auto-fill, <track-list>)` and
`repeat(auto-fit, <track-list>)` ADMIT (the T3.5 gate inverts).
Rules: the inner track-list parses per the normal grammar; a NESTED
`repeat()` inside it drops + count (CSS forbids it); the inner list
must contain ≥1 TRACK SIZE — a names-only inner list
(`repeat(auto-fill, [x])`) drops + count (§7.2.3's `<auto-repeat>`
requires ≥1 `<track-size>`); `repeat(auto-fill)` / `repeat(auto-fit)`
with no inner list drops + count; any bare `fr` span track drops +
count (the deferred-admission gate); a SECOND auto-repeat in one list
drops + count (§7.2.3, one auto-repeat per list); **row-axis
content-based span tracks** (`min-content`/`max-content`/`auto` in
`grid-template-rows: repeat(auto-fill|auto-fit, …)`) drop + count
(the block-axis intrinsic basis is a named deferred admission —
column-axis content bases are correct-axis and admitted).

**Representation** (`GridTrackList` gains):

```rust
enum GridRepeat { None, Count(u8), AutoFill, AutoFit }
// On GridTrackList:
auto_repeat_span: Option<(u8 /*start*/, u8 /*count*/, GridRepeat)>
// plus the span's OWNED line-name range (which of line_names'
// entries are span-inner vs suffix), so expansion shifts only
// suffix-owned names and serialization partitions by ownership.
```

`repeat(N, list)` keeps today's parse-time expansion into N copies
(`GridRepeat::None` after expansion). `repeat(auto-fill|auto-fit,
list)` stores the inner list as one copy with
`auto_repeat_span = Some((start_index, inner_count, kind))` — the
span is contiguous (prefix tracks, then the span, then suffix tracks).
Full-list repeats have `start = 0, count = inner_count`.

Named lines inside the span repeat with their tracks (the named-set's
Nth-match first-match indexing applies — the corrected rule).

## D2. The repetition count (layout time, exact)

At track-list resolution (the container's definite main size `S` and
`gap` known): per the span's base sizes (D3's basis rules),
`rep_size = Σ span bases`, and

```text
N = floor((S + gap) / (rep_size + gap)), clamped to at least 1
```

(CSS Grid §7.2.3.2: "the largest integer such that (number of
repetitions × size of track list) ≤ available space + gutter, where
size of track list = sum of track sizes + gutter".) The count is
recomputed EVERY pass (layout-time data, not stored); a size change
marks the container style-dirty and re-runs the whole solve — no
special invalidation path, no cache.

**Basis rules for the count:** fixed → px; percent → % of S; minmax →
its max bound IF DEFINITE (§7.2.3.2: "treated as its max track sizing
function if that is definite"), else its min bound (resolved);
`min-content` → the items' `h_min` max over the measured views;
`max-content`/`auto` → `h_max` (one global max per basis kind — exact
for single-track spans; per-position multi-track refinement is named
and deferred). Content bases are COLUMN-AXIS ONLY (h_min/h_max are
inline-axis values); row-axis content-based span tracks drop at parse
(D1). **A bare `fr` never appears here**: `repeat(auto-fill, …fr…)` /
`repeat(auto-fit, …fr…)` where ANY span track is a bare `fr` DROPS AT
PARSE with a counted report (the zero-basis case makes the formula
diverge; the content-based repetition basis §7.2.3.2 requires for it
is a named deferred admission — see D4's
`grid_repeat_bare_fr_drops_at_parse` pin and the registry note). A
`fr` MAX inside minmax (the idiom) is fine: the count basis is the
minmax's min bound (a `fr` max is not definite).

**Capacity clamp (layout-time analog of the parse-time >12 gate):**
the expanded track lane is capacity-bounded (12). The count clamps:
`N = min(N_formula, floor((12 − prefix − suffix) / span_count))`,
clamped ≥ 1. **The clamp compares in f32 BEFORE any integer
conversion** — the quotient is unbounded for astronomical/infinite
definite sizes; any quotient above the capacity clamps to it (metric
fires) and conversion happens only on the clamped value. When the
clamp fires (the formula wanted more tracks than the lane holds), a
`grid_repetition_count_capped` layout-time metric increments — cap +
count, graceful, exactly the parse-time capacity-gate pattern. Pinned
by `grid_repeat_count_capped_at_lane_capacity` and
`grid_repeat_count_clamps_before_conversion`.

## D3. Expansion, sizing, placement

1. **Expand**: the span expands to N copies in the track lane (prefix,
   N × span, suffix; line names repeat with their tracks; the parse
   recorded which line-name entries are span-OWNED vs suffix-owned —
   only suffix-owned names shift, and the serializer partitions by
   ownership so a name on the boundary line is emitted exactly once,
   in its owning section).
2. **Size**: the T1/T2 track-sizing phases run on the expanded tracks
   (init → intrinsic → maximize → fr-expand (with the per-restart cap)
   → stretch auto).
3. **Place**: items place into the expanded grid per the existing
   §8.5 machinery (auto-placement, spans, areas all compose — the
   expansion is transparent to placement).
4. **Auto-fit collapse (auto-fit only, PER-AXIS)**: after placement,
   any span track whose cells contain NO item collapses to `0.0`
   size — a `collapsed_columns` bitmap feeds the column sizing and a
   symmetric `collapsed_rows` bitmap feeds the row sizing
   (§7.2.3.2 is per-axis); the fr distribution then RE-RUNS over the
   non-collapsed tracks only in each axis (the collapsed tracks are
   treated as fixed-size 0 per §7.2.3.2), so the freed space flows to
   surviving flexible tracks; if no flexible tracks survive, the freed
   space stays as container free space and the container's
   justify/align-content applies as today. Collapsed tracks still
   exist for placement accounting (their lines stay) but contribute
   zero to sizing.

## D4. The pin table (exact)

| Pin | Asserted |
|---|---|
| `grid_repeat_auto_fit_minmax_fr_card_wall` | `repeat(auto-fit, minmax(240px, 1fr))` in 600px, 2 items → N=2, each track 300 (240 base + (600−480)/2 = 60 each), items fill both |
| `grid_repeat_auto_fit_collapses_empty_tracks` | 600px, 1 item → N=2 created, the empty second track collapses to 0, the fr distribution re-runs over the surviving track → the item's track gets 600 |
| `grid_repeat_auto_fill_keeps_empty_tracks_flexed` | `repeat(auto-fill, minmax(240px, 1fr))`, 600px, 1 item → NO collapse, fr-expand verbatim: both tracks 300 (item 300, the empty "ghost" track 300) — the auto-fill/auto-fit difference is exactly the collapse |
| `grid_repeat_recount_on_container_resize` | 600 → 2 cols (300 each); 400 → 1 col (item 400); 800 → 3 cols (240 base, free 80, 266.67 each — assert as 800/3 within float tolerance); assert all three after separate updates |
| `grid_repeat_count_with_gutters` | `repeat(auto-fit, minmax(240px, 1fr))` + `column-gap: 10px` at 600 → N = floor(610/250) = 2, tracks (600−10)/2 = 295 each |
| `grid_repeat_multi_track_list_span` | `repeat(auto-fit, 100px 200px)` (a fixed 2-track list) in 600 → rep_size 300, N = floor(600/300) = 2 → 4 tracks (100,200,100,200) |
| `grid_repeat_nested_repeat_drops` | `repeat(auto-fit, repeat(2, 1fr))` → drop + count |
| `grid_repeat_bare_fr_drops_at_parse` | `repeat(auto-fit, 1fr)` → drop + count at parse (the named deferred admission: content-based repetition basis; registry note) |
| `grid_repeat_count_capped_at_lane_capacity` | `repeat(auto-fill, 10px)` in 600px → formula wants 60, clamps to 12, `grid_repetition_count_capped` increments |
| `grid_repeat_named_lines_repeat_with_tracks` | `repeat(auto-fit, [main] minmax(240px, 1fr))` → `main` lines repeat in the expanded lane; `grid-column: main` → first match |
| `grid_repeat_auto_fit_and_auto_fill_admit` | the T3.5 gate inverts (both forms parse and project); `repeat(auto-fill)` bare → drop + count |
| `grid_repeat_names_only_span_drops` (B1) | `repeat(auto-fill, [x])` → drop + count at parse; NO layout panic |
| `grid_repeat_count_clamps_before_conversion` (B2) | `width: 2000000000000px` + `repeat(auto-fill, 10px)` → no panic, N = 12, `grid_repetition_count_capped` fires |
| `grid_repeat_boundary_line_name_ownership` (B3) | `repeat(auto-fill, 100px) [after] 200px` at 600px, `grid-column: after` → item at left 600, width 200 (the expanded position); `canonical_style_snapshot` round-trips with `after` emitted exactly once, in the suffix |
| `grid_repeat_auto_fit_collapses_rows` (B4) | `height: 600px; grid-template-rows: repeat(auto-fit, minmax(40px, 1fr))`, 1 item → the 11 empty rows collapse, fr re-runs → item height 600 |
| `grid_repeat_row_axis_content_basis_drops` (NB1) | `grid-template-rows: repeat(auto-fit, min-content)` → drop + count at parse (block-axis intrinsic basis deferred; registry note) |
| `grid_repeat_minmax_definite_max_basis` (NB2) | `repeat(auto-fill, minmax(100px, 200px))` at 600px → basis 200 → N=3 |

## D5. Explicitly untouched (named)

Bare-fr repetition (dropped at parse; the content-based repetition
basis is a named deferred admission), row-axis content-based
repetition bases (dropped at parse; the block-axis intrinsic basis is
a named deferred admission), the zero-basis span (`repeat(auto-fill,
0px)`: clamps to the lane capacity with the metric every pass —
graceful, same divergence class as bare-fr, registry-noted), the
inline-grid intrinsic-constraint fill (§7.2.3.1's max-content
repetition count — inline-grid auto-repeat yields N=1, registry-noted
as deferred), the per-position multi-track content-basis refinement
(the basis is one global max per kind; exact for single-track spans),
subgrid (gated), baselines (deferred), the §12.1 re-run (deferred),
the T1–T3 sizing phases (reused verbatim — the ONLY new sizing step
is the auto-fit collapse's fr re-run over surviving tracks, D3 step
4), the still-gated names (`grid-template`/`grid` shorthands, tail
properties).

## D6. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/style/computed/grid_track_list.rs` | the `GridRepeat` enum + the span field |
| `crates/ui-dom/src/domains/style/decl_parse.rs` | the auto-fill/fit grammar + nested-repeat gate + bare-fr-in-span parse gate |
| `crates/ui-dom/src/domains/style/decl_value.rs` | the wire serialization for the span (`repeat(auto-fit, …)` round-trip) |
| `crates/ui-dom/src/domains/layout/grid.rs` | the count computation (formula + capacity clamp + metric), the expansion, the auto-fit collapse + fr re-run |
| `crates/ui-dom/src/domains/layout/grid_track_lane.rs` (if the expansion lives there) | the expanded-track append |
| `crates/ui-dom/src/domains/layout/grid_line_context.rs` | expanded line names feed `resolve_named` (the expansion is what name resolution sees) |
| `crates/ui-dom/src/domains/layout/layout_metric_kind.rs` (or the metric enum's file) | the `grid_repetition_count_capped` metric |
| `crates/ui-dom/tests/layout_proofs/style/grid_admission.rs` | the admission inversions + nested/bare-repeat/bare-fr gates |
| `crates/ui-dom/tests/layout_proofs/layout/grid_layout.rs` | the D4 family |
| `crates/ui-dom/tests/layout_proofs/README.md` | the gate row (T3.5 inversions, the bare-fr deferred admission, the capacity clamp) |
| `crates/ui-dom/tests/perf_metric.rs`, `tests/perf_allocation.rs` | CONDITIONAL re-pin only, documented cause |

## D7. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
pin moves except the D4-specified inversions (the T1–T3 families and
still-gated pins must NOT move); (3) a perf pin moves without a
documented cause; (4) the resize recount produces any cross-pass state
(it must be pure per-pass computation — if you find yourself caching a
count, STOP). ONE commit, full gate green.
