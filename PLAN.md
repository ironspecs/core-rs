# PLAN.md — the five-PR arc (decision record and debate brief)

This document plans the next arc of work on `ui-dom`. It is written to be
read cold (survives context compactions) and to be attacked (every
load-bearing claim is stated with its evidence; known weaknesses are named
before anyone else names them). Sections:

1. **Problem statement** — the five problems this arc exists to solve.
2. **Background** — the system as it stands, verified: the architecture,
   its strengths, and its honest weaknesses.
3. **The proposals** — PRs 1–5, each with design, evidence, costs,
   alternatives, and proof obligations. PRs 2 and 3 carry a **debate
   section that must be resolved, with third-party agreement, before any
   implementation**, and each carries a kill criterion (a debate without
   one is a ratchet).
4. **Decision log** — everything already adjudicated, with rationale.
5. **Vocabulary carried from the redesign** — one-line definitions of every
   term this document reuses from the landed plans, so nothing requires
   git archaeology (AGENTS.md points readers here).

Status markers: `SETTLED` items are decided and need only execution.
`DEBATE` items are open positions that require the written debate and
third-party agreement named in their section before code lands.

---

## 1. Problem statement

Five problems, in the order this arc addresses them.

- **P1 — The vocabulary gap is now the top consumer pain.** The style
  engine's admitted vocabulary (69 properties, 68 at v0.6.0 and growing via admission per the D6
  admission policy) is *correct* but narrow. Real CSS that consumers write
  every day — `margin: 8px`, `box-sizing: border-box`, `!important`,
  `width: 50%`, `margin: auto`, `opacity: 1.2`, `transition: transform` —
  is dropped at the boundary, counted, and inert. The coverage accounting
  pinned every one of these behaviors with running proofs, so we know
  exactly what we do; what we do is not enough for the consumers we have.
  Evidence: the pin families `unsupported_shorthand_drops` (B3's file),
  `unsupported_box_sizing_drops_and_border_box_sizing_holds`,
  `unsupported_important_declarations_drop_but_the_rule_survives` and its
  inline sibling, `unsupported_percentage_lengths_drop`,
  `unsupported_auto_lengths_drop`,
  `off_spec_opacity_out_of_range_rejected`,
  `off_spec_adjacent_negative_block_margins_collapse_to_most_negative`
  (`layout/normal_flow.rs`), and
  `off_spec_transform_transitions_do_not_schedule`
  (`style/admitted_gaps.rs`) — all in `tests/layout_proofs/` — plus the
  admission queue analysis in the architect's library review (2026-10-18)
  and the registry itself.
- **P2 — The style runtime's sharing mechanism is a global mutex in the
  style path.** `RootDomContext` shares `Arc<RootDomContext>` with
  `Mutex<StyleRuntime>`; the style pass locks it once per element per pass
  for candidate lookup and block reads. Correct for today's single-threaded
  update loop; a structural contention point and a semantic muddle if
  consumers ever drive documents concurrently or at high element counts.
  The revision counter already version-stamps the rule set — the sharing
  mechanism does not exploit that fact.
- **P3 — The flex solver is bespoke, and it is the users' biggest
  complaint.** The active solver (`resolve_active_flex_container`,
  `src/domains/layout/flex.rs`, 5,356 lines) was intuited rather than
  derived from Blink/Gecko structures. It works for its admitted batches
  (3–7, 135 proofs in `layout/flex_layout.rs` plus 59 unit tests), but
  its data structures do not map to the engines' flex lines/items, so
  correctness arguments are hard, coverage expansion is slow, and user
  complaints about flex behavior have no shared vocabulary to resolve
  against. The style-system redesign proved the method — a source-verified
  mapping to the engines, not intuition. The remedy's SHAPE shifts here,
  and the shift is named rather than smuggled: the style redesign adopted
  the engines' DATA STRUCTURES (field-level isomorphism with Servo,
  auditable field-by-field); the binding layout-pass doctrine forbids
  engine-shaped mutable structures, so this arc adopts the engines'
  SEMANTICS on doctrine-conforming data — a weaker verification position
  whose substitute (per-phase behavioral pins) is priced into PR 3's
  proof obligations.
- **P4 — Text production is the dangerous seam, and it is unsealed.** One
  hardcoded font family (`TextFontFamilyId::UiSans`, the only variant) while
  `font-family`/`font-weight`/`font-style` compute but are unconsumed (a
  registered divergence, the named step-5 follower). Fallback is implicit
  in one function. cosmic-text asserts (`font_size != 0`,
  `line_height != 0`) leaked through as panics until the accounting found
  them; the guards now stand at every site the probes reached (the
  intrinsic measurement pair, wrapped-layout and caret-path functions, the
  control measurement, and the three text-op staging sites in paint — see
  the B1 hardening record in git history for the enumeration). Any change to
  this seam risks closing doors for non-Latin, RTL, and vertical text that
  must stay open — which is why the seam proposal carries an explicit
  prove-or-abandon gate.
- **P5 — `RootDom` is a 7,340-line god object and it is still growing.**
  `src/types/root_dom.rs` gained ~580 lines *through* a redesign that
  deleted entire subsystems from it (`apply_styles`, the ownership map, the
  attr projection). It cohabitates tree mutation, attribute writes, the
  style-attr route, the inline declaration store, the transition scheduler,
  scroll, focus, the popup/options system, layer queues, metrics, and the
  computed read surface — 48 public methods. Structural rule 5 (impls live
  in the type's file) means no lint can stop the accretion; only a type
  decomposition can. Left alone it crosses 8k lines inside this arc.

---

## 2. Background — the system as it stands (verified 2026-10-18)

Ground truth for the adversarial reviewer: what the architecture is, what
it does well, and where it is weak. Citations are load-bearing.

### 2.1 The style engine (landed at v0.6.0)

- **Declaration store.** `DeclStore` = `SlotMap<DeclBlockKey, DeclBlock>` +
  a `TextInterner`, in two instances: the context-shared stylesheet arena
  (`StyleRuntime.decl_store`, behind the mutex) and each `RootDom`'s inline
  arena (element-owned blocks). `DeclBlock` = `SmallVec<[Declaration; 4]>`
  in source order + a `PropertySet` bitset ([u64; 2]) for O(1) membership
  (the Servo `PropertyDeclarationBlock` shape). Upsert is the A10
  minimal-edit (update-in-place, position retained, changed flag).
- **Typed values.** `DeclValue { Length(DeclLength), Color([f32;4]),
  Keyword(Keyword), Number(f32), Integer(i64), Text(TextKey) }` — parsed
  once at the declaration boundary by A1 (`decl_parse.rs`) against the
  closed `DeclGrammar` table in `presentation_property.rs` (69 properties, growing via admission;
  per-property grammar, group, inherited flag, name). Fail-closed: anything
  outside the table drops with a counted report (CSS 2.1 §4.1.7, the scoped
  exception recorded in AGENTS.md).
- **The winners fold** (`src/domains/visitor/winners_fold.rs`): per element,
  behavior base →
  active pseudo-state behavior → matched rules ascending
  `Precedence { origin_priority, inline, specificity, order }`
  (`decl_value.rs:39-44`) → inline block (`i32::MAX`, `inline: true`), into
  a stack-local `[Option<(DeclValue, Precedence)>; PresentationProperty::COUNT]` (69 today). The fold and
  resolution are zero-allocation per element; the collect phase carries
  two pinned per-element allocations plus a dedup `HashSet`
  (`winners_fold.rs:140-146` — the deferred proposal J's target, see the
  vocabulary section).
- **The computed projection.** Seven small `Copy` groups (font, text,
  color, box, flex, visual, transform) inline in the node record
  (`NodePresentation`), seeded by group-copy inheritance (A5) and
  unit-resolved at style time (A6: font-size em → parent, others → own).
  Group-diff classification (A7): box → layout+paint+layer-membership;
  flex → layout+paint; transform → paint+membership; visual/color → paint;
  font/text → layout+paint(+style bit for remeasure). Field-level diffs
  feed the transition scheduler (`field_mapping.rs`).
- **Atomic load.** `load_stylesheet` is parse-and-stage then commit:
  nothing mutates on `Err` (F1 fix); two-tier dedup (active rules and
  same-load staged rules share one `RuleEquivalence` comparison behind an
  identity-hash bucket, O(N)); the G drop counter still accumulates on
  failed loads (drops are observations, not mutations).
- **Transitions.** Typed `DomTransition` (property, `DeclValue`
  from/to/presented, timing), scheduled by the pass on field changes at the
  detection update (D2 one timing), presented through the overlay merge in
  the `computed_*` accessors (CSS Cascade §6.1 tier-1 semantics, including
  inheritance of interpolated values). `ResolvedTransitionLonghands` on the
  node, capacity 4 per list. Interpolation lerps `Length(Px)`/`Color`/
  `Number`; everything else steps. `transform` transitions do not schedule
  (interned-text value is uninterpolable — a registered divergence, PR 1
  item 6).
- **The enforcement.** Presentation is written only through `style_mut`
  (strict `set`, drop-and-continue `set_all`, `remove`) or the `style`
  attribute (routes into the inline store with counted drops);
  `set_attr`/`set_attr_str` on presentation names fails with
  `DomError::PresentationRequiresStyleMut`; `DomAttribute` lost its
  presentation variants entirely (the D5 hard break, v0.6.0).

### 2.2 The pipeline and its doctrine

- **Dirty-bit doctrine:** per-node per-domain booleans (style, layout,
  paint-repaint-state), upward propagation to the enclosing layer root,
  work-on-visit passes, zero-allocation metrics. Not up for redesign.
- **Scheduling:** composite is layer-first — lifecycle reconciliation, then
  style → layout → paint → composite per live layer. The style pass's DFS
  is ungated below the layer-root entry gate (a pass rooted above crosses
  nested layer boundaries; a clean nested root is a strong no-op — both
  pinned). New layer roots materialize by the end of the update that styles
  them (post-style-phase append step — a pinned invariant).
- **Damage rects:** written and unit-tested (bounded 8-rect buckets,
  overflow promotes to full repaint), **never consumed** — the paint stage
  repaints full layers and clears the whole surface per layer per apply.
  The largest known perf item in the crate (D7: preserved, staged goal,
  driver is animation on large surfaces).
- **Preserved subsystems:** layer lifecycle reconciliation, hit test and
  scroll, popup clamp/placement, the intrinsic system (equality-based
  change detection; the phantom-dirty lesson is institutionalized in the
  visitor README).

### 2.3 The text path

cosmic-text 0.18.2. `TextLayoutRequest` carries text, font size, line
height, wrap policy, alignment, a family ID (one variant: `UiSans`), and a
language hint; `preferred_repo_font_family` (`text_render.rs:45-63`) maps
language hints to the four installed faces (IBM Plex Sans variable +
Source Han Sans ×3, JP/SC/K). Intrinsic measurement via
`measure_intrinsic` (zero-size guarded); caret maps for editing; the
multi-stage pipeline (measure → shape → rasterize/cache) exists but is not
a sealed boundary. Font properties compute but are unconsumed (registered
divergence). Justify is consumed (on-spec); `pre-line` is on-spec;
`break-spaces` folds into `pre-wrap` (registered divergence). Quoted
family names keep literal quotes (string-token gap, recorded for the
follower). Zero-size guards now cover every cosmic call site after the
accounting found two crash sites the hard way.

### 2.4 The flex solver

`resolve_active_flex_container` (`flex.rs:1586`): staged measure → line
collection → resolution phases over `MeasuredFlexItem`/`FlexLine`/
`FlexContainerStyle`. Coverage: batches 3–7 pinned (135 proofs in
`layout/flex_layout.rs`, 59 solver unit tests including the
`measured_item_record_*` family). Two GRACEFUL support gates (landed
2026-10-19, replacing the former fail-fast asserts — see the decision
log): outside the supported content-based auto-min subset (horizontal,
non-reversed, no-wrap containers; non-growing walk children) the
auto-min degrades to the item's measured main size, registered
divergent. The unadmitted remainder is gated by the closed grammar and
these gates; there is no placeholder fallback (the old one died in the
typed-group migration). The file is 5,356 lines and its data structures do
not map to Blink's `FlexLine`/`FlexItem` or Gecko's equivalents.

### 2.5 `RootDom` and `RootDomContext`

`root_dom.rs` (7,340 lines, 48 public methods) cohabitates: tree mutation,
attribute writes, the style-attr route, the inline declaration store, the
transition scheduler, scroll, focus, the popup/options system
(`PresentedOptions*`), layer queues, metrics accessors, and the computed
read surface. `RootDomContext` is an `Arc` holding
`Mutex<StyleRuntime>` and friends (font catalog, render flow), shared
across documents; the revision counter version-stamps the rule set and
`sync_stylesheet_runtime` is its single reader (once per update — the one
sanctioned counter consumer). `RootDomFragment` holds no declarations
(documented limitation).

### 2.6 The verification infrastructure (the moat)

426 layout proofs (418 in
`style/`/`layout/`/`text/`/`widgets/`/`pipeline/` plus 8 in the
cross-cutting `robustness.rs`; updated 2026-10-19 after flex arc step 3c);
the off-spec registry (19 divergent proofs in 15 rows, 5 deliberate,
grouped `unsupported_*` gates, a robustness family — census unit is
PROOFS, not rows: the font-family and font-weight/style divergences
share one registry row) as a living divergence map with
reclassification discipline;
`perf_metric.rs`/`perf_allocation.rs` pinning exact work counters and
allocator windows (history assertions ordering-only); structural lint
rules 1–5 (mechanical, no exemptions); the zero-allocation metrics
doctrine (enum-indexed counters, fixed-capacity history ring, levels via
read accessors); the process norms that no commit is red and behavior
change travels with its proof. Third-party adversarial review is standard
practice here and has caught real defects in every round (a non-atomic
load path, a sign-policy bypass through shorthand expansion, vacuous pins,
a dead registry citation, a mistimed transition pin, and the
zero-size-line inconsistency).

### 2.7 Honest weaknesses (named before anyone else names them)

1. The god objects: `root_dom.rs` (7,340) and `dom_node.rs` (2,425);
   `flex.rs` (5,356) close behind.
2. `DomError` carries no payload — 16 flat variants, and
   `InvalidAttributeValue` names neither property, value, nor node.
3. The vocabulary gap (P1) and the eight registered divergences.
4. Damage rects unconsumed (the largest known perf item).
5. Matched-rule caching deliberately unbuilt (evidence: passes visit 1–3
   elements in pinned scenarios — the deferred proposal J, asleep on
   evidence, not forgotten).
6. Text pipeline thinness (P4).
7. The bespoke flex solver (P3).
8. Bevy/3D-engine coupling at the composite stage (accepted as the
   product; the render-ops seam is the one clean decoupler; the door to an
   abstraction is documented but closed by choice).
9. `RootDomFragment` cannot hold declarations (documented; import
   re-projects correctly).
10. Two pinned-but-uncomfortable admissions of record: the near-white
    initial color and the zero initial border widths (both deliberate,
    both re-reviewed on revisit).

---

## 3. The proposals

### PR 1 — margin and style gaps (the admission queue) `SETTLED`

**LANDED (2026-10-18, branch `pr1-margin-style-gaps`, 17 commits, PR #17
merged to main):** all seven items plus the transition proof-family companion bundle,
followed by a third-party review round with all three findings adjudicated:
(a) percentages that silently no-op'd on line-width/radius grammars were
rejected at admission (`DeclGrammar::LineWidth`/`SignedLineWidth` — the
width families are spec-closed `<line-width>`/`<length>`; the radius
rejection is admission-gated on a corner-pair elliptical model, never a
silent resolve); (b) canonical style snapshots now preserve `!important`
for every value kind via a CSS-string serializer (colors included, with a
round-trip pin through the style-attribute route); (c) percentage-authored
transitions stepping is pinned as a documented follow-up (schedule-time
basis resolution, admission-gated per D6). Item 1
margin/padding shorthands; item 2 the three-branch negative-margin collapse
plus the deliberate spillover decision pinned at the composite
(`clip_world`) and integration levels; item 3 `box-sizing` (vocabulary 68
→ 69, the D6 policy's first post-v0.6.0 admission); item 4 the `!important`
tier (`CascadeTier` descending `ImportantAuthor > NormalAuthor > NormalUa`,
importance bits parallel to declarations, CSSOM upsert rules, identity hash
and dedup importance-aware); item 5 opacity clamping (the registry's last
reject-side divergence closed); item 6 transform transitions
(`DeclValue::Translate`, with a double-scheduling fix to the transform
moving-field mapping found during landing); item 7 the `BoxLength`
computed/used split with the `margin: 0 auto` centering pattern working in
shorthand and longhand forms (public API break, recorded in
`MIGRATION.md`); plus the seven-test transition behavior family (easing
control points, negative delay, re-targeting, list-repeat, completion,
from==to, overlay-beats-important). The off_spec census at landing stood
at 5 (font-family unconsumed, font-weight/style unconsumed,
`break-spaces`, range pill, `line-height: normal` → 1.0×); the
post-landing adversarial review (2026-10-19, see the decision log)
registered two more divergent §8.3.1 boundary rows, moving the census
to 7 proofs in 6 rows.

Seven items, ordered cheapest-first. Each is its own commit on one branch;
each flips its registry pin in the same change. No debate required — the
designs below are settled and the pins are the witnesses.

1. **`margin` / `padding` shorthands.** Add `Shorthand::Margin` /
   `Shorthand::Padding` to `property_shape` (`decl_parse.rs`) reusing the
   exact 1–4 corner mapping of `expand_border_radius`. No new structures;
   the cascade and groups only ever see longhands (the reason all
   shorthand admissions are cheap). Pin flips:
   `unsupported_margin_shorthand_drops`, `unsupported_padding_shorthand_drops`.
   Spec: CSS Box 3 §4.2–4.3; Servo/Blink expand-at-parse.
2. **The negative-margin collapse fix.** `collapsed_sibling_margin_overlap_px`
   (`src/domains/layout/flow.rs:962-975`) computes the collapse overlap as
   `min(a, b).max(0.0)`, and the used sibling gap is `a + b − overlap` —
   so for (−10, −5) today's overlap is 0 and the gap is −15, where CSS 2.1
   §8.3.1 requires −10. **The correct fix computes the used margin directly
   in three branches — both-positive → `max(a, b)`; both-negative →
   `min(a, b)`; mixed-sign → `a + b` — and derives the overlap as
   `a + b − used`** (removing the clamp alone yields overlap = −10 and a
   gap of −5, which is still off-spec; the three-branch form is the only
   one that yields −10 for the pinned case). The mixed-sign branch already
   matches today's behavior (summation is spec-correct for one positive +
   one negative margin, and the comment must say so — today's subset note
   at `flow.rs:966-968` scopes this path to the non-negative slice).
   Pin flip:
   `off_spec_adjacent_negative_block_margins_collapse_to_most_negative`
   relabels on-spec. The registry's oldest divergent entry closes. **With
   it lands the spillover verification pin** (see the decision log):
   a negative-margin descendant is clipped at its clipped layer's
   bounds (deliberate — the pin drives a first child; the mechanism is
   sibling-independent), and the Normal-layer escape hatch paints it.
3. **`box-sizing`** (CSS UI 4 §3.1). Add the property (box group):
   `BoxSizingKw { ContentBox, BorderBox }` on `ComputedBoxGroup` (initial
   `ContentBox` — the crate's single model today, which is also the CSS
   initial). One branch in the sizing projection (landed as
`content_box_size_px`, `domains/layout/flow.rs`):
   border-box subtracts padding+border from the specified size to reach the
   content box; content-box is today's path verbatim. Pin flip:
   `unsupported_box_sizing_drops_and_border_box_sizing_holds`.
4. **`!important`** (CSS Cascade 5 §6.1, §6.4.1 — F stage 1, pre-shaped).
   `DeclBlock` gains a parallel importance bit-vector (the Servo
   `declarations_importance: SmallBitVec` shape named in the redesign's
   F verdict; `decl_block.rs`'s header notes only that importance bits
   "arrive later with cascade tier path F"). Declaration parse strips the
   `!important` suffix and sets the bit (it is part of the declaration,
   not the value). **The `Precedence` record gains a tier field, NOT a
   bool-first flag:** `Precedence { tier, origin_priority, inline,
   specificity, order }` where `tier` descends `ImportantAuthor >
   NormalAuthor > NormalUa` (today's behavior sheet). This projects CSS
   Cascade §6.1 correctly — important author declarations beat all normal
   declarations including normal inline style (important inline style is
   `ImportantAuthor` at top specificity); transitions still win above
   everything (the overlay is tier-1-equivalent and untouched); and the
   important-vs-important case across author origins sorts by
   `origin_priority` *within* the `ImportantAuthor` tier, matching §6.4.1's
   class reversal (the UA/user/author ladder, with the behavior sheet as
   the UA-ish rung) without the bool-first shape's baked-in violation.
   The tier field is also the honest reading of `decl_value.rs:34-38`'s
   contract ("widened later for importance/UA/animation/layer tiers
   without reshaping the key"): stages 2–4 (a real UA tier, the animation
   tier, cascade layers) extend the tier enum additively — `ImportantUa`
   above, `Animation` between `ImportantAuthor` and `NormalAuthor`, layer
   indices within a tier — rather than reshaping its semantics.
   Pin flips:
   `unsupported_important_declarations_drop_but_the_rule_survives` and
   `unsupported_important_in_style_attribute_drops`.
5. **`opacity` clamping** (CSS Color 4 §12.2). The `Opacity` grammar
   clamps any finite f32 to [0, 1] instead of rejecting outside it. Pin
   flip: `off_spec_opacity_out_of_range_rejected` — the registry's last
   reject-side divergent entry closes.
6. **`transform` transitions.** Admit a typed translate pair into the
   scheduler's interpolation table: the transform group already computes
   `translate_px: [f32; 2]`; the scheduler lerps the two channels like
   lengths; `none` steps. Pin flip:
   `off_spec_transform_transitions_do_not_schedule` (the one whose act-3
   timing the review retimed to a discriminating progress-0.5).
7. **Percentages and `auto` margins** — the large item, deliberately last
   and splittable into its own landing if it outgrows PR 1. The
   computed/used split (CSS Cascade §4.5): box-group length fields become
   `BoxLength { Auto, Px(f32), Percent(f32) }` (still `Copy`, still
   value-compared — the S1 invariants hold). Computed keeps the authored
   form; layout resolves `Percent` against the containing-block dimension
   it owns (width and margin/padding against the containing-block WIDTH,
   even vertically, per CSS Values §percentages); `auto` margins resolve by
   distributing free space (the block-layout centering case); `auto` for
   width/height maps to the existing `None`/initial (free).
   **Blast radius, stated fully:**
   (a) **public API break** — `ComputedBoxGroup`'s length fields are on the
   computed read surface; `BoxLength` replacing `f32` lands in
   `MIGRATION.md` with a named decision, the same standard PR 5 applies to
   its own contract;
   (b) **reader fan-out** — "layout resolves Percent" is not one branch but
   every consumer of every box length (`flow.rs`, `flex.rs`, walker,
   sizing projection), each needing the containing-block width at the point
   of use;
   (c) **flex auto margins are explicitly OUT of scope for this item** —
   Flexbox §8.1's per-item free-space absorption is a different algorithm
   from the block case and is the single most common consumer use of
   `margin: auto`; it is named as its own later admission (gated by
   consumer need per D6) rather than silently bundled, because guessing
   either way ships as a bug.
   Pin flips: the percentage rows of `unsupported_percentage_lengths_drop`
   and both rows of `unsupported_auto_lengths_drop`.

**PR 1 proof obligations:** every admission flips its pin in the same
change; the full gate per item; perf pins re-pin only with a documented
cause; the spillover verification pin lands with item 2. **Costs and
honest risks:** item 2 changes visual output for a rare case and needs a
consumer-visible note; item 4 touches the cascade's core comparison (the
winners fold and the block — the highest-blast-radius item in PR 1,
mitigated by the entire cascade proof family); item 7 is the only one that
reshapes a group field type, and its used-value resolution must not grow
per-frame allocation (the perf pins are its judge).

### PR 2 — style snapshot sharing `KILLED 2026-10-19 — per its own kill criterion; no code`

**Kill record (2026-10-19).** Killed per the S10 kill criterion below:
the lock is provably contention-free in every realistic scenario we can
name (single-threaded update loop; uncontended acquisition is tens of
nanoseconds against per-element match work orders of magnitude larger,
in passes visiting 1–3 elements in the pinned scenarios), and no
consumer has named a concurrent-reader need — the plan itself could
state consumers' multi-document expectations only as "a question, not a
fact," which fails the crate's own D6 standard. The one non-perf
justification found in debate preparation — per-pass consistency
(today's per-element lock admits a rule-set tear if a load interleaves
a pass, `winners_fold.rs:201-227`) — is latent under the SAME missing
precondition (interleaving requires the concurrent consumer nobody has
named); it wakes with that consumer, not before. Alternative (d)
(per-document runtimes) was also rejected as speculative: it would
remove a deliberately built sharing capability on the same absence of
evidence. No lock-wait counter was landed: instrumentation for a killed
item is scope creep; a revival arrives with a named need and the
counter lands with it. **Wake conditions** (any one revives this PR
with its debate intact): (1) a consumer names concurrent or interleaved
multi-document driving; (2) a named per-pass consistency requirement
from a real interleaving pattern; (3) pass element counts grow until
even same-thread lock cost is measurable (proposal J's gate counters
would show it first); (4) a measured nonzero lock wait in a realistic
scenario. The debate framework below is preserved verbatim as the
revival starting position.

**Problem (restated honestly).** The style pass locks
`Mutex<StyleRuntime>` once per element per pass. It is correct. It may
also be mostly *unimportant* today: passes visit 1–3 elements in the
pinned scenarios and the lock is uncontended on a single-threaded loop.
Any claim that this PR is a performance win must survive that sentence —
which is why this PR is debate-gated.

**The proposal (starting position).** Snapshot-per-revision sharing:
`StyleRuntime` publishes an immutable snapshot of the rule set per
revision behind a cheap swap; the style pass takes one snapshot reference
at update start and matches against it lock-free for the whole pass;
`load`/`unload` take the mutex briefly to build and publish the next
snapshot. The per-element lock disappears from the hot path.

**Alternatives on the table.**
(a) `arc-swap` crate for the swap primitive.
(b) `Mutex<Arc<StyleSnapshot>>` — clone the `Arc` once per pass under a
nanosecond lock; no new dependency; arguably achieves everything (a) does
at this scale.
(c) Do nothing and instrument first (a lock-wait counter), letting
evidence decide — the proposal J discipline applied to itself.
(d) Per-document style runtimes (give up sharing; rejected unless the
debate shows context-sharing is unused — consumers' multi-document
expectations are a question, not a fact in this debate).

**Open questions the debate must answer.**
1. What is the measured cost of the current lock (instrument before
   proposing demolition — is this a clean-up sold as a speed win)?
2. Snapshot memory: deep copy of rules+indexes per revision (O(rules) per
   load — loads are rare) is almost certainly sufficient at this scale;
   structural sharing (Servo rule-tree style) is explicitly out unless
   measurements force it. Does anyone disagree, and on what evidence?
3. Lifetime and GC: `Arc` keeps the previous snapshot alive while a pass
   holds it; passes are same-thread and bounded — is any harder story
   needed?
4. What does NOT belong in the snapshot: the font catalog and render flow
   are context-scoped but must NOT be captured into snapshot lifetimes
   (they are not style data). Confirm the boundary.
5. The revision seam: `sync_stylesheet_runtime` currently compares a `u64`
   per update (via the `pub(crate)` accessor on `RootDomContext`).
   Does the snapshot identity/revision subsume it cleanly, and
   does that accessor's contract survive unchanged?

**What the reviewer should attack:** the absence of measurement; the
hidden assumption that contention will ever exist; the risk of shipping a
new primitive (a) when (b) suffices; and whether this PR is really
P2-solving or architecture-for-its-own-sake.

**Kill criterion (added per the S10 review):** this PR dies if the debate
concludes alternative (c) — measurement shows the lock is contention-free
in every realistic scenario AND no consumer names a concurrent-reader
need. A debate that can only ratify is not a debate.

### PR 3 — the two-fold layout pass and the flex fix sequence `DEBATE — mapping document WRITTEN and AMENDED under adversarial review 2026-10-19 (FLEX_MAPPING.md); third-party agreement against the enumerated obligations (FLEX_DEBATE_BRIEF.md) required before implementation`

**Problem.** The solver works for batches 3–7 and is the users' biggest
complaint anyway. The complaint decomposes into: coverage gaps (the
unadmitted subset), and confidence (no shared vocabulary with the engines
when behavior is questioned). The file is 5,356 lines of bespoke
structures.

**Scope, stated honestly (rewritten per the 2026-10-19 adversarial
review).** This PR is not a flex-solver migration in the framing this
section originally carried. The binding layout-pass doctrine (decision
log, 2026-10-19) mandates ONE bottom-up intrinsic fold and ONE top-down
resolution/placement fold per update for ALL of layout — zero-allocation,
with the recursive auto-min walk and per-container allocations dying by
doctrine, not by this debate. PR 3 is therefore two things: (1) the
two-fold re-architecture of the layout pass — every dirty node carries
the measurement record, and flex is the first full consumer — and (2) the
sequenced flex fix landings on the new structures, ordered by the
two-axis complaint list. The debate ratifies THIS scope; the old title's
"migration" framing is retired.

**The first deliverable (before any code): the mapping document.** Blink's
`flex_layout_algorithm.cc` and the `core/layout/flex/` headers (fetched at
Chromium `61ef1bef` — `flex_layout_utils.cc` does not exist upstream;
`.tmp/engine-corpus/MANIFEST.md`) and Gecko's flex
container/frame code (mozilla-central `0fdb388b`), mapped field-by-field to `MeasuredFlexItem`,
`FlexLine`, `FlexContainerStyle`, and the `resolve_active_flex_container`
phases — with the engines' durable shapes named (flex lines as
first-class objects with hypothetical main sizes; items with min/max
clamps; violation accumulation for balancing; free-space distribution
after constraint resolution), every delta named, and each marked
justified-ours or fix-to-theirs. **The document carries an explicit
engine tie-break rule:** when Blink and Gecko disagree (they do — line
balancing and violation accumulation differ structurally), WPT decides
first and Blink second, matching the proof suite's per-claim provenance
rule; without it every contested phase becomes its own unplanned debate.
The document landed 2026-10-19 and was AMENDED the same day under
adversarial review: the per-row restructure/in-place classification
(§1), the record's basis-layering respec and the lane lifecycle (§3.1),
the run-aggregation, whitespace, and static-position models (§3.2), the
refold budget (§3.4), the step-1 policy split (§4), the
verification-burden restatement (§6), and the third-party obligations
(§8). This document is the debate's subject.

**The work (only after the debate).** (1) The two-fold re-architecture
lands first, semantics-preserving: the up fold writes records for all
dirty nodes, the flex measurement sites cut over to record reads where
the values are identical by construction, and the per-container `Vec`s
and the `HashMap` re-join die — with ZERO pin flips and the full suite
green, because it is the algorithm change, reviewed as a performance
change per the structural-migration policy. The recursive auto-min walk
and the staged double-layout survive the structural step deliberately;
their deaths are semantic and land with their pin flips in the sequence.
(2) The semantic landings follow FLEX_MAPPING §4, each flipping its rows
in the same change, each running the full gate. **Scope honesty,
retargeted per review:** the pre-review warning ("a behavior-preserving
migration moves zero coverage by construction; the migration without the
admissions is aesthetics") applied to a framing that no longer exists.
The equivalent failure mode in the new shape is landing the folds and
STOPPING — architecture without the fix sequence. The complaint list,
not the folds, is the point; the debate must not let the sequence be
descoped after the architecture lands.

**The remedy's shape and its verification burden (added per review).**
P3's original premise cited the style redesign as proof of the remedy —
"adopt the engines' data structures." The style redesign's correctness
argument was STRUCTURAL ISOMORPHISM: `DeclBlock` mirrors Servo's
`PropertyDeclarationBlock` field-for-field, so correctness could be
audited at the field level against the source. The doctrine forbids
engine-shaped mutable structures (in-place item mutation, persisted
solver caches), so this PR adopts the engines' SEMANTICS on
doctrine-conforming data — a strictly weaker verification position with
no field-level isomorphism to audit. The substitute is behavioral
equivalence: per-phase engine-behavior proof families keyed by spec
section with WPT provenance, INCLUDING phases that move no registered
row (FLEX_MAPPING §6). The per-row pin flips are necessary, not
sufficient — the rows pin the known divergences; the phase families pin
the skeleton the review can no longer audit structurally.

**The debate must answer.**
1. The complaint list — answered by construction (FLEX_MAPPING §1). The
   debate's remaining product is SEQUENCING via the §1 classification:
   which NO/PARTIAL rows, if any, pull forward as in-place pin-flip
   landings on the current solver on client criticality (accepting double
   work — the row flips in place, then re-pins on the new structures),
   each pull recorded in §4.
2. Map-first vs admit-first — re-scoped per review. The restructure
   stands on the doctrine independently of the mapping's conclusion (the
   recursive walk and per-container allocations die regardless). The live
   question is the per-row classification, adjudicated row by row; the
   debate may move any row's classification on evidence.
3. What does "done" mean — unchanged: the named subset resolving the
   complaint list (FLEX_MAPPING §4 steps 1–9), plus the step-10
   admissions the debate scopes (auto margins §8.1 first). Full
   structural alignment (baseline, collapse struts, fragmentation,
   balanced wrap) stays gated, named.
4. The solver's 59 unit tests (verified counts; pre-review figures were
   stale: 17 `measured_item_record_*`, 23 `resolve_active_flex_container_*`,
   19 others) — genuine unit assertions re-home onto the record/lane
   unit surface; cross-module assertions re-express through the public
   surface; bespoke-internal tests die with the bespoke structures. The
   per-file split is a debate artifact produced at the structural step.
5. **Kill criterion (RESTATED 2026-10-19 — the original form was
   void).** The original — "dies if the solver is already closer to the
   engines than feared" — failed twice over: "closer than feared" is
   unmeasurable, and the layout-pass doctrine settled the same day
   already mandates the restructure regardless of the mapping's
   conclusion, so the criterion could not fire by construction (a debate
   that can only ratify is not a debate — the rule PR 2 was killed
   under). Restated operationally: the restructure dies only if (a) the
   debate produces pinned-scenario measurements showing the recursive
   walk plus per-container allocations OUTPERFORM the two-fold
   architecture on the workloads the proof suite pins — i.e., the
   doctrine itself is reversed on evidence — or (b) the third party
   refutes the mapping's §2.1 skeleton claims from the engine sources.
   Falsifiable in both directions. The in-place/admit-first question is
   NOT killed by the restructure's survival: it lives on as the per-row
   classification of question 1.
6. **Grid's arc position (REQUIRED OUTPUT, added per review).** The
   client signal names grid alongside flex; the gate enumeration (14
   properties + display) is ready. The debate MUST output a dated
   decision: grid enters the arc at a named step, or is explicitly out
   of the arc with rationale. "Later, unspecified" is a non-decision and
   does not close the question. FLEX_MAPPING §8 carries the
   recommendation (enter immediately after the fix sequence, before
   PR 4 — the intrinsic fold grid needs will exist, and the signal ranks
   grid with flex); the debate may overrule it with rationale.

**PR 3 proof obligations (added per review — the section previously
lacked them; PR 1's standard applied).** The structural step lands with
ZERO pin flips and the full suite green, perf pins as judge; every
semantic step flips its rows in the same change; the full gate per step;
the new perf counters and the pinned text-heavy refold scenario land
with the structural step; per-phase engine-behavior proof families per
the verification-burden paragraph; perf pins re-pin only with a
documented cause.

**What the reviewer should attack:** whether the §1 classification hides
structural dependencies in "in-place" rows; whether the basis-layering
rule leaks (any basis-dependent value computed in the up fold); the
run-aggregation cursors against heterogeneous content; the refold
budget's number; and the sequencing (why third, not first, if it's the
biggest user complaint — answer recorded in the decision log, open to
challenge).

### PR 4 — the text seam, proven open (second-to-last) `SETTLED in shape, GATED on proof`

**The design.** Formalize text production as a sealed three-stage seam,
the render-ops seam's shape for text:

- **Measure** — intrinsic sizing (min/max content, line height) feeding
  layout.
- **Shape** — line breaking, glyph runs, caret maps feeding paint and
  editing.
- **Rasterize/cache** — glyph masks and atlas feeding the GPU.

The seam owns: the font catalog, a fallback policy TABLE (family list →
catalog query → generic mapping → guaranteed-final face — one file, tested
per script, replacing today's implicit fallback), and every cosmic
interaction (nothing outside the seam constructs a cosmic `Buffer` — the
rule the zero-size crash saga taught). The style system hands the seam a
resolved `TextStyle` (family list, size, weight, style, language hint).
**Font-family/weight/style consumption lands here, at `TextStyle`
construction** — the named step-5 follower, unblocking the top consumer
gap. The quoted-family string-token decoding (recorded caveat) lands with
it.

**The proof gate (explicit; this is what makes the PR second-to-last).**
Before merge, the seam must render through the public pipeline with proofs:
1. New font faces installed through the catalog (not hardcoded additions —
   a consumer-supplied face path).
2. Scripts beyond the pinned CJK coverage (choose at least one with
   shaping complexity, e.g. Arabic or Devanagari). **This gate depends on
   gate 1:** the four bundled faces (IBM Plex Sans variable + Source Han
   J/SC/K) do not cover those scripts, so complex-script proofs are only
   possible via the consumer-supplied-face mechanism — an implementer may
   not "pass" gate 1 with a Latin face and strand gate 2.
3. RTL text: bidirectional shaping and caret behavior.
4. Vertical writing: either a working path or an explicitly named
   boundary (today `writing-mode` is unadmitted — the proof documents
   which, honestly).

**Kill criterion:** if the new interface cannot express those honestly, we
do not make the change. The fallback is today's seam, documented.

**Ownership and boundaries (stated per the S7 review):** the seam's font
catalog and fallback policy are **context-scoped** (shared across
documents, like today's `TextLayoutConfig`) — the joint-boundary
question with PR 2's snapshot died with PR 2 (fonts were never inside
`StyleRuntime`; the exclusion boundary was the type boundary); making
the seam document-scoped
would be a multi-document sharing regression and is rejected. **The
line-height 1px floor stays in the style resolver** (`project.rs`: it is a
computed-value policy, and the registry pins it permanently); the seam
owns the cosmic call sites and the floor's render-side backstop — the
computed read surface does not change observably, and the permanent pin
does not move.

**Honest risks:** the seam formalization could itself narrow what can be
requested (a door-closer by accident — hence the gate); fallback policy
centralization changes rendered output for some scripts (pin flips where
observable); RTL exposes that our caret and alignment models are
LTR-first (real work, or the gate's point 3 names the boundary).

### PR 5 — `RootDom` decomposition (last) `SETTLED in direction`

**The design (advice accepted 2026-10-18):** narrow newtypes with the
existing facade:

```rust
pub struct RootDom {
    tree: RootDomTree,               // nodes, links, ids, fragments, hit-test state
    styling: RootDomStyling,         // inline decl store, computed texts, style revision
    transitions: RootDomTransitions, // scheduler state, active nodes, clock
}
```

One file per subsystem with its own impl (structural rules 1/5 hold).
**The compatibility contract, stated precisely:** the 48 `pub fn` on
`RootDom` stay public, as thin delegations on the facade — no
consumer-visible change without a named decision recorded here. The
remaining ~70 `pub(crate)` and private methods (the actual body of
`root_dom.rs`) distribute to the subsystems, which is where the line count
goes. Follow-on ownership correction, sequenced INSIDE this PR (it is not
a trigger from outside): the popup/options system leaves `RootDom` for the
widget layer (`control_widgets.rs`/`element_behaviors/`).

**Migration policy:** structural-migration — one subsystem at a time,
byte-identical where behavior must not move, full gate per move.

**Honest cost (stated for the reviewer):** no user-visible win. It is a
large mechanical diff whose only return is that the crate's core file
stops absorbing everything. It is sequenced last precisely because it is
the least valuable per risk at any earlier point — and its scope should be
re-evaluated when it starts: if work earlier in the arc has already shrunk
`root_dom.rs` materially, this PR descopes to match (the challenge is
welcomed; what it may NOT do is fire on PR 1, whose items do not touch
`root_dom.rs` materially — the file stands as it is until this PR's own
moves begin).

---

## 4. Decision log (dated; consolidated so nothing is re-litigated)

- **2026-10-18 — the layer-spillover decision.** Layers clip; spillover is
  a layering decision, never a surface-expansion decision. Content pulled
  outside a clipped layer's bounds (negative margins, `overflow: visible`
  children, transforms) is clipped, where a browser would paint it.
  Rationale: (1) bounded layer surfaces ARE the render cache — scroll,
  animation, and targeted repaint speed is the product, and surface
  expansion defeats the cache geometry and the damage-rect model;
  (2) negative margins are rare, and legitimate visible spillover is a
  structural decision the author expresses with an unclipped (Normal) or
  dedicated layer; (3) it is **decided** as a deliberate visual divergence —
  registration of the registry row and the verification pin are **owed in
  PR 1 item 2** (the pin asserts both the clipped behavior and the
  Normal-layer escape hatch painting the spill; the registry entry lands
  with it, class deliberate). Orthogonal to the
  negative-margin collapse math, which is a plain spec violation fixed in
  PR 1 item 2 regardless (it is cheap, and the registry is more valuable
  when every entry is a stance, not an accident).
- **2026-10-18 — the `/types` advice accepted.** Narrow newtypes over
  trait-based capability splits; popup/options moves to the widget layer
  as the follow-on ownership correction. (Option A over Option B; Option C
  as the follow-on.)
- **2026-10-18 — the arc order.** Quick wins (PR 1) → sharing (PR 2) →
  the user-visible big win (PR 3) → the prove-or-abandon seam (PR 4) →
  the structural cleanup (PR 5). PRs 2 and 3 gate implementation on the
  written debate plus third-party agreement recorded in their sections;
  PR 4 gates merge on the doors-open proofs. Sequencing rationale: PR 1 is
  settled design with pins as witnesses; PR 2 must not be sold as a perf
  win without measurement, so it needs the debate; PR 3 is the biggest
  user complaint but also the biggest risk, so it waits for the arc's
  credibility and its own mapping debate; PR 4's kill criterion makes it
  safer second-to-last than earlier; PR 5 is deferred value, last by
  design.
- **2026-10-18 — the admission queue order within PR 1** (shorthands →
  collapse fix → box-sizing → !important → opacity → transform transitions
  → percentages/auto last and splittable).
- **2026-10-18 — the transition pinning gap** (from the architect's
  library review, user-confirmed): a dedicated transition proof family is
  owed — easing-curve correctness against CSS control points, negative
  delays, mid-flight re-targeting, list-repeat, completion/overlay
  removal, `from == to` skip. It slots into PR 1's branch as an
  uncontroversial companion bundle (`pipeline/transitions.rs` rows) unless
  a better home is named. **LANDED in PR 1** (the seven-test family).

- **2026-10-18 — the percentage-semantics table and the resolution-semantics
  rule (process correction).** After two review-blocking defects in item 7's
  percentage work (silent f32-path no-op; indefinite-basis height collapse),
  the rule for the rest of the arc is: **any feature touching resolution
  semantics gets its full semantics table written here BEFORE
  implementation.** The percentage table as fixed:
  width/min/max-width/margin×4/padding×4/left/right resolve against the
  containing-block width (always definite via the viewport chain, CSS 2.1
  §10.1); font-size/line-height against parent/own font size (always
  definite); **height** against the cb height with Percent+indefinite →
  **auto** (§10.5); **max-height** → **none** (§10.7); **min-height** →
  **0** (§10.7, and `Auto` → 0 per CSS Sizing 3); top/bottom percentages
  resolve at offset-application time against the laid-out containing block
  (deferred, pinned `relative_top_percentage_defers_to_offset_time`);
  border/outline widths, outline-offset, radius (elliptical gate), gap, and
  flex-basis percentages are rejected at admission and pinned. The domain
of percentage resolution is `Option<basis>` plus a per-field rule, never
an f32 basis.

- **2026-10-19 — the post-PR-1 adversarial review and its remediation.**
  A seven-agent adversarial review of the PR 1 landing (the architect
  re-verified the load-bearing claims firsthand) found the landing claims
  substantially true with the full gate green, plus one critical defect and
  a set of contract gaps. Adjudication:
  (a) **critical, fixed** — `strip_importance_marker` panicked on
  non-ASCII boundary tails (a byte-index `str` slice at a non-char
  boundary, e.g. `width: 10px !ééééé`), violating `decl_parse.rs`'s own
  header invariant and the AGENTS.md panic policy. Fixed in `395c97e`
  (boundary-safe `str::get`; an unrecognized marker falls through to the
  grammar as a counted drop / typed error) with four unit pins at both
  parse entry functions, plus the public-route robustness pin
  `robustness_non_ascii_importance_marker_drops_without_panic` (in
  `e237763`). (b) **registry contract repairs** (`e237763`) — two
  divergent rows for the §8.3.1 boundary clamps item 2 did not touch:
  `off_spec_parent_boundary_negative_margins_clamp` (first-child top /
  last-child bottom / empty-intermediary `.max(0.0)` clamps) and
  `off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative`
  (plain `max`, predating item 2's three-branch rule; admission is the
  same shape, deferred per D6); the flex auto-margin row rescoped to the
  truth (main-axis computes to 0; a block-level item in a column
  container absorbs cross-axis free space through the block solver —
  accidental, §8.1-aligned for the pinned case — pinned by
  `column_flex_block_item_cross_auto_margins_absorb_free_space`);
  robustness count 7 → 8. (c) **doc-drift corrections (this change)** —
  the §2.6 census numbers and the proofs-not-rows unit rule, the
  landing-note commit count (17, not 13), item 3's branch citation
  (`content_box_size_px`, `domains/layout/flow.rs`), the item 2 spillover
  pin's descendant wording, and the MIGRATION.md flex auto-margin note.
  (d) **the weak-pin backlog** — the four items the review classed as
  owed (not optional) were closed the same day in `d7b6cd9`: the
  `transition_completion_removes_the_overlay` discrimination gap (now
  asserts the `TransitionEnd`-event counter 0-before / 1-at completion);
  the malformed-`!important` block-parse counted-drop pin; the
  canonical-snapshot `!important` round-trips for the
  keyword/number/integer/text value kinds (the text kind round-trips
  stably — the literal-quotes caveat is now pinned, not just recorded);
  and the `min`/`max`-under-`box-sizing` registration
  (`off_spec_box_sizing_min_max_sizes_clamp_in_content_box_terms`,
  divergent: 120px pinned current vs 100px spec-correct, admission
  deferred per D6). The remaining set is hardening the plan never
  promised, owned by the arc's coverage effort and not blocking PR 2's
  debate: a 3+-declaration mixed-importance middle-removal pin for the
  `DeclBlock` bit-vector; a 3-value `padding` shorthand case and padding
  through the style-attribute route; box-sizing's border contribution,
  height-axis subtraction, and zero-clamp pins; end-to-end percentage
  rows for padding×4, min/max-width, and left/right; and a
  non-finite-opacity robustness row (unit-only today).

- **2026-10-19 — PR 2 killed per its own kill criterion; the arc order
  updates.** PR 2 (style snapshot sharing) is dead: the kill criterion's
  condition (contention-free in every realistic scenario AND no named
  concurrent-reader need) was checkable from debate preparation alone —
  running the full debate to reach it would have been the theater the
  criterion exists to prevent. The arc is now PR 1 (landed) → PR 3 (the
  flex mapping debate, next) → PR 4 (the text seam) → PR 5 (the
  `RootDom` decomposition). P2's mechanism (the `Mutex<StyleRuntime>`
  per-element lock) remains correct for the single-threaded loop and is
  a named boundary with four wake conditions recorded in PR 2's kill
  record; the per-pass consistency observation is preserved there so it
  survives context compaction. PR 3's debate is unaffected and is a
  different kind from PR 2's: it HAS a named driver (the users' biggest
  complaint), so its debate proceeds with real substance.

- **2026-10-19 — PR 3's driver named (consumer signal).** Several client
  teams cannot use the crate's flex or grid layout for real designs and
  fall back to absolute positioning — which is plain-Bevy usage and
  defeats the library's purpose. This satisfies the named-driver bar
  PR 2 failed. Two consequences: (1) the signal is the existence proof
  of the complaint list, not the list — the list must be collected as
  reproducible rows (authored input / expected / actual / workaround)
  before the mapping document's debate, per PR 3's debate question 1;
  (2) "grid" is NOT inside PR 3's current scope (the flex-solver
  migration): `display: grid` is unadmitted (the
  `unsupported_display_values_drop_to_initial` gate), and grid admission
  is a NEW layout algorithm, not a migration. Whether grid enters the
  arc, and where, is a named decision owed before PR 3's debate closes.

- **2026-10-19 — the complaint list is produced by SPEC-SURFACE
  enumeration. (Scenario accounting was proposed and rejected — for the
  second time; a previous scenario-based effort became a boundless black
  hole and still missed cases. The lesson, recorded so it is not
  re-proposed: design-pattern space is unbounded and its coverage is
  unauditable; the spec's own structure is the only enumerable test
  space.)** Clients cannot write spec-shaped complaints ("elements are
  not moving where they should" is the honest ceiling of consumer
  reporting), so verbal intake is out. Instead: systematically enumerate
  the Flexbox 1 behavior surface (axes establishment, line breaking,
  flex factors, min/max clamping, violation accumulation, alignment,
  §8.1 auto margins, item interactions — margins/padding/percentages/
  nesting/abs-pos children) and the grid vocabulary, map every cell to
  an existing proof, a registered boundary, or an UNCOVERED gap, and
  fill the gaps with table-driven proof families keyed by spec section
  (the README's table conventions make coverage enumerable in review —
  the enumeration matrix IS the thoroughness proof). The divergent +
  unimplemented rows ARE the complaint list — spec-shaped, with repros —
  ordered by product criticality against the named client signal.
  Grid's thoroughness is a complete gate enumeration (fail-closed,
  counted), which also sizes grid admission for the
  flex-first/grid-first/both-tracks decision empirically. This is
  evidence-gathering for PR 3's debate, not implementation.

- **2026-10-19 — the flex/grid spec-surface enumeration is COMPLETE;
  the complaint list exists.** Every cell of the 135-cell flex matrix
  and the 18-cell grid vocabulary is PROVED, registered, or gated (the
  condensed matrix lives in the layout_proofs README as the audit
  artifact). The complaint list for PR 3's debate is the new divergent
  set, ordered by product criticality against the client signal:
  anonymous inline runs not merged (§4 — hits every text-in-flex
  design), the auto-min/clamp families (inflexible items skipping
  hypothetical clamps; hypothetical size not floored by auto-min;
  container min/max bypassed; percentage margins/padding resolving
  against the item instead of the container — the literal "elements are
  not moving where they should"), abs-pos/`display:none` children
  occupying item slots (§4.1 — conditional-rendering patterns), mixed
  violation freeze ordering (§9.7), space-evenly negative-free-space
  fallback (§8.2/§8.4), paint ignoring order-modified sequence
  (§5.4.1), and the two auto-min graceful-gate rows (§4.5). Two
  suspected divergences correctly landed ON-SPEC (cross-axis min/max;
  basis `content`) — the empirical discipline's false-positive filter
  working. Grid: 14 property-name gates plus the `display: grid` value
  gate make the admission scope enumerable. **Three authored-input
  panics were found and fixed in this round** (non-ASCII importance
  marker, nested `inline-flex` blockification per CSS Display 3 §2.7,
  and the two auto-min asserts converted to measured-size graceful
  fallbacks): the assert-shaped fail-fast gate is retired as a pattern
  for boundary data — gates on authored input are graceful, registered,
  and pinned. PR 3's mapping-document debate now has its evidence base:
  the target list is these rows.

- **2026-10-19 — the composition axis (Run C): deep hierarchies and
  stacked features.** The spec-surface matrix proved branches on FLAT
  trees; Run C enumerated the composition space (content kind × stacked
  feature × host shape, 33 row-scenarios, no cell skipped) and found
  three more divergences: `align-items: stretch` distorts replaced
  images (two rows — the most visible one for real card/media designs),
  `white-space: nowrap` text floors its auto-min at the widest word
  instead of the full run (§4.5), and first-child margins are lost at
  the flex-item subtree boundary (§8.3.1 in composition). The negative
  results matter as much for the debate: text-align composition is
  clean (alignment uses RESOLVED widths), deep mixed hierarchies
  measure correctly through two flex levels with no panics, subtree
  percentages resolve against the item's content box (the item-level
  wrong-basis divergence does not propagate), and break-spaces needed
  only a citation extension. The complaint list now has two axes —
  spec-surface and composition — both exhausted.

- **2026-10-19 — third-party review of the graceful-gate PR (accepted,
  two observations incorporated) and the layout-pass architecture
  doctrine (settled, binding PR 3 and PR 5).** The review verified the
  blockification scoping, the fallback arithmetic (measured border-box
  recovered exactly), and the grow-check relocation. Incorporated:
  (a) blockification is PARTIAL — CSS Display 3 §2.7 blockifies ALL
  inline-level outer displays for flex items (inline → block,
  inline-block → block, inline-flex → flex); `display: inline` items
  still measure via the inline shrink-wrap path. A verification pin is
  owed (first link of the corpus chain); registration follows the
  empirical result. (b) the nested graceful over-estimate threads the
  TOP-LEVEL item's measured size (one unsupported grandchild inflates
  the whole item's auto-min) — already documented in the registry rows;
  confirmed as a named PR-3 input (the mapping document's auto-min
  design replaces the approximation, see below). **The layout-pass
  doctrine (settled):** (1) a reflow MUST NOT allocate — node-inline
  fixed-size measurement records (S1: Copy, value-compared), transient
  solver state in context-owned reused scratch at high-water-mark, no
  per-element or per-container allocation in steady state (the
  perf_allocation window is the judge); (2) layout is ONE bottom-up
  intrinsic fold and ONE top-down resolution/placement fold per update,
  gated by dirty bits — the recursive auto-min walk is subsumed by the
  up fold and dies; (3) neighbor access only: nodes read
  child/sibling/parent records through arena links and the visitor's
  carried context; no free recursion outside the two folds; (4) the
  visitor carries downward distribution state (containing-block sizes,
  accumulated offsets); (5) subtrees whose cross size depends on the
  resolved main size are flagged in the up fold and re-folded once, in
  the down fold, at the resolved width (Gecko's
  `CanMainSizeInfluenceCrossSize` semantics) — the engines' repeated
  ad-hoc reflows become measure-once plus flagged-refold. Task hygiene
  for the rest of the arc: implementation/research tasks are narrow,
  chained, and corpus-building; data structures and algorithms are
  decided   by the architect, never delegated.

- **2026-10-19 — the mapping document is written; the debate may
  convene.** `FLEX_MAPPING.md` lands the debate's subject: the
  field/phase mapping (fetched Blink + Gecko sources), the two-axis
  complaint list with fix-site assignments, the two adjudications
  (space-evenly → engine 0-clamp; subtree-margin split reclassification,
  landed `b3e9aec`), the target architecture (two-fold,
  zero-allocation, scratch lanes, anonymous runs as virtual items, the
  auto-min walk subsumed), the 10-step fix sequence, and the five
  debate questions answered with evidence. Headline positions the
  debate must attack: the kill criterion does NOT fire (skeleton close,
  deltas real); map-first vs admit-first DISSOLVES (every Tier-1 fix
  site is a structural delta — the structure work and the behavior fix
  are the same landing); "done" = §4 steps 1–9 plus debate-scoped
  admissions (auto margins §8.1 first). Also landed under the review
  incorporation: `7946aa7` (inline blockification pin — cross-axis
  divergence registered; the review's predicted main-size divergence
  correctly did not manifest). **Amended the same day by the
  adversarial-review entry below** — the "DISSOLVES" headline is
  re-scoped to the per-row classification and the kill criterion is
  restated; the entry is kept verbatim as the record of what the review
  attacked.

- **2026-10-19 — the THIRD-PARTY adversarial review of PR 3 and its folded
  amendments (seat: the reviewer, not the architect).**
  A full adversarial review of PR 3 against PLAN.md and FLEX_MAPPING.md
  (counts re-verified against the tree: `flex.rs` 5,356 lines, 135 flex
  proofs, 59 solver unit tests; `root_dom.rs` 7,340 lines, 48 public
  methods; `dom_node.rs` 2,425) found the DESTINATION defensible and the
  PROCESS not: a kill criterion that could not fire (the doctrine settled
  the same day already mandates the restructure; "closer than feared" is
  unmeasurable), a debate whose questions were pre-answered by its own
  subject, a scope its title did not describe (the doctrine makes this a
  layout-pass re-architecture, not a flex-solver migration), and a
  flagship record that baked the resolution-time layering error it
  existed to kill (write-time `margin_sums`/`hypothetical_main` for
  percentage-admitting quantities whose basis is down-fold). Folded
  amendments, rationale in place at each site: (1) **PLAN.md PR 3
  rewritten** — honest scope and title; kill criterion restated
  operationally against the doctrine (dies on measured reversal of the
  doctrine, or on third-party refutation of the mapping's §2.1 skeleton
  claims — falsifiable in both directions); grid added as REQUIRED
  debate-output question 6; proof obligations added (they were missing;
  PR 1's standard applied); stale counts corrected throughout. (2)
  **FLEX_MAPPING amended** — the per-row restructure/in-place
  classification (four Tier-1 rows strictly require the restructure; the
  classification's product is SEQUENCING: pull-forward landings on
  client criticality, accepting double work); the record respec'd to
  basis-independent up-fold quantities only — which also SHRANK it to
  four intrinsic sizes plus flags (~20 bytes/node), because authored
  style is read from the S1 computed groups at down-fold time rather
  than duplicated into the record; lanes made update-scoped (the
  paint-order fix's lifecycle demands it — per-container reset could not
  serve a later paint pass) with `u32` indices (the `u16` had no
  overflow story) and the insertion bound named with an evidence-gated
  merge fallback; run-aggregation, the collapsibility-conditioned
  whitespace drop, and the 0×0 order-modified static-position model
  specified (Tier-1 row #1 previously rested on one sentence); the
  refold budget with a designed within-update memo fallback (text in
  shrink contexts flags everywhere — the most common content meets the
  most expensive measurement); step 1 split per the structural-migration
  policy (the algorithm change with zero pin flips, semantic landings
  after — the bundling ban cuts both ways); the verification burden
  restated (per-phase behavioral pins substitute for the structural
  isomorphism the style redesign enjoyed — the remedy's premise shifted
  and is now owned); the third party's obligations enumerated (agreement
  by name, per obligation — a general assent is not agreement). (3) The
  review's credit list stands and is recorded so the amendments are not
  read as a repudiation: the spec-surface methodology and its
  false-positive filter, the pre-committed tie-break rule with exercised
  adjudications, §7's existence and its best attack point (the refold
  hit rate — now the budget), the kill ASSESSMENT's substance (skeleton
  close, deltas real — matches the evidence), and the two-fold
  destination itself.

- **2026-10-19 — the architect's countersign on the PR 3 review
  amendments.** The amendments are accepted in full; each was re-derived
  against the tree and the corpus, and the factual claims re-verified
  (59 solver unit tests: 17 `measured_item_record_*` + 23
  `resolve_active_flex_container_*` + 19 others; `line_collection_items`
  present at `flex.rs:177/388/1639`; 135 flex proofs). Named errors of
  record in the pre-review draft: the basis-layering violation (the
  flagship record stored down-fold quantities — the same
  resolution-time error as `off_spec_percentage_resolution_against_flex_container`);
  the per-container lane lifecycle (could not serve paint); the
  unconditioned whitespace drop; the one-line abs-pos model; the
  bundling-ban violation in step 1 (the policy's author, hoisted); the
  void kill criterion (failed the rule PR 2 was killed under); the
  "DISSOLVES" overstatement. One probe carried into the obligations:
  the paint-order NO classification assumes the order-modified sequence
  is consumable by paint TODAY — `line_collection_items` exists but its
  retention past relayout is unverified; obligation (c)'s classification
  audit should answer it specifically. The debate package stands as
  amended; the next act is the third party executing obligations
  (a)–(f) BY NAME.

- **2026-10-19 — review round 2 adjudicated: evidence, budget, fire
  path, seats, and the work-order repairs.** Round 2 (the third-party
  reviewer, same chain) found the response real but structurally flawed
  — an ephemeral evidence base, a bootstrap deadlock, an unscoped fire
  path, unnamed seats, and an orphaned brief. All findings accepted and
  fixed: (1) **ephemeral evidence base (critical)** — the fetched
  engine sources lived in OS temp, unmanifested and unreproducible by
  construction. The corpus now lives at `.tmp/engine-corpus/`
  (GITIGNORED — the product owner's policy, per the round-3 correction:
  licensed third-party source stays out of this repo on hygiene, scope,
  and reproducibility grounds, not by legal bar — both upstream
  licenses permit vendoring under conditions this repo declines) with
  `MANIFEST.md`
  carrying per-file upstream URLs, resolved revisions (Chromium
  `61ef1bef`, mozilla-central `0fdb388b`), fetch date, and
  revision-pinned re-fetch instructions; 15 files (13 Blink, 2 Gecko;
  the listing proved `flex_layout_utils.cc/.h` and `flex_item.cc` do
  NOT exist upstream — PR 3's citations corrected). (2) **obligation
  (d) bootstrap deadlock (critical)** — the scenario could not exist
  before step 1, which required the ratified number. Split: (d1) policy
  ratified NOW (metric definitions, the ≤2× construction bound, memo
  doctrine compliance, the ceiling as policy, the authored-input
  scenario definition: 240px wrapping row, 12 text items + 4 nested
  flex items with text, one text mutation per update); (d2) baseline
  observed at step 1's gate, breach lands the memo. (3) **fire path
  unscoped (high)** — rules written: per-claim consequence (a refuted
  claim N re-maps the phases resting on N; the criterion fires only
  when the refuted set defeats the doctrine's LOCAL, engine-independent
  recursion/allocation evidence — no skeleton refutation alone fires
  branch (a)); branch (a) firing amends the doctrine entry and
  re-scopes PR 5 in the same decision; verdict (a) gains
  CONFIRMED-WITH-DELTA. (4) **seats (high)** — the third-party reviewer
  named by role (the 2026-10-19 review chain's author; independence
  criteria in the brief: not the architect, not the implementation
  agents, demonstrably able to read engine sources, holds
  kill-criterion authority); the review entry above retitled so author
  and reviewer seats are distinguishable. (5) **obligation (f) +
  output 5 (medium)** — substitute-surface coverage (no confirmed
  skeleton phase unpinned) and the step-10 admission scope (default:
  §8.1 auto margins in-arc first; flex-basis %, intrinsic keywords,
  aspect-ratio deferred to a post-sequence consumer re-check; silence
  adopts the default). (6) **orphaned brief (medium)** — linked from
  PR 3's header and FLEX_MAPPING.md's header. (7) **sequencing note
  (minor)** — (b), (c), (e) locally executable today; (a), (f) need
  the corpus.

- **2026-10-19 — review round 3: the round-2 fixes verified firsthand;
  residual items folded; the debate may execute.** The reviewer ran the
  gate on the adjudication claims rather than trusting the table: all
  seven findings confirmed FIXED against the tree and the corpus
  (`.tmp/engine-corpus/` with MANIFEST-pinned revisions; the (d1)/(d2)
  split with the authored-input scenario; the fire-path rules including
  the local-evidence clause; the retitled seats; obligation (f); output
  5; the brief linked from both headers; the sequencing note). The
  writer's provenance work is credited with catching a citation error
  both prior rounds missed — `flex_layout_utils.cc` and `flex_item.cc`
  never existed upstream; the original `.b64` fetch artifacts were
  error payloads. Residual items folded in this change: (1) **the
  assignee-conflict rule (substantive)** — the named reviewer is
  independent of the author but not of the amendments: obligations (c),
  (e), (f) audit reviewer-authored material (the §1 classification, the
  restated kill criterion, the §6 verification-burden plan). Rule, in
  the brief: those three require EITHER product-owner co-signature per
  verdict OR a fresh seat with no review-chain context — the owner
  names which before they execute, recorded here; (a), (b), (d) are
  conflict-free. (2) output 5's deferral closed (baseline alignment,
  `visibility: collapse` explicitly deferred to the same re-check).
  (3) the two stale `(a)–(e)` references corrected.   (4) the license
  rationale corrected in the manifest, this log, and the brief — the
  gitignored-corpus decision stands on repo policy (hygiene, scope,
  reproducibility), not a misstated legal bar. State: the debate
  package is executable; obligations (a), (b), (d1) may start today;
  (c), (e), (f) await the owner's named execution rule.

- **2026-10-19 — the debate executes and CLOSES: obligations (a)–(f)
  verdicted, outputs recorded, PR 3 implementation UNLOCKED.** Execution
  rule (named by the product owner per the round-3 requirement): no
  fresh seat exists; the architect executes; the owner co-signs by
  blanket approval (2026-10-19) — any verdict reopenable by naming it;
  (d1) is the owner's ratification. **Verdicts.** (a) all five §2.1
  skeleton claims CONFIRMED: claim 1 (Blink `layout_block.cc:147-171` +
  `flex_child_iterator.cc:18-24` — whose `needs_sort` lazy sort
  independently corroborates the order lane's adaptive insertion; Gecko
  `CreateNeededAnonFlexOrGridItems:8137` with the whitespace drop at
  :8156-8182, read firsthand); claim 2 (`flex_item.h:47-48`, firsthand);
  claim 3 (`line_flexer.cc` in full: ctor freeze at hypothetical
  :26-43, fractional factor-sum clamp :99-104, reverse running-share
  with its rounding rationale :117-139, one-class freeze by signed
  total :165-178); claim 4 (Gecko `CanMainSizeInfluenceCrossSize:2546`,
  used at :5412, firsthand; Blink cross-post-flex per the round-2
  sampling); claim 5 (`ClampNegativeToZero:1678/1684`, firsthand;
  Gecko safe-collapse per round 2). (b) NO counterexample found against
  the run-aggregation semantics or the refold bound; two findings
  folded: (i) cursor (ii)'s cross-boundary fragment joining needs a
  fragment-level text-intrinsic API the current whole-node
  `TextIntrinsicMetrics` cannot expose — surface note owed at step 3a,
  folded into FLEX_MAPPING §3.2; (ii) the ≤2× refold bound covers only
  single-level nesting — the honest bound is (flagged_depth + 1)×,
  folded into §3.4 and the brief's (d1). (c) every §1 NO/PARTIAL row
  holds EXCEPT paint-order, reclassified NO → PARTIAL: the probe found
  the solve is consumed at exit (`relayout_active_flex_node` →
  `exit_active_flex_container`, `flex.rs:270-335`), so the
  order-modified sequence dies with it and paint cannot consume it
  today; retention is the structural half, and an in-place retention
  would add exactly the per-container allocation the doctrine forbids
  (lands with step 2's lanes; pull-forward priced at a retained `Vec`
  per flex container). (d1) RATIFIED by the owner with the sharpened
  bound: metric definitions, the (flagged_depth + 1)× construction
  bound, the memo fallback's doctrine compliance, the ≤50%
  refold-fraction ceiling on the pinned scenario, and the scenario
  definition (240px wrapping row, 12 text items + 4 nested text-bearing
  flex items, one text mutation per update). (e) the kill criterion
  does NOT fire: branch (a) has no reversing measurement (the doctrine
  evidence is local — the recursion/allocation inventory, verified
  firsthand in round 2); branch (b) has no refuted claim (all five
  CONFIRMED); the per-claim consequence list is empty. (f) holds with
  the explicit phase-family enumeration: item-formation, order,
  measurement/hypothetical, line-breaking, container-sizing, §9.7
  resolution, cross-sizing, line-packing, positioning, abs-pos static,
  intrinsic contribution, paint-order; gated planes named (baseline,
  collapse struts, fragmentation, balanced wrap) — no confirmed phase
  unpinned. **Outputs.** 2. sequencing pulls: NONE (the paint-order
  reclassification does not change the default — it lands with step 2's
  lanes). 3. (d1) ratified as above; (d2) baseline recorded at step 1's
  gate. 4. grid ADOPTED per the recorded recommendation: enters the arc
  immediately after the fix sequence, before PR 4 (owner veto open).
  5. step-10 default adopted: §8.1 auto margins in-arc first;
  flex-basis %, intrinsic keywords, aspect-ratio, baseline alignment,
  `visibility: collapse` deferred to the post-sequence consumer
  re-check. Implementation unlocks at FLEX_MAPPING §4 step 1 (the
  structural step: zero pin flips, full suite green, perf pins as
  judge). Next artifact: the step-1 execution plan (the architect's
  chained task decomposition).

- **2026-10-19 — PR 3 step 1 (the structural step) LANDED; the (d2)
  baseline recorded.** The chained execution (FLEX_STEP1_EXECUTION.md)
  completed: T1 `a653abb` (the record + node integration;
  `measurement_flags.rs` split per structural rule 1), T2 `0c1f640`
  (block-world record writes at exit, per-kind identities), T3
  `0f3dedd` (the lanes + RootDom-owned scratch, reset at
  `RootDom::update`), T4a `bd9afb5` (the flex measurement fold + item
  view; the record-write gate ruled read-first with the re-pin
  protocol), T4b `4eefaef` (the lane-based solve: four lanes — item,
  order, line, resolution; `Vec<MeasuredFlexItem>`/`Vec<FlexLine>`/
  order-sort `Vec`/`HashMap` re-join all deleted; nested-container lane
  interleaving solved by the range-start stack + transient truncation;
  placement DOM-order preserved via the item lane), T5 `beda916` (the
  66-test migration split: 26 re-home, 40 re-express, 0 deaths — shape
  pins died at compile time in the cutover), T6 `72002d4` +
  T6b `db4e790`/`3702b81` (counters, the (d1) scenario pin, two
  mechanical allocation kills, the amended allocation contract).
  **The (d2) baseline:** the ratified scenario (240px wrapping row, 12
  bare text items + 4 nested text-bearing flex items — interpretation
  ruled bare-text, one text mutation per update) measures
  `FlexNodesMeasured` = 24 and `FlexNodesPlaced` = 20 per update
  (placement relayouts EVERY child = the double-layout ≡ 100% refold
  analog; the ≤50% refold-fraction ceiling binds step 7's mechanism,
  not step 1). Allocation: the reflow machinery is zero after warmup
  (lanes at high-water mark, no `Vec`/`String`/`collect` in the flex
  solve — the two mechanical sources killed in T6b); the text
  layout-phase window carries 1934 allocations / ~267 KB of inherited
  cosmic-text shaping (the per-pass re-measure cadence; the
  change-detection-gated fold at step 4+ is the designed answer);
  the text-free window is 7/7/2 (clean-update base + the winners
  fold's proposal-J traffic + a 5-alloc/1-realloc flex-path residual,
  unattributed beyond "outside flex.rs", best-read paint-damage
  bookkeeping — owed a scoped attribution task). Perf-pin motion, all
  with documented causes: node-store Get 139→141 (fold record
  read-compare) then 141→138 (placement no longer DOM-walks), GetMut
  27→29 (fold record writes when changed). The 135 flex proofs passed
  unchanged at every link. Step 1's zero-pin-flip contract is met
  (only sanctioned perf re-pins moved), and the full gate is green.

- **2026-10-19 — flex arc step 2 LANDED (`6da221d`, PR #21's
  follow-up): order-modified paint.** Paint now stages flex children in
  order-modified document order (CSS Flexbox §5.4.1), flipping the
  `off_spec_flex_paint_ignores_order_modified_sequence` row on-spec —
  the first complaint-list row closed by the sequence. The mechanism
  was amended at dispatch (recorded in FLEX_MAPPING §4): paint DERIVES
  the sequence from computed `order` values (fast path DOM order when
  orders are uniform; selection iteration only when they differ) rather
  than consuming the update-scoped order lane, because a
  paint-dirty-only update has empty lanes — the paint-only-update pin
  proves the divergence cannot silently return on that path. One
  documented re-pin (node-store Get 138→140: the order-detection reads).
  Also landed since step 1's PR: `26a8ee7` (the review follow-up
  unifying the record write gate and assembly — zero pin movement).
  Proofs 424 → 426; divergent registry 23-in-20 → 22-in-18. Next: step
  3 (item-view formation — anonymous runs, abs-pos/none exclusion +
  static-position model, blockification completion), the Tier-1 rows.

- **2026-10-19 — flex arc step 3a LANDED (`1ac6d39`, `7dc007b`, plus
  follow-ups `d5456fd` and `1269113`): anonymous inline-run flex
  items.** The complaint list's Tier-1 row #1 closed: contiguous inline
  runs form ONE virtual item (fold-local `RunAccumulator` + the
  source-range variant — no DOM mutation, per the doctrine), with the
  three-cursor min-content fold (widest unbreakable segment, with
  cross-boundary fragment joining per CSS Text 3 §4), the
  collapsibility-conditioned whitespace drop (Gecko's semantics), and
  run-aware paint sequencing. This was the first landing under the
  corrected task discipline: the complete design was written FIRST
  (`FLEX_STEP3A_DESIGN.md`, every decision made, no open questions) and
  the implementation task was transcription-only — and the discipline
  paid immediately: the transcription disclosed three ambiguities
  instead of improvising, and two of its escalations caught two of the
  architect's own design errors: the `flex_shrink: 0` field (corrected
  to the CSS initial 1, `d5456fd`) and the never-live wrapping path
  (runs now wrap at their resolved width, `1269113`). The (d1)
  scenario's shape changed with merging (bare-text children now form
  runs): `FlexNodesPlaced` 20 → 9, the text layout-phase allocation
  window 1934 → 1014 (fewer lane entries) — all re-pinned with
  documented causes. Divergent registry 22-in-18 → 21-in-17 (the
  anonymous-runs row moved on-spec). The D1 atom-with-block-descendants
  edge and the span-boundary fragment approximation are named
  boundaries in the design doc. Next: step 3b (abs-pos/`display:none`
  exclusion + the static-position model) and step 3c (blockification
  completion).

- **2026-10-19 — flex arc step 3b LANDED (`7d1c067`): abs-pos and
  `display:none` children are not flex items.** Exclusion at formation
  (no slots, no gap adjacencies, no justify count — both engines' skip
  semantics), inline runs join across excluded children, and the 0×0
  order-modified static-position model (§4.1, Blink's `AxisEdge`
  semantics): abs children now lay out per the real abs-pos rules at
  their computed static position inside the container's padding box and
  PAINT (previously zero-squashed at the origin). The registered row
  moved on-spec with three new pins (run joins across, center static,
  end-slot static). One documented perf re-pin (Get 142→145). Named
  boundary (the transcription's observation, recorded): explicit
  `left`/`top` non-auto offsets on an abs flex child take precedence
  over the static position per CSS 2.1 §10.3.7 — currently unpinned;
  owed a row when the offset family is next touched. Divergent
  registry 21-in-17 → 20-in-16. Next: step 3c — re-adjudicated under
  the run model: the lone-`display: inline` case is a length-1
  anonymous run per both engines (the span is NOT blockified; the
  anonymous wrapper stretches and the inline content renders at its
  inline height inside it), so 3c is the blockification row's
  re-adjudication plus the residual inline-block verification, not a
  naive `inline → block` map.

- **2026-10-19 — flex arc step 3c LANDED (`3ffab7b`): the
  blockification row re-adjudicated under the run model.** The
  verification-first protocol (D1) decided the landing: a lone
  `display: inline` child DOES form a length-1 run (D1.1 ✓) and
  `inline-block` still block-measures (D1.3 ✓), but the stretch path
  did NOT reach run items (D1.2 ✗) — so the conditional fix landed:
  run items now resolve their cross to the line cross under stretch
  while their inline content renders at its own inline height at
  cross-start (the engine model — anonymous wrapper stretches, content
  does not). The T1 divergence closes; `blockified_flex_item_style`
  stays `InlineFlex → Flex` (inline elements are run members, never
  items — the naive map was correctly avoided). Divergent registry
  20-in-16 → 19-in-15. **Step 3 complete: all three item-formation
  landings plus two follow-ups are in** (3a `1ac6d39`/`7dc007b`/
  `d5456fd`/`1269113`, 3b `7d1c067`, 3c `3ffab7b`). Next: step 4 —
  auto-min by fold (the hypothetical floor folded into the record;
  nowrap mode-aware min-content; both graceful gates die), designed
  first per the task discipline.


## Vocabulary carried from the redesign (self-contained definitions)

One line per term this document reuses from the landed plans, so nothing
requires git archaeology (AGENTS.md's scoped-exception note points readers
to this section). Terms marked † are defined in this document's own
problem statement (P1–P5) and are not repeated.

- **A1** — the declaration parser (`decl_parse.rs`): closed per-property
  grammars, drop-and-continue block parsing, the system's single
  shorthand-expansion point.
- **A3** — selector parse + specificity (the closed subset in
  `domains/style/mod.rs`).
- **A5** — defaulting/inheritance: inherited groups seed from the parent,
  non-inherited from initial (group-copy inheritance).
- **A6** — unit resolution: `font-size` em → parent's computed size; every
  other property's em → the element's own.
- **A7** — group-diff dirty classification: the static table mapping a
  changed computed group to layout/paint/remeasure/layer-membership marks.
- **A10** — the minimal-edit `DeclBlock` upsert (update-in-place keeping
  source position; changed flag).
- **D2** — one timing: programmatic writes mark style-dirty; the rendering
  update's style pass resolves everything (no second clock).
- **D5** — the v0.6.0 hard-break migration contract (see `MIGRATION.md`).
- **D6** — the admission policy: a property or grammar form enters only
  with a named consumer need, a spec citation plus the Blink/Gecko
  reference, and a proof in the same change.
- **D7** — damage rects are preserved; targeted repaint is a staged goal.
- **D8** — preserve all functionality; where behavior is reinterpreted,
  bias toward Blink/Gecko semantics.
- **F (cascade tier path)** — staged tier admissions: stage 1 `!important`,
  stage 2 a real UA tier, stage 3 the animation tier, stage 4 cascade
  layers; the `Precedence` tier field is shaped so these are additive.
- **G** — the drop-and-continue declaration-parsing rule (CSS 2.1 §4.1.7,
  css-style-attr §3): malformed declarations drop, parsing continues, and
  every drop is counted by metric AND returned in an explicit report —
  never silent. Scoped to declaration lists; single-declaration APIs stay
  strict (the AGENTS.md scoped exception).
- **J (proposal J)** — matched-rule caching / selector-aware invalidation,
  deferred on evidence: style passes visit 1–3 elements in the pinned
  scenarios, so scoped dirtying already carries the load. Its named target
  is the collect phase's two per-element allocations
  (`winners_fold.rs:140-146`); it wakes only if the counters demand it.
- **S1** — the landed substrate: declarations in arenas with generational
  keys; computed style as seven small `Copy` groups stored inline on the
  node (visit locality, value-compared, no refcounting); typed `DeclValue`
  at the declaration boundary; bounded inline lists (capacity 4) for
  transition longhands. "The S1 invariants" = those group properties.
- **B1–B12** — the coverage accounting's twelve bundles (landed; proof
  files in `tests/layout_proofs/`): B1 sign rules & numeric edges and B2
  units (`style/unsupported_values.rs`), B3 box shorthands and B10 other
  shorthands (`style/unsupported_shorthands.rs`), B4 text consumption &
  admitted quirks and B5 preserved candidates (`style/admitted_gaps.rs`),
  B6 layout-impact and B11 property tail (`style/unsupported_properties.rs`),
  B7 colors (`style/unsupported_colors.rs`), B8 selectors/at-rules
  (`style/unsupported_selectors.rs`), B9 cascade tiers
  (`style/cascade_tiers.rs`), B12 robustness (`robustness.rs`).
- **The redesign / the spine** — the landed declaration-store style system
  (v0.6.0; its plan lives in git history at the v0.6.0 merge), whose
  implementation sequence was called the spine.
- **P1–P5 †** — this document's own problem statements (§1); note they are
  unrelated to the old plan's P1–P3 (programmatic-style loss, two clocks,
  one-bag attributes — all fixed and closed).

