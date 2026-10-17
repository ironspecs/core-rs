# PLAN.md — ui-dom style system redesign (working brainstorm)

Living document for the style/CSS internals redesign. It records possibilities
and learnings, not decisions: nothing here is agreed unless marked **DECIDED**
(and dated). Approaches stay listed with their trade-offs until one is chosen;
the chosen one gets its own design pass before any implementation.

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
  stringly-typed from parse through storage to matching with every consumer
  re-parsing, and per-node storage mixes authored and derived values. As the
  supported property set grows — and as real nodes style sparsely — the
  current structures waste memory and re-do parsing work.

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
  `RootDom::apply_styles` (**public but unused by any consumer** — bypasses
  D1–D3; demotion candidate), `RootDom::compute_style_map` (public one-off
  style query; used by consumers as a measurement/debug probe — 10+ call
  sites in `stream-pro2`; browser-inherited freedom makes its successor
  an on-demand debug recalc, see open question 9), `RootDom::set_transition`;
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
value to the stored one and returns without dirtying), so dirty marking
is already per-attribute *value-diff* driven — but the *class* of
dirtiness is static, never computed from the actual used-effect
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
the crate, previously backlogged. Any invalidation redesign must decide
targeted repaint or deletion (open question 11).

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
- **D6 (2026-10-17).** Browser-conformance principle: **inputs, outputs,
  and timings match browser behavior.** Any observable deviance from
  Blink/Gecko semantics will be reported by users as a bug. Internal
  implementation shape remains free (documented Rust/Bevy-justified
  mechanics are fine). This resolves open questions 2, 3, 4, 6, and 7: the
  observable answers are the spec's answers.

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

Not mutually exclusive; a coherent design likely composes one from each row.
Each entry names its Blink/Gecko analog, what it fixes, its risks, and what
it does to the off-spec registry.

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

### D. Provenance-layered storage on the node

The node keeps declaration sources layered by provenance — direct-authored,
stylesheet-pass, inherited, behavior — instead of one flattened attr map, so
removal of any layer restores the next.
- *Analog:* Blink/Gecko's separation of declarations from computed style;
  Gecko's rule tree shares declaration blocks structurally.
- *Fixes:* the delete-instead-of-restore half of P1 even without A; makes
  `style_pass_attrs_by_node` ownership obsolete or explicit.
- *Risks:* overlaps heavily with A — likely absorbed into A rather than
  done alone; per-node memory shape must respect sparse styling.

### E. Sparse declaration storage shape

However declarations are stored, the physical structure should cost ~nothing
for unstyled nodes and scale with properties actually declared, not with the
property vocabulary (pooled declaration blocks, small-map structures, or
interned property ids).
- *Analog:* Servo's shared `PropertyDeclarationBlock` storage; Blink's
  property-id-indexed sets.
- *Fixes:* P3's sparse/growth inefficiency.
- *Risks:* interning/pooling must not violate the zero-allocation doctrine
  on the emission path; read-side ergonomics for consumers stay public-API
  stable where possible.

### F. Cascade tier growth path (how the absent features slot in)

The current precedence tuple `(origin.priority, specificity, order)` is a
projection of the §6.1 tier list onto one axis. The growth path toward spec
coverage, staged by consumer need and each stage proof-gated: importance
flag per declaration → built-in UA tier (a real sheet below all author
rules, replacing ad-hoc behavior styling) → animation tier → cascade
layers. The sorting structure should be designed now so these are additive
(a wider precedence key), not a rework. Observable behavior at each stage
is pinned first; internal mechanics stay free per D6.

### G. Declaration-drop parsing (spec error handling)

Both declaration stores (stylesheet and inline) parse with
forward-compatible error handling: malformed declarations are dropped,
parsing continues, and the drop is reported by metric, not silence.
Replaces today's whole-sheet rejection. Registry: new `off_spec_*` pin of
the current behavior first; reclassify on landing.

### H. Difference-computed dirty classification

Replace or augment the static per-attribute dirty classes with a
consequence classification computed from the old/new comparison at apply
time (Gecko change-hint / Blink style-difference shape): a change whose
used effect is nil dirties nothing; a paint-class change never dirties
layout. This is coarseness point C2's fix. Cheaper variant: keep static
classes but add used-effect short-circuits at known hot properties.

### I. Damage-rect consumption

Decide the write-only damage machinery (previously backlogged): consume
it for targeted repaint — the rect pass already stages ops, and its
unconditional per-layer surface clear is the largest known perf item in
the crate — or delete it. The style redesign determines the producer side
(what damage a style change emits); the rect-pass clear decision lands
with the consumer side. Gate: the perf pins in
`perf_metric.rs`/`perf_allocation.rs`.

### J. Scoped style propagation and matched-rule caching

Attack coarseness points C1/C3: scope recompute to elements whose matched
set can actually change (selector-aware invalidation, the shape of
Blink's invalidation sets for class/id changes), and/or cache matched
rule sets per element (Blink matched-properties cache / Servo
`rule_cache`) so re-matching is skipped when the rule store and element
state are unchanged. Must respect the zero-allocation metrics doctrine,
with metric counters pinned before and after.

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

## Emerging candidate shapes (NOT decided — recorded for the design pass)

Correction, 2026-10-17: an earlier draft of this section proposed "a
single declaration-block abstraction." The source research above rules
that out: the engines run families of structures, and our candidates are
families too, differing in how much of the engine shape we adopt.

**Variant 1 — Servo-like:** ordered declaration vectors with parallel
importance bits and arena-shared blocks (for both stylesheet rules and
inline style); computed style split into inheritance-grouped structs
interned and shared across nodes (substrate: proposal S1 — its COW
substrate is superseded); difference-computed dirty hints (H). Maximal
memory elegance and sharing; largest migration; most new machinery (id
sets, intern discipline, rule cache).

**Variant 2 — Blink-like:** typed parsed declarations in per-source
stores; a computed store partitioned by inheritance with per-feature
groups; matched-rule caching against a dirty-scoped element set (J);
damage-aware paint propagation (I). Strong recompute economics; mid
migration; caching adds invalidation surface we must proof-gate.

**Variant 3 — Minimal evolution (closest to today):** keep the flattened
per-node attr map as the computed store; add the inline declaration store
beside the existing rule store, both with typed values (B); keep static
per-attribute dirty classes but scope style propagation (J) and consume
damage rects (I). Smallest break from the codebase; leaves memory/sharing
wins on the table; the attr vocabulary split (C) is still required.

All three share the decided skeleton: declaration stores per source → one
cascade pass (D2) → computed projection on the node, resolved only when
dirty at the style stage (D3) → used projections derived per frame. The
choice among them is about the computed store's physical shape and the
invalidation economics, and it gates on open questions 6 and 11.

The acceptance spine is unchanged: G's error-handling pin first, then
A's inline-precedence acceptance test; every variant is judged against
the same suite.

## Proposal S1 — arena-resident style storage with view accessors (2026-10-17)

**Status: PROPOSED, not decided.** This section replaces the Arc/COW
substrate implied by Variant 1. COW is old-Rust; this crate already runs
better patterns — `SlotMap` arenas with generational keys (rules, nodes,
layer slots), validated leases (the layer pool), and borrow-view
discipline (`as_*` accessors that hand out views, never ownership or
clones — the `Option::as_ref` / `as_slice` / `SlotMap::get` standard).
S1 applies those patterns to the whole style-data family.

### S1.1 The structures (signatures, not final code)

Declaration side — one arena for *all* declaration blocks (stylesheet
rules and per-element inline style):

    enum DeclValue {                          // typed at parse; no strings retained
        Length(DeclLength),                   // Unset | Px(f32) | Em(f32)
        Color([f32; 4]),
        Keyword(KeywordId),                   // u16, per-property closed tables
        Number(f32),
        Integer(i64),
        Text(TextKey),                        // font-family, transform: interned strings
    }

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

Computed side — inheritance-grouped, interned, immutable:

    // Inherited groups (seed from parent keys during the pass):
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
    // Every field typed; lengths resolved to px at style time where the
    // grammar allows (em resolves against the element's own font size —
    // computable without layout). Nearly all groups are plain Copy.

    struct ComputedGroups { font: GroupKey, text: GroupKey, color: GroupKey,
                            box_: GroupKey, flex: GroupKey, visual: GroupKey }
    // 6 × 8-byte generational keys = 48 bytes per element node, replacing
    // the presentation half of the flattened attr map.

    struct ComputedStyleStore {
        fonts:   InternArena<ComputedFontGroup>,
        texts:   InternArena<ComputedTextGroup>,
        colors:  InternArena<ComputedColorGroup>,
        boxes:   InternArena<ComputedBoxGroup>,
        flexes:  InternArena<ComputedFlexGroup>,
        visuals: InternArena<ComputedVisualGroup>,
    }
    // InternArena = SlotMap<K, T> + hash→keys intern table. Groups are
    // immutable once interned: mutation is intern-new-and-rekey on the node.
    //
    // (Storage location REVISED in S1.5: these groups live inline in the
    // node record; the interning arenas are dropped. Group definitions
    // stand; only placement changed.)

Text-valued properties (`font-family`, `transform`) intern their strings
in a small string arena (`TextKey`), or stay stringly node attrs until
approach B's value-type table lands — deferred detail, flagged.

### S1.2 The methods (signatures and contracts)

Declaration writes (the D5 programmatic API, css-style-attr §4 semantics):

    impl RootDom {
        fn style_mut(&mut self, node: DomNodeSlotKey) -> StyleMut<'_>;
    }
    impl StyleMut<'_> {
        /// Parses and upserts one declaration into the node's inline block,
        /// then marks style dirty up to the layer root. Never marks layout
        /// or paint directly: the pass classifies consequences.
        fn set(self, property: &str, value: &str) -> Result<(), DomError>;
        fn set_all(self, declaration_list: &str) -> Result<(), DomError>; // G semantics
        fn remove(self, property: &str) -> Result<(), DomError>;
    }

Computed reads (the `as_*` standard: borrows, no ownership, no clones):

    impl RootDom {
        fn computed_style(&self, node: DomNodeSlotKey) -> ComputedStyleRef<'_>;
    }
    impl ComputedStyleRef<'_> {
        fn as_font(&self) -> &ComputedFontGroup;
        fn as_text(&self) -> &ComputedTextGroup;
        fn as_color(&self) -> &ComputedColorGroup;
        fn as_box(&self) -> ComputedBoxView<'_>;       // view may merge transition overlay
        fn as_flex(&self) -> &ComputedFlexGroup;
        fn as_visual(&self) -> ComputedVisualView<'_>;
    }
    // The debug successor to `compute_style_map` is a separate on-demand
    // recalc query (open question 9), NOT a read over this view.

The style pass per dirty element (D2/D3 mechanics, visitor doctrine):

    1. seed inherited keys from parent (ancestor seed at layer roots — unchanged)
    2. fold declarations into a stack-local fixed array:
         winners: [Option<(DeclValue, Precedence)>; PRESENTATION_PROPERTY_COUNT]
       source order: behavior sheet (UA-ish tier) → matched rules sorted by
       (origin.priority, specificity, order) → inline block (top specificity)
    3. resolve the six groups from winners + inherited seeds
    4. intern each group → the node's new group keys
    5. diff old vs new keys (six u64 compares) → classify dirties:
         box_/flex changed  → mark layout+paint
         visual/color       → mark paint only (damage rect)
         font/text          → mark layout+paint, remeasure text intrinsics,
                               and mark children style-dirty (inherited seeds moved)
    6. clear the style bit

### S1.3 Flow diagrams

Storage topology:

    DeclStore (SlotMap)                ComputedStyleStore (interned arenas)
    ┌───────────────┐                  ┌──────────────────────────────┐
    │ K1: rule blk  │◄─── StyleRule    │ fonts   F7  {14.0, 18.0, …}  │
    │ K2: rule blk  │     {decl: K1}   │ texts   T3  {start, ltr, …}  │
    │ K3: row blk   │◄─── node inline  │ colors  C9  {#f7f9ff…}       │
    └───────▲───────┘                  │ boxes   B2  {flex, 180×…}    │
            │                          │ flexes  X1  {row, grow 1, …} │
    node: { decl: K3,                  │ visuals V5  {bg #2A313D, …}  │
            computed: {F42,T3,C9,      └───────▲──────────────────────┘
                       B2,X1,V5} }             │ interned by content hash
                                               │
    read: root.computed_style(node).as_visual().background_rgba
          → &[f32; 4]  (borrow; no clone, no ownership)

Write → invalidation flow (the P2 timing fix):

    root.style_mut(row).set("background-color", "#2A313D")?
        │
        ▼  typed parse; malformed → dropped + metric (G)
    DeclStore: upsert into row's DeclBlock
        │
        ▼  mark style-dirty: row → … → layer root   (ONE bit per node)
        │  (layout/paint NOT marked here — the pass classifies)
        ▼
    next update → style pass: winners fold → resolve → intern → rekey row
        │
        ▼  key diff: visual changed; box_/flex/font/text/color unchanged
        ▼
    paint-only dirty + damage rect; layout never touched

Inheritance flow (unifies the two current channels):

    parent  { font: F7, text: T3, color: C9, … }
        │   DFS carries parent's inherited keys
        ▼
    child winners { font-size: 20px }
        font  = intern(F7 with size_px = 20.0) → F42   (new group)
        text  = T3          (key copy — no recompute, no clone)
        color = C9          (key copy)
        ▼
    grandchild, no declarations: font F42, text T3, color C9 (all key copies)

### S1.4 Tradeoffs, made visible

| axis | today (flattened attrs) | Arc+COW groups (rejected) | S1 arena + views |
|---|---|---|---|
| node presentation storage | BTreeMap + heap strings | 6 Arc ptrs + refcount | 6 u64 keys, no atomics |
| inline write cost | normalize, map insert, static dirty class | COW clone-on-write | block upsert + one style bit |
| read path | `presented_attr(...).cloned()` | Arc deref | borrow view, zero clones |
| recompute diff | per-attr compare in `set_attr_internal` | deep group equality | six key compares |
| inheritance | materialize strings into child attrs + context carry | Arc clone per group | key copy per group |
| N identical rows | N full maps | N ptrs + 1 shared group | N keys + 1 interned group |
| shared-data mutation | n/a | AtomicBool COW dance | forbidden (interned immutable) |
| reclamation | node drop frees map | refcount | ~~append-only arenas; epoch/trim policy~~ superseded in S1.5 (no computed arena) |
| stale access | n/a | Arc keeps alive | generational key rejects |

Costs S1 must answer for, honestly:

- **Interning overhead:** hash + lookup per group per recomputed node. The
  bet: themed UIs share groups heavily (rows of identical items), so hits
  dominate. Verdict comes from counters (intern hit/miss, live groups per
  arena — enum-indexed, zero-alloc), pinned before and after.
- **Whole-group granularity:** a one-property font change interns a new
  font group. Groups are small `Copy` structs; acceptable, but it means
  group boundaries are a real design decision (they are the dirty
  classification classes — H becomes "which keys moved").
- **Append-only arenas:** distinct-style growth is bounded by the product
  of themes × states, small for retained UIs, but an epoch/trim policy is
  owed (candidate: trim on stylesheet unload epochs; decide at design).
- **Snapshot contract:** canonical snapshots serialize *resolved
  projections*, never keys (session-local). Another v0.6.0-class contract
  note.
- **Transition overlay merge:** `as_box()`/`as_visual()` merge the
  per-node transition overlay at view level; the merge mechanics are a
  design detail, not settled here.

Effect on the variants: S1 is the physical substrate for V1 and V2 (V1's
"COW sharing" is superseded — the remaining V1 essence is inheritance-
grouped interned structs; V2 adds matched-rule caching (J) on the same
substrate). V3 (flattened attrs stay) remains the fallback if interning
measures poorly. The variant choice collapses to: computed-store
granularity and caching policy, on one shared substrate.

Proof obligations if S1 is adopted: the full 249-proof suite unchanged in
observable outcome; new zero-alloc counters (intern hit/miss, live groups,
style-pass nodes visited) pinned alongside the existing perf pins; the
cascade families unchanged (observable cascade semantics are identical);
`off_spec_direct_attr_write...` relabels on-spec when A lands on this
substrate.

### S1.5 Revision (2026-10-17, second pass): computed groups travel with the node

**Why the revision:** the visit-locality doctrine, re-read in the code.
`DomNodeBase` carries `box_model`, `intrinsic`, `layout`, `paint`,
`style`, and `transitions` *inline in the node record*, so a visitor that
fetched the node has everything at hand on one fetch — layout, paint,
and text passes never chase a second pointer. S1's original computed
side (six keys into a separate interned arena) forces exactly that
second fetch per node per visit. Rejected.

**Revised placement:** the six computed groups store *inline in the node
record*, beside the existing bundles (they replace the presentation half
of today's flattened attrs). The arena survives only on the
**declaration side** — rule blocks and inline blocks are fetched only
during the style pass, so indirection there costs nothing on the visit.
The arena crate is a category choice (slab / generational-arena /
slotmap-class), decided by measurement, not a SlotMap default.

**Consequences, honestly re-tabulated:**

- Cross-node *sharing* of computed groups is sacrificed. Memory behavior
  comes from **compactness** instead: typed `Copy` groups (no BTreeMap
  nodes, no heap strings) — a full computed set lands in the low
  hundreds of bytes per styled node; unstyled nodes carry only
  initial-value groups.
- Inheritance propagates by **value-copy** of the small inherited groups
  down the DFS (same flow as S1.3's diagram, minus the arena). Group
  equality replaces key equality for the dirty classification: six
  memcmp-cheap `Copy` compares per recomputed node. Approach H survives.
- The append-only/epoch problem **disappears** — no computed arena to
  trim.
- `as_*` views now borrow *into the node* — the cheapest possible read,
  fully doctrine-compliant.

Revised topology:

    DeclStore arena (style pass only)     DomNode record (one fetch per visit)
    ┌───────────────┐                     ┌────────────────────────────────────┐
    │ K1: rule blk  │◄─── StyleRule       │ base: style, layout, paint,        │
    │ K2: rule blk  │     { decl: K1 }    │       box_model, intrinsic,        │
    │ K3: row blk   │◄─── node inline     │       transitions, links           │
    └───────────────┘                     │ computed: font, text, color,       │
                                          │           box_, flex, visual       │
                                          │   (inline Copy structs, typed)     │
                                          └────────────────────────────────────┘

    read: root.computed_style(node).as_visual().background_rgba
          → &[f32; 4]  (borrow into the node; zero indirection)

**What did not change:** the declaration-side arena and typed `DeclValue`;
the winners fold; dirty classification by group difference; the
write→invalidation and cascade flows; the variant collapse (granularity
and caching policy remain the open axes). The "N identical rows" row of
S1.4's table now reads: N compact inline copies — still far below N
BTreeMaps with heap strings.

## Open questions

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
8. Fate of public `apply_styles`: unused by any consumer and bypasses
   D1–D3 — demote to `pub(crate)` in the redesign (no migration cost
   observed).
9. ~~Successor for `compute_style_map`~~ — reframed by the browser
   reality: `getComputedStyle` is a debug/inspection tool in Gecko and
   Blink. It performs real work on demand, is not cheap, and is not
   wired into the internals' retained state — it recalculates. We
   inherit that freedom: our successor is an **on-demand debug recalc
   query**, debug-priced, which does not constrain the computed store's
   design. Consumers' probe call sites become debug-priced calls; no
   special read surface is owed.
10. Presentational-hint tier: do any of our content attributes carry
    presentational semantics that should map at §6.5's weakest tier
    (candidate: `img` width/height sizing hints)? Enumerate before the
    tier model lands.
11. Damage rects: consume for targeted repaint or delete (approach I) —
    decided together with the rect-pass unconditional-clear item, the
    largest known perf item in the crate.

## Learnings log

- **2026-10-17 — Phantom dirties were real.** The removed `generation`
  counters weren't just dead weight: the preferred-size and icon paths
  compared them, so every style recompute manufactured layout/paint work for
  form controls and icon images. Equality-over-measured-fields fixed it; no
  perf pins moved (the pinned scenarios didn't cover those node kinds).
- **2026-10-17 — Two divergences pinned, now the redesign's acceptance
  tests.** Timing-dependent direct-write precedence (P1) and shorthand
  coexistence (P3's shorthand face) are running `off_spec_*` proofs; any
  chosen approach must relabel them, and the suite will catch accidental
  movement.
- **2026-10-17 — Browsers confirm the user's direction.** Programmatic
  styling in Blink/Gecko is a declaration store plus a dirty bit resolved in
  the same rendering update as stylesheets (D1–D3 are the spec shape, not an
  invention).
- **2026-10-17 — Two-channel inheritance is pinned architecture,** not a
  bug: four properties materialize into descendant attrs, four are
  context-carried into text intrinsics. Unify or formalize (open question 7).
- **2026-10-17 — The transition overlay is already spec-ordered.**
  `presented_attr` consults active transitions before stored attrs, which is
  observably the §6.1 tier-1 position. Not every divergence candidate is
  real; verify the observable before designing mechanics.
- **2026-10-17 — New divergence discovered: declaration error handling.**
  Browsers drop malformed declarations and continue (CSS 2.1 §4.1.7,
  css-style-attr §3); we reject the entire stylesheet on the first bad
  declaration. Users would report this as a bug per D6. Registry candidate;
  pin before parser redesign (approach G).
- **2026-10-17 — Consumer evidence for D5.** `stream-pro2` themes via 100+
  direct presentation writes (constants and raw strings mixed), and uses
  `compute_style_map` as a measurement probe; `apply_styles` has zero
  external callers. The migration cost concentrates in `set_attr_str`
  presentation writes.
- **2026-10-17 — Shorthand expansion has exactly one honest home.**
  Cascade §3 ("as if expanded in place") puts expansion at declaration
  time; any design that expands later duplicates the point. This collapses
  approach B's shorthand work into the declaration store itself.
- **2026-10-17 — The engines run families, not shapes.** Servo's
  declaration block is an ordered `ThinVec` + importance bit-vector +
  property-id set + COW atomic; Blink's computed style is a generated
  base plus per-feature group structs; Gecko shares per-topic structs via
  the rule tree. The durable principles: declaration order matters,
  membership checks are separate from storage, sharing is copy-on-write,
  change detection lives at the declaration level, and dirtiness is
  classified from the actual difference.
- **2026-10-17 — Our invalidation is value-diff driven but statically
  classified.** `set_attr_internal`'s same-value short-circuit filters
  no-op writes; the *class* of dirtiness is a compile-time property of
  the attribute. The engine moves we lack: classify from the used-effect
  difference (C2/H), scope the recompute set (C1/C3/J), and consume
  damage rects (I).
- **2026-10-17 — Damage rects are a tested producer with no consumer.**
  The bounded-8-rect propagation to layout roots works and is unit
  tested; nothing repaints from it, and the rect pass clears the whole
  surface per layer per apply regardless. Producer exists; the consumer
  decision is open question 11.
- **2026-10-17 — COW is superseded before it was ever adopted.** The
  crate already runs the better patterns: SlotMap arenas with
  generational keys, validated leases, and `as_*` borrow-view discipline.
  Proposal S1 applies them to the style-data family and turns the
  style-difference problem into six key compares per node — the cheap
  version of the engines' change-hint classification. The honest new
  costs it introduces: interning overhead (counter-measured), whole-group
  granularity (group boundaries = dirty classes), and an owed epoch/trim
  policy for append-only arenas.
- **2026-10-17 — Debug queries inherit browser pricing.**
  `getComputedStyle` does on-demand work in Gecko and Blink and is not
  cheap; it is a debug tool, not an internals read. Our successor query
  is likewise an on-demand recalc, which frees the computed store from
  serving debug traffic.
- **2026-10-17 — Visit locality corrected S1 (see S1.5).**
  `DomNodeBase` inlines style/layout/paint/box_model/intrinsic/
  transitions so visitors have everything on one fetch; a side arena for
  computed groups would add a second fetch per node per visit. Computed
  groups moved inline; the arena survives for declarations only, its
  crate a category choice by measurement; the epoch/trim problem
  evaporated with the computed arena.
