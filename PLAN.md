# PLAN.md — ui-dom style system redesign (working brainstorm)

Living document for the style/CSS internals redesign. It began as a
brainstorm; as of 2026-10-17 it is a **decision record and design
specification**: `DECIDED` items and dated verdicts (ADOPT / ABSORBED /
DEFER / staged) are settled unless explicitly reopened; `PROPOSED`
markers survive only where a measurement checkpoint still gates the call
(S1's counters, open questions 5 and 6). Implementation follows the
spine; every step names its proof obligations and, where applicable, its
structural-compliance budget.

**Reading map (2026-10-17, self-contained edition).** This document is
written to be implementable by a reader with no conversation context:

1. **Root problems** (P1–P3) — what is broken for consumers, with the
   proofs that pin it.
2. **Decided constraints** (D1–D8) — the settled ground rules.
3. **Current architecture, verified** — background, invalidation
   boundaries, and layer-lifecycle boundaries: how the system works today,
   including its coarseness points (C1–C3) and preservation lists.
4. **Spec conformance contract** — the exact observable semantics we are
   bound to, and **engine data structures** — how Blink/Gecko/Servo
   actually store style data (source-verified).
5. **Vocabulary divisions** and the **presentation value table** — the
   property registry, with conformance gaps flagged.
6. **Proposals with adoption assessments** (A–J, S1) and the
   **algorithms** (A0–A11) — what we would build, what it costs, and the
   verdict shape for each.
7. **The decision record** — the substrate choice and what was deferred.
8. **Open questions** (deliberately open), the **implementation spine**,
   and proof obligations.

Historical note: comments in `domains/render_ops/`, `tests/render_ops_staging.rs`,
`tests/perf_metric.rs`, and `tests/perf_allocation.rs` reference `PLAN.md §x.y`
anchors. Those point to a previous planning document that no longer exists in
this repo. Do not renumber this document to match them.

---

## Root problems

Stated from the consumer's seat, verified against the pinned proof suite.

- **P1 — Programmatic styles are wiped out by style recompute.** Consumers
  reach for the programmatic styling methods (direct attr writes, e.g.
  `set_attr_str(node, "width", "10px")`) because they apply immediately and
  *look* correct. Any later style recompute — an unrelated class change, a
  hover, a stylesheet load — silently restores the stylesheet's value and
  claims the attribute; unloading the stylesheet deletes the attribute instead
  of restoring the direct value. Pinned as **divergent** by
  `off_spec_direct_attr_write_loses_to_stylesheet_on_next_recompute`
  (`tests/layout_proofs/cascade.rs`).
- **P2 — Two timing models.** Direct writes apply synchronously to the node's
  presented attributes; stylesheet declarations apply only on the style pass.
  The winner of a direct-write-vs-stylesheet contest is therefore
  *timing-dependent*, and the system as a whole is hard to predict.
- **P3 — One bag, many kinds.** `DomAttribute` conflates at least five
  different things (see "Vocabulary divisions" below), values stay
  stringly-typed from parse through storage to matching with every
  consumer re-parsing, and per-node storage mixes authored and derived
  values. As the supported property set grows — and as real nodes style
  sparsely — the current structures waste memory and re-do parsing work.
  The worst single corner: `color` and `background-color`, the two
  highest-traffic CSS properties, ride as `DomAttribute::Custom(String)`
  — a heap-allocated string per attribute instance, used as a BTreeMap
  key, string-compared on every cascade fold, string-matched at every
  paint consumer, and embedded in canonical snapshots.

## Background (verified current architecture)

Ground truth as of the proof-baseline merge (main @ `355772a`). Module docs:
`src/domains/style/README.md`, `src/domains/visitor/README.md`; proof
contract: `tests/layout_proofs/README.md`.

- **One attribute enum for everything.** `DomAttribute` carries HTML content
  attributes (`id`, `class`, `src`, `value`, `placeholder`, ...), CSS
  properties (`width`, `flex-grow`, `background-color`, ...), transition
  longhands, and widget-presented state (`PresentedOptions*`). The same enum
  names both what a consumer *sets* and what the node *presents*.
- **Stringly-typed values.** Declarations parse to `DomAttributeValue`
  (mostly `String`) and stay that way through the rule store, matching, and
  retained node attrs. Consumers re-parse at use: layout's `NodeStyle`, the
  inherited-style context, text measurement, paint.
- **Cascade = sort-then-fold.** `(origin.priority, specificity, order)` into
  `BTreeMap<DomAttribute, DomAttributeValue>`; later insert wins. No
  `!important`, no UA sheet, no cascade layers (registered **unimplemented**).
- **Style application owns what it touches.** `RootDom::apply_styles` writes
  the matched map onto retained attrs and records ownership in
  `style_pass_attrs_by_node`; unload removes owned attrs outright. It cannot
  distinguish a direct-authored value from a style-pass-applied one (P1's
  root cause).
- **Inheritance runs on two channels.** `color`, `font-size`, `text-align`,
  `direction` materialize into descendant attrs; `line-height`,
  `white-space`, `overflow-wrap`, `word-break` are context-carried into text
  intrinsics only (architecture pin, not a divergence).
- **Three writers of presentation values today:** the style pass
  (`apply_styles`), direct consumer writes (`set_attr_internal`), and the
  transition scheduler (presented-value overlay), plus behavior-provided
  pseudo-state declarations from `DomStyleObject` acting like a widget sheet.
- **Dirty-bit doctrine.** Per-node per-domain booleans, upward propagation,
  work-on-visit passes, zero-allocation metrics. This doctrine is **not** up
  for redesign.
- **Transition overlay.** `DomNode::presented_attr` consults the node's
  active-transition map *before* stored attrs, so a running transition wins
  over every stored value. This is observably equivalent to transitions'
  tier-1 position in the spec cascade — achieved mechanically, not by
  cascade participation. A candidate case of "internal shape free,
  observable answer already correct."
- **Public styling API surface today:** `RootDom::set_attr` /
  `set_attr_str` (validated string ergonomics consumers love),
  `RootDom::apply_styles` (public; **its only caller is the production
  style-recompute visitor** (`style_recompute.rs:165`) — it is the write
  path of the current pipeline, and it dies with that pass in the
  redesign; no external consumer uses it), `RootDom::compute_style_map`
  (public one-off
  style query; used by consumers as a measurement/debug probe — 10+ call
  sites in `stream-pro2`; browser-inherited freedom makes its successor
  an on-demand debug recalc, see open question 9),
  `RootDom::set_transition`;
  `RootDomContext::load_stylesheet` / `unload_stylesheet`.
- **Parse error contract today:** one malformed declaration fails the
  entire `load_stylesheet` / `parse_style_attr_map` call with
  `DomError::InvalidAttributeValue`. Browsers drop just the malformed
  declaration and keep the rest (CSS 2.1 §4.1.7, css-style-attr §3).
  **New divergence** — registry candidate; pin with an `off_spec_*` proof
  before the parser is redesigned.

## Current invalidation boundaries (researched 2026-10-17)

How a value change becomes scheduled work today, end to end. This is the
area flagged for improvement: more specific dirty setting, damage-rect
consumption, and style-propagation optimization.

**Write-side classification (static, per attribute).** Every attribute
maps to a fixed `AttributeInvalidation` bit set at compile time:
layout+paint (`attribute_affects_layout` — box and flex properties,
font-size/line-height/wrap policies), paint-only
(`attribute_affects_paint` — colors, transform, text-align, direction,
widget state), style (`attribute_affects_style_object` — exactly `class`,
`style`, `line-height`, `text-align`, `direction`, `white-space`,
`overflow-wrap`, `word-break`), and layer-membership (`display`,
`overflow`, `transform`, `isolate`, `position`). A write first passes a
same-value short-circuit (`set_attr_internal` compares the normalized new
value to the stored one and returns without dirtying) — with one caveat:
the transition-shorthand expansion runs *before* the comparison, so a
same-value `transition` write still re-expands its longhands. Otherwise,
dirty marking is already per-attribute *value-diff* driven — but the
*class* of dirtiness is static, never computed from the actual used-effect
difference (a `width` write that cannot change the used size still marks
layout).

**Upward propagation (layer-root bounded).** Layout and style dirtiness
walk parents to the enclosing layer root (`mark_dirty_up_to_layer_root`),
never across the boundary; layout early-exits at an already-dirty
ancestor, style always reaches the root so the layer-root style bit
summarizes its subtree. Paint dirtiness walks to the layout root
(`mark_node_dirty_up_to_layout_root`) carrying a *damage rect* computed
from the node's current border box: each ancestor accumulates it in a
bounded 8-rect `ArrayVec` bucket; overflow promotes the whole chain to
`FullLayerRepaint`; propagation stops at an already-full ancestor or the
layout root. Pseudo-state changes mark style dirty the same way
(`pseudo_state.rs`).

**Scheduling.** Composite is layer-first: lifecycle reconciliation, then
style → layout → paint → composite per live layer in DOM order.
`style_tree_needs_recompute` reads one bit on the layer root;
`tree_needs_layout` scans for layout-dirty content. The style recompute
pass itself crosses nested layer boundaries via the ancestor seed, and
its writes ride back through `set_attr_internal`, so changed values
re-dirty layout/paint per their static class. Intrinsic remeasurement
(text, controls, icons) marks dirty only on real measured change
(post-`generation`-removal equality).

**Damage rects: written, tested, never consumed.** The bounded-rect
machinery is real and unit-tested (`paint/mark_dirty.rs`), but the paint
stage repaints full layers and the rect pass unconditionally clears the
whole surface per layer per apply — the largest known performance item in
the crate, previously backlogged. D7 settles the verdict: the machinery
stays, targeted repaint is a goal (animation on large surfaces), and the
consumption design is approach I.

**Learning (2026-10-17): phantom dirties were real.** The removed
`generation` counters on intrinsic state were not just dead weight: the
preferred-size and icon construction paths compared them, so every style
recompute manufactured layout/paint work for form controls and icon
images. Equality-over-measured-fields fixed it, and no perf pin moved
(the pinned scenarios did not cover those node kinds). Lesson carried
into the redesign: change detection must always be over *measured
content*, never over counters or versions.

**Coarseness points the redesign should attack:**

- **C1.** Any style-affecting change dirties the *entire layer-root
  subtree* for re-matching; there is no per-element scoping and no
  matched-rule caching (Blink's matched-properties cache / Servo's rule
  cache have no analog).
- **C2.** Dirtiness *class* is static per attribute, not computed from
  the actual style difference (Gecko's change-hint / Blink's
  style-difference model classifies layout-vs-paint-vs-compositing
  consequences from the old/new comparison).
- **C3.** Style recompute folds the full matched map onto every visited
  element and relies on `set_attr_internal`'s compare to filter; cheap,
  but the pass walks everything under a dirty root even when one
  declaration changed.

## Layer lifecycle boundaries: creation, destruction, and the preservation list (2026-10-17)

**Scope rule:** the layer creation/destruction algorithms, the hit-test
and scroll algorithms, and the popup behavior are *preserved*, not
redesigned. The style system redesign touches this area at exactly one
seam: **what triggers layer-membership re-evaluation when a style change
flips a membership attribute.**

**The membership vocabulary (today).** Five attributes carry layer
consequences (`attribute_changes_layer_membership`): `display`,
`overflow`, `transform`, `isolate`, `position`. `layer_kind_for_node`
derives the node's layer kind from current attrs; the composite stage's
lifecycle reconciliation creates or destroys Bevy-backed layers to match
(layer-first scheduling, `composite/mod.rs`).

**Current trigger mechanics (write-time).** Every attr write computes the
previous and next layer kind inside `set_attr_internal`; a change marks
the node and walks the consequences: the *previous* layer root's
remainder is dirtied (`mark_previous_layer_dirty_when_membership_changes`),
newly connected layer roots are queued
(`queue_newly_connected_layer_roots_in_subtree`), and the next update's
lifecycle reconciliation performs the actual create/destroy. Transitions
receive the same treatment on the scheduler path (per-attribute
membership checks for active transitions).

**The redesign consequence (S1/A7).** In the new model, style-driven
presentation writes no longer pass through per-attribute write-time
detection: membership-relevant properties (`display`, `position`, `overflow`,
`isolate`, `transform`) resolve into the computed groups during the style
pass. The trigger therefore **migrates from the write path to the
group-diff classification**: A7's static table carries the membership
row — a box-group or transform-group change forces `layer_kind_for_node`
re-evaluation, and a kind change feeds the same three consequences as
today: previous-layer remainder dirtied, new-root queue, lifecycle
reconciliation on the same update. Observable behavior is pinned
unchanged: a style change that turns an element into a layer root
presents the layer by the end of that update, and destroying one dirties
the previous remainder identically.

**Preservation list (do not redesign):**

- **Lifecycle reconciliation** (`composite/mod.rs`): layer-first
  creation/removal against the live Bevy layer hierarchy.
- **Hit test and scroll:** `domains/hit_test.rs`, `domains/layer_hit.rs`,
  `domains/visitor/event_target.rs` (point → topmost target), and
  `domains/visitor/scroll.rs` + `apply_vertical_scroll_to_layer_root`
  (scroll application, viewport/content-height math). Behavior preserved
  verbatim.
- **Popup behavior** for context menus (browser-unique clamp/placement
  behavior): pinned by `popup_menus.rs` (fixed-row clamp math) and
  `popup_text_rows.rs` (text-row popup layout: growth, shrink, absolute
  positioning, wrapped rows).
- **Acceptance surface:** `fixed_layers.rs`, `popup_menus.rs`,
  `popup_text_rows.rs`, `transitions.rs` proofs, plus the hit/scroll
  unit tests in the files above. Any redesign change that moves these is
  a regression by definition.

## The spec and the Blink/Gecko approach

The user's direction has direct spec backing; browsers solved exactly P1/P2
by construction.

- **CSS Cascade 5 §4 (value stages).** Every property on every element has a
  ladder of values: *declared* → *cascaded* → *specified* → *computed* →
  *used* → *actual*. What an author sets is a **declared value**; what layout
  consumes is a **computed/used value**. The spec's own names confirm the
  suspicion that "the calculated styles have a different name than the
  settable ones." <https://drafts.csswg.org/css-cascade-5/#value-stages>
- **css-style-attr (CSS Style Attributes).** The `style` attribute is a
  *declaration block* that participates in the cascade at a defined
  precedence — above all normal author rules (CSS Cascade §6.4.3). Inline
  style is an **input** to the cascade, not a write to the computed style.
  <https://drafts.csswg.org/css-style-attr/>
- **CSSOM `CSSStyleDeclaration` and CSS Typed OM (`attributeStyleMap`).** The
  programmatic styling interfaces are declaration stores — stringly
  (CSSOM) or typed (Typed OM) — that dirty the element and let the next
  rendering update re-run the cascade. <https://drafts.csswg.org/cssom-1/>,
  <https://drafts.csswg.org/css-typed-om-1/>
- **Blink.** Inline style is a `MutableCSSPropertyValueSet` collected by
  `ElementRuleCollector` into `MatchResult` alongside stylesheet rules, with
  inline precedence applied at cascade sort. Computed style lives separately
  (`ComputedStyle`). Programmatic and stylesheet declarations resolve in the
  *same* pass: `Document::UpdateStyleAndLayout` runs style recalc before
  layout inside one rendering update (already pinned by
  `stylesheet_lifecycle.rs`). Files: `core/css/resolver/element_rule_collector.cc`,
  `core/css/resolver/match_result.cc`, `core/css/style_resolver.cc`,
  `core/css/css_property_value_set.h`,
  `core/css/style_recalc/style_recalc.cc`, `core/dom/document.cc`.
- **Gecko/Servo.** The style attribute parses into a
  `PropertyDeclarationBlock` collected as an applicable declaration with
  inline priority; specified and computed values are distinct type trees
  (`values/specified/`, `values/computed/`); the stylist
  (`servo/components/style/stylist.rs`) resolves one pass over both
  declaration sources. Files: `servo/components/style/stylist.rs`,
  `servo/components/style/properties/declaration_block.rs`,
  `servo/components/style/values/specified/mod.rs`,
  `servo/components/style/values/computed/mod.rs`. (Pin exact revisions when
  an approach is chosen, per the proof-tree convention.)
- **HTML presentational hints.** Presentational attributes map to CSS
  properties as author-level zero-specificity declarations *below* the
  author stylesheet (CSS Cascade §6.4.4). This is the spec's own answer to
  "two programmatic-feeling sources with different precedence" — a third,
  weakest tier, distinct from inline style.

**Key insight for P2.** Browsers make programmatic styling *feel* immediate
without a second timing model: a programmatic write mutates a declaration
store and marks the element style-dirty, and the frame loop's rendering
update re-runs the cascade before layout/paint. There is exactly one clock.
Our D2 adopts precisely this.

**Spec conformance contract (researched 2026-10-17).** The exact observable
semantics D6 binds us to:

- **Style attribute (css-style-attr §4):** declarations in a style
  attribute "are considered to have author origin and a specificity higher
  than any selector," and "must be interpreted exactly as if they were
  given in a CSS style rule that applies to the element." That is the
  entire inline-style contract: author origin, top specificity, same
  interpretation as rules.
- **Style attribute parsing (css-style-attr §3):** the value is a
  declaration-list parsed with forward-compatible error handling — a
  malformed declaration is skipped, never fatal; a stray `}` does not
  terminate the value. Stylesheets follow the same rule (CSS 2.1 §4.1.7:
  illegal declarations are ignored, parsing continues).
- **Cascade sorting order (CSS Cascade 5 §6.1),** descending:
  1. Transition declarations
  2. Important user-agent declarations
  3. Important user declarations
  4. Important author declarations
  5. Animation declarations
  6. Normal author declarations
  7. Normal user declarations
  8. Normal user-agent declarations
  Within an origin: (cascade layers,) specificity, then order of
  appearance.
- **Shorthands (CSS Cascade 5 §3):** "A shorthand property sets all of its
  longhand sub-properties, exactly as if expanded in place"; omitted
  longhands take their initial value; `!important` on a shorthand is
  `!important` on every longhand. Expansion happens **at declaration
  time**, which makes the declaration store the one expansion point in the
  system.
- **Presentational hints (CSS Cascade 5 §6.5):** non-CSS presentational
  attributes translate to author-level, zero-specificity declarations
  placed at the *start* of the author level — the weakest author tier,
  below every author rule, and strictly below inline style.
- **Defaulting (CSS Cascade 5 §7):** every property on every element ends
  with exactly one specified value; `initial`/`inherit`/`unset`/`revert`
  are the explicit defaulting keywords. (Our `unset` length keyword
  exists; the other keywords are currently unsupported.)

## Implementation data structures, source-verified (2026-10-17)

Correction of an earlier framing in this document: the engines do **not**
run one singular style data shape. They run a *family* of structures with
distinct roles, and they restructure them constantly. Any candidate for
us must be a family too.

**Servo/Gecko — `PropertyDeclarationBlock` (the declaration store),
verbatim from vendored source
(`servo/components/style/properties/declaration_block.rs`):**

    declarations: ThinVec<PropertyDeclaration>,   // ordered, deduplicated — NOT a map
    declarations_importance: SmallBitVec,          // parallel !important bits
    property_ids: PropertyDeclarationIdSet,        // fast membership (longhand bitset + custom name set)
    immutable: AtomicBool,                         // copy-on-write: set when shared across elements

Structural lessons: declarations keep *source order* (cascade
order-of-appearance and logical-group rules need it); importance rides a
parallel bit-vector; a separate id set answers "contains" without
scanning; the block is *shared* (the style attribute's block too) and
copied on write; update ops (`prepare_for_update`/`update`) compute
minimal edits and return *whether anything changed* — change detection
lives at the declaration level. Shorthands expand at parse into longhand
drains; serialization reconstructs them per CSSOM rules (logical groups
included).

**Blink — generated base + feature group structs
(`third_party/blink/renderer/core/style/`, current main):** a generated
`ComputedStyleBase` with per-property storage partitioned by inheritance,
plus hand-written group structs for feature areas
(`style_cached_data.h`, `style_highlight_data.h`,
`style_content_alignment_data.h`, `style_self_alignment_data.h`,
`style_flex_wrap_data.h`, `style_will_change_data.h`, ...). The group set
changes across versions — the durable constant is *grouping by
inheritance class and access frequency*, never one shape.

**Gecko — style structs + rule tree:** computed values split into
per-topic structs (font, color, background, border, box, text, ...) that
inherit as units and are shared structurally across elements through the
rule tree (see `stylist.rs`, `rule_tree/` as cited in `cascade.rs`).

**Change classification (both engines):** dirtiness is *computed from the
old/new style difference*, not statically assigned per property — Gecko's
change hints (`nsChangeHint.h`, name-level citation; pin at design time:
repaint-only vs reflow vs frame-reconstruct) and Blink's style-recalc
change types classify the consequence; matched-declaration caching
(Blink's matched-properties cache, Servo's `rule_cache`) skips
re-matching selectors when inputs are unchanged.

## Decided constraints

- **D1 (2026-10-17).** Programmatic styling methods write to a **different
  store** than the node's calculated styles — a declaration store, not the
  computed/presented attribute set.
- **D2 (2026-10-17).** The new store merges into the **same flow the
  stylesheets use**: one style pass, one timing, resolving declarations from
  every source in one place.
- **D3 (2026-10-17).** The node's calculated styles are resolved **only when
  dirty** and **only at the style stage** of the pipeline.
- **D4 (standing).** The dirty-bit doctrine, visitor-style passes, and
  zero-allocation metrics stay. Every move is gated by the proof suite:
  nothing on-spec becomes off-spec; off-spec → on-spec moves must follow
  Blink/Gecko algorithm shape (or carry a documented Rust/Bevy-justified
  mechanics divergence).
- **D5 (2026-10-17).** Migration is a **hard break** for consumers of the
  current direct styling writes, but the replacement API must keep the
  lesson of the existing one: string property names and string values with
  crate-side validation and normalization (the `set_attr_str(key, "width",
  "10px")` ergonomics users love), with typed constants optional above.
  Decision basis: trusting the caller's statement — assume unseen
  customers/clients; the styling API may break, and users love
  string-based attribute setting over enums. Local scans (100+ direct
  presentation writes in `stream-pro2`) are illustrative, not the basis.
  **Enforcement point (specified in review, 2026-10-17):** post-split,
  `set_attr*` on a *presentation* property fails fast with a typed
  `DomError` naming `style_mut` — loud at runtime, rule-honest, never a
  silent route and never a panic (boundary data). Content attributes keep
  `set_attr*` unchanged. Because callers are string-keyed, the compiler
  cannot flag the ~100+ known call sites; the enforcement error plus the
  migration artifact are the whole conversion story.
  **Release mechanics:** the break ships behind a version bump (v0.6.0
  candidate, given the canonical-snapshot break already on main) with a
  `MIGRATION.md` at repo root (what changed, the enforcement error, the
  `style_mut` mapping per call-site pattern) and a sister PR converting
  the known consumer call sites. Both are spine steps below.
- **D6 (2026-10-17, scoped in review).** Browser-conformance principle:
  **inputs, outputs, and timings match browser behavior *for the admitted
  vocabulary*.** Any observable deviance from Blink/Gecko semantics
  *within the admitted vocabulary* will be reported by users as a bug.
  Internal implementation shape remains free (documented Rust/Bevy-
  justified mechanics are fine). This resolves open questions 2, 3, 4, 6,
  and 7: the observable answers are the spec's answers.
  **The admitted vocabulary** is exactly the presentation value table;
  everything outside it (percentages, `calc()`, rem/viewport units,
  hex3/hex4, the wider named-color set, additional display keywords,
  margin/padding/border shorthands, `overflow-x/y`, ...) is
  *unsupported by policy*, not a bug.
  **Admission policy:** a property or grammar form enters the table only
  with (1) a named consumer need, (2) a spec citation plus the Blink/
  Gecko algorithm reference whose shape we adopt, and (3) a proof (or
  `off_spec_*` pin) added in the same change. This policy is the valve
  that keeps D6 and fail-closed parsing compatible.
- **D7 (2026-10-17).** Damage rects are **preserved**, and targeted
  repaint is a goal, not a deletion candidate: the win is animation on
  large surfaces where only a small region is animating. The rect-pass
  unconditional-clear decision lands with the consumption design
  (approach I).
- **D8 (2026-10-17).** **Preserve all functionality.** Nothing clients
  use is removed; where behavior is reinterpreted we bias toward
  Gecko/Blink semantics. Load-bearing example: transition styling via
  pseudo-state rules (`:hover`/`:active`/`:focus`) — a pseudo-state flip
  restyles the element, the style pass applies transition longhands for
  scheduling, the value change schedules the transition, and the
  transition overlay presents intermediate values until completion.
  That chain is preserved end-to-end.

## Vocabulary divisions (the "one bag" enumerated)

Divisions the spec and the current code both say exist, recorded as candidate
seams for any data-structure approach:

1. **Content attributes** (HTML layer): `id`, `class`, `src`, `type`,
   `value`, `placeholder`, `tabindex`, form-state attrs. Not CSS at all.
2. **Presentation declarations** (CSS properties): `width`, `flex-*`,
   `background-color`, ... — settable via stylesheet *and* programmatically.
   - 2a. **Inherited** (the closed 8) vs **non-inherited** — different
     transmission mechanics, already pinned.
   - 2b. **Shorthand vs longhand** — no expansion today (divergent,
     registered).
   - 2c. **The `Custom(String)` escape hatch and its fate:** today's
     color properties ride as `Custom(String)` with a closed name list
     (dom_attr_parse.rs). In the redesign, admitted custom presentation
     names become closed `PresentationProperty` variants;
     `Custom(String)` survives **only** as the content-side escape hatch
     for non-presentation data. Custom *presentation* names outside the
     closed table are governed by the admission policy (D6's scoping):
     names consumers actually use are admitted through it, so D8's
     "preserve all functionality" and fail-closed parsing coexist —
     admission is the valve, silence is not the answer.
3. **Presentational hints** — HTML attrs with CSS meaning (the spec's
   weakest tier); today they're indistinguishable from inline style.
4. **Behavior/widget-presented declarations** — `DomStyleObject` pseudo-state
   declarations and `PresentedOptions*` state; the UA-sheet analog.
5. **Transition-presented values** — the scheduler's overlay; in CSS,
   transitions/animations sit in the cascade as their own origin tier
   (CSS Cascade §6.1), *above* normal author declarations.
6. **Computed/used projections** — layout's `NodeStyle`, the inherited-style
   context, paint's reads: derived, currently re-parsed per consumer.

## Possible approaches for algorithm and data structures

Not mutually exclusive; a coherent design composes one from each row.
Each entry names its Blink/Gecko analog, what it fixes, its risks, what
it does to the off-spec registry, and — dated 2026-10-17 — a **verdict**
with the reasoning an adopter needs.

### A. Inline-style declaration store per element (css-style-attr analog)

Programmatic styling methods write a per-element **declaration block**
(separate from presented attrs). The style pass collects it as a cascade
input with inline precedence, alongside matched rules; the node's presented
attrs become a pure **computed projection** written only by the style pass.
- *Analog:* Blink `MutableCSSPropertyValueSet`; Servo
  `PropertyDeclarationBlock`. Directly implements D1–D3.
- *Fixes:* P1 (inline precedence beats stylesheet; unload restores it because
  the declaration store is consumer-owned and never claimed by the pass);
  P2 (one pass, one timing).
- *Registry:* reclassifies
  `off_spec_direct_attr_write_loses_to_stylesheet_on_next_recompute` →
  on-spec. This is its acceptance test.
- *Risks:* consumer-visible contract change for direct writes on
  presentation attrs (immediate-apply semantics end); needs a migration
  story and a release gate. Storage shape for sparse per-element
  declarations is an open question (see E below).
- *Conformance contract:* css-style-attr §4 verbatim semantics — author
  origin, specificity higher than any selector, interpreted exactly as a
  style rule. The declaration store parses per css-style-attr §3
  (declaration-list; malformed declarations skipped, see G).
- *API ergonomics (D5):* the programmatic API presents string names/values
  with crate-side validation (`set("width", "10px")`-class ergonomics),
  with typed builder sugar optional above it. It replaces `set_attr*` for
  presentation properties; content attributes keep `set_attr*`.
- **Verdict (2026-10-17): ADOPT** — the centerpiece; D1–D3 require it and
  its acceptance test already runs (off-spec, waiting to flip). Adopting
  gives P1+P2's fix and the unload-restore behavior for free (A11). Costs:
  the hard-break migration (D5, accepted) and the S1 substrate beneath it.
  Without it: the redesign's main user-visible bug stays. Lands together
  with B and C on S1.

### B. Typed value model at declaration time (Typed OM analog)

Parse declarations once, at the declaration store's boundary, into typed
values (length, color, keyword, number...) behind a closed property table;
matching, storage, and consumers all carry typed values instead of strings.
- *Analog:* css-typed-om-1; Servo's `values/specified` vs `values/computed`
  type split.
- *Fixes:* P3's re-parse-per-consumer cost; gives the cascade a real
  shorthand-expansion point (expansion at declaration time, per CSS).
- *Registry:* enables fixing `off_spec_shorthand_and_longhand_coexist...`
  (divergent) — but expansion policy is its own decision.
- *Risks:* biggest surface area of any approach; touches every consumer;
  must not grow allocation on the hot path (zero-allocation doctrine);
  keyword/length tables must stay closed and strict (fail-closed parsing
  stays).
- *Shorthand expansion point (D6 + Cascade §3):* expansion happens at
  declaration-parse time inside the store, so the cascade and all
  consumers only ever see longhands. The shorthand divergence resolves as
  a by-product of adopting B, not as separate work.
- **Verdict (2026-10-17): ADOPT** — but never alone: A's store is where
  typed values live, so A and B land as one work item ("the declaration
  store"). The value table (below) is its registry. Adopting gives the
  end of re-parse-per-consumer and the shorthand fix by construction.
  Costs: the largest surface area (every consumer touched) and strict
  allocation discipline on the parse path. Without it: A would store
  strings and the fold would re-parse per node — pointless.

### C. Split the attribute vocabulary by role

Separate `DomAttribute` into distinct sets for content attributes,
presentation declarations, behavior-presented state, and transition
longhands — so "settable" and "presented" stop sharing one namespace.
- *Analog:* the spec's own divisions (HTML content attributes vs CSS
  properties vs presentation hints).
- *Fixes:* P3's "one bag" confusion; makes the divisions in "Vocabulary
  divisions" explicit in the type system; clarifies what the style pass may
  write (only presentation).
- *Risks:* large mechanical migration across the public API; naming must be
  settled before implementation (see "Value-stage vocabulary").
- **Verdict (2026-10-17): ADOPT** — a prerequisite, not a choice: B's
  `PresentationProperty` enum *is* the split. Adopting gives a type system
  in which the style pass can only write presentation and consumers can
  only declare it — the "one bag" confusion ends structurally. Costs: the
  mechanical migration (large but behavior-preserving; the proof suite is
  the gate). Without it: B has no closed property vocabulary to type.

### D. Provenance-layered storage on the node

The node keeps declaration sources layered by provenance — direct-authored,
stylesheet-pass, inherited, behavior — instead of one flattened attr map, so
removal of any layer restores the next.
- *Analog:* Blink/Gecko's separation of declarations from computed style;
  Gecko's rule tree shares declaration blocks structurally.
- **Verdict (2026-10-17): ABSORBED — do not adopt separately.** A11 made
  the layering algorithmic: the pass re-folds winners from current
  sources, so restore-not-delete falls out of the cascade itself and
  `style_pass_attrs_by_node` dies by construction. What survives of D is
  the *principle* (provenance is visible in the fold order), not a
  storage layer.

### E. Sparse declaration storage shape

However declarations are stored, the physical structure should cost ~nothing
for unstyled nodes and scale with properties actually declared, not with the
property vocabulary (pooled declaration blocks, small-map structures, or
interned property ids).
- *Analog:* Servo's shared `PropertyDeclarationBlock` storage; Blink's
  property-id-indexed sets.
- **Verdict (2026-10-17): ABSORBED into S1 — with the reckoning made
  honest.** The sparse answer is S1's concrete shape, but its "~nothing
  for unstyled nodes" goal is only half-met: unstyled nodes save
  `decl: None` yet still carry the seven-group inline footprint (and
  unstyled *descendants* must carry copied inherited groups regardless —
  inheritance). The mitigating truth the earlier verdict missed: today's
  unstyled descendants are not sparse either — each pays four
  materialized inherited attrs, including a heap-allocated `Custom`
  string for color. The group footprint is the price of visit locality,
  and the comparison is closer than the goal statement implied. The
  remaining judgment — arena crate and whether the group split earns its
  keep — is measurement, pinned by counters (open question 6).

### F. Cascade tier growth path (how the absent features slot in)

The current precedence tuple `(origin.priority, specificity, order)` is a
projection of the §6.1 tier list onto one axis. The growth path toward spec
coverage, staged by consumer need and each stage proof-gated: importance
flag per declaration → built-in UA tier (a real sheet below all author
rules, replacing ad-hoc behavior styling) → animation tier → cascade
layers. The sorting structure should be designed now so these are additive
(a wider precedence key), not a rework. Observable behavior at each stage
is pinned first; internal mechanics stay free per D6.
- **Verdict (2026-10-17): ADOPT THE STRUCTURE, DEFER THE STAGES.** The
  precedence key is widened in the declaration-store design so importance,
  a UA tier, an animation tier, and layers are additive later; no stage
  ships until a consumer needs it (open question 5 stays open on
  purpose). The first structural tenant is the behavior sheet, placed in
  the fold below author rules and inline style. **Behavior-sheet storage
  (specified):** per-tag declaration data — today's
  `DomElementStateStyleDeclarations` (per-element smallvec) becomes
  arena-resident blocks keyed by tag behavior and pseudo-state, folded
  first and conditionally on state, exactly as today; no per-element
  duplication is introduced.

### G. Declaration-drop parsing (spec error handling)

Both declaration stores (stylesheet and inline) parse with
forward-compatible error handling: malformed declarations are dropped,
parsing continues, and the drop is reported by metric, not silence.
Replaces today's whole-sheet rejection. Registry: new `off_spec_*` pin of
the current behavior first; reclassify on landing.
- **Verdict (2026-10-17): ADOPT FIRST.** Small, spec-backed
  (CSS 2.1 §4.1.7, css-style-attr §3), independent of every other
  proposal, and the acceptance spine's opening move: pin the current
  whole-sheet rejection as off-spec, then land drop-and-continue. Adopt
  before the declaration store so the store is born with the correct
  parse contract.
- **Repo-rule adjudication (recorded, 2026-10-17):** drop-and-continue
  is a scoped exception to AGENTS.md's "fail fast and be strict" for
  boundary data, adjudicated by D6 (CSS mandates the behavior) and made
  rule-shaped by its form: drops are counted by metric *and* returned in
  an explicit report type — never a graceful silent fallback. Single-
  declaration APIs stay strict (see `StyleMut::set` vs `set_all`). When
  G lands, AGENTS.md gains the scoped amendment in the same change.

### H. Difference-computed dirty classification

Replace or augment the static per-attribute dirty classes with a
consequence classification computed from the old/new comparison at apply
time (Gecko change-hint / Blink style-difference shape): a change whose
used effect is nil dirties nothing; a paint-class change never dirties
layout. This is coarseness point C2's fix. Cheaper variant: keep static
classes but add used-effect short-circuits at known hot properties.
- **Verdict (2026-10-17): ADOPT — and it is already designed.** S1's
  group-diff classification (A7) is H at group granularity: seven `Copy`
  compares map to layout/paint/font-descendant classes, with the
  layer-membership row added (see the layer-lifecycle section). Adopting
  costs nothing beyond S1; without it, C2 stands.

### I. Damage-rect consumption

D7 settled the verdict: the machinery is preserved and targeted repaint
is a goal — animation on large surfaces with small animated regions is
the driver. Remaining design work: the consumption side (the rect pass
reads damage buckets and repaints only damaged regions) and the
unconditional per-layer clear decision that lands with it (the largest
known perf item in the crate). The style redesign determines the
producer side (what damage a style change emits: A7's paint-class row).
Gate: the perf pins in `perf_metric.rs`/`perf_allocation.rs`.
- **Verdict (2026-10-17): ADOPT AS A STAGED GOAL.** It is not part of
  the style-landing work item; it follows, consuming A7's damage output.
  Deleting the machinery is off the table (D7).

### J. Scoped style propagation and matched-rule caching

Attack coarseness points C1/C3: scope recompute to elements whose matched
set can actually change (selector-aware invalidation, the shape of
Blink's invalidation sets for class/id changes), and/or cache matched
rule sets per element (Blink matched-properties cache / Servo
`rule_cache`) so re-matching is skipped when the rule store and element
state are unchanged. Must respect the zero-allocation metrics doctrine,
with metric counters pinned before and after.
- **Verdict (2026-10-17): DEFER — evidence gate.** C1/C3 are real
  coarseness points, but the dirty-bit doctrine plus subtree scoping may
  already be adequate for real workloads, and caching adds an
  invalidation surface that must be proof-gated. The deciding evidence:
  style-pass counters (nodes visited vs. nodes changed) measured *after*
  the declaration store lands. Do not build it speculatively.

## Value-stage vocabulary (proposed naming to settle)

Candidate adoption of the CSS Cascade §4 ladder, so code names stop
colliding:

- **declaration** — what a consumer or stylesheet sets (style attribute,
  stylesheet rule). Store: declaration block.
- **computed presentation** — what the style pass writes onto the node after
  the cascade and inheritance run (today's presented attrs).
- **used projection** — what layout/paint derive per frame (`NodeStyle`,
  inherited-style context): derived, never stored as authoritative.
- Settle: whether "computed" or "resolved" names the node-level store;
  whether the programmatic API names itself after the style attribute
  (`set_style`/`style_mut`) to make the inline-style contract legible to
  consumers.

Mapping to the current pipeline (research-confirmed 2026-10-17):

| Cascade §4 stage | Meaning | Current `ui-dom` home |
| --- | --- | --- |
| declared | every declaration applying to the element | stylesheet rule store + (future) inline declaration store |
| cascaded | the one winning declaration | the sort-then-fold in `matched_style_rules_for_element` |
| specified | defaulted; exactly one per property per element | implicit in the folded map (no explicit stage today) |
| computed | absolutized for inheritance (§4.4: the value inheritance transfers) | presented attrs on the node + the inherited-style context |
| used | resolved through layout | layout's `NodeStyle` + box solving |
| actual | environment-adjusted | staged paint ops |

This mapping is why D1–D3 are safe: the node-level store already *is* the
computed stage. What is missing is the declaration stage for programmatic
input and an explicit specified/defaulting stage.

## The decision record (2026-10-17)

What the variant exploration settled, and what it deliberately left open.

- **Substrate: proposal S1, adopted as the leading candidate.** Its
  verdict shape is measurement: decl-arena and allocation counters pinned
  before/after, with the flattened-attrs fallback (below) retained if the
  group split measures poorly.
- **Variant 1 (Servo-like):** its essence — inheritance-grouped computed
  structs — is adopted *inside* S1, without COW. Its excesses (interning
  arenas, id-set machinery, rule cache) are dropped or deferred.
- **Variant 2 (Blink-like):** its distinct piece — matched-rule caching —
  is deferred per proposal J's evidence gate. Everything else of value in
  V2 is S1.
- **Variant 3 (minimal evolution):** retained as the fallback branch. If
  S1's counters reject the group split, V3 is the landing shape: the
  inline declaration store (A+B) still lands, but the flattened attr map
  stays as the computed store and H/J/I are re-scoped to it.
- **Left open on purpose:** open question 5 (tier admissions — staged by
  consumer need) and open question 6 (sparse-storage final call —
  decided by counters). See the open-questions preamble.

## Implementation spine

The landing order, each step gated by the full suite (fmt, clippy,
workspace tests, `cargo xtask lint`) plus the listed proof obligation.
Structural compliance (file placement, inline tests, README updates) is
specified in its own section and attached to steps 2 and 3.

0. **Housekeeping.** Re-point or delete the ten stale `PLAN.md §x.y`
   references in `perf_metric.rs`, `perf_allocation.rs`,
   `render_ops_staging.rs`, and `render_ops/mod.rs`. No behavior change.
1. **Registry honesty first.** Pin the declaration-error-handling
   divergence (`off_spec_*`, G) and the font-family inheritance gap
   (divergence candidate 3) as running proofs. No behavior change.
2. **The declaration store (A+B+C on S1).** `PresentationProperty`
   vocabulary split (C), typed `DeclValue` parsing with drop-and-continue
   semantics (B+G, shorthand expansion at parse — including the
   `transition`-shorthand admission and `gap`/`border-radius`
   expansions), the arena-backed `DeclStore` for rule blocks and inline
   blocks, and the `style_mut` API with its two contracts (D5). Gate:
   cascade families and stylesheet-lifecycle proofs unchanged in outcome;
   new drop-count metric pinned; G's proof relabels on-spec;
   `src/domains/style/README.md` rewritten in the same change.
3. **The computed projection.** Winners fold (A4), the seven inline
   groups (S1), defaulting/inheritance resolution (A5/A6), group-diff
   classification with the layer-membership row (A7/H), `style_mut`
   writes flowing through the one cascade timing (D2/D3). Gate: the
   full suite unchanged; `off_spec_direct_attr_write...` relabels
   on-spec (A's acceptance test); **the font-family pin relabels here —
   group-copy inheritance transmits it for free in this step**;
   `style_pass_attrs_by_node` and `apply_styles` are deleted with the
   old pass (A11, corrected open question 8); the `set_attr*`
   enforcement error for presentation properties activates (D5);
   `src/domains/visitor/README.md` rewritten in the same change.
4. **Measurement checkpoint.** Counters before/after: decl-arena
   live-block accessor values, style-pass nodes visited vs. changed,
   allocation pins, intrinsic-memory baselines. This checkpoint decides
   open question 6 and whether proposal J ever wakes up. **Also the
   compute_style_map audit (open question 9):** enumerate consumer probe
   call sites for per-frame usage before the debug-priced successor
   ships; any hot call site gets a migration plan in the same release.
5. **Staged followers, each its own work item:** damage-rect
   consumption (I, per D7); matched-rule caching (J, only if step 4's
   evidence demands it); tier admissions (F stages, per consumer need);
   **font-family *consumption*** — the real font-family follower:
   text-layout currently hardcodes the UiSans family and ignores the
   attribute entirely (`text_layout_request.rs`); plumbing the computed
   family into the font catalog is its own task with its own proofs.
6. **Release.** Version bump (v0.6.0 candidate) with `MIGRATION.md`
   (the API mapping, the enforcement error, the snapshot-contract
   notes) and a sister PR converting the known consumer call sites
   (~100+ presentation `set_attr_str` uses in `stream-pro2`).

## Proposal S1 — arena-resident style storage with view accessors (2026-10-17)

**Status: ADOPTED as the leading candidate (verdict 2026-10-17), gated on
its measurement checkpoint (spine step 4); the flattened-attrs fallback
(V3) is retained if the counters reject the group split.** The physical
substrate for the
redesign, built from patterns this crate already runs: arena stores with
generational keys (rules, nodes, layer slots), validated leases (the
layer pool), and borrow-view discipline (`as_*` accessors that hand out
views, never ownership or clones — the `Option::as_ref` / `as_slice` /
`SlotMap::get` standard). Two placement rules, each with its rationale:

- **Declarations live in an arena.** Rule blocks and per-element inline
  blocks are fetched only during the style pass, so indirection there
  costs nothing on the visit. The arena crate is a category choice
  (slab / generational-arena / slotmap-class), decided by measurement —
  not a SlotMap default.
- **Computed style travels with the node.** `DomNodeBase` already
  inlines `box_model`, `intrinsic`, `layout`, `paint`, `style`, and
  `transitions`, so a visitor holding the node has everything on one
  fetch (visit locality). Computed groups follow the same rule: inline
  in the node record, replacing the presentation half of today's
  flattened attrs. An earlier draft of this proposal keyed computed
  groups into a side arena; it was rejected because layout, paint, and
  text passes would pay a second lookup per node per visit.

### S1.1 The structures (signatures, not final code)

Declaration side — one arena for *all* declaration blocks (stylesheet
rules and per-element inline style):

    enum DeclValue {                          // typed at parse
        Length(DeclLength),                   // Unset | Px(f32) | Em(f32)
        Color([f32; 4]),
        Keyword(KeywordId),                   // u16, per-property closed tables
        Number(f32),
        Integer(i64),
        Text(TextKey),                        // font-family, transform: interned text
    }
    // Precisely: no *per-node* string copies. Text-valued properties
    // (font-family, transform, transition lists) intern in a bounded text
    // arena — an open cost, flagged: the arena grows with distinct
    // authored strings until an admission/eviction policy lands
    // (candidates: per-document epoch reset; eviction at block removal).
    // Decided with the arena-crate measurement (open question 6).

    struct Declaration { property: PresentationProperty, value: DeclValue }
    // PresentationProperty: the closed presentation vocabulary AFTER the
    // role split (approach C). Content attributes stay DomAttribute.

    struct DeclBlock {
        declarations: SmallVec<[Declaration; 4]>,  // source order preserved
        present: PropertySet,                      // fixed bitset over PresentationProperty
        // importance bits arrive with tier path F — parallel SmallBitVec, Servo shape
    }

    struct DeclStore { blocks: SlotMap<DeclBlockKey, DeclBlock> }
    // StyleRule holds DeclBlockKey instead of an embedded attr_map.
    // An element's inline style is its DeclBlockKey (None when never written).

Computed side — inheritance-grouped, typed, stored inline in the node
record (beside `box_model`, `layout`, `paint` in `DomNodeBase`):

    // Inherited groups (seed from the parent's group during the pass):
    struct ComputedFontGroup  { size_px: f32, line_height_px: Option<f32>,
                                family: TextKey, weight: u16, style: FontStyleKw }
    struct ComputedTextGroup  { align: TextAlignKw, direction: DirectionKw,
                                white_space: WhiteSpaceKw, overflow_wrap: OverflowWrapKw,
                                word_break: WordBreakKw }
    struct ComputedColorGroup { color_rgba: [f32; 4] }
    // Non-inherited groups (seed from initial values):
    struct ComputedBoxGroup   { display, position, overflow,
                                width, height, min_w, min_h, max_w, max_h,
                                margin[4], padding[4], border[4], top, left, right, bottom }
    struct ComputedFlexGroup  { direction, wrap, grow, shrink, basis, order,
                                justify, align_items, align_self, align_content, gap[2] }
    struct ComputedVisualGroup{ background_rgba, border_rgba[4], radius[4],
                                outline_color, outline_width, outline_offset,
                                caret_color, opacity }
    struct ComputedTransformGroup { translate_px: [f32; 2] }
    // A seventh single-purpose group: transform is paint-class (never
    // layout) AND layer-membership-relevant — it cannot live in the box
    // group (whose class is layout+paint) without changing today's dirty
    // classes. Its diff row: paint + membership re-evaluation.
    // Every field typed; lengths resolved to px at style time where the
    // grammar allows (em resolves against the element's own font size —
    // computable without layout). All groups are small plain Copy structs:
    // shared by value, compared by value, no refcounting or COW anywhere.

    // The node's presentation state becomes:
    struct NodePresentation {
        decl: Option<DeclBlockKey>,   // inline style block, arena-resident
        computed: ComputedGroups,     // the seven groups, inline by value
    }

Text-valued properties (`font-family`, `transform`) intern their strings
in a small string arena (`TextKey`), or stay stringly node attrs until
approach B's value-type table lands — deferred detail, flagged.

### S1.2 The methods (signatures and contracts)

Declaration writes (the D5 programmatic API, css-style-attr §4 semantics):

    impl RootDom {
        fn style_mut(&mut self, node: DomNodeSlotKey) -> StyleMut<'_>;
    }
    impl StyleMut<'_> {
        /// STRICT single-declaration contract: parses and upserts one
        /// declaration into the node's inline block, then marks style
        /// dirty up to the layer root. Malformed input is a typed
        /// `DomError` — fail fast, no silent drops (repo rule).
        /// Never marks layout or paint directly: the pass classifies.
        fn set(self, property: &str, value: &str) -> Result<(), DomError>;
        /// DECLARATION-LIST contract (G semantics): parses a
        /// declaration-list; malformed declarations are dropped and
        /// parsing continues (CSS 2.1 §4.1.7). The returned report names
        /// applied and dropped counts — an explicit report type, never a
        /// graceful silent fallback.
        fn set_all(self, declaration_list: &str) -> Result<DeclDropReport, DomError>;
        fn remove(self, property: &str) -> Result<(), DomError>;
    }
    // The two contracts are deliberately different: a single-declaration
    // call has no "rest of the list" to continue to, so fail-fast is the
    // honest answer; a list call follows CSS error handling and reports.

Computed reads (the `as_*` standard: borrows, no ownership, no clones):

    impl RootDom {
        fn computed_style(&self, node: DomNodeSlotKey) -> ComputedStyleRef<'_>;
    }
    impl ComputedStyleRef<'_> {
        // Every accessor merges the node's active-transition overlay
        // (transitions.rs's chain is load-bearing, D8): the presented
        // value wins where a transition is running on that property.
        fn as_font(&self) -> ComputedFontView<'_>;
        fn as_text(&self) -> ComputedTextView<'_>;
        fn as_color(&self) -> ComputedColorView<'_>;
        fn as_box(&self) -> ComputedBoxView<'_>;
        fn as_flex(&self) -> ComputedFlexView<'_>;
        fn as_visual(&self) -> ComputedVisualView<'_>;
        fn as_transform(&self) -> ComputedTransformView<'_>;
    }
    // The debug successor to `compute_style_map` is a separate on-demand
    // recalc query (open question 9), NOT a read over this view.

The style pass per dirty element (D2/D3 mechanics, visitor doctrine):

    1. seed inherited groups from the parent's groups (ancestor seed at
       layer roots — unchanged mechanics)
    2. fold declarations into a stack-local fixed array:
         winners: [Option<(DeclValue, Precedence)>; PRESENTATION_PROPERTY_COUNT]
       source order: behavior sheet (UA-ish tier) → matched rules sorted by
       (origin.priority, specificity, order) → inline block (top specificity)
    3. resolve the seven groups from winners + inherited seeds
    4. diff old vs new groups (seven memcmp-cheap Copy compares) → classify:
         box_ changed       → mark layout+paint + layer-membership re-eval
         flex changed       → mark layout+paint
         transform changed  → mark paint + layer-membership re-eval
         visual/color       → mark paint only (damage rect)
         font/text          → mark layout+paint + remeasure text intrinsics
    5. clear the style bit

    (Inheritance propagation needs NO children-marking row: the pass walks
    the full dirty subtree, so every visited descendant is re-seeded from
    its parent during the same pass — A5. A pass started above a nested
    layer boundary crosses it (`style_recompute_crosses_nested_layer_
    boundaries`); a clean nested layer root is a strong no-op even when
    ancestors change (`..._clean_layer_is_strong_noop_even_when_ancestor_
    changes`). Both pins hold unchanged.)

### S1.3 Flow diagrams

Storage topology:

    DeclStore arena (style pass only)     DomNode record (one fetch per visit)
    ┌───────────────┐                     ┌────────────────────────────────────┐
    │ K1: rule blk  │◄─── StyleRule       │ base: style, layout, paint,        │
    │ K2: rule blk  │     { decl: K1 }    │       box_model, intrinsic,        │
    │ K3: row blk   │◄─── node decl key   │       transitions, links           │
    └───────────────┘                     │ computed: font, text, color,       │
                                          │           box_, flex, visual,      │
                                          │           transform                │
                                          │   (inline Copy structs, typed)     │
                                          └────────────────────────────────────┘

    read: root.computed_style(node).as_visual().background_rgba
          → &[f32; 4]  (borrow into the node; zero indirection)

Write → invalidation flow (the P2 timing fix):

    root.style_mut(row).set("background-color", "#2A313D")?
        │
        ▼  typed parse; malformed → dropped + metric (G)
    DeclStore: upsert into row's DeclBlock
        │
        ▼  mark style-dirty: row → … → layer root   (ONE bit per node)
        │  (layout/paint NOT marked here — the pass classifies)
        ▼
    next update → style pass: winners fold → resolve groups → write inline
        │
        ▼  group diff: visual changed; box_/flex/font/text/color unchanged
        ▼
    paint-only dirty + damage rect; layout never touched

Inheritance flow (unifies the two current channels):

    parent  { font, text, color, … }   (by value)
        │   DFS carries the parent's inherited groups
        ▼
    child winners { font-size: 20px }
        font  = parent.font with size_px = 20.0   (new value, copied)
        text  = parent.text                        (Copy)
        color = parent.color                       (Copy)
        ▼
    grandchild, no declarations: all three groups copied verbatim

### S1.4 Tradeoffs, made visible

| axis | today (flattened attrs) | Arc+COW groups (rejected) | S1 (arena decls, inline computed) |
|---|---|---|---|
| node presentation storage | BTreeMap + heap strings | 6 Arc ptrs + refcount | seven inline Copy groups (styled nodes only) |
| inline write cost | normalize, map insert, static dirty class | COW clone-on-write | block upsert + one style bit |
| read path | `presented_attr(...).cloned()` | Arc deref | borrow into the node, zero clones |
| recompute diff | per-attr compare in `set_attr_internal` | deep group equality | seven memcmp-cheap Copy compares |
| inheritance | materialize strings into child attrs + context carry | Arc clone per group | value-copy of small groups |
| N identical rows | N full maps | N ptrs + 1 shared group | N compact inline copies (sharing sacrificed for visit locality) |
| shared-data mutation | n/a | AtomicBool COW dance | none exists (groups are values) |
| reclamation | node drop frees map | refcount | node drop frees its groups; decl blocks die with their owner |
| stale access | n/a | Arc keeps alive | generational keys on the declaration side |

Costs S1 must answer for, honestly:

- **Sharing sacrificed for visit locality:** identical computed groups
  are not shared across nodes. The bet: compact typed `Copy` groups (no
  BTreeMap nodes, no heap strings) keep per-node cost low enough that
  sharing is unnecessary for real workloads. Verdict from the
  intrinsic-memory and allocation pins, before and after.
- **Whole-group granularity:** group boundaries are a real design
  decision — they are the dirty classification classes (H becomes
  "which groups moved"). A one-property font change copies the font
  group; groups are small, so this is acceptable.
- **Snapshot contract:** canonical snapshots serialize *resolved
  projections*, never arena keys (session-local). Another v0.6.0-class
  contract note.
- **Transition overlay merge:** every `as_*` accessor merges the node's
  active-transition overlay for its group's properties — today's
  transitionable set is *any style-allowed property except
  transition-config* (`dom_transition.rs`), so font-size, color, and
  text-align transitions must present through `as_font()`/`as_color()`/
  `as_text()` exactly as through the box/visual views (D8). The merge
  mechanics are a design detail, not settled here.
- **Declaration arena crate choice:** slab / generational-arena /
  slotmap-class — chosen by measurement (allocation counts, lookup
  cost), with the existing SlotMap rule store as the baseline to beat.

Effect on the variants: S1 settles the substrate question — V1's essence
(inheritance-grouped structs) is adopted without its COW; V2's
matched-rule caching (J) is an orthogonal later optimization on the same
substrate; V3 (flattened attrs stay) is the fallback branch if
measurements reject the group split. The remaining open axis is caching
policy only.

Proof obligations if S1 is adopted: the full proof suite unchanged in
observable outcome; observability shaped per doctrine — levels
(decl-arena live blocks) as read accessors over retained state, emission
as monotonic counters (upserts, declaration drops, style-pass nodes
visited, per-class dirty classifications) — pinned alongside the
existing perf pins; the cascade families unchanged (observable cascade
semantics are identical); `off_spec_direct_attr_write...` relabels
on-spec when A lands on this substrate.

## Structural compliance (added in review, 2026-10-17)

The redesign introduces ~17 behavior-bearing types. This repository's
structural rules are mechanical and absolute; the plan budgets them
here, attached to spine steps. Rule 1 (a behavior type lives in its own
snake_case file), rule 2 (every behavior file has a real inline test
module), rule 5 (impls live in the type's file), and `mod.rs` holds no
behavior-bearing types.

**New files (step 2, declaration side):** `presentation_property.rs`
(the closed vocabulary + admission-table metadata: grammar, group,
inherited flag), `decl_value.rs` (+ keyword-table parse helpers),
`declaration.rs`, `decl_block.rs` (upsert = A10's minimal-edit logic —
prime unit-test target), `decl_store.rs` (the arena; crate choice per
measurement), `style_mut.rs` (the two write contracts), and
`decl_drop_report.rs` if the report type outgrows a tuple. **New files
(step 3, computed side):** one file per group —
`computed_font_group.rs`, `computed_text_group.rs`,
`computed_color_group.rs`, `computed_box_group.rs`,
`computed_flex_group.rs`, `computed_visual_group.rs`,
`computed_transform_group.rs` — plus `computed_groups.rs`,
`node_presentation.rs`, `computed_style_ref.rs` (+ per-group view types
in their files), and the group-diff classifier
(`group_dirty_classification.rs` — A7's static table, prime unit-test
target). Every file above ships with a real inline test module; the
winners-fold precedence ordering, keyword grammars, A10 upsert
correctness, and A7's classification table are the named unit-test
candidates.

**Documentation in the same changes (AGENTS.md rule):** step 2 rewrites
the declaration-side claims in `src/domains/style/README.md` (rule store
→ declaration store, typed values, drop-and-continue); step 3 rewrites
`src/domains/visitor/README.md` (the fold, group-copy inheritance —
materialization claims die) and the cascade/pipeline claims in
`crates/ui-dom/README.md` if they move. The G landing carries the scoped
"fail fast" amendment to `AGENTS.md` (see G's adjudication note). A
housekeeping step re-points or deletes the ten stale `PLAN.md §x.y`
references in `perf_metric.rs`, `perf_allocation.rs`,
`render_ops_staging.rs`, and `render_ops/mod.rs` (they cite a dead
planning document; file comments must tell the truth).

**Observability shape:** levels (decl-arena live blocks, group counts)
are exposed as **read accessors over retained state**, per the doctrine;
emission stays monotonic counters and rings (upserts, drops, nodes
visited, per-class dirty classifications). No gauges on the emission
path.

## The presentation value table (approach B, enumerated from source 2026-10-17)

The closed presentation vocabulary with its value types, grammars, and
computed forms — the concrete registry `DeclValue` and the keyword tables
come from. Grammars are today's closed sets (`dom_attr_parse.rs`); groups
follow S1's split; "Inherited?" follows CSS — with two gaps flagged
`(gap)` where our pinned transmitting set disagrees.

### Inherited groups

| Property | Group | DeclValue | Grammar (closed set) | Computed form |
| --- | --- | --- | --- | --- |
| `font-size` | font | Length | `0`, px, em (no `unset`) | px f32; em → parent font |
| `line-height` | font | Length | `normal`, unitless ≥0, px, em | px f32, 1px floor; `normal` → None |
| `font-family` | font | Text | string list (unparsed) | interned text — inherited in CSS, not transmitted today **(gap)** |
| `font-weight` | font | Keyword | string today; CSS: `normal`,`bold`,`bolder`,`lighter`,100–900 | keyword **(gap)** |
| `font-style` | font | Keyword | `normal`, `italic` (CSS adds `oblique`) | keyword **(gap)** |
| `text-align` | text | Keyword | start, left, center, right, justify→justified, end | keyword |
| `direction` | text | Keyword | ltr, rtl | keyword |
| `white-space` | text | Keyword | normal, nowrap, pre, pre-wrap, pre-line, break-spaces | keyword |
| `overflow-wrap` | text | Keyword | normal, break-word, anywhere | keyword |
| `word-break` | text | Keyword | normal, break-all, break-word, keep-all | keyword |
| `color` | color | Color | `transparent`, hex6/8, `rgb`/`rgba`, 5 named | [f32;4] at boundary |

### Non-inherited groups

| Property | Group | DeclValue | Grammar (closed set) | Computed form |
| --- | --- | --- | --- | --- |
| `display` | box | Keyword | block, inline, inline-block, none, flex, inline-flex | keyword |
| `position` | box | Keyword | static, relative, absolute, fixed | keyword |
| `overflow` | box | Keyword | string today (scroll/clip consumers); CSS also has `overflow-x`/`overflow-y` — admission candidates | keyword table owed |
| `isolate` | box | Keyword | CSS `isolation`: auto, isolate (today tolerates bool/string) | keyword |
| width/height, min/max w/h, top/left/right/bottom, margin×4, padding×4, border×4 | box | Length | `unset`, `0`, px, em | px f32 (em → own font-size; verify vs CSS Values §em) |
| `opacity` | visual | Number | 0.0–1.0 | f32 |
| `background-color`, `border-*-color`×4, `outline-color`, `caret-color` | visual | Color | as `color` | [f32;4] |
| `outline-width`, `outline-offset` | visual | Length | px string today | px f32 |
| `border-radius` | visual | shorthand | expands to four per-corner longhands at parse (B); each a Length — DeclValue stays single-length | px f32 ×4 |
| `flex-direction` | flex | Keyword | row, row-reverse, column, column-reverse — parsed downstream today (flex.rs); the closed table lands with B | keyword |
| `flex-wrap` | flex | Keyword | nowrap, wrap, wrap-reverse — parsed downstream today | keyword |
| `flex-flow` | flex | shorthand | expands to direction+wrap at parse | — |
| `flex-grow`, `flex-shrink` | flex | Number | ≥0 number — parsed downstream today | f32 |
| `flex-basis` | flex | Length/Keyword | auto, content, `0`, px, em — parsed downstream today | computed as authored; resolved to used at layout (CSS Cascade §4.5) |
| `flex` | flex | shorthand | expands to grow+shrink+basis at parse | — |
| `order` | flex | Integer | integer — parsed downstream today | i64 |
| `justify-content` | flex | Keyword | start, end, flex-start, flex-end, center, space-between, space-around, space-evenly — parsed downstream today | keyword |
| `align-items`, `align-self` | flex | Keyword | alignment keywords + stretch (+ auto for align-self) — parsed downstream today | keyword |
| `align-content` | flex | Keyword | alignment + distribution keywords — parsed downstream today | keyword |
| `place-content`, `place-items`, `place-self` | flex | shorthand | expands to align/justify pairs at parse | — |
| `row-gap`, `column-gap` | flex | Length | px today | px f32 |
| `gap` | flex | shorthand | expands to row-gap+column-gap at parse (B; today it is a standalone attribute — behavior refinement toward CSS) | — |
| `transform` | transform | Text | `translateX/Y` px functions today; typed at parse with B | translate px [f32; 2] |
| `transition-property/-duration/-timing-function/-delay` | (overlay) | Text list | comma lists, ms values | feeds the transition scheduler, not a computed group |

Notes and newly surfaced items:

- **Shorthand admission (ruling, 2026-10-17):** the vocabulary today has
  no `margin`/`padding`/`border` shorthands (longhands only) — they are
  admission candidates under the admission policy (see D6's scoping), not
  silently absent. The shorthands the table *does* admit (`flex-flow`,
  `flex`, `place-*`, `gap`, `border-radius`) expand to longhands at
  parse (B), so `DeclValue` needs no multi-value variant: `border-radius`
  is four per-corner declarations; `gap` is two.
- **Quirk (conformance):** the `transition` *shorthand* is rejected in
  stylesheet declarations today (`style_attr_allows_property`), though
  browsers accept it; only longhands parse. Admission lands in spine
  step 2 with the value table; registry candidate.
- **Divergence candidate 3:** `font-family`, `font-weight`, `font-style`
  are inherited properties per CSS Fonts but absent from the pinned
  transmitting set (the closed 8). Pin with an `off_spec_*` proof before
  any inheritance redesign (spine step 1; relabels in step 3 — see the
  spine's staging note).
- `dir` (content attribute) and `direction` (presentation property)
  coexist today; the presentation property wins for text layout. Keep
  the split: `dir` stays a content attribute.
- `em` resolution rule (CSS Values §em): `font-size` em resolves against
  the parent's computed font-size; every other property's em against the
  element's own. Today's box-length em handling follows the same rule
  through layout's style projection — verify at implementation.
- **Used-vs-computed honesty:** some properties defer to layout for their
  final value (`flex-basis: auto|content`, `width: auto`): their computed
  form stays as authored and the *used* value resolves at layout, per
  CSS Cascade §4.5. The table marks those rows.

## Algorithms (2026-10-17)

The missing aspect, recorded: every algorithm the redesigned system runs,
with inputs, outputs, steps, cost, and its Blink/Gecko algorithm shape.
The session's prime directive applies: adopt the engines' algorithm
shapes; diverge only for documented Rust/Bevy memory or performance
reasons, and only in mechanics, never in observable output.

### A0 — Stylesheet compile (exists; keep)

In: CSS text + origin. Out: compiled rules + index updates. Steps: strip
block comments → split blocks → parse selector (A2) → parse declarations
(A1) → dedup by identity hash → insert into the rule store → update
right-most/origin/identity indexes → bump revision. Cost:
O(declarations). Analog: Blink `StyleEngine` rule-set insertion; Servo
`Stylist` update. No divergence.

### A1 — Declaration parse (G + B's expansion point)

In: declaration-list text. Out: typed `Declaration`s + drop count.
Steps: split top-level `;` → split `name: value` → property lookup in
the value table → per-property grammar parse → on shorthand, expand to
longhands in place (Cascade §3) → on malformed, drop the declaration,
count it, continue (CSS 2.1 §4.1.7). Cost: O(declarations). Analog:
css-syntax-3 §5.4 consume-a-declaration; Servo's `DeclarationParser`.
Divergence: hand-rolled closed grammars per property family (justified:
closed vocabulary, fail-closed, zero backtracking).

### A2 — Selector parse + specificity (exists; keep)

In: selector text. Out: `Vec<SimpleSelector>` + (a,b,c) specificity.
Closed subset: `#id`, `.class` (+multi), `tag`, `tag.class`, `*`,
descendant combinator, four pseudo-classes. Specificity counted per
Selectors §17. Analog: Blink `CSSSelector` specificity. No divergence
beyond the closed subset (recorded unimplemented).

### A3 — Rule matching (exists; keep)

In: the element's (id, classes, tag) + the rule store. Out: matched
rules. Steps: right-most inverted-index lookup (id/class/tag/universal
buckets) → dedup candidates → right-to-left match with a parent-chain
scan per preceding compound. Cost: O(candidates × tree height). Analog:
Blink `RuleSet` bucketing + match; Servo `SelectorMap`. Same shape; the
subset narrows only the combinator vocabulary.

### A4 — The cascade: collect → sort → winners fold (A + F)

In: the node's inline block, matched rule blocks, behavior sheet. Out:
`winners: [Option<(DeclValue, Precedence)>; PROPERTY_COUNT]` (stack,
fixed-size, zero-alloc). Steps: fold behavior sheet (UA-ish tier) →
fold matched rules sorted by (origin.priority, specificity, order) →
fold the inline block at top specificity (css-style-attr §4); a later or
higher-precedence declaration takes the slot. Cost: O(declarations
applied). Analog: Blink `MatchResult` sort-then-apply; Servo's
`ApplicableDeclarations` cascade. Divergence: slot-array fold instead of
sorted-vector apply (justified: the closed property vocabulary enables
O(1) slot indexing; observable order semantics identical — pinned by
cascade.rs families 1–3).

### A5 — Defaulting and inheritance (Cascade §7; Servo `StyleBuilder` shape)

In: winners + the parent's inherited groups (or ancestor seed). Out: the
node's seven groups. Steps: inherited groups (font, text, color) start as
a copy of the parent's group, then winners override; non-inherited
groups start from initial values, then winners override. Analog: Servo's
`StyleBuilder`, which splits inherited vs reset structs and seeds each
differently — our seven groups are that algorithm applied to our
vocabulary. No output divergence; today's two-channel materialization
mechanics are superseded by group copies.

### A6 — Unit resolution (CSS Values §em)

In: typed lengths + font context. Out: px f32 where resolvable without
layout. Rules: `font-size` em → parent's computed font-size; every
other property's em → the element's own computed font-size. Current
algorithm: pending-scale accumulation on the ancestor climb (nearest
absolute ancestor wins; em scales multiply down the chain) —
output-equivalent to the spec rule, pinned by cascade.rs's em-chain
rows. Keep; recorded as mechanics-divergence with identical output.

### A7 — Dirty classification (group diff → hint map) (H)

In: old seven groups, new seven groups. Out: dirty bits. Steps: compare
per group (seven `Copy` compares) → map changed groups via a static
table: box → layout+paint + layer-membership re-evaluation; flex →
layout+paint; transform → paint + layer-membership re-evaluation;
visual/color → paint (+damage rect); font/text → layout+paint + text
remeasure. No children-marking row exists: the pass walks the full dirty
subtree, so every visited descendant is re-seeded from its parent during
the same pass (A5); a pass started above a nested layer boundary crosses
it, and a clean nested layer root is a strong no-op — both behaviors are
pinned by existing unit tests and hold unchanged. Analog: Gecko
`CalcStyleDifference` → change hints (repaint vs reflow vs reconstruct);
Blink style-recalc change types. Divergence: classified by *group
identity* rather than per-property difference — coarser, zero-alloc,
and exact at our group granularity; scheduling outcomes pinned by the
perf counters.

### A8 — Recompute traversal (exists; keep; doctrine home)

In: a dirty layer root + its ancestor seed. Out: a recomputed subtree,
cleared bits, emitted counters. Steps: seed from ancestors outside the
root → DFS, work-on-visit per node (match, fold, resolve, diff, apply,
remeasure text, clear) → no batch state leaves the pass. Analog: Blink
`StyleRecalc` traversal with change propagation; Gecko `RestyleManager`
pending-restyle flush — phase shape already pinned by
`stylesheet_lifecycle.rs`. No divergence; structural rule 4 lives here.

### A9 — Damage propagation (crate-owned mechanics)

In: a damaged node + its border box. Out: per-ancestor bounded damage
buckets up to the layout root. Steps: compute the node's damage rect →
walk parents to the layout root pushing the rect into each bucket
(capacity 8) → overflow promotes the chain to full repaint → stop at an
already-full ancestor. Analog: none claimed — browsers invalidate visual
rects through compositing structures we do not have. Documented
mechanics divergence, justified by the retained-layer/Bevy world;
consumption design is approach I (verdict: staged goal, per D7).

### A10 — Inline block upsert (Servo `prepare_for_update` shape)

In: an inline block + a new declaration. Out: a minimal edit + a changed
bool. Steps: membership bitset check → update-in-place or append
preserving source order → return whether the block changed. Cost:
O(block size); blocks are tiny. Analog: Servo's `DeclarationUpdate`
machinery (`None` / `Append` / `UpdateInPlace` / `AppendAndRemove`). No
divergence.

### A11 — Unload/reload: re-fold from sources (D's provenance fix)

In: a stylesheet unload (origin tombstone). Out: affected elements
recomputed. **Dirty trigger (specified, 2026-10-17):** the revision bump
marks *all* layer roots style-dirty — today's coarse mechanism
(`sync_stylesheet_runtime`), preserved unchanged; per-element scoping is
proposal J's deferred, evidence-gated optimization, not part of A11.
Algorithm: the pass re-folds winners from the *current sources* — inline
blocks are element-owned and simply win again where the stylesheet
previously won, so restore-not-delete falls out of the algorithm rather
than out of storage bookkeeping. `style_pass_attrs_by_node` ownership
tracking is deleted by construction. Analog: browsers recompute against
the current rule set; there is nothing to restore *to* except the
remaining declarations. This is the algorithmic statement of D1's fix —
and A's acceptance test.

### Cost and observability map

| Algorithm | Cost | Observed by |
| --- | --- | --- |
| A0 compile | O(declarations) per load | stylesheet-lifecycle proofs |
| A1 parse | O(declarations) per block | drop-count metric (new) |
| A3 match | O(candidates × height) per element | style-pass counters |
| A4 fold | O(declarations applied) per element | style-pass counters |
| A5 default | O(groups) per element | — |
| A7 classify | seven compares per element | LayoutUpdated / paint counters |
| A8 traversal | O(dirty subtree) | StyleUpdated counter, perf pins |
| A9 damage | O(depth) per dirty node | paint counters |

## Open questions

Questions 5 and 6 remain open **deliberately**: answering them now would
box the design into solutions without evidence (D8 biases us to
preserving functionality, not to guessing). Question 5 is staged
admission driven by actual consumer need; question 6 is decided by S1's
counters after the substrate lands. Everything else is resolved.

1. ~~Migration contract~~ — DECIDED (D5): hard break, string ergonomics
   preserved.
2. ~~Inline style vs behavior sheet~~ — DECIDED (D6): inline style wins
   (author origin, specificity higher than any selector; the behavior
   sheet is a UA-tier concern, see F).
3. ~~Transitions as a cascade tier~~ — DECIDED OBSERVABLY (D6): transitions
   already win via the overlay, matching §6.1 tier-1 ordering. Whether to
   *mechanically* unify them into the cascade is an internal-shape
   decision deferred to design.
4. ~~Shorthand expansion point~~ — DECIDED (D6 + Cascade §3): at
   declaration-parse time; the declaration store is the only expansion
   point.
5. `!important`, UA sheet, animations, layers: admission path is F, staged
   by consumer need, each stage proof-gated. The precedence-key structure
   must not preclude them.
6. Sparse storage shape: internal freedom (D6) — proposal S1 is the
   candidate answer (arena + interning + view accessors); its counters
   decide. The V3 fallback (flattened attrs) stands if interning
   measures poorly.
7. Inheritance channels: observable behavior is spec inheritance; channel
   unification is internal freedom (D6) — decide with the store design.
8. ~~Fate of public `apply_styles`~~ — DECIDED (2026-10-17, corrected in
   review): `apply_styles` is the current style-recompute pass's write
   path (its only caller); it is **rewritten or deleted with that pass**
   in spine step 3, alongside the `style_pass_attrs_by_node` deletion —
   not merely demoted. No external consumer uses it.
9. ~~Successor for `compute_style_map`~~ — reframed by the browser
   reality: `getComputedStyle` is a debug/inspection tool in Gecko and
   Blink. It performs real work on demand, is not cheap, and is not
   wired into the internals' retained state — it recalculates. We
   inherit that freedom: our successor is an **on-demand debug recalc
   query**, debug-priced, which does not constrain the computed store's
   design. Consumers' probe call sites become debug-priced calls — **with
   a mandatory audit first (spine step 4):** if any probe is per-frame,
   the debug-pricing delivers a consumer regression under D8's banner,
   and that call site gets a migration plan instead.
10. ~~Presentational-hint tier~~ — RESOLVED: what looked like hints are
    the `intrinsic` system (`DomIntrinsic`, `element_behaviors/
    intrinsic_size.rs`): img/text/controls carry intrinsic values with
    their own loading lifecycles, and authored width/height feed
    intrinsic measurement, never the cascade. No presentational-hint
    tier is owed unless true presentational attributes are admitted
    later.
11. ~~Damage rects: consume or delete~~ — DECIDED (D7, 2026-10-17):
    preserved, and targeted repaint is a goal (animation on large
    surfaces with small animated regions). The remaining design work is
    the consumption side (approach I) and the rect-pass
    unconditional-clear decision that lands with it.

## Adversarial review adjudication (2026-10-17)

A third-party adversarial review verified 18 load-bearing claims against
the tree (17 held exactly) and attacked the design at its claimed-finished
points. Every point and its disposition:

| # | Point | Disposition |
| --- | --- | --- |
| 1 | `apply_styles` "unused" claim false — it is the recompute pass's write path | **Accepted, fixed.** Background and open question 8 corrected: rewritten/deleted with the old pass in spine step 3, not "demoted." |
| 1a | Flex grammar column aspirational (free strings today) | **Accepted, fixed.** Flex/overflow/transform rows now say "parsed downstream today; closed table lands with B." |
| 1b | Short-circuit runs after transition-shorthand expansion | **Accepted, fixed.** Caveat added to the invalidation boundaries. |
| 2a | A7's children-marking row is internally inconsistent with color inheritance | **Accepted with a correction to the review's premise.** The review's claim that color propagates across nested-layer boundaries *today* is false — the strong-noop unit test pins that it does not. But the verdict stands: the font/text children-marking row was a dead mechanism (the full-subtree pass re-seeds every visited descendant by construction), so it was removed, and the two-tier propagation rule (crosses-nested within a pass; strong-noop for clean nested roots) is now written into A7 and S1.2 with its pins. |
| 2b | Overlay merge only in `as_box`/`as_visual` contradicts the transitionable set | **Accepted, fixed.** All seven accessors merge the overlay; today's transitionable set (any style-allowed property except transition-config) is named. |
| 2c | `isolate` and `transform` homeless | **Accepted, fixed.** `isolate` joins the box group with its CSS grammar; `transform` gets a seventh single-purpose group (paint-class + membership-relevant — it could not live in the box group without changing today's dirty classes). The layer section now names "box-group or transform-group change." |
| 2d | Shorthand coverage arbitrary; `border-radius` unrepresentable in `DeclValue` | **Accepted, fixed.** Ruling recorded: margin/padding/border shorthands are admission candidates (not in vocabulary today); `gap` becomes a shorthand; `border-radius` expands to four longhand declarations at parse so `DeclValue` stays single-length; `overflow-x/y` noted as admission candidates. |
| 2e | "No strings retained" false | **Accepted, fixed.** The honest claim: no *per-node* string copies; text-valued properties intern in a bounded arena whose admission/eviction policy is an open cost tied to open question 6. |
| 2f | A11's dirty-trigger unspecified | **Accepted, fixed.** One sentence: the revision bump marks all layer roots style-dirty (today's mechanism, preserved; scoping is J's deferred optimization). |
| 2g | `StyleMut::set` silent-Ok reading violates repo rules | **Accepted, fixed.** Two contracts specified: `set` is strict (typed error), `set_all` follows G and returns an explicit `DeclDropReport`. The silent-Ok reading is off the table. |
| 2h | Font-family staging self-contradictory | **Accepted, fixed.** The pin relabels in step 3 (group-copy inheritance transmits family for free); the real step-5 follower is named: text-layout's hardcoded UiSans family ignores the attribute today. |
| 3a | Structural rules 1/2/5 unbudgeted | **Accepted, fixed.** The structural-compliance section maps ~17 new files to spine steps and names the unit-test candidates. |
| 3b | README updates absent from the spine | **Accepted, fixed.** README rewrites attached to steps 2 and 3; the G landing carries the AGENTS.md amendment. |
| 3c | D6 unfalsifiable; no admission policy | **Accepted, fixed.** D6 scoped to the admitted vocabulary with a three-part admission policy (named need, spec+engine reference, proof). |
| 3d | G trades "fail fast" without recording it | **Accepted, fixed.** The scoped exception is adjudicated in G's entry; AGENTS.md gains the amendment when G lands. |
| 3e | D5 has no enforcement point or release mechanics | **Accepted, fixed.** Enforcement: typed error on `set_attr*` for presentation properties (loud, never panic, never silent). Release: version bump + `MIGRATION.md` + consumer conversion PR, all in the spine. |
| 3f | "Live blocks" gauge proposed to a counter-only system | **Accepted, fixed.** Levels are read accessors over retained state; emission stays monotonic counters/rings. |
| 3g | Stale `PLAN.md §x.y` anchors acknowledged, never cleaned | **Accepted, fixed.** Spine step 0 re-points or deletes them. |
| 4a | P3 undersells the `Custom(String)` color corner; its fate unspecified | **Accepted, fixed.** P3 now names it the worst corner; vocabulary divisions rule on the fate (custom presentation names admitted via policy; `Custom(String)` survives as the content-side hatch). |
| 4b | `compute_style_map` successor asserted, not costed | **Accepted, fixed.** Spine step 4 adds the consumer per-frame audit obligation. |
| 4c | E's sparse goal half-met, unnoted | **Accepted, fixed.** E's verdict now reckons honestly: the group footprint is universal below the root, and today's unstyled descendants pay four materialized attrs (one a heap string) — the comparison is closer, and that is an argument *for* S1 the plan now makes. |
| 5 | ADOPT-vs-PROPOSED authority confusion | **Accepted, fixed.** The header declares the document a decision record; S1's status is "adopted as leading candidate, gated on its checkpoint." |
| 5b | `flex-basis` used-vs-computed staging | **Accepted, fixed.** The value table marks deferred-to-layout rows per CSS Cascade §4.5. |
| 5c | Behavior-sheet storage unspecified | **Accepted, fixed.** F's verdict now specifies it: per-tag arena-resident blocks keyed by tag behavior and pseudo-state, folded first, conditionally on state. |

The review's closing condition — "until items 1–7 are resolved, this is a
well-researched problem statement, not the design its spine presupposes" —
is met: items 1–7 are resolved in the sections named above.
