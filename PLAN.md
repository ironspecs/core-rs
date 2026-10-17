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
  style query; used by consumers as a measurement probe — 10+ call sites in
  `stream-pro2`; needs a successor or survival), `RootDom::set_transition`;
  `RootDomContext::load_stylesheet` / `unload_stylesheet`.
- **Parse error contract today:** one malformed declaration fails the
  entire `load_stylesheet` / `parse_style_attr_map` call with
  `DomError::InvalidAttributeValue`. Browsers drop just the malformed
  declaration and keep the rest (CSS 2.1 §4.1.7, css-style-attr §3).
  **New divergence** — registry candidate; pin with an `off_spec_*` proof
  before the parser is redesigned.

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
  Consumer evidence: `stream-pro2` themes components through 100+ direct
  presentation writes across `ui-components` and `remote-app`, using both
  constants and raw strings.
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

## Emerging candidate shape (NOT decided — recorded for the design pass)

The research pushes the approaches toward one coherent composition:

**a single declaration-block abstraction** — typed values at parse time
(B), sparse physical storage (E), declaration-drop error handling (G) —
used for *both* stylesheet rules and the per-element inline style store
(A); one cascade pass collecting from both stores (D2); the node retaining
only the computed projection (D3); the precedence key structured wide
enough for the tier growth path (F); provenance layered so unload restores
instead of deletes (D). This is, deliberately, the shape Blink and Servo
already run — the conformance contract (D6) is satisfiable by
construction, and every internal choice left open by it is ours.

Open before it becomes a candidate design: the physical store layout (E's
decision), the value-type table (B's property registry), the migration
surface (D5's API), and the proof obligations ordering (G's pin first,
then A's acceptance test).

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
6. Sparse storage shape: internal freedom (D6) — decide at design time
   with an allocation budget and a perf-metric pin.
7. Inheritance channels: observable behavior is spec inheritance; channel
   unification is internal freedom (D6) — decide with the store design.
8. Fate of public `apply_styles`: unused by any consumer and bypasses
   D1–D3 — demote to `pub(crate)` in the redesign (no migration cost
   observed).
9. Successor for `compute_style_map`: consumers use it as a measurement
   probe (10+ call sites in `stream-pro2`). It must survive or be replaced
   by an equivalent computed-value query over the new store.
10. Presentational-hint tier: do any of our content attributes carry
    presentational semantics that should map at §6.5's weakest tier
    (candidate: `img` width/height sizing hints)? Enumerate before the
    tier model lands.

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
