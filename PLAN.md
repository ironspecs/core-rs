# PLAN.md — the five-PR arc (2026-10-18)

Fresh plan document for the next arc of work, following the landed
declaration-store style system (v0.6.0) and the completed coverage
accounting (364 layout proofs, divergent registry at 8). This document is
the self-contained record: the arc, the scope and design sketches per PR,
the debate and proof gates, and the decision log.

## Standing state (what every reader needs)

- The style system is typed and browser-shaped: declarations in arenas
  (`DeclStore`/`DeclBlock`), seven `Copy` computed groups on the node,
  winners fold (behavior sheet → matched rules → inline block),
  group-diff dirty classification (A7), pass-scheduled transitions.
- The admission policy (**D6**, `MIGRATION.md`): a property or grammar form
  enters only with (1) a named consumer need, (2) a spec citation plus the
  Blink/Gecko algorithm reference, (3) a proof in the same change.
  **Every admission flips its registry pin on-spec in the same change.**
- The proof conventions: table-driven proofs, registry classes
  (divergent/deliberate/unimplemented/on-spec/robustness), no red commits,
  behavior change and its proof move together, full gate per landing
  (fmt, clippy, workspace tests, `cargo xtask lint`, perf suites when
  production code moves).
- Process for this arc: each PR is its own branch, its own review, its own
  gates. Two PRs (2 and 3) require a written debate and third-party
  agreement **before implementation begins** (see their sections).

## Recorded decision: negative margins and layer spillover (2026-10-18)

**Decision: layers clip spillover; spillover is a layering decision, never
a surface-expansion decision.**

Content pulled outside a clipped layer's border box — by negative margins,
`overflow: visible` children, or transforms — is **clipped**, where a
browser would paint it. Rationale, recorded with the decision:

1. **The render cache is the product.** Layer surfaces with bounded
   geometry are what make scroll, animation, and targeted render fast in
   this engine. Expanding a surface because a sibling sticks out of it
   defeats the cache geometry and the damage-rect model (the bounded
   8-rect buckets and the whole targeted-repaint design depend on bounded
   surfaces).
2. **Negative margins are rare**, and the legitimate use of visible
   spillover is a *design* decision the author can express structurally:
   content that must spill lives in an unclipped (Normal) layer or in its
   own layer. "More render layers that own boundaries" is the sanctioned
   mechanism — not surface growth.
3. **This is a deliberate visual divergence**, registered as such (class
   deliberate): browsers paint `overflow: visible` spillover across layer
   boundaries; we clip at the clipped layer's bounds. The escape hatch is
   named above and is pinned with it.
4. **Orthogonal to the collapse math.** The sibling-margin collapsing
   divergence (`flow.rs`'s clamp sums −10 and −5 to −15 instead of −10) is
   a plain spec violation with no render-model consequence; it is fixed in
   PR 1 regardless of this decision (it is cheap and the registry is more
   valuable when every entry is a stance, not an accident).

Verification owed inside PR 1: pin the actual spillover behavior as
deliberate (assert a negative-margin sibling is clipped at its clipped
layer's bounds; assert the Normal-layer escape hatch paints it), after
verifying what the layer model actually does today.

## PR 1 — margin and style gaps (the admission queue)

Six admissions plus one correctness fix, in this order. Each flips its
registry pin on landing. No debate required; the designs are settled
below and the pins are the witnesses.

1. **`margin` / `padding` shorthands** (CSS Box 3 §4.2–4.3; Servo/Blink
   shorthand expansion at parse). The cheapest item: add
   `Shorthand::Margin`/`Shorthand::Padding` variants and names to
   `property_shape` (`decl_parse.rs`), reusing the exact 1–4 corner-mapping
   of `expand_border_radius`. No new structures; the cascade and groups
   only ever see longhands. Pin flips: the `unsupported_margin_shorthand_drops`
   / `unsupported_padding_shorthand_drops` rows.
2. **The negative-margin collapse fix** (CSS 2.1 §8.3.1, final paragraph:
   adjoining negative margins collapse to the most negative).
   `collapsed_sibling_margin_overlap_px` in `src/domains/layout/flow.rs`
   currently clamps the overlap to ≥ 0 (summing instead). Remove the clamp
   for the all-negative case (mixed-sign case keeps today's behavior —
   verify against CSS 2.1 §8.3.1's three rules and state the covered subset
   in the comment). Pin flip: `off_spec_adjacent_negative_block_margins_...`
   relabels on-spec. Registry's oldest divergent entry closes.
3. **`box-sizing`** (CSS UI 4 §3.1). Add `box-sizing` to the property table
   (box group, `BoxSizingKw { ContentBox, BorderBox }` on `ComputedBoxGroup`
   — the crate's single model today is content-box, which stays the
   initial), and branch the sizing projection in `domains/layout/style.rs`
   (border-box subtracts padding+border from the specified size to get the
   content box; content-box is today's path verbatim). Engine reference:
   Blink/Gecko compute-time `box-sizing` on the same two fields. Pin flip:
   `unsupported_box_sizing_drops_and_border_box_sizing_holds`.
4. **`!important`** (CSS Cascade 5 §6.1, §6.4.1). F stage 1, already shaped:
   `DeclBlock` gains a parallel importance bit-vector (the Servo
   `declarations_importance: SmallBitVec` shape, noted in `decl_block.rs`'s
   header); declaration parse strips the `!important` suffix and sets the
   bit (it is part of the declaration, not the value); `Precedence` gains
   `important: bool` as its FIRST field (important author beats all normal
   declarations, including normal inline style; transitions still win above
   everything via the overlay). Pin flips:
   `unsupported_important_declarations_drop_but_the_rule_survives` and
   `unsupported_important_in_style_attribute_drops`.
5. **`opacity` clamping** (CSS Color 4 §12.2: clamp to [0,1] at computed
   time). Change the `Opacity` grammar to clamp instead of reject (admit
   any finite f32, clamp to 0..=1). Pin flip:
   `off_spec_opacity_out_of_range_rejected` — the registry's last
   reject-side divergent entry closes.
6. **`transform` transitions** (CSS Transitions 1; the typed `[f32; 2]`
   translate pair is the admission shape, recorded in the B4 pin and in
   MIGRATION.md). Admit a typed translate pair into the scheduler's
   `DeclValue` interpolation table (the transform group already computes
   `translate_px: [f32; 2]`; the scheduler interpolates the two channels
   like lengths; `none` steps). Pin flip:
   `off_spec_transform_transitions_do_not_schedule`.
7. **Percentages and `auto` margins** (CSS Values 4 §percentages; CSS Box
   §auto) — **the large item; deliberately last, and splittable into its
   own landing if it outgrows PR 1.** The computed/used split (CSS Cascade
   §4.5): box-group length fields become `BoxLength { Auto, Px(f32),
   Percent(f32) }` (still `Copy`, still value-compared — the S1 invariants
   hold); computed keeps the authored form; layout resolves `Percent`
   against the containing-block dimension it already owns (width and
   margin/padding resolve against the containing-block WIDTH even
   vertically; `auto` margins resolve by distributing free space — the
   centering case). `auto` for width/height maps to the existing
   `None`/initial (free). Engine references: Blink's `Length` percentage
   resolution inside layout; Gecko's percentage base at reflow. Pin flips:
   `unsupported_percentage_lengths_drop` (the length rows) and
   `unsupported_auto_lengths_drop`.

**PR 1 proof obligations:** every admission above lands with its pin flip
in the same change; the spillover verification pin (from the recorded
decision) lands with item 2; the full suite stays green per item (each is
its own commit); perf pins re-pin only with a documented cause.

## PR 2 — the better sharing mechanism (style snapshot sharing)

**Requires a written debate in this document and third-party agreement
before any implementation.** No code lands for PR 2 until that section is
resolved and externally reviewed.

**The problem.** `RootDomContext` shares one `Arc<RootDomContext>` holding
`Mutex<StyleRuntime>`. The style pass locks the mutex once per element per
pass (candidate lookup + block reads). GPU-bound resources (the font
catalog, the render flow) genuinely want single ownership; the concern is
only the *mechanism* of sharing, not the sharing itself. Today the
mechanism is a global mutex in the style path, with the revision counter
as its correctness seam.

**The proposal (starting position for the debate).** Snapshot-per-revision
sharing: `StyleRuntime` publishes an immutable style snapshot per revision
behind an `Arc` swap (arc-swap or equivalent); the style pass takes one
`Arc` at update start and matches against it lock-free for the entire pass;
`load_stylesheet`/`unload_stylesheet` take the mutex briefly to build and
publish the next snapshot. The per-element lock disappears from the hot
path; readers are wait-free; writers pay snapshot construction per load
(which is already O(rules)).

**Debate section to fill before implementation:** measured cost of the
current lock (is this a real win or a clean-up? — the counters decide:
`StylePassNodeVisited` and the matching allocation profile); snapshot
memory semantics (structural sharing vs deep copy per revision — Servo's
rule tree shares structurally; our arena already shares blocks);
multi-document expectations (do consumers actually share contexts today?);
GC of old snapshots (who holds them, for how long); alternatives (RwLock,
per-document runtimes, lock-free indexes); what the font catalog does and
does not share with the style runtime (it is already context-scoped and
must stay out of the snapshot's lifetime questions).

## PR 3 — the flex solver (source-verified migration)

**Requires the mapping document below (or a successor of it) and a written
debate plus third-party agreement on the mapping before any
implementation.** This is the users' biggest current complaint and the
largest item in the arc; it is sequenced third so the quick wins and the
sharing win land first, and so the debate has the registry's full weight
behind it.

**The mapping document (to produce first, as its own deliverable):**
Blink's `flex_layout_algorithm.cc` + `flex_layout_utils.cc` and Gecko's
flex container/frame code, mapped field-by-field to our
`MeasuredFlexItem`/`FlexLine`/`FlexContainerStyle` and the
`resolve_active_flex_container` phases. The engines' durable shapes:
flex lines as first-class objects with hypothetical main sizes; items with
min/max clamps resolved through a violation/balancing pass; free-space
distribution after constraint resolution. Ours approximate these
bespoke; the document names every delta and its justification-or-fix.

**Then the migration:** behavior-preserving data-structure alignment with
the proof suite as the gate (the `layout/flex_layout.rs` batch families,
the `measured_item_record_*`/`resolve_active_flex_container_*` unit tests,
the perf pins). Structural-migration policy applies: moves are
byte-identical where behavior must not move; algorithm changes are their
own commits, reviewed as performance changes.

## PR 4 — the text seam, proven open (second-to-last)

Formalize the three-stage text boundary as a sealed seam (the render-ops
seam's shape, for text):

- **Measure** — intrinsic sizing (min/max content, line height) feeding
  layout.
- **Shape** — line breaking, glyph runs, caret maps feeding paint and
  editing.
- **Rasterize/cache** — glyph masks and atlas feeding the GPU.

The seam owns the font catalog, the fallback policy table (family list →
catalog query → generic mapping → guaranteed-final face), and every cosmic
interaction; nothing outside the seam constructs a cosmic `Buffer`. The
style system hands the seam a resolved `TextStyle` (family list, size,
weight, style, language hint). **Font-family/weight/style consumption
lands here**, at `TextStyle` construction, unblocking the named step-5
follower.

**The proof gate (explicit):** the seam change is justified only if it
provably closes no doors. Before it merges, it must render — through the
public pipeline, with proofs:

- new fonts beyond the current four faces (new faces installed through the
  catalog, not hardcoded),
- alternative languages/scripts (beyond the CJK coverage already pinned),
- RTL text (bidirectional shaping and caret behavior),
- vertical writing (if the seam cannot express it honestly today, the
  proof documents that as a named, accepted boundary — not a silent one).

**Kill criterion:** if the new interface cannot express those honestly, we
do not make the change. The fallback is to keep today's seam and document
why.

## PR 5 — `RootDom` decomposition (last)

The narrow-newtype facade (advice accepted 2026-10-18):

```rust
pub struct RootDom {
    tree: RootDomTree,               // nodes, links, ids, fragments, hit-test state
    styling: RootDomStyling,         // inline decl store, computed texts, style runtime revision
    transitions: RootDomTransitions, // scheduler state, active nodes, clock
}
```

One file per subsystem with its own impl (structural rules 1/5 hold);
`RootDom`'s 48 public methods shrink to the consumer-called facade over
thin delegations; the rest become `pub(crate)` on the subsystems. Follow-on
ownership correction: the popup/options system leaves `RootDom` for the
widget layer (`control_widgets.rs`/`element_behaviors/`). Behavior-
preserving, proof-gated, structural-migration policy throughout; the 48
public methods are the compatibility contract — no consumer-visible change
without a named decision.

## Decision log (this arc)

- **2026-10-18 — the layer-spillover decision** (recorded in full above):
  layers clip; spillover is a layering decision, never surface expansion;
  negative margins are rare; the escape hatch is unclipped/own layers;
  class deliberate.
- **2026-10-18 — the `/types` advice accepted**: narrow newtypes over
  trait-based capability splits; popup/options moves to the widget layer as
  the follow-on ownership correction.
- **2026-10-18 — the arc order**: quick wins (PR 1) → sharing (PR 2) → the
  user-visible big win (PR 3) → the prove-or-abandon seam (PR 4) → the
  structural cleanup (PR 5). PRs 2 and 3 gate implementation on a written
  debate plus third-party agreement; PR 4 gates merge on the doors-open
  proofs.
- **2026-10-18 — the admission queue order within PR 1** (shorthands →
  collapse fix → box-sizing → !important → opacity → transform transitions
  → percentages/auto last and splittable).
