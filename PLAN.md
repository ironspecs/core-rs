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

## Open questions

1. Migration contract for `set_attr_str` on presentation attrs: hard break
   (writes become declarations) vs. deprecated alias with the new semantics
   vs. dual APIs. Which release carries it (v0.6.0 candidate)?
2. Does inline-style precedence include beating behavior pseudo-state
   declarations (the widget sheet)? In CSS the style attribute beats UA —
   expected yes, pin with a proof.
3. Transitions: do they enter the cascade as their own tier (spec shape) or
   stay a presented-overlay (current mechanics)? A registry-level decision.
4. Shorthand expansion: at declaration-parse time (spec) vs. at cascade
   fold time; and which shorthands are admitted first (`flex`, `place-*`,
   `margin`/`padding` families are candidates; none exist today).
5. `!important`, UA stylesheet, cascade layers: admit with the new
   structures or formally exclude (registry currently: unimplemented).
6. Sparse storage: pooled blocks vs. small-map vs. interned ids — decide
   with an allocation budget and a perf-metric pin before implementation.
7. Do inherited-channel properties unify into one channel, or is the
   two-channel split formalized as permanent architecture (with docs)?

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
