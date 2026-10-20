# FLEX_STEP3A_DESIGN.md — anonymous inline-run flex items (complete design)

Status: `COMPLETE DESIGN — no open questions; the transcription task is
mechanical`. Date: 2026-10-19. Author: the architect. Authority: CSS
Flexbox 1 §4/§4.1/§5.4.1, CSS Text 3 §4, CSS Display 3 §2.7; the engine
corpus (`.tmp/engine-corpus/`: Blink `layout_block.cc:147-171` +
`flex_child_iterator.cc:18-24`; Gecko `nsCSSFrameConstructor.cpp:8137`
+ whitespace drop `:8156-8182`); FLEX_MAPPING §3.2 (the reviewer's
amendments, adopted); the registered divergence
`off_spec_anonymous_inline_runs_are_not_merged`.

This document is the whole design. Sections D1–D10 decide everything;
§11 maps files; §12 is the transcription contract. Any implementation
question not answered here is a DEFECT IN THIS DOCUMENT — stop and
return to the architect; do not improvise.

---

## D0. The engine baseline (what we copy semantically)

Both engines form anonymous items BELOW the algorithm by mutating
their trees: Blink wraps each contiguous run of inline content in an
anonymous `LayoutBlock` at `LayoutBlock::AddChild`; Gecko wraps runs
into `-moz-anonymous-item` `nsBlockFrame`s in
`CreateNeededAnonFlexOrGridItems` and drops whitespace-only runs when
nothing wrappable follows. Semantic adopts: ONE item per maximal run;
whitespace-only runs dropped under collapsible conditions; the wrapper
inherits the container's style. Mechanism ours: NO DOM mutation — a
run is a virtual item (fold-local accumulation + a source range), per
the doctrine (DOM identity stability).

## D1. The run-break predicate

A flex container's in-flow children classify per their PRE-blockification
display (`LayoutDisplay` as routed today, before `InlineFlex → Flex`):

- **Member**: text nodes; `LayoutDisplay::Inline` elements.
- **Breaker**: `Block`, `InlineBlock`, `Flex`, `InlineFlex` elements;
  every `position: Absolute | Fixed` child; every `display: None`
  child (the last two keep TODAY's zero-item treatment — their
  exclusion is step 3b's flip, out of scope here; on 3b's landing they
  stop breaking runs, and the registry row flips then).

**The atom rule (named edge):** an `Inline` element participates in a
run as ONE ATOM — its whole subtree measures as one inline unit. The
engines split an inline box around block-level descendants (CSS 2.1
§9.2.1.1); we do not, in this landing. Named boundary: an inline
element with block-level descendants is measured atomically; if a proof
shows the split matters, it registers as its own row (expected rare;
the transcription task includes one probe test to check whether the
crate's inline measure even reaches that shape).

## D2. The item-source representation

`FlexItemSource` gains (revives) the variant:

```rust
AnonymousInlineGroup { first_child_key: DomNodeSlotKey, child_count: u32 }
```

Run identity = the first child's slot key (children are contiguous
siblings: walk `first_child_key` → `next_sibling` × `child_count`).
`FlexItemSource::key()` returns `first_child_key` for the variant
(documented). The variant is `Copy` like the rest of the enum.

## D3. The run-formation fold

Runs form inside the measurement fold's child walk (the
`build_staged_measurement_solve_from_container` collection phase):
children are classified by D1; consecutive Members accumulate into a
pending run; a Breaker (or the end of the child list) closes and emits
the pending run BEFORE the breaker is emitted as its own view (today's
path, unchanged).

```text
pending: Option<RunAccumulator> = None
for child in dom_order_children(container):
    match classify(child):                                 // D1
        Member  => { measure_child(child);                 // today's measure + record write
                     pending.get_or_insert_default().accumulate(child) }
        Breaker => { if pending.is_some() { emit_run(pending.take()) }
                     emit_node_item(child) }                 // today's path
flush: if pending.is_some() { emit_run(pending.take()) }
```

`emit_run` emits ONE item view per D5 and pushes it into the item lane
+ order lane exactly like a node item (uniform downstream).

### The accumulator and the join arithmetic (exact)

```rust
struct RunAccumulator {
    first_child_key: DomNodeSlotKey,
    child_count: u32,
    max_content_sum_px: f32,        // cursor (iii): plain sum of members' max-content
    widest_segment_px: f32,         // cursor (i): widest unbreakable segment (run min-content)
    trailing_fragment_px: f32,      // cursor (ii): the run's current trailing unbreakable fragment
    trailing_join_open: bool,       // may the NEXT member join to the trailing fragment?
    strut_line_height_px: f32,      // max fragment_line_height_max over members
    all_members_text_and_all_whitespace: bool,  // the drop predicate's data
}
```

Per text member (reading its 3a-i intrinsic: `min_content`,
`max_content`, `leading_fragment`, `trailing_fragment`,
`starts_with_wrap_opportunity`, `ends_with_wrap_opportunity`,
`fragment_line_height_max`):

```text
boundary_is_break = edge_is_break(trailing side) || edge_is_break(leading side)
    where edge_is_break(flag) = flag && mode_allows_wrapping(container_mode)
    // D7: nowrap/pre modes force flag=false — see D7.
joined = !boundary_is_break && !all_members_text_and_all_whitespace-is-first
if child_count == 0:
    widest_segment_px = child.min_content
    trailing_fragment_px = child.trailing_fragment
else if joined:
    widest_segment_px = max(widest_segment_px,
                            trailing_fragment_px + child.leading_fragment,
                            child.min_content)          // interior segments compete too
    // The joined segment TERMINATES at the child's first internal break:
    // single-fragment children (min == max) continue it through their whole
    // width; multi-fragment children REPLACE the trailing fragment with
    // their own trailing segment. (The original pseudocode used the
    // single-fragment form for both — the bug the third party caught
    // 2026-10-19: a multi-fragment join over-counts by prev_trailing.)
    trailing_fragment_px = if child.min_content == child.max_content
        { trailing_fragment_px + child.trailing_fragment }
        else { child.trailing_fragment }
else:
    widest_segment_px = max(widest_segment_px, child.min_content)
    trailing_fragment_px = child.trailing_fragment
trailing_join_open = !edge_is_break(child.ends_with_wrap_opportunity)
max_content_sum_px += child.max_content
strut_line_height_px = max(strut_line_height_px, child.fragment_line_height_max)
all_members_text_and_all_whitespace &= child.text_is_all_collapsible_whitespace
child_count += 1
```

(`trailing_fragment` of a single-fragment child IS its whole width, so
the single-fragment arm needs no width test beyond `min == max`; a
multi-fragment child always has `min < max` — total width exceeds the
widest segment — which makes the test exact.)

Per inline-ELEMENT member (an atom, D1): the crate's inline intrinsic
gives `min_content`/`max_content` but no edge fragments; the atom rule
for cursors: `leading_fragment = trailing_fragment = min_content`, and
both edges are BREAKS (no fragment joining across an element boundary
— the named approximation: a span's trailing text joined to a
following text's leading text with no space is under-measured; pinned
if it ever matters, registered if seen). Accumulate with the same
arithmetic using those values.

**The whitespace drop (D3's emit gate):** at `emit_run`, drop the run
(no view emitted) iff `all_members_text_and_all_whitespace &&
container_mode_is_collapse` (collapse = `normal` | `pre-line`; preserve
= `pre` | `pre-wrap` → the run forms an item). `child.text_is_all_collapsible_whitespace`
is computed by reading the text node's data (every char in the
collapsible set; the 3a-i helper set). An inline-element member makes
the run non-droppable (the span-only-whitespace edge: named, not
handled).

## D4. The run's measurement

- record.h_min = `widest_segment_px`; record.h_max = `max_content_sum_px`.
- **Measured cross at the measure width** (`measured_cross_size_input`):
  drive the EXISTING inline accumulation over the run's sibling range —
  the same per-child loop as `measure_inline_node_without_layout_write`
  (flex.rs:887-917: `InlineFlowState`, `next_inline_child_context`
  reducing available width by used line width, leaf text wrapping at
  the remaining width, height = max child height), sourced from the
  run's `[first_child_key, child_count)` range with the flex
  container's child context. This is reuse, not reshape: the wrapper is
  the loop with a different child source. The run measures EXACTLY as
  an inline element with the same children — the inline model's
  existing height approximation (max child height) is shared, not a new
  run-specific divergence. ESCALATION: if the loop cannot be sourced
  from a sibling range without new production behavior, STOP — the
  architect re-examines.

## D5. The virtual item view (exact fields)

| View field | Value |
|---|---|
| `source` | `AnonymousInlineGroup { first_child_key, child_count }` |
| `order` | `0` |
| `align_self` | `None` |
| `flex_grow` / `flex_shrink` | `0.0` / `1.0` — the CSS INITIALS: anonymous wrappers inherit nothing, and `flex-shrink`'s initial is 1, so anonymous items DO shrink in negative free space (clamped at their min-content). The 3a-ii landing shipped `0.0` in error (the architect's D5, corrected 2026-10-19); shrinking makes the D4 wrapping-cross path live (a shrunk run wraps), which the shrink-0 field had suppressed |
| `flex_base_size` | `record.h_max` (basis auto → content → max-content) |
| `hypothetical_main` | `flex_base_size` (no clamps; `base ≥ auto_min` always) |
| `automatic_min_main` | `record.h_min` (widest segment — §4.5 content-based minimum, no specified size) |
| `min_main` / `max_main` | `None` |
| `main_axis_border_padding` | `0.0` |
| margins (all four) | `0.0` |
| `measured_cross_size_input` | the D4 joint-measured height |

The run's content sizes come from the accumulator/wrapper, NOT a node
record (there is no node) — the one view exempt from the record-read
contract; documented at the site.

## D6. Placement — `relayout_anonymous_inline_run`

The run item resolves like any item (§9.7, line membership, alignment).
Placement lays out the run's children inside the resolved item box as
ONE inline formatting context:

- Drive the layout-write inline machinery over the sibling range
  (the same accumulation as D4 but the layout path: children written at
  inline positions from the resolved box's content origin, width
  reducing per `next_inline_child_context`), with the resolved item
  box as the inline container. Children's own node boxes are written by
  the normal paths; the anonymous block itself has no node and paints
  nothing.
- Style/inheritance: the children resolve against the flex container
  as today (it is their DOM parent) — the anonymous wrapper needs no
  style context of its own (its inheritance from the container is the
  children's existing inheritance).
- Cross size: the item's resolved cross = the joint laid-out height at
  the resolved width (the double-layout's usual final measure),
  consistent with node items' placement.

## D7. The mode-aware break rule (correcting the 3a-i flag usage)

A boundary is a soft-wrap opportunity iff BOTH: the mode allows
wrapping (`normal`, `pre-wrap`, `pre-line` — NOT `nowrap`, NOT `pre`),
AND the boundary char is collapsible whitespace. The 3a-i flags
(`starts/ends_with_wrap_opportunity`) capture the char + collapse-mode
half; the fold ANDs them with `mode_allows_wrapping(container_mode)`.
Consequences: under `nowrap`, members ALWAYS join (boundaries never
break) — the whole run is one unbreakable segment, so run min-content
== run max-content == sum (correct per nowrap); under `pre` (preserve,
no wrap), the same; under `normal`/`pre-line`, boundary spaces break
(the space collapses, no join); under `pre-wrap`, boundary spaces break
(preserved spaces still take width inside the members' own max-content
— already in the measurements).

## D8. The paint-order amendment (step-2 helper, run-aware)

`domains/paint/order_modified_children.rs` gains run awareness:
contiguous inline-level children (the D1 predicate) form ONE paint
unit that sequences with order `0`; members' own `order` values are
IGNORED inside a run (per §5.4: order applies to flex items — the
anonymous wrapper is the item; its children are inline content, not
items). Block-level items keep their orders; the fast path (all
uniform) still yields DOM order. Pins: (i) a span with `order: 1`
inside a run does not reorder (stays in the run's DOM position, layout
and paint agree); (ii) a run beside an `order: 1` block item sequences
the block last.

## D9. The pin-flip table (exact)

Family renamed `anonymous_inline_runs_merge_into_single_flex_items`
(old `off_spec_` name in a relabel comment; registry row moves
on-spec):

| Row | New asserted behavior |
|---|---|
| (a) text+text, `column-gap: 10px` | ONE item; second text contiguous at `text1_w` (NO gap); item width = max-content sum |
| (b) span+text | ONE item; text at `span_w`, no gap |
| (c) text+block+text (control) | UNCHANGED: two anonymous items + the block item; gaps BETWEEN items (10px each) |
| (d) whitespace-only text between two blocks, `normal` mode | run DROPPED: block 2 at `block1_w + 10` (one gap, no phantom item) |
| (e) NEW: whitespace-only under `pre-wrap` | run FORMS an item (preserved whitespace takes its width) |
| (f) NEW: fragment joining | `"foo"+"bar"` (no space) in shrink context: min-content = `w("foobar")`; `"foo "+"bar"`: min-content = `max(w("foo "),w("bar"))` |
| (g) NEW: order semantics | span `order:1` inside a run keeps DOM position (layout + paint); run vs `order:1` block: block last |
| (h) NEW: joint cross | two short texts on one line at the measure width: run cross == strut (NOT 2×strut) |
| (i) NEW: shrink with the CSS initial | a run in negative free space SHRINKS (`flex-shrink: 1`), distributing by scaled shrink factor and clamping at its min-content (the widest segment); a shrunk run's cross reflects the wrapping the shrink causes |

**The audit rule (binding):** any OTHER existing proof whose behavior
moves under run-merging (the §4 text-participation family, two-column
diagnostics, composition families) is examined per-row: spec-correct
new behavior → pin updates with a documented-cause comment and a
report entry; spec-wrong → STOP.

## D10. Explicitly untouched

The auto-min walk and graceful gates for NODE items; blockification
completion (step 3c); abs-pos/none exclusion (step 3b); the lanes and
the record for node items; the solve algorithm; the fast/slow paint
machinery except the D8 amendment. No new counters (run formation is
fold-local state; behavior is pinned by the proofs).

---

## 11. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex_anonymous_run.rs` | NEW: `RunAccumulator` + the D3 fold mechanics + the D7 rule + unit tests (cursors: join, no-join, nowrap-join, whitespace drop decision, element-atom) |
| `crates/ui-dom/src/domains/layout/flex_item_source.rs` | the D2 variant |
| `crates/ui-dom/src/domains/layout/flex.rs` | formation in the fold, the D5 view, the D4 wrapper, the D6 placement |
| `crates/ui-dom/src/domains/paint/order_modified_children.rs` | the D8 amendment + tests |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D9 family + new pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry move |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## 12. The transcription contract

The task implements THIS DOCUMENT exactly. STOP conditions (return to
the architect, do not improvise): (1) the D4 inline wrapper cannot be
sourced from a sibling range without new production behavior; (2) any
existing proof moves in a spec-wrong direction (D9's audit rule);
(3) any case this document does not answer (it is a document defect —
name it); (4) a perf pin moves without a documented cause. The full
gate per commit; ONE commit.
