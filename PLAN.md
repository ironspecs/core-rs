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
  `src/domains/layout/flex.rs`, 4,986 lines) was intuited rather than
  derived from Blink/Gecko structures. It works for its admitted batches
  (3–7, ~109 proofs in `layout/flex_layout.rs` plus ~50 unit tests), but
  its data structures do not map to the engines' flex lines/items, so
  correctness arguments are hard, coverage expansion is slow, and user
  complaints about flex behavior have no shared vocabulary to resolve
  against. The style-system redesign proved the remedy: adopt the engines'
  data structures with a source-verified mapping, not a rewrite.
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
- **P5 — `RootDom` is a 7,023-line god object and it is still growing.**
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
`FlexContainerStyle`. Coverage: batches 3–7 pinned (~109 proofs in
`layout/flex_layout.rs`, ~50 solver unit tests including the
`measured_item_record_*` family). One explicit support gate:
`flex_container_content_based_auto_min_is_supported` (`flex.rs:1402` —
content-based auto-min only for horizontal, non-reversed, no-wrap
containers). The unadmitted remainder is gated by the closed grammar and
this one gate; there is no placeholder fallback (the old one died in the
typed-group migration). The file is 4,986 lines and its data structures do
not map to Blink's `FlexLine`/`FlexItem` or Gecko's equivalents.

### 2.5 `RootDom` and `RootDomContext`

`root_dom.rs` (7,023 lines, 48 public methods) cohabitates: tree mutation,
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

367 layout proofs in subsystem directories (360 in
`style/`/`layout/`/`text/`/`widgets/`/`pipeline/` plus 7 in the
cross-cutting `robustness.rs`); the off-spec registry (8
divergent, 4 deliberate, grouped `unsupported_*` gates, a robustness
family) as a living divergence map with reclassification discipline;
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

1. The god objects: `root_dom.rs` (7,023) and `dom_node.rs` (2,358);
   `flex.rs` (4,986) close behind.
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

**LANDED (2026-10-18, branch `pr1-margin-style-gaps`, 10 commits):** all
seven items plus the transition proof-family companion bundle. Item 1
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
from==to, overlay-beats-important). The off_spec census stands at 5
(font-family unconsumed, font-weight/style unconsumed, `break-spaces`,
range pill, `line-height: normal` → 1.0×).

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
   a negative-margin sibling is clipped at its clipped layer's bounds
   (deliberate), and the Normal-layer escape hatch paints it.
3. **`box-sizing`** (CSS UI 4 §3.1). Add the property (box group):
   `BoxSizingKw { ContentBox, BorderBox }` on `ComputedBoxGroup` (initial
   `ContentBox` — the crate's single model today, which is also the CSS
   initial). One branch in the sizing projection (`domains/layout/style.rs`):
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

### PR 2 — style snapshot sharing `DEBATE — no code until resolved with third-party agreement`

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

### PR 3 — the flex solver: source-verified migration `DEBATE — mapping doc and agreement required before implementation`

**Problem.** The solver works for batches 3–7 and is the users' biggest
complaint anyway. The complaint decomposes into: coverage gaps (the
unadmitted subset), and confidence (no shared vocabulary with the engines
when behavior is questioned). The file is 4,986 lines of bespoke
structures.

**The first deliverable (before any code): the mapping document.** Blink's
`flex_layout_algorithm.cc` + `flex_layout_utils.cc` and Gecko's flex
container/frame code, mapped field-by-field to `MeasuredFlexItem`,
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
This document is the debate's subject.

**The migration (only after the debate):** behavior-preserving
data-structure alignment, gated by the entire flex proof suite (the
batch families, the solver unit tests, perf pins). Structural-migration
policy: moves are byte-identical where behavior must not move; algorithm
changes are their own commits, reviewed as performance changes.
**Scope honesty (stated before the debate, not inside it):** a
behavior-preserving migration moves zero coverage by construction — the
proofs pin only admitted behavior (batches 3–7). What the migration buys
is the shared vocabulary and the review surface that makes the missing
batches *admissible and reviewable*; the admissions that fix the named
bugs are subsequent, behavior-changing landings on the aligned structures,
and they are the actual point. The migration without them is aesthetics;
the debate must not let it become that.

**The debate must answer.**
1. What is the actual complaint list? (The users' top flex bugs, named —
   the subsequent admissions target them, not aesthetics.)
2. Map-first vs admit-first: is aligning structures to Blink/Gecko
   justified *before* admitting the missing batches, or should the missing
   batches be admitted into the current solver first (cheaper, keeps the
   debt)? What evidence decides?
3. What does "done" mean: full structural alignment, or the named subset
   that resolves the complaint list?
4. What does the migration cost the solver's ~50 unit tests, and which of
   them are testing bespoke internals that should die with the bespoke
   structures (per the test-migration policy: genuine unit assertions
   re-home; cross-module assertions re-express through the public surface)?
5. **Kill criterion (added per the S10 review):** this PR dies if the
   mapping document concludes the current solver is already closer to the
   engines than feared — in which case the arc admits the named missing
   batches into the current solver and stops.

**What the reviewer should attack:** whether this is a rewrite in mapping
clothes; whether the complaint list is real or assumed; whether the
current solver's proofs actually pin the behavior the migration claims to
preserve; and the sequencing (why third, not first, if it's the biggest
user complaint — answer recorded in the decision log, open to challenge).

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
documents, like today's `TextLayoutConfig`) — this is a joint boundary
with PR 2, whose snapshot explicitly excludes them from snapshot
lifetimes (fonts are not style data); making the seam document-scoped
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
  a better home is named.

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

