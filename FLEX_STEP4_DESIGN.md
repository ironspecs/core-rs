# FLEX_STEP4_DESIGN.md — auto-min by fold (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-19. Author: the architect. Authority: CSS
Flexbox 1 §4.5 (automatic minimum size), §9.2 (hypothetical main size);
the engine corpus (`.tmp/engine-corpus/`: Blink folds the automatic
minimum into `main_axis_min_max_sizes` before the clamp
(`flex_layout_algorithm.cc:1060-1178`, `flex_item.h:47-48`); Gecko
resolves it in `UpdateMainMinSize` before `CSSMinMax`
(`nsFlexContainerFrame.cpp:660-698`)); FLEX_MAPPING §3.1 (the amended
derivation rule); the four registered rows this step flips.

This document decides everything for step 4. Anything unanswered is a
document DEFECT — stop and return to the architect.

---

## D0. The step in one paragraph

Every flex item's automatic minimum main size is DERIVED at the item
view's seed from the record and the authored style —
`auto_min = min(min_content_main, definite_specified_main_size)` for
the overflow-visible subset, `0` otherwise — exactly the engines'
formula, for EVERY shape (no gates). The recursive auto-min walk and
both graceful gates die by construction: shape-generality (the record's
intrinsics are axis- and wrap-agnostic), grow-blindness (records know
no flex factors), and mode-awareness (the 3a-i intrinsic made
min-content mode-aware; the nowrap divergence lived only in the walk's
manual whitespace split). The hypothetical main size gains the
automatic-minimum floor at the same site (both engines fold it into the
min clamp).

## D1. The derivation (exact)

At item-view assembly (the existing `supported_automatic_min_main_size_px`
call site, which this step replaces):

```text
automatic_min_main_size_px(view) =
    if overflow != Visible: 0.0                                     // unchanged (scroll/hidden/clip/auto)
    else:
        let content_min = min_content_main(view)                    // D2/D3
        match definite_specified_main_size(view):                   // authored definite main size
            Some(definite) => content_min.min(definite)             // §4.5's min(specified, content)
            None         => content_min
```

`definite_specified_main_size`: the authored `width`/`height` (per the
container's main axis) when it resolves definite — `BoxLength::Px` (or a
percentage against a definite down-fold basis); `flex-basis` does NOT
count as definite (unchanged from the current gate's rule); `auto` /
indefinite percentages → `None`.

## D2. `min_content_main` — horizontal main axis

`record.h_min_content_px` — mode-aware for every node kind EXCEPT
nested flex containers (text: the 3a-i intrinsic's min-content buffer;
img/control: intrinsic width; block/inline: the bottom-up fold). NO new
computation for those kinds in this step. For nested flex containers
the record today carries the stretched/available width — corrected by
D2a (added 2026-10-19 under the transcription's STOP).

**The nowrap divergence dies here:** the record's `h_min_content_px`
for text is the cosmic-computed min-content under the node's
white-space mode (nowrap/pre → one run = full width; normal → widest
fragment), proven by 3a-i's tests. The walk's manual
whitespace-splitting leaf (`text_min_content_width_px`) is the only
place the divergence existed, and it dies with the walk.

## D2a. The flex container's §9.9 intrinsic main sizes (the STOP's correction)

The transcription STOP proved D2's premise false: a nested flex
container's record carried the resolved width, not intrinsic sizes, and
the derivation therefore produced spec-wrong values (320 vs 140.53;
50 vs 173.398). The correction is a small computation — arithmetic
over data the inner measurement solve already holds (item views carry
each item's `record.h_min`/`h_max` and margins) — at the inner
container's record write in `measure_flex_node_without_layout_write`:

```text
contributions_min(view) = view.record.h_min_content_px + view.main_margins_sum
contributions_max(view) = view.record.h_max_content_px + view.main_margins_sum
gap = container main-axis gap (between items); n = real item count

h_min_content_px =
    row, nowrap        => Σ contributions_min + gap × (n−1)
    row, wrap/reverse  => max(contributions_min)          // lines can break anywhere
    column (any wrap)  => max(contributions_min)          // width = widest item
h_max_content_px =
    row (any wrap)     => Σ contributions_max + gap × (n−1) // single-line arrangement is the max
    column (any wrap)  => max(contributions_max)
```

Anonymous-run items contribute with zero margins (run h_min = widest
segment, h_max = sum — already correct per 3a). Empty container: both 0.

**Consequence (named, with its divergence):** the item view's
auto-basis base for nested flex items reads `record.h_max_content_px`,
which after this computation is the §9.9 MAX-CONTENT main size — per
§9.2 the flex base size for auto basis without a definite main size.
Today's value is the available/stretched width (block-modeled): an
UNREGISTERED divergence the nested-flex proofs pin as correct. The
corrected base moves those proofs (fractional-grow, shrink,
auto-sized-column families): each move is spec-correct per §9.2 and is
updated with a documented cause and a report entry (the D6 audit rule
applies); the move is recorded in the decision log (no registry row —
the divergence is fixed in the same change that discovered it, so
there is no current-divergent state to pin).

## D3. `min_content_main` — vertical main axis (column containers)

The min-content BLOCK size, `record.v_min_content_px` — COMPUTED in
this step (the named consumer §3.1 reserved it for). Per CSS Sizing
§5.1: the block size at min-content inline width.

- **Text**: (min-content buffer's run count) × line_height. The
  min-content buffer wraps at every soft-wrap opportunity, so its
  `layout_runs().count()` IS the fragment count — NO new cosmic call.
  Add `v_min_content_px` to `TextIntrinsicMetrics` (text_layout) and to
  the stored `TextIntrinsic` (dom_node.rs) computed exactly so (one
  more field; the canonical-serde carry updates with a comment, as in
  3a-i).
- **Img / element controls**: the intrinsic content height
  (`ImgIntrinsic.height_px` / the preferred-size height) — min-content
  block size of a replaced/control is its fixed block size.
- **Block**: Σ children's `v_min` (block stacking), reading child
  records (the up fold writes children first). Specified-height blocks:
  the specified height is a definite used size, not an intrinsic — the
  record's `v_min` still carries the content sum (the definite clamp
  happens in D1's `definite_specified_main_size`, not in the record).
- **Inline**: Σ children's `v_min` (at min-content width nothing shares
  a line; the inline model's approximation, named).
- **Nested flex container**: row inner → max over its items'
  `v_min` (a row's min block size = the tallest item's); column inner →
  Σ (its items stack). Read from child records at the inner container's
  exit.
- `v_max_content_px` stays `None` (no consumer; D6 discipline).

## D4. The hypothetical floor (exact)

At hypothetical computation (the existing `clamp_main_axis_size` call
site in item-view assembly):

```text
hypothetical = clamp(base, resolved_min, resolved_max).max(automatic_min_main_size_px)
```

Equivalent to the engines' clamp with `min' = max(min, auto_min)`
(including the `auto_min > max` case, where the floor wins exactly as
`min > max` wins today). `outer_hypothetical = hypothetical + margins`
(unchanged derivation — the floor then governs line breaking, killing
the wrap-point half of the hypothetical-floor divergence) and the
seed's source value is correct when step 5 switches the seed to
hypothetical.

## D5. What dies (the death list, exact)

- `supported_automatic_min_content_width_px` / `supported_automatic_min_main_size_px`
  (the gate + dispatch),
- `content_based_auto_min_main_size_px`,
  `horizontal_content_based_auto_min_content_width_px`,
  `horizontal_flex_container_content_based_auto_min_content_width_px`,
  `horizontal_flex_container_auto_min_child_border_box_contribution_px`,
  `text_min_content_width_px` (the manual whitespace-splitting leaf),
  `flex_container_content_based_auto_min_is_supported` (the support
  predicate itself),
- the two graceful-gate fallbacks and their comments,
- `HorizontalFlexContainerAutoMin...` test-only scaffolding that pinned
  the walk's internals (per the test-migration policy: genuine
  semantics re-express through the derivation; bespoke internals die).

The auto-min derivation is then exactly two reads (record + authored
style) and one `min` — no recursion of any kind.

## D6. The pin-flip table (exact)

| Row | New asserted behavior |
|---|---|
| `off_spec_hypothetical_main_size_not_floored_by_auto_min` (3 rows) | (a) wrapping container: the line-break point floors at auto-min (items break earlier, per the floored outer hypothetical); (b) inflexible visible item with base < content-min: floored at content-min; (c) content-min > container: overflow from the floored size. Family moves on-spec |
| `off_spec_flex_auto_min_nowrap_text_floors_at_widest_word` | nowrap auto-min == the full run width (mode-aware min-content from the record); control `normal` keeps widest-fragment. Moves on-spec |
| `off_spec_flex_auto_min_unsupported_shapes_use_measured_size` (gate 1, 5 rows) | nested column/reversed/wrap/rtl containers and the grandchild recursion case compute the true `min(min_content, definite)` (the pinned 200px fallback becomes the computed spec-correct value, e.g. 40px for the row-(a) scenario) — the gate and the measured-size over-estimate die. Moves on-spec |
| `off_spec_flex_auto_min_growing_children_use_measured_size` (gate 2, 3 rows) | a growing child no longer forces the fallback: the titlebar row's auto-min becomes the factor-blind content minimum (16px icon basis + empty title min-content, not 200px measured). Moves on-spec |
| NEW: column-container vertical auto-min | a column container's text item clamps shrink at its `v_min` (fragment count × line height), not at measured height; a definite-height item clamps at its definite height (D1's `min`) |
| NEW: v_min fold | block v_min == Σ children v_min; inline v_min == Σ; nested row flex v_min == max(items) |

**Audit rule:** any other proof moving under the new derivation (the
`measured_item_record_*` auto-min families, the nested-flex family,
composition families) follows the standing rule (spec-correct → update
with documented cause and report; spec-wrong → STOP). The overflow-zero
families (scroll/hidden/clip → auto-min 0) must NOT move.

## D7. R6's status (recorded; dormant)

The R6 register's condition — a consumer reading END-OF-PASS flex-item
records — does NOT fire in this step: the derivation reads records only
during the measurement fold (pre-overwrite, by construction; the outer
solve reads nested records before placement overwrites them). The
register stays open, owed to the first consumer that reads end-of-pass
values. The derivation's read discipline (fold-time only) is named in
the code comment so a future consumer can't drift into the pollution
silently.

## D8. Perf statement

The derivation adds zero measurement work: horizontal min-content is a
record read; vertical min-content for text reuses the min-content
buffer's run count (no new cosmic call); block/inline/nested v_min is
fold arithmetic. `TextIntrinsic` grows by one `f32` (the serde carry is
the sanctioned shape change, documented). No perf pin should move; any
that does gets the conditional documented-cause re-pin and a report
entry.

## D9. Explicitly untouched

The §9.7 seed (step 5's hypothetical-seed switch), the resolution
algorithm, the lanes, placement, paint, the overflow-zero gate
(unchanged semantics), `v_max` (no consumer).

## D10. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | the D1 derivation at item-view assembly, the D4 floor, the D5 death list, test migration |
| `crates/ui-dom/src/domains/text_layout/text_layout_context.rs` (+ `mod.rs` as needed) | `v_min_content_px` in `TextIntrinsicMetrics` from the min-content buffer's run count + tests |
| `crates/ui-dom/src/types/dom_node.rs` | `TextIntrinsic.v_min_content_px` (record field) |
| `crates/ui-dom/src/domains/layout/measurement_record.rs` | the fold's v_min computation for block/inline (reading child records) in the unified assembly |
| `crates/ui-dom/src/domains/paint/mod.rs` | the `TextIntrinsicMetrics → TextIntrinsic` mapping carries the new field (as in 3a-i) |
| `crates/ui-dom/tests/canonical_serde.rs` (+ fixtures) | carry the new field, comment (only if the snapshot includes `TextIntrinsic` — it does, per 3a-i) |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D6 family flips + new pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | four registry rows move on-spec |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D11. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any proof
moves spec-wrong (D6's audit rule); (3) the min-content buffer's run
count is NOT the fragment count under some admitted white-space mode
(prove or disprove per mode; if it lies, return with evidence);
(4) a perf pin moves without a documented cause. ONE commit, full gate
green.
