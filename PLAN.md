# PLAN.md — style-system coverage accounting (2026-10-18)

The declaration-store style redesign (proposals A+B+C on substrate S1) is
LANDED at v0.6.0 (see `MIGRATION.md` for its API record; the redesign plan
itself lives in git history). This document is the **coverage accounting
plan**: the spec areas the engine has deliberately set aside — as complete
an inventory as we can currently name, **growing as new gaps are
discovered** (the honesty rule forbids claiming closed-world exhaustiveness;
B11's tail table is regenerated from a CSS property index diffed against
the 68-name table whenever an admission lands) — bundled by
effort/popularity/priority, with the working discipline for pinning CURRENT
behavior with running proofs.

The purpose is accounting, not redesign:

1. **Know what we do.** Every set-aside area gets a running proof pinning
   the current observable behavior (usually: declaration dropped + counted,
   typed error, or defined no-effect).
2. **Nothing may crash us.** A discovered panic on boundary data is a
   repo-rule bug (AGENTS.md "Panic and boundary policy") and is hardened in
   the same change as its pin (typed error or drop — never panic), with the
   fix noted here and in the pin.
3. **The registry stays honest.** Every pin is classified (below) and
   registered in `tests/layout_proofs/README.md` in the same change. No
   production behavior change except crash-hardening and explicitly
   adjudicated divergence fixes (the B1 sign-rule family, the F1 atomicity
   fix; anything an adjudication forces — each recorded here).

## The phases

- **Phase 0 — file and folder rearrangement (batch 0).** The
  `layout_proofs/` suite moves to subsystem directories ahead of its
  growth. No behavior change; full suite green.
- **Phase 1 — the accounting (B1–B12).** Every item in this document gets a
  running proof pinning its current behavior. Crashes on boundary data are
  fixed immediately (typed error or drop, never panic — the B12 rule). The
  pre-adjudicated correctness fixes land here too, each named: **(i)** the
  B1 sign-rule family (longhand AND shorthand paths, plus the `font-size:
  unset` admission-to-seed); **(ii)** the F1 `load_stylesheet` atomicity
  fix (buffer-then-commit on success). The B5 adjudications record here.
  Exit criteria: every row pinned and
  verified, the registry current, the full suite green, this document's
  "expected current" columns corrected against reality. **No new features
  ship in Phase 1.**
- **Phase 2 — the march on-spec.** Item by item, with the pins as
  witnesses: a named consumer need opens the D6 admission (spec citation +
  Blink/Gecko algorithm reference + proof in the same change), the feature
  is implemented per the data-structure/algorithm sketches below (they are
  starting designs, revisited at admission), and its pin flips on-spec with
  the registry updated in the same change. **The flip is the acceptance
  test.** The march is slow and methodical by design: one admission per
  landing, ordered by the bundle table and consumer need. Some items may
  never be admitted (grid layout, floats) — their pins are the permanent
  record of the boundary, which is itself a deliverable of this plan.

**Scope guards (what this plan is NOT):** no vocabulary admissions in
Phase 1 (pins only, plus the two named fixes); no perf work (the pins watch
behavior, not counters — perf regressions found in passing are reported,
not chased); `flex_layout.rs` stays whole and `support.rs` stays rooted;
the redesign's step-5 followers (damage-rect consumption, matched-rule
caching, font-family CONSUMPTION, cascade-tier stages) are out of scope
here except where B4/B9 pin their current absence; the stream-pro2
conversion is a separate sister PR (see `MIGRATION.md`); PLAN.md edits
ride in the same commit as the work they describe, always.

## Conventions (read before writing any pin)

- **Pin classes** (extends the README's registry classes — CLOSED
  vocabulary; no hedge suffixes, every row carries exactly one class):
  - **divergent** (`off_spec_*`) — current behavior contradicts the spec
    within or beside the admitted vocabulary; pinned at current values.
  - **unimplemented** (`unsupported_*`) — policy-gated absence (D6 admission
    policy in `MIGRATION.md`); the pin asserts the gate behavior (dropped +
    counted, or load error) and no crash.
  - **deliberate** — crate-owned value replacing a spec default; permanent.
  - **on-spec** — accounting found the behavior already correct; pin to keep.
  - **robustness** (`robustness_*`) — crash-resistance pins over adversarial
    inputs; orthogonal to the classes above.
- **The admit-side/reject-side adjudication rule** (stated so structurally
  identical situations stop getting ad-hoc treatment): when our behavior
  *admits* what the spec rejects (accepting negative padding, illegal
  values flowing into layout), the divergence is a hazard and is **fixed in
  Phase 1** with its pin. When our behavior *rejects* what the spec admits
  (clamped opacities, wider grammars, unsupported features), the divergence
  is a gate and is **pinned, then moved through the D6 admission policy**
  when a consumer need is named. Reject-side divergences that contradict
  the semantics of an *admitted* property (e.g. `opacity` clamping) are
  classed **divergent** and pinned; absences outside the table are
  **unimplemented**.
- **Array-order contracts (cite once, use everywhere):** `margin`, `padding`,
  `border_width` are `[top, right, bottom, left]`; `radius` is `[TL, TR, BR,
  BL]` (`computed_box_group.rs`/`computed_visual_group.rs` document these;
  e.g. `margin-top` is `margin[0]`).
- **Every pin header states**: spec claim + citation, CURRENT (asserted),
  SPEC-CORRECT, ROOT CAUSE (or the D6 admission reference for `unsupported_*`).
  The word "verified" may only precede a claim the author checked against
  the tree in that change; expectations stay labeled expectations until the
  pin proves them (the honesty rule).
- **Proof shape**: table-driven; public `RootDom` APIs; assert computed
  groups (`root.computed_style(...)`), load reports, metric counters
  (`declaration_drop_count`, `StyleDeclarationDropped`), layout rects,
  staged paint ops. A passing test is the no-crash proof (panics fail tests).
- **`assert_decl_drop(root, css, applied, dropped)` contract:** the helper
  builds a FRESH root per call and asserts BOTH the per-load
  `StylesheetLoadReport` counts AND `context().declaration_drop_count()`
  (which is cumulative per context — fresh root means cumulative == per-load
  for that call). Table families share the helper, never a root.
- **Honesty rule**: "expected current" columns are best knowledge at writing
  time; the pinning task verifies each row, and where reality differs the
  PIN records reality and this document's row is corrected in the same change.
- **Gates per landing**: fmt, clippy, workspace tests, `cargo xtask lint`.

## Execution playbook (how to land a bundle)

Two landing shapes, both governed by the rule that **behavior change and
its proof move together** and no commit is ever red:

1. **Pin-only bundles** (B2, B3, B6, B7, B8-rows-that-are-not-F1, B9, B10,
   B11, B12): ONE commit — new proof file + its `style/mod.rs`
   registration + the README registry rows + this document's rows marked
   verified. No production changes.
2. **Fix-bearing bundles** (B1's sign-rule family; B8's F1 atomicity fix):
   TWO commits — (a) `off_spec_*` pins of the CURRENT behavior (they pass);
   (b) the production fix + the pin relabel on-spec + the registry update +
   the corrected doc rows, atomically.

Per-bundle checklist:

1. Read the bundle's section (table + scope-and-changes + data structures).
2. Verify each row's "expected current" against the tree FIRST (the honesty
   rule); correct the doc in the same commit.
3. Write the proof file to the conventions (table-driven, per-row names in
   assertion labels, headers citing spec + CURRENT + SPEC-CORRECT + ROOT
   CAUSE or the D6 admission reference).
4. Register the module and grow the README registry in the same change.
5. Run the full gate (fmt, clippy, workspace tests, `cargo xtask lint`,
   perf suites if production code moved).
6. Update the Landing status table at the end of this document.
7. Commit in repo style (`test(ui-dom): …` for pins, `fix(ui-dom): …` for
   production fixes, `docs:` for doc-only).

## The landed surface (how the pins observe)

The accounting asserts through these public/`pub(crate)` surfaces (all
landed at v0.6.0):

| What to assert | Through |
| --- | --- |
| a declaration dropped | `StylesheetLoadReport.declarations_dropped` per load + `RootDomContext::declaration_drop_count()` (cumulative) |
| a `style`-attribute drop | `RootDom::count(RootDomMetricName::StyleDeclarationDropped)` |
| an inline-block mutation | `RootDomMetricName::StyleDeclarationUpsert` |
| style-pass work | `RootDomMetricName::StylePassNodeVisited` / `StylePassNodeChanged` |
| a property's computed value | `RootDom::computed_style(key) -> Option<ComputedStyleRef>` → `.as_box()/.as_flex()/.as_visual()/.as_font()/.as_text()/.as_color()/.as_transform()` (typed `Copy` groups) |
| computed font family text | `ComputedStyleRef::font_family()` (resolves via the document's computed-text interner) |
| live declaration blocks | `RootDomContext::declaration_block_count()` (level accessor) |
| a transition's overlay | the same `computed_*` reads (overlay merges at read) |
| geometry/paint outcomes | `assert_node_layout`, staged layer paint plans (support.rs) |
| a programmatic write | `root.style_mut(key).set/.set_all/.remove` (Elements only; `InvalidAttributeTarget` otherwise) |
| a presentation write via attrs | `set_attr`/`set_attr_str` → `Err(DomError::PresentationRequiresStyleMut)` (except `style`, which routes) |

Key structural facts to rely on (verified at v0.6.0): the winners fold
order is behavior base → active pseudo-state → matched rules ascending
`(origin_priority, inline, specificity, order)` → inline block (inline wins
all); declarations intern in TWO arenas (the context-shared stylesheet
store and the per-document inline store); `font-family` computes into the
document's computed-text interner; fragment trees hold no declarations;
the `style` attribute route rejects non-Element targets; `pointer-events`
is a content-side `Custom` channel, not a presentation property.

## Verified this session (evidence registry)

Facts verified against the tree during the planning session — do not
re-verify or "correct" the document back to the contrary belief:

| Fact | Evidence | Used by |
| --- | --- | --- |
| `parse_authored_length` is sign-agnostic; negatives parse for all length properties except line-height | `dom_attr_parse.rs:116-130`; `decl_parse.rs:330-346` (`parse_non_negative_f32`/`parse_non_negative_length` reject there) | B1 |
| `load_stylesheet` is NOT atomic on the error path (partial insert, revision unbumped) | `style_runtime.rs:65-132` | B8/F1 |
| `order` clamps i64 → i32 saturating | `project.rs:444-445` | B1 |
| `text-align: justify` IS consumed | `text_render.rs:104-106` (`TextAlign::Justified → cosmic Align::Justified`) | B4 |
| `pointer-events` is `Custom("pointer-events")`: `none`→disabled, `auto`→enabled, else inherit | `hit_test.rs:331-337` | B4 |
| `line-height: 0` floors at 1px at render | `text_layout/mod.rs:719-722` | B1 |
| font-family/weight/style are computed but unconsumed (UiSans hardcoded) | `text_layout_request.rs:33-48`, `text_layout/mod.rs:36-39` | B4 |
| initial text color is near-white `[240/255;3, 1.0]` | `computed_color_group.rs:23` (`DEFAULT_TEXT_COLOR_RGBA`) | B5 |
| batch-0 moves are viable (proof files import support via crate-absolute paths; no `super::`) | reviewer verification, e.g. `normal_flow.rs:26` | batch 0 |
| `decl_value_matches_grammar` is kind-only ("Range constraints are enforced at parse time") | `root_dom.rs:4768` | B1/F2 |

This document itself passed one full third-party adversarial review
(2026-10-18; all findings adjudicated at commit `85de600`), after passing
three review rounds on the redesign plan it succeeds. Rows still marked
"expected current" are pending pin-verification per the honesty rule;
everything else above is settled evidence.

## Vocabulary carried from the redesign (self-contained definitions)

Terms this document actively uses, defined here so nothing depends on
archaeology:

- **D5** — the v0.6.0 hard-break migration contract: typed `set_attr` lost
  its presentation variants (compile break); `set_attr_str` fails with
  `DomError::PresentationRequiresStyleMut`; string ergonomics live on
  `style_mut` (see `MIGRATION.md`).
- **D6** — browser-conformance principle: inputs, outputs, and timings match
  Blink/Gecko *for the admitted vocabulary*. Admission of a property or
  grammar form requires (1) a named consumer need, (2) a spec citation plus
  the Blink/Gecko algorithm reference, (3) a proof in the same change.
- **D7** — damage rects are preserved; targeted repaint is a staged goal.
- **D8** — preserve all functionality; where behavior is reinterpreted,
  bias toward Blink/Gecko semantics.
- **G** — the drop-and-continue declaration-parsing rule (CSS 2.1 §4.1.7,
  css-style-attr §3): malformed declarations drop, parsing continues, and
  every drop is counted by metric AND returned in an explicit report
  (`StylesheetLoadReport`, `DeclDropReport`) — never silent. Scoped to
  declaration lists; single-declaration APIs stay strict.
- **F (cascade tier growth path)** — staged admissions: stage 1
  `!important` (importance bits), stage 2 a real UA tier, stage 3 the
  animation tier, stage 4 cascade layers. The `Precedence` record was
  widened so these are additive.
- **S1** — the substrate: declarations in arenas (`DeclStore`,
  generational keys), computed style as seven small `Copy` groups stored
  inline on the node (visit locality, value-compared, no refcounting),
  typed `DeclValue` at the declaration boundary, bounded inline lists for
  transition longhands (capacity 4).
- **A1 / A3 / A5 / A6 / A7 / A10** — the algorithms: A1 declaration parse
  (the ONE shorthand-expansion point, closed grammars); A3 selector parse +
  specificity (closed subset); A5 defaulting/inheritance (inherited groups
  seed from the parent, non-inherited from initial); A6 unit resolution
  (font-size em → parent, others → own); A7 group-diff dirty
  classification; A10 minimal-edit block upsert.
- **P1 / P2 / P3** — the redesign's root problems (now fixed): P1
  programmatic styles lost to recompute; P2 two timing models; P3 the
  one-bag stringly attribute model.

## Batch 0 — the suite reorganization (land first)

`layout_proofs/` moves to subsystem directories mirroring the engine's
domain boundaries. Byte-identical relocations (git mv + the mod.rs tree +
README registry paths + module-header self-references); full suite green;
the structural migration policy applies (tests relocate, never rewritten).

    tests/layout_proofs/
      README.md · mod.rs · support.rs · robustness.rs        (shared root)
      style/     cascade.rs (moved) · computed_projection.rs (absorbed from tests/)
                 unsupported_values.rs · unsupported_colors.rs · unsupported_shorthands.rs
                 unsupported_properties.rs · unsupported_selectors.rs
                 cascade_tiers.rs · admitted_gaps.rs
      layout/    block_inline.rs · normal_flow.rs · content_height.rs · flex_layout.rs
                 fixed_layers.rs · replaced_elements.rs · form_control_auto_size.rs
      text/      white_space.rs · text_auto_size.rs
      widgets/   popup_menus.rs · popup_text_rows.rs
      pipeline/  stylesheet_lifecycle.rs · transitions.rs

Notes: `support.rs` stays at root so `perf_metric.rs`/`perf_allocation.rs`'s
`#[path = "layout_proofs/support.rs"]` includes don't move; `flex_layout.rs`
stays whole (splitting by batch is a later call); `off_spec`/`unsupported`/
`robustness` test-name filters are substring matches and keep working at
depth; `tests/computed_projection.rs` folds into the suite as
`style/computed_projection.rs` (one target).

**Batch 0 scope and changes.**

- **Moves (byte-identical, `git mv`):** `cascade.rs` → `style/cascade.rs`;
  `block_inline.rs`, `normal_flow.rs`, `content_height.rs`, `flex_layout.rs`,
  `fixed_layers.rs`, `replaced_elements.rs`, `form_control_auto_size.rs` →
  `layout/`; `white_space.rs`, `text_auto_size.rs` → `text/`;
  `popup_menus.rs`, `popup_text_rows.rs` → `widgets/`;
  `stylesheet_lifecycle.rs`, `transitions.rs` → `pipeline/`.
- **`tests/computed_projection.rs` absorption (the one non-byte-identical
  move):** delete the standalone target; register `computed_projection` as a
  module under `style/`; its `#[path = "layout_proofs/support.rs"] mod support;`
  include becomes `use crate::layout_proofs::support::{...}` (the same
  symbols it imports today). Flag this in the landing note.
- **Module tree:** `tests/layout_proofs/mod.rs` declares `mod style; mod
  layout; mod text; mod widgets; mod pipeline; mod support;` — each
  subdirectory gains its own `mod.rs` declaring its files (mod.rs files hold
  no test bodies, matching today's shape).
- **README (`tests/layout_proofs/README.md`):** every file citation in the
  registry table and spec-coverage map updates to the new paths
  (`style/cascade.rs`, `layout/normal_flow.rs`, `layout/replaced_elements.rs`,
  `pipeline/transitions.rs`, …). Also correct the stale precedence claim in
  the same change: the registry's cascade row says precedence is
  "(origin.priority, specificity, order)" — the actual `Precedence` record
  is `{ origin_priority, inline, specificity, order }`
  (`decl_value.rs:39-41`, with the `inline` flag sorting above any selector
  per css-style-attr §4).
- **Module headers:** check each moved file's `//!` header for self-path
  citations and update them (e.g. `style/cascade.rs`'s header still points
  at `tests/layout_proofs/README.md` — that reference is path-stable).
- **New proof files land in their final homes** (the `style/` files for
  B1–B11, root `robustness.rs` for B12) in their bundle's landing — never
  moved twice.
- **Gate:** full suite green with zero content changes to moved files
  (except the `computed_projection` include). No production code touched.


## The bundles (ratings + landing order)

Effort = pinning work only (S mechanical, M some verifications, L unbounded).
Popularity = real-world CSS usage frequency. Priority = drift risk × crash
risk × consumer/admission likelihood. Landing order = table order.

| # | Bundle (proof file) | Effort | Popularity | Priority | Why |
| --- | --- | --- | --- | --- | --- |
| B1 | Sign rules & numeric edges (`style/unsupported_values.rs`) | M | H | **H** | The only *divergent* rows (negative `padding`/`width`/`border-width`/`radius`/`gap`/`font-size` accepted today; CSS rejects them) + crash-adjacent numeric edges. Small grammar fixes land with the pins. |
| B2 | Units & sizing keywords (`style/unsupported_values.rs`) | M | H | **H** | Percentages, `calc()`, viewport units, `rem`, `auto`, intrinsic keywords. Highest-likelihood admission requests. |
| B3 | Box shorthands (`style/unsupported_shorthands.rs`) | S–M | H | **H** | `margin`/`padding` (the #1 shorthand in CSS), `border-width`, `inset`, `outline` — one expansion family. |
| B4 | Text consumption & admitted quirks (`style/admitted_gaps.rs`) | M | H | **H** | The named step-5 follower (`font-family`/`weight`/`style` unconsumed) + `justify`/`pre*` verification, `transform` transitions, range-`0px` pill, `pointer-events` channel. |
| B5 | Preserved candidates (`style/admitted_gaps.rs`) | S | M | **H** | Three visible-every-frame defaults awaiting adjudication: near-white initial color, `line-height: normal` → 1.0×, border initial `0`. |
| B6 | Layout-impact properties (`style/unsupported_properties.rs`) | M | H | M–H | `z-index`, `sticky`, `box-sizing`, `aspect-ratio`, `object-fit`, `float`/`clear`, `display: grid/contents/table`. |
| B7 | Color grammar (`style/unsupported_colors.rs`) | S | H | M | hex3/4, wider named colors, `currentColor`, modern `rgb()` — one grammar-widening family. |
| B8 | Selectors & at-rules (`style/unsupported_selectors.rs`) | M | M–H | M–H | `>`, `+`, `~`, `[attr]`, more pseudo-classes/elements, selector lists, `@media`/`@supports`/`@keyframes` — currently **fatal whole-load rejections**. Includes the stray-`}` adjudication. |
| B9 | Cascade tiers (`style/cascade_tiers.rs`) | M | M | M | `!important` (declaration currently dropped), custom properties/`var()`, `@layer`, animation tier, UA-sheet-workaround pin (on-spec). |
| B10 | Other shorthands & overflow axes (`style/unsupported_shorthands.rs`) | S | M | M | `font`, `background`, `text-decoration`, `all`, `overflow-x`/`overflow-y`. |
| B11 | Text/effects property tail (`style/unsupported_properties.rs`) | M–L | L–M | L | `text-decoration`, `box-shadow`, `text-overflow`, `letter-spacing`, `line-clamp`, `writing-mode`, `visibility`, `cursor`, filters, masks, table-*, column-*, … |
| B12 | Robustness (`robustness.rs`) | M–L | — | M | The no-crash safety net. **Crash discoveries jump the queue** — they become boundary-hardening fixes in the same change. |

## B1 — sign rules & numeric edges (divergent + crash-adjacent)

Current length grammar (`unset | 0 | <px> | <em>`) is sign-agnostic per
property except where a grammar already says otherwise — **line-height is
the existing exception**: `parse_line_height` (`decl_parse.rs:330-346`)
rejects negatives today through `parse_non_negative_f32` /
`parse_non_negative_length`, the helpers the B1 fix generalizes. Everywhere
else, negatives parse (verified: `parse_authored_length` at
`dom_attr_parse.rs:116-130` is sign-agnostic).

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `padding-left: -10px` | accepted → negative inset | illegal, CSS rejects | **divergent — fix candidate** |
| `width: -10px`, `height: -1px`, `min-width: -1px`, `max-width: -1px`, `min-height: -1px`, `max-height: -1px` | accepted | illegal (non-negative) | **divergent — fix candidate** |
| `border-top-width: -2px`, `outline-width: -2px` | accepted | illegal | **divergent — fix candidate** |
| `border-radius: -4px` (SHORTHAND path), `gap: -4px` (shorthand), `border-top: -2px` (side alias) | **accepted** (expansion helpers bypass the grammar layer — see the fix spec) | illegal | **divergent — fix candidate** |
| `outline-offset: -2px` | accepted | **legal** | on-spec (pin to keep) |
| `margin-left: -10px`, `top: -5px` | accepted | legal | on-spec (already covered; keep) |
| `font-size: -10px` | accepted → negative font into text layout | illegal | **divergent + crash-adjacent — fix candidate (reject at grammar)** |
| `font-size: unset` | dropped+counted (grammar is `LengthNoUnset`) | `unset` is CSS-wide (≈ inherit here) | **divergent — small fix in the same family (admit, resolve to seed)** |
| `font-size: 0`, `font-size: 0px` | admitted → 0px font into text layout | legal | robustness (must not crash shaping/intrinsics) |
| `line-height: -5px` | dropped+counted (already rejected — `parse_non_negative_*`) | illegal | on-spec (pin to keep) |
| `flex-shrink: -1` | dropped+counted | non-negative | on-spec (pin to keep; sibling of `flex-grow: -1`) |
| `line-height: 0` | admitted → **render-side 1px clamp** (`line_height_px.max(1.0)`, `text_layout/mod.rs:719-722`) | legal (zero-height lines are meaningful) | **divergent** — a render-side clamp of a legal value; resolve at pin time: remove the floor if cosmic tolerates zero-height lines (render nothing), else reclassify deliberate-with-crash-reason and document it |
| `width: 0` (unitless) | admitted (the grammar's `0` arm) → `0px` | legal | on-spec (guard pin against the other direction) |
| `width: 1e400px`, `NaNpx`, `infpx`, `-infpx` | dropped+counted (non-finite rejected) | finite only | robustness |
| `order: 9223372036854775807` (i64::MAX) | accepted → clamped to i32 (`project.rs:444-445`) | CSS admits any integer | unimplemented (implementation range limit; pin names the clamp) |
| `order: 99999999999999999999` (beyond i64) | dropped+counted | CSS admits any integer | unimplemented (same range limit, same rationale, same class) |
| `width: 3.4028235e38px`, `width: 1.4e-45px` (subnormal) | accepted (finite) | valid | robustness |
| `border-radius: 10000px` on a 10px box | accepted (verify paint clamp) | CSS caps radii at render | robustness |
| `transform: translate(999999px, -999999px)` | accepted (no clamp) | valid | robustness |
| `flex-grow: -1` | dropped+counted | non-negative | on-spec (pin to keep) |
| `order: 1.5` | dropped+counted | integer | on-spec (pin to keep) |
| `opacity: 1.5`, `opacity: -0.5` | dropped+counted (range 0..=1) | CSS clamps out-of-range at computed time | **divergent** (reject-side contradiction of an admitted property's semantics; pinned; moves through admission) |


**B1 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_values.rs`
  (new; registered in `style/mod.rs`; holds the B1+B2 rows, one table-driven
  test per row family, ~15–20 tests).
- **Shared helper (support.rs):** add `assert_decl_drop(root, css, applied,
  dropped)` — loads a stylesheet via `theme_stylesheet_origin()`, asserts
  `StylesheetLoadReport.declarations_applied/dropped` and
  `context().declaration_drop_count()`. Most unsupported_* rows collapse to
  this one helper plus a computed-group read.
- **Production change — the sign-rule divergence fix** (adjudicated above;
  the fix covers BOTH the longhand and shorthand paths):
  - `src/domains/style/presentation_property.rs`: split `DeclGrammar::Length`
    into `Length` (signed — margins ×4, top/right/bottom/left, outline-offset)
    and `NonNegativeLength` (padding ×4, width/height/min/max ×6,
    border-width ×4, border-radius ×4, row-gap/column-gap, outline-width);
    change `FontSize`'s grammar from `LengthNoUnset` to a non-negative form
    (`NonNegativeLengthNoUnset` or a flag — implementer's shape, the grammar
    enum is the admission table so the sign policy lives there as metadata);
    the `FlexBasis` grammar arm also rejects negative lengths (CSS: negative
    flex-basis is illegal). Reuse the existing `parse_non_negative_f32` /
    `parse_non_negative_length` helpers (`decl_parse.rs`) — the non-negative
    concept already exists in-tree (line-height); the fix generalizes it.
  - **The expansion layer applies the per-longhand grammar, never the
    primitive directly.** Today `expand_border_radius` (`decl_parse.rs:660`),
    `expand_gap` (:720), and `expand_border_side` (:930) call the signed
    length helpers directly and construct `Declaration`s by hand — so the
    sign policy would be bypassed via `border-radius: -4px`, `gap: -4px`,
    `border-top: -2px`. The fix: every expansion helper parses each produced
    token THROUGH the target longhand's grammar (`parse_value` with the
    longhand's `DeclGrammar`), making the invariant structural: expansion =
    tokenization + per-longhand grammar application. (For the B1 grammars
    the three helpers are all non-negative, so this is a policy-parametrized
    call, not a redesign of expansion.)
  - `src/domains/style/decl_parse.rs`: the new grammar arms reject negative
    scalars (`scalar < 0.0 → None`). `parse_authored_length`
    (`src/types/dom_attr_parse.rs:116-130`) stays sign-agnostic; the policy
    lives in the grammar layer, not the primitive.
  - The resolver in `src/domains/style/computed/project.rs` needs no change
    (rejected negatives never reach it).
  - **`font-size: unset` admission (same family):** `font-size`'s grammar
    admits `unset`, resolving to the inherited seed (CSS: `unset` on an
    inherited property = inherit; mechanically it resolves exactly like an
    absent declaration — the winners fold's seed path). The resolver's font
    size stays a plain `f32` (seed guarantees a value).
- **Landing order within the batch (TWO commits — the plan's own gate
  forbids a deliberately-red commit):** (a) the `off_spec_*` pins of the
  current accepted-negative behavior (they pass); (b) the grammar fix AND
  the pin relabel AND the registry update in one atomic commit (behavior
  change and its proof move together).
- **`font-size: 0`:** stays admitted (CSS-legal). The robustness pin proves
  `CosmicMetrics::new(0, …)` (`text_layout/mod.rs:189,279`) and intrinsic
  measurement survive. If cosmic panics at 0, the hardening is render-side
  (a zero-size text run renders nothing) — never a clamp that would render
  1px text (that would be wrong per CSS).
- **Numeric edges:** `order` clamp verified saturating at
  `project.rs:444-445` (`i64` clamped to `i32::MIN..=MAX`); pin it on-spec.
  Non-finite rejection (`1e400px`, `NaNpx`, `infpx`) is already enforced by
  `parse_finite_length_scalar`; pin on-spec.
- **Registry:** the B1 divergent rows (padding/width/min-max/border-width/
  outline-width negatives via longhand AND shorthand, font-size negatives,
  `font-size: unset`) enter as `off_spec_*`, relabel on-spec
  in commit (b); the on-spec/robustness rows register as pins with no
  divergence entry. `opacity` and the `line-height: 0` floor enter as
  **divergent** and stay pinned (reject-side — they move through admission,
  not the B1 fix).

**B1 data structures and algorithms (Phase 1 fix, referenced for the record).**
Current state: the admission table is `DeclGrammar` in
`presentation_property.rs` — a `Copy` enum where each variant IS a grammar
family, attached per property in the static `METADATA` table. That shape is
deliberate (redesign approach C): every per-property policy is metadata in
ONE table, so a policy question ("is this length signed?") has exactly one
answer-site. The fix extends the same shape: `Length` (signed) and
`NonNegativeLength` variants, with the sign policy assigned per property in
`METADATA`; the primitive (`parse_authored_length`,
`dom_attr_parse.rs:116-130`) stays sign-agnostic — policy lives in the
grammar layer, not the primitive, because the primitive is shared by
grammars with different sign rules. Algorithm: one comparison
(`scalar < 0.0 → None`) in the two new `parse_value` arms of
`decl_parse.rs`; rejection flows through the existing drop-and-continue
report (CSS 2.1 §4.1.7 treats out-of-range as malformed). The non-finite
rejection already lives in `parse_finite_length_scalar` (NaN/inf never
enter a group — finite f32 is a group-field invariant the A7 `memcmp`-style
diffs rely on).



## B2 — units & sizing keywords

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `width: 50%`, `margin-left: 10%`, `font-size: 120%`, `line-height: 150%` | dropped+counted | percentages | unimplemented |
| `width: calc(100% - 10px)`, `calc(1px + 1px)`, `min()/max()/clamp()` | dropped+counted | math functions | unimplemented |
| `width: 100vw`, `height: 50vh`, `10vmin`, `10vmax` | dropped+counted | viewport units | unimplemented |
| `font-size: 2rem` | dropped+counted | root em | unimplemented |
| `1ex`, `2ch`, `1lh`, `1rlh` | dropped+counted | relative units | unimplemented |
| `width: auto`, `margin-top: auto` | dropped+counted (observably ≈ initial for width) | `auto` keyword | unimplemented |
| `width: min-content` / `max-content` / `fit-content` | dropped+counted | intrinsic size keywords | unimplemented |
| `width: 10` (unitless non-zero) | dropped+counted | **CSS also rejects** | on-spec (pin to keep) |
| `width: inherit` / `initial` / `revert` / `revert-layer` | dropped+counted (only `unset` admitted) | CSS-wide keywords | unimplemented |
| ~~`font-size: unset`~~ | **RESOLVED in B1b** — admitted, resolves to the inherited seed; pinned on-spec by `font_size_unset_resolves_to_inherited_seed` (`style/unsupported_values.rs`) | `unset` is CSS-wide | on-spec |

**B2 scope and changes.**

- **Proof file:** same `style/unsupported_values.rs` as B1 (the B2 rows form
  their own table family inside it).
- **No production changes** — every row is a drop-pin through the B1
  `assert_decl_drop` helper plus a computed read
  (`root.computed_style(node).expect(...).as_box()` etc.) proving the
  property stayed at its group initial.
- **The `auto` nuance:** `width: auto` drops today, which is observably
  close to the CSS initial — the pin must assert BOTH facts explicitly (the
  declaration is dropped+counted AND the computed width is `None`), so the
  accounting never confuses "correct outcome" with "admitted grammar".
  `margin-top: auto` gets the same two-sided pin (dropped AND `margin[0] == 0.0`).
- **Percentage rows** assert the drop across the four grammar shapes
  (length box, font-size, line-height, and one inset) so a future percentage
  admission has one flip-site per shape.
- **Registry:** all rows register `unsupported_*` with the D6 admission
  reference; no divergent entries.

**B2 data structures and algorithms (Phase 2 preview).** Current state:
`DeclLength { Unset, Px(f32), Em(f32) }` (`decl_value.rs`) is the only
length shape, and computed groups store RESOLVED px (`Option<f32>`),
because A6 resolves everything resolvable at style time — that was the
redesign's deliberate choice: groups are `Copy`, compared by value, and
consumers never re-parse. Percentages break the "resolve at style time"
premise: a percentage resolves against the containing block, which exists
only at layout. The admission-time shape therefore splits
**computed vs used** (the redesign's own "used-vs-computed honesty" note,
CSS Cascade §4.5): the box group's length fields become an enum
(`BoxLength { Auto, Px(f32), Percent(f32) }` — still `Copy`, still
value-compared) holding the COMPUTED form, and layout resolves
`Percent(x)` against the containing-block dimension it already owns (CSS
Values §percentages: width → cb width; margin/padding → cb width even
vertically; font-size → parent font-size; line-height → element font-size).
`calc()` is a separate, later admission: a bounded expression type
(`CalcExpr`, depth-capped tree over our admitted units — Blink's
`MathExpressionNode`/Servo's `calc` module are the engine references),
resolved to `BoxLength` when all terms are absolute, kept symbolic
otherwise. `rem` resolves at style time against the ROOT element's
computed font-size (the pass already seeds from the root; no used-value
split needed — same shape as `em`, `DeclLength::Rem(f32)`).
`auto`/intrinsic keywords ride the existing `DeclValue::Keyword`
vocabulary: `auto` maps to the group's `None`/Auto initial (free), while
`auto` MARGINS are the honest exception — CSS resolves them at layout
(absorbing free space), so they need the same `BoxLength::Auto` used-value
resolution (the layout algorithm that distributes free space — the
centering case consumers will actually ask for). Viewport units
(`vw`/`vh`/`vmin`/`vmax`) resolve at style time against the viewport
dimensions the composite stage already knows — a style-time available
context, no used-split.



## B3 — box shorthands

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `margin: 8px`, `margin: 1px 2px 3px 4px` | dropped+counted | margin ×4 | unimplemented |
| `padding: 8px` … | dropped+counted | padding ×4 | unimplemented |
| `border: 1px`, `border-width: 1px 2px` | dropped+counted | border widths ×4 (style/color components unadmitted) | unimplemented |
| `inset: 0`, `inset: 1px 2px` | dropped+counted | top/right/bottom/left | unimplemented |
| `outline: 2px` | dropped+counted | outline-width (style/color unadmitted) | unimplemented |

**B3 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_shorthands.rs`
  (new; holds B3 and B10 rows as separate table families).
- **No production changes** — drop-pins through `assert_decl_drop`, plus a
  per-shorthand expansion-behavior pin: each row asserts the corresponding
  longhand group fields stay at initial (e.g. after `margin: 1px 2px 3px
  4px`, `computed_box().margin == [0.0; 4]`).
- **Admission notes for the future flip:** the 1–4 corner mapping is already
  implemented for `border-radius` (`decl_parse.rs` `expand_border_radius`)
  and is the template for `margin`/`padding`/`border-width`/`inset`;
  `outline` and `border` (width-only subset) follow the
  `Shorthand::BorderSide` alias pattern already in `property_shape`.
- **Registry:** `unsupported_*` rows; the margin/padding row headers note
  these are the #1 admission candidates (popularity).

**B3 data structures and algorithms (Phase 2 preview).** Current state: the
system has exactly ONE shorthand-expansion point — A1 in `decl_parse.rs`
(CSS Cascade 5 §3: "a shorthand sets all of its longhands exactly as if
expanded in place," at declaration time) — with the `Shorthand` enum +
`property_shape` name table and `expand_border_radius` as the 1–4-value
corner-mapping reference implementation. The admission of every shorthand
in this bundle and B10 is the same mechanical shape: a new `Shorthand`
variant, a name in `property_shape`, and an expansion fn returning
`SmallVec<[Declaration; 4]>`; `margin`/`padding`/`border-width`/`inset`
reuse the corner map verbatim (inset maps to top/right/bottom/left);
`outline` and `border` follow the existing `Shorthand::BorderSide`
width-only-alias precedent (the unadmitted style/color components are
dropped with a counted note or rejected per the fail-closed rule — the
call is made at admission with the pin flipping). No new data structures:
the cascade and consumers only ever see longhands, which is WHY the
expansion is safe — the winners fold, groups, and diff machinery are
shorthand-agnostic by construction (the redesign's B decision).



## B4 — text consumption & admitted quirks

Admitted grammars whose computed values are ignored, approximated, or
quirky downstream. Verify each at pin time.

| Behavior | Expected current (verify) | Spec-correct | Class |
| --- | --- | --- | --- |
| `font-family` computed but ignored by text layout (hardcoded UiSans) | text renders UiSans regardless | consume the computed family | **divergent (step-5 follower)** — pin |
| `font-weight` / `font-style` computed but ignored | rendered regular/upright | consume weight/style | divergent (same family) — pin |
| `text-align: justify`/`justified` consumption | **consumed** — `TextAlign::Justified → cosmic_text::Align::Justified` (`text_render.rs:104-106`) | justified text | on-spec (pin to keep) |
| `font-style: oblique` | dropped+counted (closed set normal/italic) | oblique admitted in CSS | unimplemented |
| `white-space: pre-line` consumption | **verified**: `collapse_spaces_preserving_segment_breaks` (`white_space.rs:147-180`) collapses spaces/tabs and preserves newlines | full white-space modes | on-spec (pin to keep) |
| `white-space: break-spaces` consumption | **verified**: folds into `pre-wrap`'s `preserve_space_run_advances` (`white_space.rs:255-277`); repeated trailing spaces wrap via NBSP substitution but a lone trailing space is not preserved | a distinct break-spaces policy (preserve every trailing space) | **divergent (approximated as `pre-wrap`)** — pin |
| `transform` transitions (`transition: transform 100ms`) | **no transition schedules** — the transform value jumps at the next update (its `DeclValue` is interned text, which the scheduler cannot interpolate; the typed `[f32; 2]` pair is the admission shape; v0.5 had a delayed step) | transform interpolates | **divergent (documented, MIGRATION.md)** — pin |
| range control + authored `border-radius: 0px` | falls to pill default (flattened presence) | authored wins over control default | **divergent (documented)** — pin; fix is the behavior-sheet tier (F) |
| `dir` content attr vs `direction` property | `direction` wins (documented) | HTML/CSS interaction | on-spec (pin to keep) |
| `pointer-events` content-attr channel | `Custom("pointer-events")`: `"none"` → disabled, `"auto"` → enabled, else inherit (`hit_test.rs:331-337`) | behavior input (not cascaded) | on-spec for `auto`/`none`; wider CSS values stored-but-inherit — unimplemented |
| `pointer-events` STYLESHEET channel | `.overlay { pointer-events: none }` → dropped+counted (name not in the presentation table), hit-testing unaffected | consumers reasonably expect it to work | **unimplemented** (the consumer-surprise row; admission would move the channel into the presentation table) |

**B4 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/admitted_gaps.rs`
  (new; holds B4 and B5 rows).
- **`font-family` unconsumed (divergent, the step-5 follower):** pin through
  text-intrinsic measurement — a container styled `font-family: serif`
  produces text intrinsic widths identical to the default (the family is
  hardcoded at `text_layout_request.rs:33-48` to `TextFontFamilyId::UiSans`,
  the one-variant enum at `text_layout/mod.rs:36-39`, resolved by
  `preferred_repo_font_family` at `text_render.rs:23-63`), while
  `root.computed_style(node).expect(...).font_family()` resolves the declared
  string through `RootDom::resolve_computed_text`. The pin asserts both:
  computed family present AND rendering unchanged. Class: **divergent**;
  consumption is the named step-5 follower.
- **`font-weight`/`font-style` unconsumed:** same shape (computed
  `as_font().weight`/`style` present; rendered run unchanged). Class: divergent.
- **`text-align: justify`:** verified CONSUMED — `text_render.rs:104-106` maps
  `TextAlign::Justified → cosmic_text::Align::Justified`. Pin on-spec with a
  two-word line in a constrained width (justified spacing must differ from
  start-aligned; the `text_auto_size.rs` harness provides the measurement
  pattern).
- **`font-style: oblique`:** drop-pin (closed set is normal/italic) —
  unimplemented.
- **`white-space: pre-line` / `break-spaces`:** verify each keyword's actual
  text-layout mode against the consumed `normal`/`nowrap`/`pre`/`pre-wrap`
  set (pinned in `text/white_space.rs`). If a keyword maps to the same mode
  as another (approximation), pin the approximation and classify it
  (divergent vs deliberate at pin time).
- **`transform` transitions (divergent, documented in MIGRATION.md):** pin
  that `transition: transform 100ms` with a `translateX` change presents the
  target immediately at the next update (no scheduled transition — the
  typed `Text` value cannot interpolate through the scheduler; the v0.5
  delayed-step is also gone). Assert no active transition on
  `PresentationProperty::Transform` and `computed_transform().translate_px`
  at target. Fix is typed-translate interpolation (a small admission-gated
  follow-up).
- **Range control + authored `border-radius: 0px` (divergent, documented):**
  pin the pill fallback (`element_behaviors/paint.rs` range radius reads
  `radius != [0.0; 4]` as presence). Fix belongs to the behavior-sheet tier
  (F): the pill default becomes a base behavior declaration the authored
  `0px` overrides.
- **`dir` vs `direction`:** on-spec pin (property wins over the content
  attribute; already resolved in the projection's `apply_dir_content_attribute`).
- **`pointer-events` (verified):** the channel is the content-side
  `Custom("pointer-events")` attribute, consumed by hit-testing at
  `hit_test.rs:331-337`: `"none"` → disabled, `"auto"` → enabled, anything
  else → inherit the parent's state. It is NOT in the presentation table by
  design (deliberate: it is behavior input, not a cascaded style). Pins:
  `auto`/`none` on-spec (subtree disable + re-enable); wider CSS values
  (`visiblePainted`, `painted`, `fill`, `stroke`, `all`) stored-but-inherit —
  unimplemented rows. The STYLESHEET row (the consumer surprise) pins the
  other channel explicitly: `pointer-events` in a stylesheet is a
  dropped+counted declaration with no hit-test effect.

**B4 data structures and algorithms (Phase 2 preview).** Current state: the
computed font group already carries everything text layout needs —
`family: Option<TextKey>` (interned via the document's computed-text
interner, resolved by `RootDom::resolve_computed_text`), `weight: u16`,
`style: Keyword` — because the redesign's S1 font group was DESIGNED for
this follower (the fields exist, inherit correctly per CSS Fonts, and are
diff-classified so a family change re-measures text). The consumption work
is at the text-layout boundary: `TextFontFamilyId`
(`text_layout/mod.rs:36-39`) stops being a one-variant enum and becomes a
resolved family request — the `TextLayoutRequest` (`text_layout_request.rs`)
carries the resolved family STRING (parsed as a CSS family list:
comma-separated, quotes stripped, generic families `serif`/`sans-serif`/
`monospace`/`system-ui` mapped to fontdb generics), `weight` → cosmic
`Weight(u16)`, `style` → cosmic `FontStyle`; matching and fallback are
delegated to fontdb/cosmic-text (Blink/Gecko do the same: first matched
family in list order, generics last — the engine reference for the
fallback algorithm). The font catalog (`ui_dom_text_font_catalog.rs`) owns
family-name resolution to loaded faces; the intrinsic text measurement and
the render path consume the same resolved triple so measure == render (the
existing single-measure rule). `white-space` mode gaps (`pre-line`,
`break-spaces`) map into the shaper's collapse/preserve table
(`text_render.rs`); `pre-line` = collapse spaces, preserve newlines;
`break-spaces` = preserve trailing spaces — both are shaping-policy rows,
not new structures. The `transform`-transition follower is smaller than it
looks: the computed transform group is already `translate_px: [f32; 2]`,
so the scheduler interpolates two px channels exactly like a length pair —
the work is admitting a typed transform pair into the scheduler's
`DeclValue` interpolation table (the 3.3e note), not designing one.



## B5 — preserved pre-redesign candidates

Preserved per D8, never pinned. Divergent-vs-deliberate adjudication at pin
time, recorded here and in the registry:

| Behavior | Current | Spec default | Notes |
| --- | --- | --- | --- |
| initial `color` when nothing declared | near-white `[240/255;3, 1.0]` (design-system default) | `canvastext` (black) | likely **deliberate** (dark-theme crate) — adjudicate |
| `line-height: normal` | `1.0 × font-size` | UA-dependent, ~1.2 | divergent or deliberate — adjudicate |
| border widths when undeclared | `0px` | `medium` | divergent or deliberate (borders opt-in) — adjudicate |
| 16:9 placeholder ratio (never-loaded images) | already **deliberate**, registered | 300×150 default object size | already pinned; no work |

**B5 scope and changes.**

- **Proof file:** same `style/admitted_gaps.rs` (a `preserved_candidates`
  table family).
- **Adjudications (decided, recorded here — the pins assert current values
  and the registry rows carry these classes and rationale):**
  - **near-white initial `color` → deliberate.** `[240/255;3, 1.0]` is the
    crate's dark-theme design-system default (definition site:
    `DEFAULT_TEXT_COLOR_RGBA` in `computed_color_group.rs`); CSS's
    `canvastext` default has no meaning for this product surface. Pinned
    permanently; re-reviewed if the design system changes.
  - **`line-height: normal` → `1.0 × font-size` → divergent.** Every browser
    uses ~1.2 (UA-dependent); D6 favors the browser answer. Pinned at
    current values, expected to move on-spec when a consumer need is named
    (reclassify deliberate only if the design system explicitly confirms
    tight rows are wanted).
  - **border widths initial `0` → deliberate.** CSS's `medium` initial only
    matters in combination with `border-style`; since `border-style` initial
    is `none`, undeclared borders are invisible in browsers too. Our
    width-gates-border model is observably identical for the undeclared
    case. (`border-style` itself is an unadmitted property — its
    dropped+counted pin is owned by B11; the `border-style` admission would
    revisit this adjudication.)
- **Registry:** one row per candidate with the class and rationale above;
  the near-white and border rows are permanent (deliberate), the
  line-height row is expected to move (divergent).

**B5 data structures and algorithms (Phase 2 preview, line-height only —
the other two are pinned permanent).** Current state: `line-height: normal`
resolves to `None` in the font group, and the downstream default is
`font_size_px` (1.0×) at `text_layout/mod.rs:714-722`
(`effective_line_height_px`). The on-spec shape: `normal` resolves from
FONT METRICS, not a fixed factor — Blink's `FontMetrics::lineSpacing` and
Gecko's hhea-derived normal line height both compute
`ascent + descent + lineGap` (≈ 1.2 for most text faces). cosmic-text's
`FontSystem`/fontdb exposes the same metrics (`Metrics::new(font_size,
line_height)` vs the face's hhea table). The algorithm change is one
function: `normal` → query the resolved family's hhea metrics and compute
`line_height = ascent + descent (+ lineGap)` at the same site that today
returns `font_size_px`. The pin flips when it lands; the font-consumption
follower (B4) is a prerequisite in practice, since metric choice depends on
the resolved family.



## B6 — layout-impact properties

Not in the 68-property table: each is dropped+counted, inert, no crash.
Pin per property: the drop is counted AND the defined no-effect holds
(e.g. `z-index` has no effect on layering order, `sticky` behaves as
`static`, `display: grid` falls back to the initial display). Rows:
`z-index`, `float`, `clear`, `box-sizing`, `position: sticky` (value-side
drop), `display: grid` / `inline-grid` / `table` / `inline-table` /
`flow-root` / `contents` / `ruby` (value-side drops), `aspect-ratio`,
`object-fit`, `object-position`, `contain`, `content-visibility`,
`break-inside`, `resize`, `scroll-behavior`,
`scroll-snap-*`, `overscroll-behavior`, `touch-action`, `will-change`.
(`overflow-x`/`overflow-y` are owned by B10 — single-owner rule.)

**B6 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_properties.rs`
  (new; holds B6 and B11 rows as separate table families).
- **No production changes.** Each row is two-sided: the declaration is
  dropped+counted (`assert_decl_drop`) AND the defined no-effect holds. The
  no-effect assertion per family:
  - `z-index`: sibling paint order unchanged (staged paint plans keep DOM
    order; the layer order machinery at `composite/mod.rs` is unaffected).
  - `position: sticky`: layout equals `static` (the value drop leaves
    `position` at initial `Static`).
  - `display: grid`/`table`/`contents`/…: layout equals the initial display
    (the value drop leaves `DisplayKw::Block` for div).
  - `box-sizing`: sizing stays content-box (our only model — the pin header
    notes that `content-box` is the crate's single sizing model, so
    `box-sizing: content-box` names the already-active model and changes
    nothing; a `width: 100px` + `padding: 10px 10px` box has a 120px border
    box, not 100px).
  - `aspect-ratio`, `object-fit`, `object-position`: replaced-element sizing
    and paint unchanged (img keeps intrinsic/placeholder behavior pinned in
    `layout/replaced_elements.rs`).
  - `float`, `clear`: flow participation unchanged.
  - `contain`, `content-visibility`, `break-inside`, `resize`,
    `scroll-behavior`, `scroll-snap-*`, `overscroll-behavior`, `touch-action`,
    `will-change`: layout/paint/scroll outcomes equal the no-declaration
    baseline (scroll behavior pinned by the existing scroll unit tests).
- **Registry:** `unsupported_*` rows with the D6 admission reference.

**B6 data structures and algorithms (Phase 2 preview, per property).**
Admission shape follows the redesign's standing rule: a property enters the
68-row table in `presentation_property.rs` (name, group, grammar — one
metadata row), a field appears in the owning group if one is missing
(`Copy` fields only — the S1 invariant that groups stay small, comparable,
and arena-free), and consumers read it visit-locally. Per family:
`z-index` → an order key on the layer (`composite/mod.rs` already maintains
`layer_order`; the algorithm is CSS 2.1 Appendix E stacking order —
tree-order sorted by z within stacking contexts; the field is an
`Option<i32>` on the box group). `position: sticky` → `PositionKw::Sticky`
plus the layout algorithm: the sticky constraint rectangle (nearest
scroll-port ancestor) clamps the static position at scroll time (CSS
Position 3 §sticky; the scroll machinery at `domains/visitor/scroll.rs`
already knows the container). `box-sizing` → `BoxSizingKw { BorderBox,
ContentBox }` on the box group; layout's width/height interpretation moves
inside one function (the sizing projection in `domains/layout/style.rs` —
content-box stays the default and the crate's current single model).
`aspect-ratio` → `Option<f32>` field; the CSS Sizing 4 preferred-ratio rule
constrains auto sizes in the replaced-element and absolute sizing paths.
`object-fit`/`object-position` → img paint algorithm (contain/cover/fill
letterboxing inside the content box, at `paint/mod.rs`'s image op).
`float`/`clear` → flow-layout additions (out-of-flow boxes, line
shortening, clear bands in `domains/layout/flow.rs`) — the largest item in
this bundle and a likely never-admit. `display: grid`/`table`/`contents` →
`grid` is a new solver (never-admit by default); `contents` is a
box-generation rule (suppress the own box, children join the parent's
formatting context) — medium, and the only display admission with a
plausible consumer need.



## B7 — color grammar

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `color: #abc` / `#abcd` | dropped+counted (hex6/8 only) | 3/4-digit hex | unimplemented |
| `color: rebeccapurple`, `color: gray` | dropped+counted (5 named only) | full named-color set | unimplemented |
| `color: currentColor` | dropped+counted | currentColor keyword | unimplemented |
| `color: rgb(255 0 0)`, `rgb(255 0 0 / 50%)` | **verify** space/slash handling; pin actual | modern rgb syntax | verify; likely unimplemented or partial |

**B7 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_colors.rs`
  (new; one small table family).
- **No production changes.** Drop-pins via `assert_decl_drop` plus a
  computed read of `as_color().color_rgba` / `as_visual().background_rgba`
  staying at the group initial.
- **Verify first:** the modern `rgb()` space-separated and slash-alpha forms
  against `parse_css_color_rgba` (`dom_attr_parse.rs:136+`) — the parser's
  header comment documents comma or space separated channels and alpha as
  0–1 or %, so parts may already be admitted; pin the actual accept/drop per
  form, on-spec where accepted, unimplemented where dropped.
- **Registry:** `unsupported_*` rows (plus on-spec rows for anything already
  admitted).

**B7 data structures and algorithms (Phase 2 preview).** Current state:
`DeclValue::Color([f32; 4])` is normalized at the declaration boundary by
`parse_css_color_rgba` (`dom_attr_parse.rs:136+`) — colors are typed from
birth, so consumers never re-parse (the redesign's P3 fix for the worst
corner: color used to ride as a `Custom(String)` heap key). Admissions are
pure grammar widenings at that one site: hex3/hex4 (digit-doubling, one
branch), the CSS named-color set (a static `&[(&str, [f32; 4])]` table —
the full ~148 names, sorted, binary search or a phf map), and modern
`rgb()` space/slash forms (the parser's channel splitter, likely
part-admitted already). `currentColor` is the one structural note: it is
not a color but a value REFERENCE — CSS computes it to the element's
`color` value — so it resolves at style time from the color group
(`color_rgba`); the declaration stores `Keyword(CurrentColor)` and the
resolver substitutes the element's own computed color (Servo does exactly
this at computed-value time; Blink keeps it symbolic to used time — our
style-time resolution matches Servo's shape and keeps groups fully
resolved, consistent with the A5/A6 doctrine).



## B8 — selectors & at-rules

An unsupported selector fails `parse_selector` → `DomError::InvalidSelector`
→ **the whole load fails** (fatal at load, unlike declaration drops).

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `div > span { }` | whole load `Err(InvalidSelector)` | child combinator | unimplemented |
| `div + span`, `div ~ span` | whole load Err | sibling combinators | unimplemented |
| `[type]`, `[type=button]` | whole load Err | attribute selectors | unimplemented |
| `div::before`, `p::first-line` | whole load Err | pseudo-elements | unimplemented |
| `:first-child`, `:last-child`, `:nth-child(2)`, `:not(.x)`, `:focus-within`, `:checked`, `:disabled` | whole load Err | more pseudo-classes | unimplemented |
| `.a, .b { }` (selector list) | whole load Err | selector lists | unimplemented |
| `@media (min-width: 100px) { .a { } }` | whole load Err | conditional rules | unimplemented |
| `@supports (display: flex) { }` | whole load Err | feature queries | unimplemented |
| `@keyframes spin { }`, `@font-face` | whole load Err | at-rules | unimplemented |
| stray `}` before a rule | whole load `Err(InvalidSelector)` | CSS error recovery skips to next rule | unimplemented (statement recovery); pin current (fatal) — verified actual |
| missing closing `}` | whole load `Err(InvalidAttributeValue)` | CSS error recovery skips to next rule | unimplemented (statement recovery); pin current (fatal) — verified actual |
| `garbage .a { }` ("stray text" premise) | loads OK as a valid `garbage .a` descendant selector | a valid rule (arbitrary idents are tag selectors) | honesty correction: not stray text |

**B8 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/unsupported_selectors.rs`
  (new).
- **The atomicity truth (F1, adjudicated):** today's `load_stylesheet`
  (`style_runtime.rs:65-132`) is NOT atomic on the error path: the loop
  parses each block's selector (fatal `?`), but rules from EARLIER blocks
  are already inserted, active, and indexed (`self.rules.insert(...)`,
  `index_rule(...)`), and the revision bump sits after the loop so the
  early return skips it. A failed load therefore leaves a silently-mutated
  rule store: the valid prefix is live and will match at the next style
  pass triggered by anything else, while a caller holding `Err` reasonably
  believes nothing happened. That is a boundary-policy violation in
  mechanics (an `Err` that does not mean "no effect"), independent of the
  selector-support question.
- **The F1 fix (Phase 1, named in the phases list):** buffer-then-commit.
  Per block: parse selector (fatal), parse declarations into a LOCAL
  `DeclBlock` (not the arena), dedup-check against active rules, and buffer
  `(block, selector_parts, right_most, specificity, origin, identity_hash)`.
  On full success: insert blocks into the decl arena, insert rules
  (assigning `order` from the counter in buffer order), index them, and
  bump `revision` once when at least one rule committed. On any error:
  discard the buffer — no arena blocks, no rules, no revision touch. Dedup
  dedup behavior unchanged (skipped rules simply aren't buffered). This
  makes `Err` mean "nothing happened," the honest reading of the
  typed-error boundary.
- **Pin rows (two shapes):** (1) single-bad-rule rows asserting
  `Err(InvalidSelector)`; (2) **partial-application rows** — a valid rule
  BEFORE the bad selector — pinned at the CURRENT behavior first
  (`off_spec_*`: `Err` AND the valid prefix still applies at the next
  recompute, revision unbumped), then flipped with the fix in the same
  atomic commit (fix + flip + registry, per the two-commit rule): after
  the fix, `Err` AND nothing applies.
- **The consumer-facing warning, corrected:** today the failure is not
  "one unsupported selector fails the entire sheet" — it is "the load
  errors AND the valid prefix silently applies." After the fix it becomes
  "one unsupported selector fails the entire sheet, atomically" (pinned).
- **`@import` / `@charset` rows:** `@import url(...)` (the first at-rule a
  consumer bundling real CSS hits) and `@charset "utf-8"` — both whole-load
  `Err` today (the block parser sees an at-rule as a selector), class
  unimplemented. VERIFIED ACTUAL: both fail with `Err(InvalidSelector)` — they
  carry no `{`, so `parse_css_blocks` hits its no-open-brace arm.
- **Stray-`}` / block-structure adjudication (decided, recorded here):** pin
  current whole-load `Err` as **unimplemented**: statement-level error recovery
  (CSS 2.1 §4.1.7's malformed-statement rule, the statement-level sibling of
  G's declaration rule) is an admission-gated parser feature. The load error is
  loud (typed `Err` to the caller), so the current shape is acceptable until a
  consumer need is named; the natural admission shape mirrors G at statement
  level (skip the malformed statement, count it, continue). The atomicity fix
  (above) makes these error paths honest in the meantime. VERIFIED ACTUAL
  (two honesty corrections vs the earlier "all `InvalidAttributeValue`"
  claim): a stray `}` before a rule fails with `Err(InvalidSelector)`
  (`parse_css_blocks`'s leading-`}` arm), and a missing closing `}` fails with
  `Err(InvalidAttributeValue)` (its no-close-brace arm); a "stray text" premise
  does NOT fail — `garbage .a { }` loads OK as a valid `garbage .a` descendant
  selector (arbitrary idents are tag selectors).
- **Registry:** `unsupported_*` rows per family (combinators, attribute
  selectors, pseudo-classes, pseudo-elements, selector lists, at-rules,
  statement recovery), plus the `off_spec_*` atomicity row that flips with
  the F1 fix.

**B8 data structures and algorithms (Phase 2 preview, per feature).**
Current state: `SimpleSelector` (`style/mod.rs`) is a flat per-compound
enum matched right-to-left with a parent-chain scan per preceding compound
(the A3 algorithm, shaped like Blink's `RuleSet` bucket + match and Servo's
`SelectorMap`); the right-most inverted indexes
(`id/class/tag/universal_index` in `style_runtime.rs`) make candidate
lookup O(indexed); the closed grammar rejects everything else at load. Per
feature: **selector lists (`,`)** — `parse_css_blocks` splits the selector
text at top-level commas; each member compiles to its own `CompiledRule`
SHARING one `DeclBlockKey` (the decl arena already dedups and shares
blocks by key — no storage change, one line of identity-hash thought:
identical (selector, block) pairs still dedup). **Combinators (`>`, `+`,
`~`)** — a `Combinator` enum between compounds; the matcher replaces the
chain scan for that compound with the exact relation (parent / previous
sibling / preceding siblings — all reachable from the node record's
sibling/parent keys, O(1) or O(siblings)). **Attribute selectors** — a
`SimpleSelector::Attr { name, value_matcher }` variant; matching reads
stored content attrs (`id`/`class` fast paths exist). **Structural
pseudo-classes** (`:first-child`, `:nth-child`, …) — computed on the visit
from sibling links (child index, child count); state pseudo-classes
(`:checked`, `:disabled`) read the widget state bits already on elements.
**`@media`** — the block parser learns at-rule parsing; a rule gains a
media condition (parsed `min/max-width/height` over our closed feature
set); `StyleRuntime` evaluates conditions at match time against the
viewport dimensions the composite stage already owns, and a viewport
resize bumps `revision` (the existing boundary-change seam) so
conditionality re-evaluates on the next update. **`@supports`** — trivial
with the closed property table (a feature is "supported" iff admitted).
**Statement recovery** — the block parser skips to the next `}` on a
malformed statement and counts it in the load report (G's declaration rule
lifted to statement level, with the same report-and-count contract).
**`@keyframes`** — deferred to F stage 3 (the animation tier, B9).



## B9 — cascade tiers

| Input | Expected current | Spec-correct | Class |
| --- | --- | --- | --- |
| `.a { width: 10px !important }` | `!important` token fails the value grammar → declaration dropped+counted; rule otherwise applies | important tier above normal author | unimplemented (F stage 1) |
| `style="width: 10px !important"` (inline) | dropped+counted | same | unimplemented |
| `@layer a { }` / layered origins | whole load Err (at-rule) | cascade layers | unimplemented (F stage 4) |
| `.a { --brand: #112233; width: var(--brand) }` | custom property dropped; `var(--brand)` dropped+counted | custom properties + var() | unimplemented |
| `animation: spin 1s` (declaration) | dropped+counted | animation tier | unimplemented (F stage 3) |
| behavior sheet vs consumer low-priority origin (the documented UA-sheet workaround) | works today (origin.priority ordering) — pin it | UA-sheet analog | on-spec (pin the workaround) |

**B9 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/style/cascade_tiers.rs`
  (new).
- **No production changes.**
- **`!important` rows:** pin the exact current mechanics — the
  `!important` token fails the value grammar, so the DECLARATION is
  dropped+counted while the rest of the rule still applies (`.a { width:
  10px !important; height: 5px }` → height applies, width is dropped).
  Assert both the drop count and the surviving declaration. Note in the
  header that F stage 1's design is already shaped: importance bits arrive
  as a parallel bit-vector (the Servo `declarations_importance` shape named
  in the redesign's F verdict; `decl_block.rs`'s header notes the future
  arrival).
- **Custom properties / `var()`:** `--brand` drops at name lookup (counted);
  `width: var(--brand)` drops at value grammar (counted). Both rows assert
  the drops and the no-effect.
- **`@layer`:** whole-load `Err` (at-rule, same mechanics as B8).
- **Animation tier:** `animation: spin 1s` drops (name not in the table).
- **UA-sheet workaround (on-spec pin):** a stylesheet loaded at
  `StyleRuleOrigin::new("ua", -100)` (i) loses to a `priority: 0` author
  sheet at equal specificity and (ii) beats the behavior sheet (which folds
  at `i32::MIN`). This pins the documented "model a UA sheet as a
  low-priority origin" path so it cannot silently break.
- **Registry:** `unsupported_*` rows + the one on-spec workaround pin.

**B9 data structures and algorithms (Phase 2 preview, per tier).** Current
state: precedence is the `Precedence { origin_priority, inline,
specificity, order }` record (`decl_value.rs`) — widened at redesign time
exactly so these tiers are ADDITIVE (F's "adopt the structure, defer the
stages" verdict), with the behavior sheet folded at `i32::MIN` as the
UA-ish tier. **`!important` (F stage 1):** `DeclBlock` gains a parallel
importance bit-vector (the Servo `declarations_importance: SmallBitVec`
shape, already noted in `decl_block.rs`'s header); parsing strips the
`!important` suffix at declaration level (it is part of the declaration,
not the value — CSS Syntax §5); `Precedence` gains `important: bool` as its
FIRST field (important author beats all normal declarations including
normal inline; our tier order stays a projection of CSS Cascade §6.1:
transitions > important tiers > normal tiers). **Custom properties (F):**
the block gains a string-keyed custom-property map (Servo keeps a
`PropertyDeclarationIdSet` with a custom-name set — the same shape); `var()`
substitution happens at computed-value time with cycle detection and the
guaranteed-invalid concept (CSS Custom Properties §3; Blink/Servo both
substitute at computed-value time). **`@layer` (F stage 4):** a layer table
in `StyleRuntime` (first-seen layer order per origin) and a `layer: u16`
field inserted into `Precedence` per §6.1 (unlayered above layered; order
within layers by the table). **Animation tier (F stage 3):** keyframes
storage (`@keyframes` from B8), a per-property keyframed-interpolation
sampler sharing the transition machinery's `DeclValue` lerp table, and an
animation declaration tier in the winners fold between important and
normal (§6.1's order — the fold's `Precedence` already supports it). **Real
UA tier (F stage 2):** the `i32::MIN` behavior-sheet precedence is
formalized into a named tier field (the workaround pin in this bundle is
its interim guarantee).



## B10 — other shorthands & overflow axes

| Shorthand | Expected current | Expands to | Class |
| --- | --- | --- | --- |
| `font: italic 12px serif` | dropped+counted | font shorthand | unimplemented |
| `background: #112233` | dropped+counted | background-color | unimplemented |
| `all: initial` | dropped+counted | CSS-wide reset | unimplemented |
| `overflow: scroll auto` (two-token), `overflow-x`, `overflow-y` | dropped+counted | per-axis overflow | unimplemented |

**B10 scope and changes.**

- **Proof file:** same `style/unsupported_shorthands.rs` (a second table
  family next to B3's). B10 owns the `overflow-x`/`overflow-y` rows
  (single-owner rule — B6's layout-impact file does not repeat them);
  `text-decoration` is owned by B11.
- **No production changes** — drop-pins plus initial-value computed reads,
  same shape as B3. The `overflow: scroll auto` row also asserts the
  single-token admitted form still works (guard against the pin
  over-reaching).
- **Registry:** `unsupported_*` rows.

**B10 data structures and algorithms (Phase 2 preview).** Identical in shape
to B3: new `Shorthand` variants + `property_shape` names + expansion fns in
`decl_parse.rs`. `overflow-x`/`overflow-y` additionally splits the box
group's single `overflow: OverflowKw` field into `[OverflowKw; 2]` (x, y) —
the ONE group-field change in the bundle, rippling to the clip/scroll
predicates (`creates_clip` = either axis ≠ Visible; `allows_vertical_scroll`
= the y axis) and their consumers (layer kind, scroll gating, flex auto-min
— all already centralized behind those two predicates, which is why the
split stays cheap). `font` maps to the font group's five longhands with the
CSS Fonts 4 shorthand rules (size/line-height/family are the required
components per the spec's grammar, others initial — the closed-subset
projection of the full grammar).



## B11 — text/effects property tail

All dropped+counted, inert, no crash; one mechanical table. B11 owns the
`text-decoration` family and the `animation*` declaration names
(single-owner rule). Rows: `text-decoration` (shorthand + longhands),
`text-transform`, `text-indent`, `text-shadow`,
`text-overflow`, `letter-spacing`, `word-spacing`, `line-clamp` /
`-webkit-line-clamp`, `hyphens`, `box-shadow`, `vertical-align`,
`writing-mode`, `unicode-bidi`, `visibility`, `cursor`, `user-select`,
`list-style*`, `filter`, `mix-blend-mode`, `backdrop-filter`, `clip-path`,
`mask*`, `image-rendering`, `border-style` (see B5's dependency note),
`border-collapse`, `border-spacing`,
`caption-side`, `empty-cells`, `table-layout`, `column-count`, `columns`,
`column-rule`, `orphans`, `widows`, `animation*`,
and the unadmitted VALUE-FUNCTION family (each fails the value grammar,
dropped+counted): `url()` (e.g. `background-image: url(x.png)`),
`linear-gradient()` / `radial-gradient()`, `attr()`, `env()`, `counter()`,
and the `content` / `quotes` / `counter-reset` / `counter-increment`
properties.

**B11 scope and changes.**

- **Proof file:** same `style/unsupported_properties.rs` (the tail table
  family next to B6's).
- **No production changes.** ONE test driving a const table of ~30 property
  names — each row: a single-declaration stylesheet asserts `applied == 0,
  dropped == 1` through `assert_decl_drop` (the enumerable-coverage pattern
  the README conventions ask for). `text-decoration` and `box-shadow` get
  one extra no-effect assertion each (paint plan unchanged) since they are
  the two most popular rows in the tail.
- **Registry:** the family registers as one `unsupported_*` entry naming the
  whole tail table rather than 30 individual rows (the registry stays
  readable; the table itself is the enumeration).

**B11 data structures and algorithms (Phase 2 preview, by popularity).**
`text-decoration` → a text-group field (`TextDecorationKw` line set +
color) with paint emitting the decoration line under/over text runs (the
text paint path already walks runs with their baselines — decoration is a
rect per run segment). `box-shadow` → a bounded shadow list on the visual
group (`ArrayVec<ShadowSpec, 4>` — the damage-rect/transition-longhand
bounded-list precedent: fixed capacity, over-capacity rejected at
admission, revisitable via D6) with paint emitting blurred/offset rects
before the box (blur via the existing rect-op corner machinery or a new op
kind). `text-overflow: ellipsis` → a paint-time clip + ellipsis run
substitution at the line box (cosmic-text provides the ellipsis glyph; the
line-layout walk already knows overflow). Everything else in the tail
admits one property-table row and, where it has no engine meaning yet
(`cursor`, `will-change`, `visibility: collapse`-only rows), stays a
stored computed value until a consumer names the effect — storage is
cheap; behavior is admission-gated per effect.



## B12 — robustness

Every scenario asserts the defined outcome AND returns without panic.

- **Empty/whitespace**: empty stylesheet, whitespace-only, `.a { }`,
  `.a { ; ; }`, `style=""`, `style=";"`.
- **Block-structure abuse**: missing `}`, stray `}`, `}}`, `{` with no `}`
  (fatal load errors are the defined outcome — pin, no crash).
- **Comments**: `/* unterminated`, `/*a*/ width: 1px`, `// line` (verify
  handling), comments inside values.
- **Volume**: 10k-rule stylesheet (loads, no hang), one rule with 200
  declarations, a 64-compound descendant selector (no stack blow).
- **Extreme values**: `font-size: 100000px` render, `margin-left: -3.4e38px`,
  `opacity: 1.0000001`, plus B1's numeric rows' render-side proofs.
- **Typed-writer abuse**: `style_mut(key).set("", "")`, `set("width", "")`,
  `set_all` with 200 declarations, `set` on removed-node key
  (`InvalidNodeKey`), set on document/fragment/text keys
  (`InvalidAttributeTarget`).
- **Encoding**: UTF-8 BOM at stylesheet start, non-ASCII strings, NUL bytes
  in a declaration.
- **`font-size: 0` / `line-height: 0` render** (the crash-adjacent B1 rows —
  the render-side proofs live here).

**B12 scope and changes.**

- **Proof file:** `crates/ui-dom/tests/layout_proofs/robustness.rs` (new,
  at the suite root — it is cross-cutting, not style-domain-only).
- **Volume:** 10,000 rules in one stylesheet load asserts `Ok` with
  `rules_inserted == 10_000` and one match against the last rule (no hang —
  keep the assertion outcome-only, no wall-clock pins); one rule with 200
  declarations asserts the report counts and a sampled computed value; a
  64-compound descendant selector over a 64-deep div nest matches the leaf.
  **Framing (honesty):** the 64-compound row is a TRIPWIRE, not a
  discrimination proof — the load-bearing invariant is that the
  parent-chain scan is iterative by construction (`selector_matches_node`,
  `style/mod.rs`), and the row exists to catch a regression to recursion,
  not to prove a depth limit (64 frames discriminates nothing). State the
  invariant in the pin header.
- **Block-structure abuse:** the fatal load `Err` outcomes are the defined
  behavior — pin them (they overlap B8's adjudicated rows deliberately; the
  robustness file pins the no-crash side, B8 pins the class).
- **Comments:** `strip_block_comments` (`style/mod.rs:274-300`) is the
  owner; pin unterminated comments (defined outcome — verify),
  comment-adjacent declarations (work), and `// line` at both value level
  (drops with the declaration) and statement level (selector error —
  verify and pin the actual).
- **Encoding:** BOM `\u{FEFF}` at stylesheet start (verify: trimmed or
  selector error — pin actual), non-ASCII in strings (works — `font-family`
  with a CJK name interns fine), NUL bytes in a declaration (defined
  outcome, no crash).
- **Typed-writer abuse:** extends `style_mut.rs`'s existing error-path
  tests into the proof suite (empty name/value, removed-node key →
  `InvalidNodeKey`, non-element targets → `InvalidAttributeTarget`,
  `set_all` with 200 declarations → `Ok(report)` with
  `report.applied + report.dropped == 200` exactly and the call completing —
  the "bounded report" assertion).
- **Crash-hardening rule (reiterated):** any panic discovered is a
  boundary-policy bug — the fix lands in the same change (typed error or
  drop) with the pin asserting the hardened behavior, and this document
  records the fix below.
- **Registry:** the family registers as one robustness entry naming the
  file; individual hardening fixes get their own registry notes as they
  happen.

### Hardening fixes landed (running record)

1. **`font-size: 0` crash (found by B1a, fixed in B1b).** The probe
   `robustness_zero_font_size_does_not_crash` panicked inside cosmic-text
   (`assert_ne!(font_size, 0.0)`, `cosmic-text-0.18.2/src/buffer.rs:688`,
   reached via `CosmicMetrics::new` from `text_layout/mod.rs`). CSS Fonts 4
   §3.5 admits zero font sizes, so this is a boundary-policy violation, not
   a grammar rejection candidate. Pinned as `off_spec_font_size_zero_panics`
   (`should_panic`, divergent) in B1a; the B1b guard flipped it to
   `robustness_zero_font_size_renders_nothing`: zero-size text measures
   zero-width (`measure_intrinsic`, `measure_wrapped_text_height`,
   `measured_text_width_px` early-returns) and stages no text op
   (`paint/mod.rs` text-node staging, `element_behaviors/paint.rs` control
   staging) — never a 1px clamp.
   Related finding (B1a investigation): the `line-height` 1px floor is
   enforced at RESOLVE time (`project.rs:137`), not only render time, and
   is load-bearing against cosmic's `line_height != 0` assert
   (`buffer.rs:254`) — reclassified deliberate-with-crash-reason.
   **Follow-up landed (B1b-4):** the caret/editing path is now guarded too —
   `layout_text_caret` (caret at origin, effective line height),
   `layout_text_caret_map` (one zero-width line per source line via
   `zero_size_caret_map`), `layout_text_caret_stop_byte_index_at_point`
   (byte 0), and — surfaced by the pin's own scenario during landing —
   `layout_wrapped_text` (one zero-width line at the effective line height;
   the multi-line textarea value-text-run height path, a sibling cosmic call
   the caret-only guard did not cover). Every production caller of
   `build_text_buffer` / `build_wrapped_text_buffer` /
   `build_wrapped_text_buffer_and_byte_map` is now zero-guarded. Pin:
   `robustness_zero_font_size_textarea_caret_does_not_crash`.

**B12 data structures and algorithms.** No Phase 2 — this bundle is the
permanent safety net, not an admission track. The scenarios stress the
system's existing bounded structures exactly where they are designed to
hold: the fixed-capacity history ring and enum-indexed counters (metrics
doctrine — no allocation under load), the `SlotMap` arenas (10k-rule
volume exercises generational-key density, not pointer storms), the
`SmallVec`/`ArrayVec` bounded lists (200-declaration blocks), the
parent-chain selector scan (64-deep nesting — iterative, not recursive, by
construction), and the typed-error boundary (every malformed input returns
`DomError` or drops with a count — the two sanctioned outcomes, never a
panic). A failure of any of these is a boundary-policy bug with a fix in
the same change, recorded above.



## Open verifications (resolve at pin time; correct the doc in the same change)

- `rgb()` space-separated / slash-alpha handling in the current color parser.
- `text-align: justified` and the `pre*`/`break-spaces` white-space modes:
  real consumption in text layout or keyword-only (justify is verified
  CONSUMED — `text_render.rs:106`; the white-space modes are open).
- `border-radius` paint clamp behavior on oversized radii.
- Whether `font-size: 0` and negative sizes can reach a panic in
  cosmic-text metrics (then harden: grammar rejects `< 0`, and `0` renders
  nothing without crashing).

Resolved this session: the `pointer-events` channel (content-side
`Custom("pointer-events")`, `none`/`auto`/inherit at `hit_test.rs:331-337`
— see B4) and the `order` clamp target (saturating i64→i32 at
`project.rs:444-445` — see B1).

## Decision log (dated; consolidated so nothing is re-litigated)

- **2026-10-18 — B5 adjudications.** near-white initial `color` →
  **deliberate** (dark-theme design-system default; permanent).
  `line-height: normal` → `1.0 × font-size` → **divergent** (browsers ~1.2;
  D6 favors the browser answer; moves when a consumer need is named).
  border widths initial `0` → **deliberate** (observably identical to CSS's
  `medium × style:none` for undeclared borders; `border-style` admission
  revisits).
- **2026-10-18 — the admit-side/reject-side rule.** Admit-side divergences
  (we accept what the spec rejects) are hazards, fixed in Phase 1.
  Reject-side divergences (we reject what the spec admits) are gates,
  pinned, then moved through D6 admission. (Written into Conventions.)
- **2026-10-18 — F1 atomicity.** `load_stylesheet` gets buffer-then-commit
  (an `Err` must mean "nothing happened"), landing in Phase 1 with
  partial-application pins flipped in the same atomic commit.
- **2026-10-18 — statement recovery stays unimplemented.** Stray-`}` /
  block-structure errors keep the whole-load `Err` (loud, typed);
  statement-level recovery is admission-gated, with G's shape as its model.
- **2026-10-18 — `pointer-events` is behavior input, not style.** It stays
  a content-side `Custom` channel (deliberate); the stylesheet-channel
  absence is pinned as unimplemented and would move into the presentation
  table only through admission.
- **2026-10-18 — ownership.** `overflow-x/y` → B10; `text-decoration`
  family → B11; `animation` shorthand → B9 (tier row); `animation-*`
  longhands → B11 (tail rows); `border-style` → B11 (with B5's
  cross-reference).
- **2026-10-18 — two-commit rule for fix bundles; one commit for pin-only
  bundles.** No commit is ever red; behavior change and its proof move
  together (see the Execution playbook).
- **2026-10-18 — B1's fix generalizes the existing non-negative helpers**
  (`parse_non_negative_f32`/`parse_non_negative_length`), and the expansion
  layer applies per-longhand grammar so policy cannot be bypassed via
  shorthands.
- **2026-10-18 — `font-size: unset` admitted** into the B1 fix family,
  resolving to the inherited seed (CSS `unset` on an inherited property).
- **2026-10-18 — exhaustiveness is honest, not claimed.** The inventory
  grows as gaps are discovered; B11's tail regenerates from a CSS property
  index diffed against the 68-name table at each admission.
- **2026-10-18 — no new features in Phase 1.** Pins, the two named fixes,
  adjudications, registry. Everything else is Phase 2.

## Landing status (update in the same commit as each landing)

Branch: `proof-accounting` (off `main` @ v0.6.0, commit `5ab7622`).
Registry baseline before the accounting: 1 divergent (negative margins,
`layout/normal_flow.rs`), 1 deliberate (16:9, `layout/replaced_elements.rs`),
2 unimplemented (flex subsets; cascade `!important`/UA/layers).

| Landing | Status |
| --- | --- |
| Plan: replan as coverage accounting (`86c42d7`) | done |
| Plan: bundles + ratings (`991e0e6`) | done |
| Plan: scope-and-changes per bundle (`13856db`) | done |
| Plan: phases + data structures/algorithms (`6fa0a75`, `2d5b951`) | done |
| Plan: adversarial review adjudication (`85de600`) | done |
| Plan: compaction-survival pass (decision log, evidence registry, playbook) | done |
| Batch 0 — suite reorganization | done |
| B1 — sign rules & numeric edges | **done** (a `4cb6acd` pins; b — fix + flips, incl. the font-size: 0 guard; caret-path follow-up queued below) |
| B1 (a) — pins of current behavior (`style/unsupported_values.rs`) | done |
| B2 — units & sizing keywords | **done** (pins; all rows verified; the stale `font-size: unset` row was resolved by B1b and is marked above) |
| B3 — box shorthands | done (pins; all rows verified dropped+counted + group-initial) |
| B4 — text consumption & admitted quirks | done (pins; all rows verified; break-spaces classed divergent as a pre-wrap fold; see `style/admitted_gaps.rs`) |
| B5 — preserved candidates | **done** (pins; three adjudications recorded in the B5 section and decision log: near-white initial `color` deliberate, `line-height: normal` → 1.0× divergent, border widths initial `0` deliberate; see `style/admitted_gaps.rs`) |
| B6 — layout-impact properties | **done** (pins; all rows verified dropped+counted + per-family no-effect; two honesty corrections recorded: the crate's single sizing model is content-box — not the "border-box" this plan previously stated — and a bare unloaded `img` resolves 160×0 while the 160×90 placeholder is the intrinsic, materialized via ratio transfer; see `style/unsupported_properties.rs`) |
| B7 — color grammar | **done** (`d15c13b` — pins; the verified modern-rgb accept/drop map recorded: comma/space/percent admitted, slash-alpha dropped) |
| B8 — selectors & at-rules (+F1 fix) | **done** (a — pins; b — F1 atomicity fix + pin flip) |
| B8 (a) — pins of the non-F1 rows + the F1 off-spec pin (`style/unsupported_selectors.rs`) | done |
| B8 (b) — F1 atomicity fix (buffer-then-commit) + pin flip (`stylesheet_load_failure_is_atomic`) | done |
| B9 — cascade tiers | **done** (pins; all rows verified — `!important` drops the declaration while the rule survives (stylesheet and `style`-attribute routes), `@layer` fails the whole load with `InvalidSelector`, `--brand`/`var()` drop at name lookup and value grammar respectively, `animation` drops, and the UA-sheet workaround is on-spec; see `style/cascade_tiers.rs`) |
| B10 — other shorthands & overflow axes | **done** (pins; all rows verified — `font`/`background`/`all`/`overflow-x`/`overflow-y` drop+count at name lookup with group-initial computed reads; the two-token `overflow: scroll auto` drops as ONE declaration at the value grammar, and the CONTROL `overflow: scroll` single-token admitted form still applies; see `style/unsupported_shorthands.rs`) |
| B11 — text/effects property tail | **done** (pins; all rows verified dropped+counted — `text-decoration` shorthand+longhands, `box-shadow`, the 40-row tail table, the value-function family (name-lookup vs value-grammar split, with the `width: 10px` CONTROL), and `content`/`quotes`/`counter-*`; `text-decoration`/`box-shadow` carry a no-effect control — staged op count equals the unstyled baseline; see `style/unsupported_properties.rs`) |
| B12 — robustness | not started |
| Phase 2 admissions | not started (gated on Phase 1 exit + consumer need) |

