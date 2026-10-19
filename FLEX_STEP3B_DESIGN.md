# FLEX_STEP3B_DESIGN.md — abs-pos/`display:none` exclusion + the static-position model (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-19. Author: the architect. Authority: CSS
Flexbox 1 §4.1 (absolutely-positioned flex children), §5.4.1, CSS 2.1
§10.1/§10.3.7/§10.6.4, CSS Align 3; the engine corpus
(`.tmp/engine-corpus/`: Blink skips OOF children in
`ConstructAndAppendFlexItems` (`flex_layout_algorithm.cc:823-839`) and
resolves static position in `HandleOutOfFlowPositionedItems` (`:509-613`,
the `AxisEdge` mapping); Gecko skips placeholders in `GenerateFlexLines`
(`nsFlexContainerFrame.cpp:4287-4291`) and resolves via
`ReflowPlaceholders` (`:6502`)); FLEX_MAPPING §3.2 (the reviewer's
static-position amendment, adopted); the registered divergence
`off_spec_abspos_and_display_none_children_occupy_flex_slots`.

This document decides everything for step 3b. Anything unanswered is a
document DEFECT — stop and return to the architect.

---

## D0. Why this is one landing

Exclusion without abs layout would orphan abs children (no layout
path), so the two halves land together. `display:none` is complete in
itself (no box anywhere). The landing flips the registered row's
geometry AND gives abs children a real static position + real layout +
real paint.

## D1. The exclusion predicate (replaces the zero-item treatment)

In the measurement fold's child classification (the D1 predicate of
`FLEX_STEP3A_DESIGN.md`, amended here): a child with
`position: Absolute | Fixed` OR `display: None` is **EXCLUDED** — no
item view, no gap adjacency, no justify count, no line membership. AND:
excluded children are **NOT run-breakers** — inline runs join across
them (engines skip OOF/none children at formation, so a text–abs–text
sequence is one contiguous run). The 3a run predicate's "Breaker for
abs/none" rule is replaced by this section's rule.

Consequences for the fold: the item lane, order lane, line lane, and
resolution lane never carry excluded children. Justify distribution and
gap insertion read the item lane, so they follow automatically.

## D2. `display:none` children (complete behavior)

No box, no item, no layout, no paint (today's `FlowSize::default()`
early-return shape is preserved; only the slot/gap occupation flips).
Their DOM presence is inert in every pass.

## D3. The static-position model (the 0×0 order-modified insertion)

For each EXCLUDED child with `position: Absolute | Fixed`:

**Input:** the solved order-modified item sequence (the order lane),
the container's `justify_content` / `align_items` / `align_content`,
and the child's own computed `order` and `align_self`.

**Slot:** the position the child would occupy in the order-modified
sequence by `(order, dom sequence)` among the REAL items (stable: equal
orders keep DOM order).

**Static main position** — where a 0×0 item at that slot would start,
computed against the solved lines WITHOUT re-solving:
- The 0×0 item changes nothing about line breaking (zero outer size):
  real items keep their solved line membership. The slot's LINE is the
  line containing the item index interval the slot falls into (or the
  last line if the slot is past the end).
- Within that line: static main = the line's justify offset (the
  solved `main_offset` of the line's first item minus its first item's
  margin-main-start... precisely: recompute the line's packing for the
  line's items as solved) PLUS the sum of the outer resolved main sizes
  of the items before the slot PLUS one `gap` per adjacency before the
  slot. (Equivalent and simpler to verify: walk the line's solved
  items; the static position after `k` items is `item[k].main_offset +
  item[k].outer_main + gap`, with `k=0` yielding the line's initial
  offset; a slot past the end yields the position after the last item —
  no trailing gap.)
- Wrap/reverse: the same arithmetic in the container's axis mapping
  (the existing reversed-axis helpers apply unchanged).

**Static cross position** — per the child's `align_self` (falling back
to the container's `align_items`), as a 0-height item in its line
(Blink's `AxisEdge` semantics): `flex-start`/`start` → the line's
cross-start edge (plus line packing offset per `align_content`);
`flex-end`/`end` → the cross-end edge; `center` → line cross center;
`stretch` → cross-start with 0 size (a 0-height item has nothing to
stretch); `baseline` (unadmitted) → cross-start.

**Rectangles:** the static position is computed within the container's
CONTENT box; the abs layout's containing block is the container's
PADDING box (CSS 2.1 §10.1 — the reviewer's amendment). Keep the two
distinct: static-position computation reads the content-box geometry;
the abs-pos layout machinery receives the padding box as CB.

## D4. Abs children's layout and paint

- Layout: the existing abs-pos machinery (the fixed_layers path) lays
  out each abs/fixed child with the D3 static position and the
  container's padding box as CB — real abs-pos layout per CSS 2.1
  §10.3.7/§10.6.4 (the child sizes per its own rules, no longer
  zero-squashed).
- Paint: abs children paint in order-modified document order
  (§5.4.1). The step-2 derivation (`order_modified_children.rs`)
  already reads each child's computed `order` — excluded children carry
  their own `order` and participate in the paint derivation exactly as
  landed in step 2 (no amendment needed; verify with a pin).

## D5. The pin-flip table (exact)

Family `abspos_and_display_none_children_are_not_flex_items` (renamed
from the `off_spec_` family; old name in a relabel comment; registry
row moves on-spec):

| Row | New asserted behavior |
|---|---|
| (a) abs middle child between two items, `column-gap: 10px` | the abs child occupies NO slot: trailing item at 40 (= 30 + 10, one gap), NOT 50; the abs child gets its D3 static position and PAINTS |
| (b) abs only child | laid out per abs-pos rules at the D3 static position inside the container's padding box; PAINTS (today: 0×0 at origin, no paint) |
| (c) `display:none` middle child with gap | NO slot: trailing item at 40 (= 30 + 10), NOT 50 |
| (d) `display:none` only child (control) | UNCHANGED on-spec: container sizes as empty |
| (e) NEW: run joins across an excluded child | text + abs + text: ONE run item (the abs child excluded, not a run-breaker), no gap inside the run |
| (f) NEW: static position under `justify-content: center` | abs only child: static main = content-box center (0×0 centered), NOT main-start |
| (g) NEW: static position at an end slot | abs child after two items (`order: 1` on the abs child): static main = after the last item (no trailing gap) |

**Audit rule (binding):** any other proof moving under exclusion
(fixed_layers families, popup/fixed-position proofs, composition
families) is examined per-row: spec-correct → update with documented
cause and report; spec-wrong → STOP.

## D6. Explicitly untouched

The run-formation fold except the D1 predicate amendment; the lanes;
the solve; node-item measurement; the auto-min walk; blockification
(step 3c); the paint derivation except the D4 verification pin. No new
counters.

## D7. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | the D1 exclusion in formation, the D3 static-position computation, the D4 abs-child layout drive |
| `crates/ui-dom/src/domains/layout/flex_anonymous_run.rs` | the D1 predicate amendment (excluded children are not run-breakers) + unit test updates |
| `crates/ui-dom/src/domains/layout/flex_item_source.rs` | only if the source needs an excluded-child marker (decision: NOT needed — excluded children never become sources; do not add one) |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D5 family + new pins |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry move |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D8. The transcription contract

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (document defect — name it); (2) any existing
proof moves spec-wrong (D5's audit rule); (3) the D3 slot arithmetic
cannot be computed from the solved lines without re-solving (the
document claims it can — if false, return with evidence); (4) a perf
pin moves without a documented cause. ONE commit, full gate green.
